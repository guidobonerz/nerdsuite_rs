//! The names of the PETSCII control codes, as VICE petcat writes them: `{clr}`, `{home}`,
//! `{rvs on}`, `{$xx}`, `{shift-a}` ... The tables mirror petcat.c and are searched in its order,
//! case-insensitively (like `petcat -ic`).

use std::sync::OnceLock;

const E: &str = "";

// 0x00-0x1f (petcat)
#[rustfmt::skip]
const CTRL1: [&str; 32] = [
    E, "ctrl-a", "ctrl-b", "stop", "ctrl-d", "wht", "ctrl-f", "ctrl-g",
    "dish", "ensh", E, "ctrl-k", "ctrl-l", E, "swlc", "ctrl-o",
    "ctrl-p", "down", "rvon", "home", "del", "ctrl-u", "ctrl-v", "ctrl-w",
    "ctrl-x", "ctrl-y", "ctrl-z", "esc", "red", "rght", "grn", "blu",
];
// 0x00-0x1f (MikroBITTI)
#[rustfmt::skip]
const A_CTRL1: [&str; 32] = [
    E, E, E, E, E, "wht", E, E,
    "up/lo lock on", "up/lo lock off", E, E, E, "return", "lower case", E,
    E, "down", "rvs on", "home", "delete", E, E, E,
    E, E, E, "esc", "red", "right", "grn", "blu",
];
// 0x00-0x1f
#[rustfmt::skip]
const B_CTRL1: [&str; 32] = [
    E, E, E, E, E, E, E, E, E, E, E, E, E, E, E, E,
    E, E, "reverse on", E, E, E, E, E, E, E, E, E, E, E, E, E,
];
// 0x00-0x1f (tok64)
#[rustfmt::skip]
const C_CTRL1: [&str; 32] = [
    E, E, E, E, E, "white", E, E, E, E, E, E, E, E, E, E,
    E, "down", "reverse on", "home", E, E, E, E, E, E, E, E, "red", "right", "green", "blue",
];
// 0x00-0x1f (64'er / checksummer v3)
#[rustfmt::skip]
const D_CTRL1: [&str; 32] = [
    E, E, E, E, E, "white", E, E, E, E, E, E, E, "return", E, E,
    E, "down", "rvson", "home", "del", E, E, E, E, E, E, E, "red", "right", "green", "blue",
];
// 0x00-0x1f BASIC 3.5: CTRL1 with dblu
const E_CTRL1: [&str; 32] = {
    let mut t = CTRL1;
    t[0x1f] = "dblu";
    t
};

// 0x20-0x5f (petcat, tok64)
#[rustfmt::skip]
const CBMCHARS: [&str; 64] = [
    "space", E, E, E, E, E, E, E, E, E, "*", "+", E, "-", E, "/",
    E, E, E, E, E, E, E, E, E, E, E, E, "<", "=", ">", E,
    E, "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o",
    "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z", E, E, E, "^", E,
];

// 0x80-0x9f (petcat)
#[rustfmt::skip]
const CTRL2: [&str; 32] = [
    E, "orng", E, E, E, "f1", "f3", "f5", "f7", "f2", "f4", "f6", "f8", "sret", "swuc", E,
    "blk", "up", "rvof", "clr", "inst", "brn", "lred", "gry1", "gry2", "lgrn", "lblu", "gry3",
    "pur", "left", "yel", "cyn",
];
// 0x80-0x9f (MikroBITTI)
#[rustfmt::skip]
const A_CTRL2: [&str; 32] = [
    E, "orange", E, E, E, "f1", "f3", "f5", "f7", "f2", "f4", "f6", "f8", "shift return",
    "upper case", E,
    "blk", "up", "rvs off", "clr", "insert", "brown", "lt.red", "grey1", "grey2", "lt green",
    "lt.blue", "grey3", "pur", "left", "yel", "cyn",
];
// 0x80-0x9f (tok64)
#[rustfmt::skip]
const B_CTRL2: [&str; 32] = [
    E, "orange", E, E, E, "f1", "f3", "r5", "f7", "f2", "f4", "f6", "f8", E, E, E,
    "black", "up", "reverse off", "clear", E, "brown", "pink", "dark gray", "gray", "light green",
    "light blue", "light gray", "purple", "left", "yellow", "cyan",
];
// 0x80-0x9f (64'er / checksummer v3)
#[rustfmt::skip]
const C_CTRL2: [&str; 32] = [
    E, "orange", E, E, E, "f1", "f3", "f5", "f7", "f2", "f4", "f6", "f8", E, E, E,
    "black", "up", "rvsoff", "clr", "inst", "brown", "lig.red", "grey 1", "grey 2", "lig.green",
    "lig.blue", "grey 3", "purple", "left", "yellow", "cyan",
];
// 0x80-0x9f BASIC 3.5
#[rustfmt::skip]
const D_CTRL2: [&str; 32] = [
    E, "orng", "flon", E, "flof", "f1", "f3", "f5", "f7", "f2", "f4", "f6", "help", "sret", "swuc", E,
    "blk", "up", "rvof", "clr", "inst", "brn", "lred", "pink", "blgn", "lblu", "blu", "lgrn",
    "pur", "left", "yel", "cyn",
];

