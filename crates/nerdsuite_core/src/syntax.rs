//! The colours of the Code view: the tokens of a line of ACME assembler or of BASIC.
//! Line by line and without state, as nerdsuite2's `lang-acme.js` and `lang-basic.js`.

use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    Comment,
    String,
    Number,
    /// An instruction of the CPU.
    Mnemonic,
    /// A pseudo op of ACME (`!byte`, `*=`) or a statement of BASIC.
    Keyword,
    /// The definition of a label, at the start of a line.
    Label,
    /// A macro call (`+name`) or a `{control code}` in a BASIC string.
    Macro,
    /// The line number of a BASIC line.
    LineNumber,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Acme,
    Basic,
    Plain,
}

impl Language {
    pub fn of(kind: crate::project::FileKind) -> Language {
        match kind {
            crate::project::FileKind::Assembler => Language::Acme,
            crate::project::FileKind::Basic => Language::Basic,
            _ => Language::Plain,
        }
    }
}

pub type Span = (Range<usize>, Token);

pub fn highlight(language: Language, line: &str) -> Vec<Span> {
    match language {
        Language::Acme => acme(line),
        Language::Basic => basic(line),
        Language::Plain => Vec::new(),
    }
}

const MNEMONICS: &[&str] = &[
    // 6502
    "adc", "and", "asl", "bcc", "bcs", "beq", "bit", "bmi", "bne", "bpl", "brk", "bvc", "bvs",
    "clc", "cld", "cli", "clv", "cmp", "cpx", "cpy", "dec", "dex", "dey", "eor", "inc", "inx",
    "iny", "jmp", "jsr", "lda", "ldx", "ldy", "lsr", "nop", "ora", "pha", "php", "pla", "plp",
    "rol", "ror", "rti", "rts", "sbc", "sec", "sed", "sei", "sta", "stx", "sty", "tax", "tay",
    "tsx", "txa", "txs", "tya",
    // the illegal opcodes of the 6510
    "alr", "anc", "ane", "arr", "dcp", "isc", "las", "lax", "rla", "rra", "sax", "sbx", "sha",
    "shs", "shx", "shy", "slo", "sre", "jam",
    // 65C02 and 65816
    "bra", "phx", "phy", "plx", "ply", "stz", "trb", "tsb", "brl", "cop", "jml", "jsl", "mvn",
    "mvp", "pea", "pei", "per", "phb", "phd", "phk", "plb", "pld", "rep", "rtl", "sep", "tcd",
    "tcs", "tdc", "tsc", "txy", "tyx", "wai", "wdm", "xba", "xce", "stp",
    // Z80
    "ld", "push", "pop", "ex", "exx", "ldi", "ldir", "ldd", "lddr", "cpi", "cpir", "cpd",
    "cpdr", "add", "sub", "sbc", "xor", "or", "cp", "daa", "cpl", "neg", "ccf", "scf", "halt",
    "di", "ei", "im", "rlca", "rla", "rrca", "rra", "rlc", "rl", "rrc", "rr", "sla", "sra",
    "srl", "rld", "rrd", "set", "res", "jp", "jr", "djnz", "call", "ret", "reti", "retn", "rst",
    "in", "ini", "inir", "ind", "indr", "out", "outi", "otir", "outd", "otdr",
];

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '@'
}

fn scan_while(line: &str, start: usize, f: impl Fn(char) -> bool) -> usize {
    line[start..]
        .char_indices()
        .find(|&(_, c)| !f(c))
        .map_or(line.len(), |(ix, _)| start + ix)
}

fn string_end(line: &str, start: usize, quote: char) -> usize {
    line[start + 1..]
        .find(quote)
        .map_or(line.len(), |ix| start + 1 + ix + 1)
}

fn acme(line: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut ix = 0;
    // an instruction or `*=` may come: at the start of a statement, after a label or a `:`
    let mut expect_op = true;
    while ix < line.len() {
        let c = line[ix..].chars().next().unwrap();
        let mut end = ix + c.len_utf8();
        let token = match c {
            ';' => {
                spans.push((ix..line.len(), Token::Comment));
                break;
            }
            ':' => {
                expect_op = true;
                ix = end;
                continue;
            }
            c if c.is_whitespace() => {
                ix = end;
                continue;
            }
            '"' | '\'' => {
                end = string_end(line, ix, c);
                Some(Token::String)
            }
            '!' if line[end..].starts_with(|c: char| c.is_ascii_alphabetic()) => {
                end = scan_while(line, end, is_word_char);
                Some(Token::Keyword)
            }
            '*' if expect_op => Some(Token::Keyword),
            '+' if line[end..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') => {
                end = scan_while(line, end, is_word_char);
                Some(Token::Macro)
            }
            '$' | '%' | '0'..='9' => {
                let next = line[end..].chars().next();
                let is_number = match c {
                    '$' => next.is_some_and(|n| n.is_ascii_hexdigit()),
                    '%' => next.is_some_and(|n| matches!(n, '0' | '1' | '.' | '#')),
                    _ => true,
                };
                end = scan_while(line, end, |c| c.is_ascii_alphanumeric() || c == '.' || c == '#');
                is_number.then_some(Token::Number)
            }
            c if c.is_ascii_alphabetic() || c == '_' || c == '.' || c == '@' => {
                end = scan_while(line, ix, is_word_char);
                if ix == 0 {
                    spans.push((ix..end, Token::Label));
                    ix = end;
                    continue;
                }
                let word = line[ix..end].to_ascii_lowercase();
                (expect_op && MNEMONICS.contains(&word.as_str())).then_some(Token::Mnemonic)
            }
            _ => None,
        };
        if let Some(token) = token {
            spans.push((ix..end, token));
        }
        expect_op = false;
        ix = end;
    }
    spans
}

