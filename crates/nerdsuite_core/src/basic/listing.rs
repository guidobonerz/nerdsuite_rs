//! The text listings of the dialects without tokenized binaries, as nerdsuite2's `listing.js`:
//! - atari / turbo: an ATASCII listing for `ENTER "D:..."` (EOL = $9B)
//! - loco: the CPC ASCII file of `SAVE "x",A` (CR LF, ended by $1A)
//! - trs80, bbc: ASCII listings with CR line ends (`Dialect::listing`)

use std::collections::HashSet;

use super::dialects::{Dialect, Listing, Output};
use super::{BasicError, BasicLine, Tokenized};

/// An ATASCII escape (inside strings and REM; `{$xx}` works everywhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtasciiCode {
    pub name: &'static str,
    /// None for `{inv}` and `{/inv}`, which set and clear bit 7.
    pub code: Option<u8>,
    pub desc: &'static str,
}

const fn a(name: &'static str, code: u8, desc: &'static str) -> AtasciiCode {
    AtasciiCode {
        name,
        code: Some(code),
        desc,
    }
}

pub const ATASCII_CODES: &[AtasciiCode] = &[
    a("clear", 0x7d, "clear screen"),
    a("up", 0x1c, "cursor up"),
    a("down", 0x1d, "cursor down"),
    a("left", 0x1e, "cursor left"),
    a("right", 0x1f, "cursor right"),
    a("esc", 0x1b, "escape (print next control char literally)"),
    a("bell", 0xfd, "buzzer"),
    a("bs", 0x7e, "backspace / delete char"),
    a("tab", 0x7f, "tab"),
    a("insert", 0xff, "insert char"),
    a("delete", 0xfe, "delete char"),
    a("delline", 0x9c, "delete line"),
    a("insline", 0x9d, "insert line"),
    a("clrtab", 0x9e, "clear tab stop"),
    a("settab", 0x9f, "set tab stop"),
    AtasciiCode {
        name: "inv",
        code: None,
        desc: "start inverse video (sets bit 7)",
    },
    AtasciiCode {
        name: "/inv",
        code: None,
        desc: "end inverse video",
    },
];

fn atascii_by_name(name: &str) -> Option<u8> {
    ATASCII_CODES
        .iter()
        .find(|c| c.name == name)
        .and_then(|c| c.code)
}

fn atascii_by_code(code: u8) -> Option<&'static str> {
    ATASCII_CODES
        .iter()
        .find(|c| c.code == Some(code))
        .map(|c| c.name)
}

/// The CPC listing; the other ASCII ones are `Dialect::listing`.
pub const CPC: Listing = Listing {
    eol: &[0x0d, 0x0a],
    end: &[0x1a],
    upper_all: false,
    rem_quote: true,
    max_bytes: 255,
};

enum Escape {
    Code(u8),
    Inv(bool),
}

fn parse_escape(s: &[char], i: usize, atari: bool) -> Result<(Escape, usize), String> {
    let Some(close) = s[i + 1..].iter().position(|&c| c == '}').map(|p| p + i + 1) else {
        let t: String = s[i..s.len().min(i + 12)].iter().collect();
        return Err(format!("unterminated escape \"{t}\""));
    };
    let raw: String = s[i + 1..close].iter().collect();
    let name = raw.to_lowercase();
    let end = close + 1;
    if let Some(h) = name.strip_prefix('$')
        && h.len() == 2
        && h.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Ok((Escape::Code(u8::from_str_radix(h, 16).unwrap()), end));
    }
    if atari {
        match name.as_str() {
            "inv" => return Ok((Escape::Inv(true), end)),
            "/inv" => return Ok((Escape::Inv(false), end)),
            n => {
                if let Some(c) = atascii_by_name(n) {
                    return Ok((Escape::Code(c), end));
                }
            }
        }
    }
    Err(format!("unknown escape {{{raw}}}"))
}

/// JS `String.prototype.trimEnd` on bytes as Latin-1 characters.
fn trim_end_bytes(b: &[u8]) -> &[u8] {
    let n = b
        .iter()
        .rposition(|&c| !matches!(c, 0x09..=0x0d | 0x20 | 0xa0))
        .map_or(0, |p| p + 1);
    &b[..n]
}

fn starts_with_ci(s: &[char], word: &str) -> bool {
    s.len() >= word.len() && word.chars().zip(s).all(|(w, c)| c.eq_ignore_ascii_case(&w))
}

