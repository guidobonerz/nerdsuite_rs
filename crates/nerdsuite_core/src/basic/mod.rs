//! The BASIC tokenizer, as nerdsuite2's `basic.js`: Commodore BASIC to a PRG and back,
//! compatible with VICE petcat (token values and text conventions); the other dialects to their
//! text listings (`listing`) or to a ZX81 `.p` file (`zx81`).

pub mod dialects;
pub mod listing;
pub mod petscii;
pub mod zx81;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::OnceLock;

use anyhow::{Result, anyhow};

pub use dialects::{Dialect, Keyword, KeywordToken, Machine, Output, dialect, dialects};
pub use petscii::{
    ControlCode, control_code_name, control_codes, control_codes_for, lookup_control_code,
};

const MAX_LINE_NUMBER: u64 = 63999;
const MAX_LINE_BYTES: usize = 255;
const OPERATORS: &str = "+-*/^>=<";

/// An error in a line of the source (1-based).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicError {
    pub line: usize,
    pub msg: String,
}

/// A line of the program: the line of the source it came from, its number and its address
/// (the offset in the listing for the listing dialects).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BasicLine {
    pub src: usize,
    pub number: u32,
    pub addr: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenizeOptions<'a> {
    pub dialect: &'a str,
    /// The load address; default: the one of `machine`, or of the dialect.
    pub start: Option<u16>,
    pub machine: Option<&'a str>,
    /// The line of the source each line of the text came from (the preprocessor).
    pub src_lines: Option<&'a [usize]>,
}

impl Default for TokenizeOptions<'_> {
    fn default() -> Self {
        TokenizeOptions {
            dialect: "v2",
            start: None,
            machine: None,
            src_lines: None,
        }
    }
}

impl<'a> TokenizeOptions<'a> {
    pub fn dialect(dialect: &'a str) -> Self {
        TokenizeOptions {
            dialect,
            ..Default::default()
        }
    }
}

/// What `tokenize` made of a program. The valid lines are emitted even when others have errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokenized {
    pub ok: bool,
    pub errors: Vec<BasicError>,
    /// The PRG with its load address (Commodore dialects).
    pub prg: Option<Vec<u8>>,
    /// The listing or the `.p` file (the other dialects; for the ZX81 None when there are errors).
    pub listing: Option<Vec<u8>>,
    pub format: Output,
    pub start: Option<u16>,
    pub lines: Vec<BasicLine>,
}

impl Tokenized {
    /// The output file: the PRG or the listing.
    pub fn bytes(&self) -> Option<&[u8]> {
        self.prg.as_deref().or(self.listing.as_deref())
    }
}

// ------------------------------------------------------------------ text helpers

/// The lines of a text, split at CR LF, CR and LF.
pub(crate) fn split_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(p) = rest.find(['\r', '\n']) {
        out.push(&rest[..p]);
        let skip = if rest[p..].starts_with("\r\n") { 2 } else { 1 };
        rest = &rest[p + skip..];
    }
    out.push(rest);
    out
}

/// `/^\s*(\d+)/`: the digits and what follows them.
pub(crate) fn leading_number(raw: &str) -> Option<(&str, &str)> {
    let s = raw.trim_start();
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    (n > 0).then(|| (&s[..n], &s[n..]))
}

pub(crate) fn parse_saturating(digits: &str) -> u64 {
    digits.bytes().fold(0u64, |v, d| {
        v.saturating_mul(10).saturating_add((d - b'0') as u64)
    })
}