// 0xa0-0xdf keyboard names
#[rustfmt::skip]
const CBMKEYS: [&str; 64] = [
    "shift-space", "cbm-k", "cbm-i", "cbm-t", "cbm-@", "cbm-g", "cbm-+", "cbm-m",
    "cbm-pound", "shift-pound", "cbm-n", "cbm-q", "cbm-d", "cbm-z", "cbm-s", "cbm-p",
    "cbm-a", "cbm-e", "cbm-r", "cbm-w", "cbm-h", "cbm-j", "cbm-l", "cbm-y",
    "cbm-u", "cbm-o", "shift-@", "cbm-f", "cbm-c", "cbm-x", "cbm-v", "cbm-b",
    "shift-*", "shift-a", "shift-b", "shift-c", "shift-d", "shift-e", "shift-f", "shift-g",
    "shift-h", "shift-i", "shift-j", "shift-k", "shift-l", "shift-m", "shift-n", "shift-o",
    "shift-p", "shift-q", "shift-r", "shift-s", "shift-t", "shift-u", "shift-v", "shift-w",
    "shift-x", "shift-y", "shift-z", "shift-+", "cbm--", "shift--", "shift-^", "cbm-*",
];
const A_CBMKEYS: [&str; 64] = {
    let mut t = [E; 64];
    t[0x3e] = "cbm-^";
    t
};

/// The search order of petcat's tokenizer, after the `$xx` codes: (table, base, only for 3.5).
const SEARCH: [(&[&str], u8, bool); 14] = [
    (&E_CTRL1, 0, true),
    (&CTRL1, 0, false),
    (&A_CTRL1, 0, false),
    (&B_CTRL1, 0, false),
    (&C_CTRL1, 0, false),
    (&D_CTRL1, 0, false),
    (&CBMCHARS, 0x20, false),
    (&D_CTRL2, 0x80, true),
    (&CTRL2, 0x80, false),
    (&A_CTRL2, 0x80, false),
    (&B_CTRL2, 0x80, false),
    (&C_CTRL2, 0x80, false),
    (&CBMKEYS, 0xa0, false),
    (&A_CBMKEYS, 0xa0, false),
];

/// The PETSCII byte of a control-code name (without braces, any case): every name petcat
/// accepts, plus `$xx`.
pub fn lookup_control_code(name: &str, dialect: &str) -> Option<u8> {
    let n = name.to_lowercase();
    if n.is_empty() {
        return None;
    }
    if let Some(hex) = n.strip_prefix('$')
        && hex.len() == 2
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return u8::from_str_radix(hex, 16).ok();
    }
    let is35 = dialect == "v35";
    SEARCH
        .iter()
        .filter(|(_, _, only35)| is35 || !only35)
        .find_map(|(table, base, _)| table.iter().position(|t| *t == n).map(|i| base + i as u8))
}

/// The name petcat lists a PETSCII byte with, or None if it is written as `{$xx}`.
pub fn control_code_name(code: u8, dialect: &str) -> Option<&'static str> {
    let v35 = dialect == "v35";
    let name = match code {
        0x00..=0x1f => (if v35 { &E_CTRL1 } else { &CTRL1 })[code as usize],
        0x80..=0x9f => (if v35 { &D_CTRL2 } else { &CTRL2 })[(code & 0x1f) as usize],
        0xa1..=0xdf => CBMKEYS[(code - 0xa0) as usize],
        _ => E,
    };
    // petcat's tables are ambiguous (e.g. {blu} in 3.5 mode): only names that map back
    (!name.is_empty() && lookup_control_code(name, dialect) == Some(code)).then_some(name)
}

