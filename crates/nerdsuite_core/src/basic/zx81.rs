//! The program files of the Sinclair ZX81, as nerdsuite2's `zx81.js`: a `.p` file is the memory
//! from $4009 (VERSN) up to E_LINE as the ROM saves it - the system variables, the BASIC program
//! at $407D, the display file and the variables. The program is stored in the ZX81 character set
//! with its keywords as tokens and every number literal followed by $7E and its value as a 5 byte
//! float.

use super::{BasicError, BasicLine};

/// The first line of the program.
pub const PROGRAM: u16 = 0x407d;
/// The first byte after `1 REM`: where machine code in it starts.
pub const REM_CODE: u16 = 0x4082;

/// The characters $00-$3F: space, the block graphics, `" £ $ : ? ( ) > < = + - * / ; , .`,
/// 0-9 and A-Z.
const CHARS: [char; 64] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▒', '🮎', '🮏', '"', '£', '$', ':', '?', '(', ')', '>',
    '<', '=', '+', '-', '*', '/', ';', ',', '.', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
    'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

/// The character of a code (its low 6 bits).
pub fn char_of(code: u8) -> char {
    CHARS[(code & 0x3f) as usize]
}

fn char_code(c: char) -> Option<u8> {
    CHARS.iter().position(|&x| x == c).map(|i| i as u8)
}

/// The tokens $40-$42 and $C0-$FF as the ROM lists them, without their spaces.
#[rustfmt::skip]
const TOKENS: &[(u8, &str)] = &[
    (0x40, "RND"), (0x41, "INKEY$"), (0x42, "PI"),
    (0xc0, "\"\""), (0xc1, "AT"), (0xc2, "TAB"), (0xc4, "CODE"), (0xc5, "VAL"), (0xc6, "LEN"),
    (0xc7, "SIN"), (0xc8, "COS"), (0xc9, "TAN"), (0xca, "ASN"), (0xcb, "ACS"), (0xcc, "ATN"),
    (0xcd, "LN"), (0xce, "EXP"), (0xcf, "INT"), (0xd0, "SQR"), (0xd1, "SGN"), (0xd2, "ABS"),
    (0xd3, "PEEK"), (0xd4, "USR"), (0xd5, "STR$"), (0xd6, "CHR$"), (0xd7, "NOT"), (0xd8, "**"),
    (0xd9, "OR"), (0xda, "AND"), (0xdb, "<="), (0xdc, ">="), (0xdd, "<>"), (0xde, "THEN"),
    (0xdf, "TO"), (0xe0, "STEP"), (0xe1, "LPRINT"), (0xe2, "LLIST"), (0xe3, "STOP"),
    (0xe4, "SLOW"), (0xe5, "FAST"), (0xe6, "NEW"), (0xe7, "SCROLL"), (0xe8, "CONT"),
    (0xe9, "DIM"), (0xea, "REM"), (0xeb, "FOR"), (0xec, "GOTO"), (0xed, "GOSUB"),
    (0xee, "INPUT"), (0xef, "LOAD"), (0xf0, "LIST"), (0xf1, "LET"), (0xf2, "PAUSE"),
    (0xf3, "NEXT"), (0xf4, "POKE"), (0xf5, "PRINT"), (0xf6, "PLOT"), (0xf7, "RUN"),
    (0xf8, "SAVE"), (0xf9, "RAND"), (0xfa, "IF"), (0xfb, "CLS"), (0xfc, "UNPLOT"),
    (0xfd, "CLEAR"), (0xfe, "RETURN"), (0xff, "COPY"),
];

/// The listing name of a token.
pub fn token_name(code: u8) -> Option<&'static str> {
    TOKENS.iter().find(|t| t.0 == code).map(|t| t.1)
}

const NEWLINE: u8 = 0x76;
const NUMBER: u8 = 0x7e;
const REM: u8 = 0xea;
const QUOTE: u8 = 0x0b;
const OPERATORS: [(&str, u8); 4] = [("**", 0xd8), ("<=", 0xdb), (">=", 0xdc), ("<>", 0xdd)];