fn lower(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

// ------------------------------------------------------------------ dialect preparation

#[derive(Debug, Clone, Copy)]
struct Entry {
    name: &'static str,
    bytes: [u8; 2],
    len: usize,
}

impl Entry {
    fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// A dialect ready for the tokenizer.
struct Prep {
    id: &'static str,
    /// Matched first (Simons' $64xx, 7.0 $FExx/$CExx); petcat tries $FE before $CE.
    prefixed: Vec<Vec<Entry>>,
    common: Vec<Entry>,
    /// Extension tokens above the common range (BASIC 4.0 $CC-$DA).
    ext: Vec<Entry>,
    single: HashMap<u8, &'static str>,
    two_byte: HashMap<u8, HashMap<u8, &'static str>>,
}

fn prep(def: &'static Dialect) -> &'static Prep {
    static CACHE: OnceLock<Vec<Prep>> = OnceLock::new();
    let all = CACHE.get_or_init(|| dialects().iter().map(build_prep).collect());
    &all[dialects().iter().position(|d| d.id == def.id).unwrap()]
}

fn build_prep(d: &'static Dialect) -> Prep {
    // petcat: the common keyword range is $80-$CB, or $80-$FD for BASIC 3.5/7.0/65
    let max_common = if matches!(d.id, "v35" | "v7" | "v65") {
        0xfd
    } else {
        0xcb
    };
    let mut prefixed = Vec::new();
    let mut common = Vec::new();
    let mut ext = Vec::new();
    let mut single = HashMap::new();
    let mut two_byte: HashMap<u8, HashMap<u8, &'static str>> = HashMap::new();
    for kw in d.keywords {
        match kw.token {
            Some(KeywordToken::Pair(pre, b)) => {
                two_byte.entry(pre).or_default().insert(b, kw.name);
                prefixed.push(Entry {
                    name: kw.name,
                    bytes: [pre, b],
                    len: 2,
                });
            }
            Some(KeywordToken::Byte(t)) => {
                single.insert(t, kw.name);
                if t == 0xff {
                    continue; // pi: '~' is converted as a character
                }
                let e = Entry {
                    name: kw.name,
                    bytes: [t, 0],
                    len: 1,
                };
                if t <= max_common {
                    common.push(e)
                } else {
                    ext.push(e)
                }
            }
            None => {}
        }
    }
    let by_token = |e: &Entry| (e.bytes[0], if e.len == 2 { e.bytes[1] } else { 0 });
    // the $FE group before the $CE group (petcat's order); in a group the longest match wins,
    // ties go to later tokens
    let group = |f: &dyn Fn(u8) -> bool| {
        let mut g: Vec<Entry> = prefixed
            .iter()
            .filter(|e: &&Entry| f(e.bytes[0]))
            .copied()
            .collect();
        g.sort_by_key(by_token);
        g
    };
    let groups = [
        group(&|p| p != 0xfe && p != 0xce),
        group(&|p| p == 0xfe),
        group(&|p| p == 0xce),
    ];
    common.sort_by_key(by_token);
    ext.sort_by_key(by_token);
    Prep {
        id: d.id,
        prefixed: groups.into_iter().filter(|g| !g.is_empty()).collect(),
        common,
        ext,
        single,
        two_byte,
    }
}

fn starts_with(s: &[char], pos: usize, name: &str) -> bool {
    let n = name.len();
    pos + n <= s.len()
        && name
            .bytes()
            .zip(&s[pos..pos + n])
            .all(|(a, &b)| a as char == b)
}

/// The longest keyword of `list` at `pos` of the lowercase `s` (ties: the later token).
fn longest<'a>(list: &'a [Entry], s: &[char], pos: usize) -> Option<&'a Entry> {
    let mut best: Option<&Entry> = None;
    for kw in list {
        if starts_with(s, pos, kw.name) && best.is_none_or(|b| kw.name.len() >= b.name.len()) {
            best = Some(kw);
        }
    }
    best
}

/// The keyword the tokenizer picks at `pos` (petcat's precedence rules).
fn match_keyword<'a>(prep: &'a Prep, s: &[char], pos: usize) -> Option<&'a Entry> {
    for g in &prep.prefixed {
        if let Some(m) = longest(g, s, pos) {
            return Some(m);
        }
    }
    let c = longest(&prep.common, s, pos);
    let e = longest(&prep.ext, s, pos);
    match (c, e) {
        (Some(c), Some(e)) => Some(if e.name.len() >= c.name.len() { e } else { c }),
        (c, e) => e.or(c),
    }
}