/// A control code for completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlCode {
    pub name: &'static str,
    pub code: u8,
    pub desc: String,
    /// `Some("v35")` for names only BASIC 3.5 knows.
    pub dialect: Option<&'static str>,
}

fn describe(name: &str) -> String {
    let d = match name {
        "stop" => "RUN/STOP",
        "wht" => "white",
        "dish" => "disable SHIFT+C= charset switching",
        "ensh" => "enable SHIFT+C= charset switching",
        "down" => "cursor down",
        "rvon" => "reverse on",
        "home" => "cursor home",
        "del" => "delete",
        "esc" => "ESC (C128/Plus4)",
        "red" => "red",
        "rght" => "cursor right",
        "grn" => "green",
        "blu" => "blue",
        "dblu" => "dark blue (BASIC 3.5)",
        "swlc" => "switch to lowercase charset",
        "orng" => "orange",
        "f1" => "F1 key",
        "f2" => "F2 key",
        "f3" => "F3 key",
        "f4" => "F4 key",
        "f5" => "F5 key",
        "f6" => "F6 key",
        "f7" => "F7 key",
        "f8" => "F8 key",
        "help" => "HELP key (BASIC 3.5)",
        "flon" => "flash on (BASIC 3.5)",
        "flof" => "flash off (BASIC 3.5)",
        "sret" => "shifted RETURN",
        "swuc" => "switch to uppercase charset",
        "blk" => "black",
        "up" => "cursor up",
        "rvof" => "reverse off",
        "clr" => "clear screen",
        "inst" => "insert",
        "brn" => "brown",
        "lred" => "light red",
        "gry1" => "dark grey",
        "gry2" => "medium grey",
        "lgrn" => "light green",
        "lblu" => "light blue",
        "gry3" => "light grey",
        "pur" => "purple",
        "left" => "cursor left",
        "yel" => "yellow",
        "cyn" => "cyan",
        "pink" => "pink (BASIC 3.5)",
        "blgn" => "blue-green (BASIC 3.5)",
        "rvs on" => "reverse on",
        "rvs off" => "reverse off",
        "space" => "space",
        "shift-space" => "shifted space",
        _ => "",
    };
    if !d.is_empty() {
        d.to_string()
    } else if let Some(k) = name.strip_prefix("ctrl-") {
        format!("CTRL+{}", k.to_uppercase())
    } else if let Some(k) = name.strip_prefix("cbm-") {
        format!("C= + {} graphic character", k.to_uppercase())
    } else if let Some(k) = name.strip_prefix("shift-") {
        format!("SHIFT + {} graphic character", k.to_uppercase())
    } else {
        String::new()
    }
}

/// The control-code names for completion. Any byte can also be written as `{$xx}` or `{nnn}`,
/// with a repeat count: `{3 down}`, `{down*3}`.
pub fn control_codes() -> &'static [ControlCode] {
    static LIST: OnceLock<Vec<ControlCode>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut out: Vec<ControlCode> = Vec::new();
        let mut add = |name: &'static str, code: u8, dialect: Option<&'static str>| {
            if name.is_empty() || out.iter().any(|c| c.name == name && c.dialect == dialect) {
                return;
            }
            out.push(ControlCode {
                name,
                code,
                desc: describe(name),
                dialect,
            });
        };
        for (i, n) in CTRL1.iter().enumerate() {
            add(n, i as u8, None);
        }
        for (i, n) in CTRL2.iter().enumerate() {
            add(n, 0x80 + i as u8, None);
        }
        add("rvs on", 0x12, None);
        add("rvs off", 0x92, None);
        add("space", 0x20, None);
        for (i, n) in E_CTRL1.iter().enumerate() {
            if *n != CTRL1[i] {
                add(n, i as u8, Some("v35"));
            }
        }
        for (i, n) in D_CTRL2.iter().enumerate() {
            let code = 0x80 + i as u8;
            if *n != CTRL2[i] && control_code_name(code, "v35") == Some(n) {
                add(n, code, Some("v35"));
            }
        }
        for (i, n) in CBMKEYS.iter().enumerate() {
            add(n, 0xa0 + i as u8, None);
        }
        out
    })
}

/// The control codes of a dialect (the names that map to their code in it).
pub fn control_codes_for(dialect: &str) -> Vec<&'static ControlCode> {
    control_codes()
        .iter()
        .filter(|c| c.dialect.is_none_or(|d| d == dialect))
        .filter(|c| lookup_control_code(c.name, dialect) == Some(c.code))
        .collect()
}