/// Atari: everything outside strings, REM and DATA is uppercased (Atari BASIC only accepts
/// uppercase).
fn atari_line(body: &[char]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut quote = false;
    let mut literal = false; // after REM / DATA: rest of line verbatim
    let mut inv = false;
    let mut i = 0;
    while i < body.len() {
        let ch = body[i];
        if ch == '{' {
            let (e, end) = parse_escape(body, i, true)?;
            match e {
                Escape::Inv(on) => inv = on,
                Escape::Code(c) => out.push(c),
            }
            i = end;
            continue;
        }
        let c = ch as u32;
        if !(0x20..=0x7e).contains(&c) {
            return Err(format!("unsupported character \"{ch}\" (use {{$xx}})"));
        }
        // a statement starts at the beginning, after ':' or THEN
        let before = trim_end_bytes(&out);
        let statement = before.is_empty() || before.ends_with(b":") || before.ends_with(b"THEN");
        if !quote
            && !literal
            && statement
            && let Some(kw) = ["rem", "data"]
                .into_iter()
                .find(|k| starts_with_ci(&body[i..], k))
        {
            out.extend(kw.to_ascii_uppercase().bytes());
            i += kw.len();
            literal = true;
            continue;
        }
        let c = c as u8;
        if ch == '"' && !literal {
            quote = !quote;
            out.push(0x22);
        } else if !quote && !literal {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push(if inv { c | 0x80 } else { c });
        }
        i += 1;
    }
    if out.contains(&0x9b) {
        return Err("EOL character ($9B) is not allowed inside a line".into());
    }
    Ok(out)
}

/// The length of a name at the start of `s`: `/^[a-z][a-z0-9.]*[$%!]?/i`.
fn word_len(s: &[char]) -> usize {
    if !s.first().is_some_and(|c| c.is_ascii_alphabetic()) {
        return 0;
    }
    let mut n = 1 + s[1..]
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || **c == '.')
        .count();
    if matches!(s.get(n), Some('$' | '%' | '!')) {
        n += 1;
    }
    n
}

/// CPC: keywords outside strings/REM/'/DATA are uppercased, names keep their case
/// (`upper_all`: them too).
fn cpc_line(body: &[char], words: &HashSet<String>, l: &Listing) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let (mut quote, mut rem, mut data) = (false, false, false);
    let mut i = 0;
    while i < body.len() {
        let ch = body[i];
        if ch == '{' {
            let (e, end) = parse_escape(body, i, false)?;
            if let Escape::Code(c) = e {
                out.push(c);
            }
            i = end;
            continue;
        }
        let c = ch as u32;
        if !(0x20..=0x7e).contains(&c) {
            return Err(format!("unsupported character \"{ch}\" (use {{$xx}})"));
        }
        if !quote && !rem && !data {
            let n = word_len(&body[i..]);
            if n > 0 {
                let word: String = body[i..i + n].iter().collect();
                let lower = word.to_ascii_lowercase();
                let is_kw = words.contains(&lower);
                out.extend(
                    if is_kw || l.upper_all {
                        word.to_ascii_uppercase()
                    } else {
                        word
                    }
                    .bytes(),
                );
                if is_kw && lower == "rem" {
                    rem = true;
                }
                if is_kw && lower == "data" {
                    data = true;
                }
                i += n;
                continue;
            }
            if ch == '\'' && l.rem_quote {
                rem = true;
            }
        }
        if ch == '"' && !rem {
            quote = !quote;
        }
        if ch == ':' && !quote {
            data = false;
        }
        out.push(c as u8);
        i += 1;
    }
    if out.iter().any(|&b| b == 0x0d || b == 0x0a || b == 0x1a) {
        return Err("CR, LF and $1A are not allowed inside a line".into());
    }
    Ok(out)
}

/// The words of the keywords, lowercase (`PRINT#` -> `print`, `ON ERROR GOTO` -> three).
fn keyword_words(def: &Dialect) -> HashSet<String> {
    def.keywords
        .iter()
        .flat_map(|kw| kw.name.split(' '))
        .filter(|part| part.starts_with(|c: char| c.is_ascii_alphabetic()))
        .map(|part| {
            let p = part.to_lowercase();
            p.strip_suffix('#').map_or(p.clone(), str::to_string)
        })
        .collect()
}