// ------------------------------------------------------------------ tokenizer

/// The code editor holds a byte of a string that has no character of its own (a control code,
/// a graphic) as one character: U+EE00 + the byte.
pub const PETSCII_BYTE: u32 = 0xee00;

/// petcat's `ascii_to_petscii`: the byte of a character of a string.
pub fn ascii_to_petscii(ch: char) -> Option<u8> {
    match ch {
        '£' => return Some(0x5c),
        'π' => return Some(0xff),
        '←' => return Some(0x5f),
        '↑' => return Some(0x5e),
        _ => {}
    }
    let c = ch as u32;
    if (PETSCII_BYTE..=PETSCII_BYTE + 0xff).contains(&c) {
        return Some((c - PETSCII_BYTE) as u8); // a byte as the editor holds it
    }
    let c = u8::try_from(c).ok().filter(|&c| c <= 0x7e)?;
    Some(match c {
        0x7e => 0xff, // '~' = pi
        0x5b..=0x5f => c,
        0x60.. => c ^ 0x20,      // a-z -> $41-$5A
        0x41..=0x5a => c | 0x80, // A-Z -> $C1-$DA (shifted)
        _ => c,
    })
}

/// Parses the control code at `s[i] == '{'`: its bytes and the index after it. The formats of
/// petcat: `{name}`, `{$xx}`, `{nnn}` (decimal), `{n name}`, `{nname}`, `{name*n}`.
pub fn parse_control_code(s: &[char], i: usize, dialect: &str) -> Result<(Vec<u8>, usize), String> {
    let Some(close) = s[i + 1..].iter().position(|&c| c == '}').map(|p| p + i + 1) else {
        let t: String = s[i..s.len().min(i + 12)].iter().collect();
        return Err(format!("unterminated control code \"{t}\""));
    };
    let inner: String = s[i + 1..close].iter().collect();
    let mut body = inner.as_str();
    let mut count = 1u64;
    let lead = body.bytes().take_while(u8::is_ascii_digit).count();
    if lead > 0 {
        if lead == body.len() {
            let v = parse_saturating(body);
            if v > 255 {
                return Err(format!("character code out of range: {{{body}}}"));
            }
            return Ok((vec![v as u8], close + 1));
        }
        count = parse_saturating(&body[..lead]);
        body = &body[lead..];
        body = body.strip_prefix(' ').unwrap_or(body);
    }
    let mut code = lookup_control_code(body, dialect);
    if code.is_none()
        && let Some(star) = body.rfind('*')
        && star > 0
        && star + 1 < body.len()
        && body[star + 1..].bytes().all(|b| b.is_ascii_digit())
    {
        code = lookup_control_code(&body[..star], dialect);
        count = parse_saturating(&body[star + 1..]);
    }
    let Some(code) = code else {
        return Err(format!("unknown control code {{{inner}}}"));
    };
    if count > 255 {
        return Err(format!("repeat count too large in {{{inner}}}"));
    }
    Ok((vec![code; count as usize], close + 1))
}

fn tokenize_line(text: &[char], prep: &Prep) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let low: Vec<char> = text.iter().map(|&c| lower(c)).collect();
    let (mut quote, mut rem, mut data) = (false, false, false);
    let mut i = 0;
    while i < text.len() {
        let ch = text[i];
        if ch == '"' {
            quote = !quote;
        }
        if ch == '{' {
            let (bytes, end) = parse_control_code(text, i, prep.id)?;
            out.extend(bytes);
            i = end;
            continue;
        }
        let code = !quote && !rem && !data;
        if code
            && (ch.is_ascii_alphabetic() || OPERATORS.contains(ch))
            && let Some(kw) = match_keyword(prep, &low, i)
        {
            out.extend_from_slice(kw.bytes());
            i += kw.name.len();
            match kw.bytes() {
                [0x8f] => rem = true,
                [0x83] => data = true,
                _ => {}
            }
            continue;
        }
        if code && ch == '?' {
            // PRINT shortcut
            out.push(0x99);
            i += 1;
            continue;
        }
        // outside strings/REM/DATA letters are case-insensitive: always $41-$5A
        let Some(p) = ascii_to_petscii(if code { low[i] } else { ch }) else {
            return Err(format!("unsupported character \"{ch}\""));
        };
        out.push(p);
        if ch == ':' && !quote {
            data = false;
        }
        i += 1;
    }
    Ok(out)
}