/// The words, longest first; `GO TO` and `GO SUB` as the Spectrum writes them are GOTO and GOSUB.
fn words() -> Vec<(&'static str, u8)> {
    let mut w: Vec<_> = TOKENS
        .iter()
        .filter(|t| t.1.starts_with(|c: char| c.is_ascii_uppercase()))
        .map(|&(t, w)| (w, t))
        .chain([("GO TO", 0xec), ("GO SUB", 0xed)])
        .collect();
    w.sort_by_key(|x| std::cmp::Reverse(x.0.len()));
    w
}

/// A number as the 5 bytes of the ZX81: the exponent + $80, then the mantissa (0.5 <= m < 1) in
/// 4 bytes, big endian, its top bit (always 1) replaced by the sign; 0 is 5 zero bytes.
pub fn float5(v: f64) -> Result<[u8; 5], String> {
    if !v.is_finite() {
        return Err(format!("{} is no number of the ZX81", js_number(v)));
    }
    if v == 0.0 {
        return Ok([0; 5]);
    }
    let a = v.abs();
    let mut e = a.log2().floor() as i32 + 1;
    let mut m = a / 2f64.powi(e);
    if m >= 1.0 {
        m /= 2.0;
        e += 1;
    } else if m < 0.5 {
        m *= 2.0;
        e -= 1;
    }
    let mut mant = (m * 2f64.powi(32) + 0.5).floor();
    if mant >= 2f64.powi(32) {
        mant /= 2.0;
        e += 1;
    }
    if !(1..=255).contains(&(e + 128)) {
        return Err(format!("{} is out of the range of the ZX81", js_number(v)));
    }
    let mant = mant as u32;
    let sign = if v < 0.0 { 0x80 } else { 0 };
    Ok([
        (e + 128) as u8,
        (mant >> 24) as u8 & 0x7f | sign,
        (mant >> 16) as u8,
        (mant >> 8) as u8,
        mant as u8,
    ])
}

pub fn from_float5(b: [u8; 5]) -> f64 {
    if b[0] == 0 {
        return 0.0;
    }
    let mant = ((b[1] | 0x80) as f64) * 2f64.powi(24)
        + b[2] as f64 * 2f64.powi(16)
        + b[3] as f64 * 2f64.powi(8)
        + b[4] as f64;
    let sign = if b[1] & 0x80 != 0 { -1.0 } else { 1.0 };
    sign * mant / 2f64.powi(32) * 2f64.powi(b[0] as i32 - 128)
}

fn js_number(v: f64) -> String {
    if v.is_infinite() {
        if v > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
    } else if v.is_nan() {
        "NaN".to_string()
    } else {
        v.to_string()
    }
}

fn word(a: &mut Vec<u8>, v: u32) {
    a.extend([v as u8, (v >> 8) as u8]);
}