/// The keywords of BASIC V2 and of the later Commodore BASICs, longest first so that the
/// greedy match (as the tokenizer of the machine does it, spaces or not) takes `print#` before `print`.
const BASIC_KEYWORDS: &[&str] = &[
    "right$", "print#", "input#", "verify", "restore", "return", "left$", "gosub", "close", "chr$",
    "str$", "mid$", "print", "input", "spc(", "tab(", "goto", "data", "read", "stop", "wait",
    "load", "save", "poke", "cont", "list", "open", "then", "step", "peek", "next", "else", "do",
    "loop", "while", "until", "exit", "begin", "bend", "graphic", "color", "scnclr", "sound",
    "envelope", "volume", "sprite", "movspr", "fast", "slow", "trap", "resume", "key", "end",
    "for", "dim", "let", "run", "rem", "def", "clr", "cmd", "sys", "get", "new", "not", "and",
    "sgn", "int", "abs", "usr", "fre", "pos", "sqr", "rnd", "log", "exp", "cos", "sin", "tan",
    "atn", "len", "val", "asc", "if", "on", "to", "fn", "or", "go",
];

fn basic(line: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let digits = scan_while(line, 0, |c| c.is_ascii_digit());
    let mut ix = if digits > 0 {
        spans.push((0..digits, Token::LineNumber));
        digits
    } else {
        0
    };
    while ix < line.len() {
        let rest = &line[ix..];
        let c = rest.chars().next().unwrap();
        if c == '"' {
            let end = string_end(line, ix, '"');
            let mut at = ix;
            // the {control codes} in a string as their own colour
            while let Some(open) = line[at..end].find('{') {
                let open = at + open;
                let Some(close) = line[open..end].find('}') else { break };
                let close = open + close + 1;
                spans.push((at..open, Token::String));
                spans.push((open..close, Token::Macro));
                at = close;
            }
            spans.push((at..end, Token::String));
            ix = end;
            continue;
        }
        if let Some(keyword) = BASIC_KEYWORDS.iter().find(|keyword| {
            rest.len() >= keyword.len()
                && rest.is_char_boundary(keyword.len())
                && rest[..keyword.len()].eq_ignore_ascii_case(keyword)
        }) {
            let end = ix + keyword.len();
            if keyword.eq_ignore_ascii_case("rem") {
                spans.push((ix..end, Token::Keyword));
                spans.push((end..line.len(), Token::Comment));
                break;
            }
            spans.push((ix..end, Token::Keyword));
            ix = end;
        } else if c.is_ascii_digit() || (c == '.' && rest[1..].starts_with(|c: char| c.is_ascii_digit())) {
            let end = scan_while(line, ix, |c| c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E');
            spans.push((ix..end, Token::Number));
            ix = end;
        } else if c.is_ascii_alphabetic() {
            // a variable: its letters up to the next keyword are skipped one by one
            ix += 1;
        } else {
            ix += c.len_utf8();
        }
    }
    spans.retain(|(range, _)| !range.is_empty());
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens<'a>(line: &'a str, spans: &[Span]) -> Vec<(&'a str, Token)> {
        spans.iter().map(|(r, t)| (&line[r.clone()], *t)).collect()
    }

    #[test]
    fn acme_lines() {
        let line = ".copy:  lda text,x  ; the text";
        assert_eq!(
            tokens(line, &acme(line)),
            vec![
                (".copy", Token::Label),
                ("lda", Token::Mnemonic),
                ("; the text", Token::Comment)
            ]
        );
        let line = "        !byte $9e, %0101, 10";
        assert_eq!(
            tokens(line, &acme(line)),
            vec![
                ("!byte", Token::Keyword),
                ("$9e", Token::Number),
                ("%0101", Token::Number),
                ("10", Token::Number)
            ]
        );
        let line = "        +clear_screen ' ', WHITE";
        assert_eq!(
            tokens(line, &acme(line)),
            vec![("+clear_screen", Token::Macro), ("' '", Token::String)]
        );
        let line = "*= $0801";
        assert_eq!(
            tokens(line, &acme(line)),
            vec![("*", Token::Keyword), ("$0801", Token::Number)]
        );
        // the operand is not taken for an instruction
        let line = "        lda and";
        assert_eq!(tokens(line, &acme(line)), vec![("lda", Token::Mnemonic)]);
    }

    #[test]
    fn basic_lines() {
        let line = r#"20 print "{clr}hi":goto 30"#;
        assert_eq!(
            tokens(line, &basic(line)),
            vec![
                ("20", Token::LineNumber),
                ("print", Token::Keyword),
                ("\"", Token::String),
                ("{clr}", Token::Macro),
                ("hi\"", Token::String),
                ("goto", Token::Keyword),
                ("30", Token::Number)
            ]
        );
        let line = "10 rem the demo";
        assert_eq!(
            tokens(line, &basic(line)),
            vec![
                ("10", Token::LineNumber),
                ("rem", Token::Keyword),
                (" the demo", Token::Comment)
            ]
        );
        // no spaces, as typed on the machine
        let line = "30 fori=0to15";
        assert_eq!(
            tokens(line, &basic(line)),
            vec![
                ("30", Token::LineNumber),
                ("for", Token::Keyword),
                ("0", Token::Number),
                ("to", Token::Keyword),
                ("15", Token::Number)
            ]
        );
    }
}