/// Tokenizes BASIC source text: a PRG with its load address for the Commodore dialects, a text
/// listing or a `.p` file for the others. Errors only for an unknown dialect or machine.
pub fn tokenize(text: &str, opts: &TokenizeOptions) -> Result<Tokenized> {
    let def = dialect(opts.dialect)
        .ok_or_else(|| anyhow!("Unknown BASIC dialect \"{}\"", opts.dialect))?;
    let src_of = |n: usize| {
        opts.src_lines
            .and_then(|s| s.get(n - 1).copied())
            .unwrap_or(n)
    };
    match def.output {
        Output::Prg => {}
        Output::Zx81P => {
            // a .p file; the lines and errors in the lines of the source
            let r = zx81::tokenize_zx81(text);
            return Ok(Tokenized {
                ok: r.ok,
                errors: r
                    .errors
                    .into_iter()
                    .map(|e| BasicError {
                        line: src_of(e.line),
                        ..e
                    })
                    .collect(),
                prg: None,
                listing: r.bytes,
                format: def.output,
                start: Some(zx81::PROGRAM),
                lines: r
                    .lines
                    .into_iter()
                    .map(|l| BasicLine {
                        src: src_of(l.src),
                        ..l
                    })
                    .collect(),
            });
        }
        _ => return Ok(listing::tokenize_listing(text, def)),
    }
    let prep = prep(def);
    let start = match (opts.start, opts.machine) {
        (Some(s), _) => s,
        (None, Some(m)) => def
            .machine(m)
            .ok_or_else(|| anyhow!("Unknown machine \"{m}\" for dialect \"{}\"", def.id))?
            .start
            .unwrap_or(0),
        (None, None) => def.start.unwrap_or(0),
    };
    let mut errors = Vec::new();
    let mut parsed: Vec<(usize, u32, Vec<u8>)> = Vec::new();
    let mut prev: i64 = -1;
    for (idx, raw) in split_lines(text).into_iter().enumerate() {
        let line = src_of(idx + 1);
        let mut err = |msg: String| errors.push(BasicError { line, msg });
        if raw.chars().all(char::is_whitespace) || raw.starts_with(';') {
            continue; // blank line / petcat comment
        }
        let Some((digits, rest)) = leading_number(raw) else {
            err("missing line number".into());
            continue;
        };
        let number = parse_saturating(digits);
        if digits.len() > 5 || number > MAX_LINE_NUMBER {
            err(format!(
                "line number {digits} out of range (0-{MAX_LINE_NUMBER})"
            ));
            continue;
        }
        if number as i64 == prev {
            err(format!("duplicate line number {number}"));
            continue;
        }
        if (number as i64) < prev {
            err(format!(
                "line number {number} is not greater than previous line {prev}"
            ));
            continue;
        }
        prev = number as i64;
        let body: Vec<char> = rest.trim_start().chars().collect();
        if body.is_empty() {
            err(format!("line {number} is empty"));
            continue;
        }
        match tokenize_line(&body, prep) {
            Err(msg) => err(msg),
            Ok(b) if b.len() > MAX_LINE_BYTES => err(format!(
                "line {number} too long ({} bytes tokenized, max {MAX_LINE_BYTES})",
                b.len()
            )),
            Ok(b) => parsed.push((line, number as u32, b)),
        }
    }

    let mut bytes = vec![start as u8, (start >> 8) as u8];
    let mut lines = Vec::new();
    let mut addr = start as usize;
    for (src, number, b) in parsed {
        let next = addr + b.len() + 5;
        if next + 2 > 0x10000 {
            errors.push(BasicError {
                line: src,
                msg: "program too large: exceeds address $FFFF".into(),
            });
            break;
        }
        lines.push(BasicLine { src, number, addr });
        bytes.extend([
            next as u8,
            (next >> 8) as u8,
            number as u8,
            (number >> 8) as u8,
        ]);
        bytes.extend(b);
        bytes.push(0);
        addr = next;
    }
    bytes.extend([0, 0]);
    Ok(Tokenized {
        ok: errors.is_empty(),
        errors,
        prg: Some(bytes),
        listing: None,
        format: Output::Prg,
        start: Some(start),
        lines,
    })
}