/// The `.p` file of tokenized lines (number, bytes without the NEWLINE). It runs the line at
/// `run` (an address; [`PROGRAM`] is the first line), None: no line.
pub fn build_p<'a>(lines: impl IntoIterator<Item = (u32, &'a [u8])>, run: Option<u16>) -> Vec<u8> {
    let mut prog = Vec::new();
    for (number, bytes) in lines {
        prog.extend([(number >> 8) as u8, number as u8]);
        word(&mut prog, bytes.len() as u32 + 1);
        prog.extend_from_slice(bytes);
        prog.push(NEWLINE);
    }
    let dfile = PROGRAM as u32 + prog.len() as u32;
    let display = [NEWLINE; 25]; // a collapsed display: 24 empty lines
    let vars = dfile + display.len() as u32;
    let eline = vars + 1;
    let mut sys = Vec::new();
    sys.push(0); // $4009 VERSN
    word(&mut sys, 0); // $400A E_PPC
    word(&mut sys, dfile); // $400C D_FILE
    word(&mut sys, dfile + 1); // $400E DF_CC
    word(&mut sys, vars); // $4010 VARS
    word(&mut sys, 0); // $4012 DEST
    word(&mut sys, eline); // $4014 E_LINE
    word(&mut sys, eline - 1); // $4016 CH_ADD
    word(&mut sys, 0); // $4018 X_PTR
    word(&mut sys, eline + 5); // $401A STKBOT
    word(&mut sys, eline + 5); // $401C STKEND
    sys.push(0); // $401E BERG
    word(&mut sys, 0x405d); // $401F MEM: MEMBOT
    sys.push(0); // $4021
    sys.push(2); // $4022 DF_SZ
    word(&mut sys, 0); // $4023 S_TOP
    word(&mut sys, 0xffff); // $4025 LAST_K
    sys.extend([0xff, 55]); // $4027 DB_ST, $4028 MARGIN (50 Hz)
    word(&mut sys, run.map_or(dfile, u32::from)); // $4029 NXTLIN: the line LOAD runs (D_FILE: none)
    word(&mut sys, 0); // $402B OLDPPC
    sys.push(0); // $402D FLAGX
    word(&mut sys, 0); // $402E STRLEN
    word(&mut sys, 0x0c8d); // $4030 T_ADDR
    word(&mut sys, 0); // $4032 SEED
    word(&mut sys, 0xffff); // $4034 FRAMES
    word(&mut sys, 0); // $4036 COORDS
    sys.push(0xbc); // $4038 PR_CC
    sys.extend([0x21, 0x18, 0x40]); // $4039 S_POSN (column 33, line 24), $403B CDFLAG: SLOW
    sys.extend([0; 32]); // $403C PRBUFF
    sys.push(NEWLINE);
    sys.extend([0; 32]); // $405D MEMBOT, $407B
    sys.extend(prog);
    sys.extend(display);
    sys.push(0x80);
    sys
}

/// Machine code assembled for `start` ($4082 or higher) in `1 REM <code>`, run by `2 RAND USR exec`.
pub fn code_p(code: &[u8], start: u16, exec: u16) -> Result<Vec<u8>, String> {
    if start < REM_CODE {
        return Err(format!(
            "ZX81 code starts at $4082 (16514) or higher, in the REM line: *= $4082 - this one at ${start:x}"
        ));
    }
    let mut rem = vec![REM];
    rem.resize(1 + (start - REM_CODE) as usize, 0);
    rem.extend_from_slice(code);
    let mut call = vec![0xf9, 0xd4];
    call.extend(exec.to_string().chars().filter_map(char_code));
    call.push(NUMBER);
    call.extend(float5(exec as f64)?);
    let run = PROGRAM as u32 + 4 + rem.len() as u32 + 1;
    Ok(build_p([(1, &rem[..]), (2, &call[..])], Some(run as u16)))
}

// --- the BASIC listing --------------------------------------------------------------------------

fn is_letter(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphabetic())
}