/// `tokenize` for the dialects whose output is not a PRG.
pub fn tokenize_listing(text: &str, def: &Dialect) -> Tokenized {
    let atari = def.output == Output::AtasciiLst;
    let words = if atari {
        HashSet::new()
    } else {
        keyword_words(def)
    };
    let ascii = def.listing.unwrap_or(CPC);
    let max_bytes = if atari { 120 } else { ascii.max_bytes };
    let mut errors = Vec::new();
    let mut lines = Vec::new();
    let mut bytes = Vec::new();
    let mut prev: i64 = -1;
    for (idx, raw) in super::split_lines(text).into_iter().enumerate() {
        let src = idx + 1;
        let mut err = |msg: String| errors.push(BasicError { line: src, msg });
        if raw.chars().all(char::is_whitespace) {
            continue;
        }
        let Some((digits, rest)) = super::leading_number(raw) else {
            err("missing line number".into());
            continue;
        };
        let number = super::parse_saturating(digits);
        if digits.len() > 5 || number < def.min_line as u64 || number > def.max_line as u64 {
            err(format!(
                "line number {digits} out of range ({}-{})",
                def.min_line, def.max_line
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
        let body: Vec<char> = rest.trim_start().trim_end().chars().collect();
        if body.is_empty() {
            err(format!("line {number} is empty"));
            continue;
        }
        let r = if atari {
            atari_line(&body)
        } else {
            cpc_line(&body, &words, &ascii)
        };
        let r = match r {
            Ok(r) => r,
            Err(msg) => {
                err(msg);
                continue;
            }
        };
        let mut line = format!("{number} ").into_bytes();
        line.extend(r);
        if line.len() > max_bytes {
            err(format!(
                "line {number} too long ({} chars, max {max_bytes})",
                line.len()
            ));
            continue;
        }
        lines.push(BasicLine {
            src,
            number: number as u32,
            addr: bytes.len(),
        });
        bytes.extend(line);
        bytes.extend_from_slice(if atari { &[0x9b] } else { ascii.eol });
    }
    if !atari {
        bytes.extend_from_slice(ascii.end);
    }
    Tokenized {
        ok: errors.is_empty(),
        errors,
        prg: None,
        listing: Some(bytes),
        format: def.output,
        start: None,
        lines,
    }
}

fn hex(c: u8) -> String {
    format!("{{${c:02x}}}")
}

/// `detokenize` for the listing formats: the bytes back as editable text.
pub fn detokenize_listing(data: &[u8], def: &Dialect) -> String {
    let atari = def.output == Output::AtasciiLst;
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut inv = false;
    let flush = |line: &mut String, inv: &mut bool, out: &mut Vec<String>| {
        if *inv {
            line.push_str("{/inv}");
            *inv = false;
        }
        if !line.is_empty() {
            out.push(std::mem::take(line));
        }
    };
    let printable = |x: u8| (0x20..=0x7a).contains(&x); // '{' is reserved for escapes
    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        i += 1;
        if atari {
            if c == 0x9b {
                flush(&mut line, &mut inv, &mut out);
                continue;
            }
            let base = c & 0x7f;
            if let Some(name) = atascii_by_code(c) {
                if inv {
                    line.push_str("{/inv}");
                    inv = false;
                }
                line.push_str(&format!("{{{name}}}"));
            } else if c >= 0x80 && printable(base) && base != 0x22 {
                if !inv {
                    line.push_str("{inv}");
                    inv = true;
                }
                line.push(base as char);
            } else {
                if inv {
                    line.push_str("{/inv}");
                    inv = false;
                }
                if c < 0x80 && (printable(c) || c == 0x7c) {
                    line.push(c as char);
                } else {
                    line.push_str(&hex(c));
                }
            }
        } else {
            if c == 0x1a {
                break;
            }
            if c == 0x0d || c == 0x0a {
                if c == 0x0d && data.get(i) == Some(&0x0a) {
                    i += 1;
                }
                flush(&mut line, &mut inv, &mut out);
                continue;
            }
            if (0x20..=0x7e).contains(&c) && c != 0x7b {
                line.push(c as char);
            } else {
                line.push_str(&hex(c));
            }
        }
    }
    flush(&mut line, &mut inv, &mut out);
    if out.is_empty() {
        String::new()
    } else {
        out.join("\n") + "\n"
    }
}