// ------------------------------------------------------------------ detokenizer

fn hex(c: u8) -> String {
    format!("{{${c:02x}}}")
}

const LITERAL_OPS: [u8; 8] = [0x2a, 0x2b, 0x2d, 0x2f, 0x3c, 0x3d, 0x3e, 0x5e];

/// petcat's `_p_toascii` (control codes on): the source form of a byte of a string.
pub fn char_out(c: u8, dialect: &str) -> String {
    match c {
        0x40 => "@".into(),
        0x20..0x40 => (c as char).to_string(),
        0x41..=0x5a => ((c + 0x20) as char).to_string(),
        0x5b..=0x5f => ['[', '\\', ']', '^', '_'][(c - 0x5b) as usize].to_string(),
        0xc1..=0xda => ((c - 0x80) as char).to_string(),
        0xff => "~".into(),
        0xc0 => "{shift-*}".into(),
        0xa0 | 0xde | 0x0d | 0x0a => hex(c),
        _ => control_code_name(c, dialect).map_or_else(|| hex(c), |n| format!("{{{n}}}")),
    }
}

/// A byte as a character for the keyword lookahead ('\0' for bytes no keyword matches).
fn look_char(c: u8) -> char {
    match c {
        0x41..=0x5a => (c + 0x20) as char,
        0x20..0x60 => c as char,
        _ => '\0',
    }
}

/// The escaped form of raw bytes: always tokenizes to exactly these bytes.
fn escape_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&c| {
            if (0x41..=0x5a).contains(&c) {
                format!("{{{}}}", (c + 0x20) as char)
            } else {
                hex(c)
            }
        })
        .collect()
}

struct Piece {
    text: String,
    /// The escaped form.
    alt: String,
    start: usize,
    end: usize,
}

/// One line's content. The pieces are rendered petcat style, then the text is tokenized again and
/// any piece where the bytes would differ (letters merging with a following keyword: "I"+FRE ->
/// "ifre" = IF ...) is replaced by its escaped form until the line round-trips exactly.
fn detokenize_line(b: &[u8], prep: &Prep) -> String {
    let pieces = render_pieces(b, prep);
    let mut texts: Vec<&str> = pieces.iter().map(|p| p.text.as_str()).collect();
    for iter in 0..=pieces.len() {
        let joined: Vec<char> = texts.concat().chars().collect();
        let r = tokenize_line(&joined, prep);
        if r.as_deref() == Ok(b) {
            break;
        }
        let mut k = match &r {
            Err(_) => None,
            Ok(bytes) => {
                let d = bytes.iter().zip(b).take_while(|(x, y)| x == y).count();
                pieces.iter().position(|p| d >= p.start && d < p.end)
            }
        }
        .map_or(pieces.len() as isize - 1, |k| k as isize);
        // already escaped: blame an earlier piece
        while k >= 0 && texts[k as usize] == pieces[k as usize].alt {
            k -= 1;
        }
        if k < 0 || iter == pieces.len() {
            return escape_bytes(b);
        }
        texts[k as usize] = &pieces[k as usize].alt;
    }
    texts.concat()
}