/// The length of a number at the start of `s`: `/^(\d+\.?\d*|\.\d+)(E[+-]?\d+)?/i`.
fn number_len(s: &[char]) -> usize {
    let digits = |from: usize| s[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let mut n = digits(0);
    if n > 0 {
        if s.get(n) == Some(&'.') {
            n += 1;
            n += digits(n);
        }
    } else if s.first() == Some(&'.') && digits(1) > 0 {
        n = 1 + digits(1);
    } else {
        return 0;
    }
    if matches!(s.get(n), Some('E' | 'e')) {
        let sign = usize::from(matches!(s.get(n + 1), Some('+' | '-')));
        let d = digits(n + 1 + sign);
        if d > 0 {
            n += 1 + sign + d;
        }
    }
    n
}

pub(super) fn upper(c: char) -> char {
    let mut u = c.to_uppercase();
    match (u.next(), u.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

/// One line of the listing (after its number) as bytes.
fn line_bytes(body: &[char], words: &[(&str, u8)]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut inv = 0u8;
    let mut rem = false;
    let mut ident = false;
    let char = |out: &mut Vec<u8>, ch: char, inv: u8| match char_code(upper(ch)) {
        Some(c) => {
            out.push(c | inv);
            Ok(())
        }
        None => Err(format!(
            "the ZX81 has no character \"{ch}\" (use {{$xx}} for its codes)"
        )),
    };
    let escape = |out: &mut Vec<u8>, i: &mut usize, inv: &mut u8| -> Result<(), String> {
        let Some(close) = body[*i..].iter().position(|&c| c == '}').map(|p| p + *i) else {
            let s: String = body[*i..body.len().min(*i + 12)].iter().collect();
            return Err(format!("unterminated escape \"{s}\""));
        };
        let name: String = body[*i + 1..close]
            .iter()
            .collect::<String>()
            .to_lowercase();
        *i = close + 1;
        match name.as_str() {
            "inv" => *inv = 0x80,
            "/inv" => *inv = 0,
            n if n.len() == 3
                && n.starts_with('$')
                && n[1..].bytes().all(|b| b.is_ascii_hexdigit()) =>
            {
                out.push(u8::from_str_radix(&n[1..], 16).unwrap())
            }
            n => {
                return Err(format!(
                    "unknown escape {{{n}}} - {{$xx}}, {{inv}} and {{/inv}}"
                ));
            }
        }
        Ok(())
    };
    while i < body.len() {
        let ch = body[i];
        if ch == '{' {
            escape(&mut out, &mut i, &mut inv)?;
            continue;
        }
        if rem {
            char(&mut out, ch, inv)?;
            i += 1;
            continue;
        }
        if ch == '"' {
            // a string: its characters, "" in it is the quote image
            out.push(QUOTE);
            i += 1;
            while i < body.len() {
                if body[i] == '"' && body.get(i + 1) == Some(&'"') {
                    out.push(0xc0);
                    i += 2;
                    continue;
                }
                if body[i] == '"' {
                    break;
                }
                if body[i] == '{' {
                    escape(&mut out, &mut i, &mut inv)?;
                    continue;
                }
                char(&mut out, body[i], inv)?;
                i += 1;
            }
            if i >= body.len() {
                return Err("the string has no closing \"".into());
            }
            out.push(QUOTE);
            i += 1;
            continue;
        }
        if ch == ' ' {
            ident = false;
            i += 1;
            continue;
        }
        let rest: Vec<char> = body[i..].iter().map(|&c| upper(c)).collect();
        let starts = |w: &str| rest.len() >= w.len() && w.chars().zip(&rest).all(|(a, &b)| a == b);
        if let Some(&(op, t)) = OPERATORS.iter().find(|(o, _)| starts(o)) {
            out.push(t);
            i += op.len();
            ident = false;
            continue;
        }
        // a keyword or a number where no name goes on (A1, TOTAL are names)
        if !ident {
            let kw = words.iter().find(|(w, _)| {
                starts(w) && (w.ends_with('$') || !is_letter(rest.get(w.len()).copied()))
            });
            if let Some(&(w, t)) = kw {
                out.push(t);
                i += w.len();
                if t == REM {
                    rem = true;
                    if body.get(i) == Some(&' ') {
                        i += 1;
                    }
                }
                continue;
            }
            let n = number_len(&rest);
            if n > 0 {
                for &c in &rest[..n] {
                    char(&mut out, c, inv)?;
                }
                let text: String = rest[..n].iter().collect();
                out.push(NUMBER);
                out.extend(float5(text.parse::<f64>().unwrap_or(f64::NAN))?);
                i += n;
                continue;
            }
        }
        char(&mut out, ch, inv)?;
        ident = is_letter(Some(ch)) || (ident && ch.is_ascii_digit());
        i += 1;
    }
    Ok(out)
}

/// A listing as a `.p` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Zx81Program {
    pub ok: bool,
    pub errors: Vec<BasicError>,
    /// The `.p` file, None when there are errors.
    pub bytes: Option<Vec<u8>>,
    pub lines: Vec<BasicLine>,
}

/// A listing -> the `.p` file.
pub fn tokenize_zx81(text: &str) -> Zx81Program {
    let words = words();
    let mut errors = Vec::new();
    let mut lines: Vec<(BasicLine, Vec<u8>)> = Vec::new();
    let mut prev: i64 = -1;
    let mut addr = PROGRAM as usize;
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
        if !(1..=9999).contains(&number) {
            err(format!("line number {digits} out of range (1-9999)"));
            continue;
        }
        if number as i64 <= prev {
            err(format!(
                "line number {number} is not greater than the line before ({prev})"
            ));
            continue;
        }
        prev = number as i64;
        let body: Vec<char> = rest.trim_start().trim_end().chars().collect();
        if body.is_empty() {
            err(format!("line {number} is empty"));
            continue;
        }
        match line_bytes(&body, &words) {
            Ok(bytes) => {
                let n = bytes.len();
                lines.push((
                    BasicLine {
                        src,
                        number: number as u32,
                        addr,
                    },
                    bytes,
                ));
                addr += n + 5;
            }
            Err(msg) => err(msg),
        }
    }
    let bytes = errors
        .is_empty()
        .then(|| build_p(lines.iter().map(|(l, b)| (l.number, &b[..])), Some(PROGRAM)));
    Zx81Program {
        ok: errors.is_empty(),
        errors,
        bytes,
        lines: lines.into_iter().map(|l| l.0).collect(),
    }
}

/// A `.p` file -> its listing (the program from $407D to D_FILE).
pub fn detokenize_zx81(p: &[u8]) -> String {
    let get = |i: i64| -> usize {
        usize::try_from(i)
            .ok()
            .and_then(|i| p.get(i))
            .map_or(0, |&b| b as usize)
    };
    let at = |a: i64| a - 0x4009;
    let dfile = (get(at(0x400c)) | get(at(0x400c) + 1) << 8) as i64;
    let mut out = Vec::new();
    let mut k = at(PROGRAM as i64);
    while k + 4 <= at(dfile) && (k as usize) < p.len() {
        let number = get(k) << 8 | get(k + 1);
        let len = get(k + 2) | get(k + 3) << 8;
        let from = (k as usize + 4).min(p.len());
        let to = (k as usize + 4 + len)
            .saturating_sub(1)
            .clamp(from, p.len());
        let body = &p[from..to];
        let mut s = String::new();
        let (mut quote, mut rem, mut inv) = (false, false, false);
        let mut j = 0;
        while j < body.len() {
            let c = body[j];
            j += 1;
            if c == NUMBER && !rem && !quote {
                j += 5;
                continue;
            }
            let token = token_name(c);
            if let Some(w) = token
                && !rem
                && !quote
                && c >= 0xc0
            {
                if w.starts_with(|c: char| c.is_ascii_uppercase()) {
                    if !s.is_empty() && !s.ends_with(' ') {
                        s.push(' ');
                    }
                    s.push_str(w);
                    s.push(' ');
                } else {
                    s.push_str(w);
                }
                if c == REM {
                    rem = true;
                }
                continue;
            }
            if let Some(w) = token {
                s.push_str(w);
                continue;
            }
            let on = c & 0x80 != 0;
            if on != inv {
                s.push_str(if on { "{inv}" } else { "{/inv}" });
                inv = on;
            }
            if c & 0x7f >= 0x40 {
                s.push_str(&format!("{{${c:02x}}}"));
                continue;
            }
            if c == QUOTE && !rem {
                quote = !quote;
            }
            s.push(char_of(c));
        }
        if inv {
            s.push_str("{/inv}");
        }
        out.push(format!("{number} {}", s.trim_end()));
        k += 4 + len as i64;
    }
    out.join("\n") + "\n"
}