/// A line split into rendered pieces.
fn render_pieces(b: &[u8], prep: &Prep) -> Vec<Piece> {
    let look: Vec<char> = b.iter().map(|&c| look_char(c)).collect();
    let first_non_space = b.iter().position(|&c| c != 0x20).unwrap_or(b.len());
    let last_non_space = b
        .iter()
        .rposition(|&c| c != 0x20)
        .map_or(-1, |p| p as isize);
    let mut pieces = Vec::new();
    let mut emit = |start: usize, len: usize, text: String| {
        let end = (start + len).min(b.len());
        pieces.push(Piece {
            text,
            alt: escape_bytes(&b[start..end]),
            start,
            end,
        });
    };
    let (mut quote, mut rem, mut data) = (false, false, false);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let code = !quote && !rem && !data;
        if code && c >= 0x80 && c != 0xff {
            if let Some(two) = prep.two_byte.get(&c) {
                let text = match b.get(i + 1).and_then(|n| two.get(n)) {
                    Some(name) => name.to_string(),
                    None => escape_bytes(&b[i..(i + 2).min(b.len())]),
                };
                emit(i, 2, text);
                i += 2;
                continue;
            }
            emit(
                i,
                1,
                prep.single
                    .get(&c)
                    .map_or_else(|| hex(c), |n| n.to_string()),
            );
            if c == 0x8f {
                rem = true;
            } else if c == 0x83 {
                data = true;
            }
            i += 1;
            continue;
        }
        if code && c == 0x64 && prep.id == "simons" {
            match b.get(i + 1) {
                Some(&n) if n < 0x80 => {
                    let text = prep.two_byte[&0x64]
                        .get(&n)
                        .map_or_else(|| hex(0x64) + &hex(n), |s| s.to_string());
                    emit(i, 2, text);
                    i += 2;
                }
                _ => {
                    emit(i, 1, hex(0x64));
                    i += 1;
                }
            }
            continue;
        }
        let text = if c == 0x22 {
            quote = !quote;
            "\"".to_string()
        } else if c == 0x20 {
            if i < first_non_space || i as isize > last_non_space {
                hex(0x20)
            } else {
                " ".into()
            }
        } else if code && LITERAL_OPS.contains(&c) {
            format!("{{{}}}", c as char) // would become an operator token
        } else if code && c == 0x3f {
            hex(c) // would become PRINT
        } else if code && (0x41..=0x5a).contains(&c) && match_keyword(prep, &look, i).is_some() {
            format!("{{{}}}", (c + 0x20) as char) // literal keyword text, petcat style: {p}rint
        } else {
            if c == 0x3a && !quote {
                data = false;
            }
            char_out(c, prep.id)
        };
        emit(i, 1, text);
        i += 1;
    }
    pieces
}

/// A tokenized program (a PRG with its load address, a listing, a `.p` file) back to text:
/// petcat style for the Commodore dialects (lowercase keywords, `{codes}` for control
/// characters), up to the end-of-program marker.
pub fn detokenize(data: &[u8], dialect_id: &str) -> Result<String> {
    let def =
        dialect(dialect_id).ok_or_else(|| anyhow!("Unknown BASIC dialect \"{dialect_id}\""))?;
    match def.output {
        Output::Prg => {}
        Output::Zx81P => return Ok(zx81::detokenize_zx81(data)),
        _ => return Ok(listing::detokenize_listing(data, def)),
    }
    let prep = prep(def);
    let mut lines = Vec::new();
    let mut p = 2;
    while p + 4 <= data.len() && data[p + 1] != 0 {
        let number = data[p + 2] as u32 | (data[p + 3] as u32) << 8;
        p += 4;
        let end = data[p..]
            .iter()
            .position(|&c| c == 0)
            .map_or(data.len(), |e| p + e);
        lines.push(format!("{number} {}", detokenize_line(&data[p..end], prep)));
        p = end + 1;
    }
    Ok(if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    })
}
