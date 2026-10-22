//! The assembler: passes, statements, expressions, symbols, macros and the pseudo opcodes of
//! ACME 0.97, as nerdsuite2's `acme.js`. What throws there returns a [`Flow`] here.

use std::{cell::RefCell, collections::HashMap, collections::HashSet, rc::Rc};

use indexmap::IndexMap;

use super::{
    Define, Diagnostic, Options,
    tables::{self, ALL_MNEMONICS, CPUS, IMM_A, IMM_X, Mode, Modes},
    z80,
};

pub(super) const MAX_PASSES: u32 = 24;
const MAX_DEPTH: usize = 64;
const MAX_ERRORS: usize = 100;
const LOOP_LIMIT: usize = 100_000;
pub(super) const ILLEGAL_MODE: &str = "Illegal combination of command and addressing mode.";
pub(super) const OUTPUT_FORMATS: &[&str] = &["cbm", "plain", "apple", "xex", "amsdos", "zx81", "trs80", "adf"];
/// What the reader gives at the end of its text.
pub(super) const EOT: char = '\0';

// --------------------------------------------------------------------------------- values

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct Num {
    pub v: f64,
    /// a float
    pub f: bool,
    /// undefined in this pass
    pub u: bool,
    /// was undefined in a pass: not taken for zero page
    pub e: bool,
    /// an address (1), for the label lists
    pub a: i32,
    /// the size its digits ask for: 2 ($xxxx), 3 ($xxxxxx)
    pub force: u8,
}

impl Num {
    pub fn new(v: f64) -> Num {
        Num { v, ..Default::default() }
    }

    fn float(v: f64) -> Num {
        Num { v, f: true, ..Default::default() }
    }

    fn undef() -> Num {
        Num { u: true, e: true, ..Default::default() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Val {
    N(Num),
    S(String),
    L(Vec<Val>),
}

impl Val {
    fn bool(b: bool) -> Val {
        Val::N(Num::new(if b { 1. } else { 0. }))
    }

    /// The number fields of a value, all unset for a string or a list (as JS reads `v.u` of them).
    pub fn num(&self) -> Num {
        match self {
            Val::N(n) => *n,
            _ => Num::default(),
        }
    }

    pub fn is_undef(&self) -> bool {
        self.num().u
    }
}

fn val_equal(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::N(x), Val::N(y)) => x.v == y.v && x.f == y.f && x.u == y.u,
        (Val::S(x), Val::S(y)) => x == y,
        (Val::L(x), Val::L(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| val_equal(x, y)),
        _ => false,
    }
}

/// A number as JavaScript's `String(n)` writes it.
pub(super) fn js_number(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.is_infinite() {
        if v > 0. { "Infinity" } else { "-Infinity" }.into()
    } else if v.fract() == 0. && v.abs() < 1e21 {
        format!("{}", v as i128)
    } else {
        format!("{v}")
    }
}

/// JavaScript's ToInt32: what `&`, `|`, `^` and `<<` make of a number.
pub(super) fn int32(v: f64) -> i64 {
    if !v.is_finite() {
        return 0;
    }
    let x = v.trunc().rem_euclid(4294967296.) as i64;
    if x >= 0x8000_0000 { x - 0x1_0000_0000 } else { x }
}

fn bitop(a: f64, b: f64, op: char) -> f64 {
    let (a, b) = (a.trunc() as i128, b.trunc() as i128);
    (match op {
        '&' => a & b,
        '|' => a | b,
        _ => a ^ b,
    }) as f64
}

pub fn conv_pet(c: u32) -> u32 {
    match c {
        65..=90 => c | 0x80,
        97..=122 => c - 32,
        _ => c,
    }
}

pub fn conv_scr(c: u32) -> u32 {
    match c {
        97..=122 => c - 0x60,
        91..=95 => c - 0x40,
        96 => 64,
        64 => 0,
        _ => c,
    }
}

pub(super) fn is_id_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || (c as u32) >= 128
}

pub(super) fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || (c as u32) >= 128
}

fn norm_path(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in p.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    out.join("/")
}

fn dir_of(p: &str) -> &str {
    p.rfind('/').map_or("", |ix| &p[..ix])
}

fn bytes_to_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

// --------------------------------------------------------------------------------- flow

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Loc {
    pub file: String,
    pub line: usize,
    pub col: usize,
}

/// What breaks off a statement, a file or the pass.
#[derive(Debug)]
pub(super) enum Flow {
    /// An error of the source (`AsmError`): the statement ends, the error is recorded.
    Error(String, Loc),
    /// Too many errors, or `!serious`: the pass ends.
    Stop,
    /// `!eof`: the file ends.
    Eof,
}

pub(super) type R<T> = Result<T, Flow>;

// --------------------------------------------------------------------------------- reader

pub(super) struct Src {
    pub file: String,
    pub text: Rc<Vec<char>>,
}

#[derive(Clone)]
pub(super) struct Block {
    src: Rc<Src>,
    start: usize,
    end: usize,
    line: usize,
    line_start: usize,
}

pub(super) struct Reader {
    pub src: Rc<Src>,
    pub pos: usize,
    pub end: usize,
    pub line: usize,
    pub line_start: usize,
}

impl Reader {
    fn of(block: &Block) -> Reader {
        Reader {
            src: block.src.clone(),
            pos: block.start,
            end: block.end,
            line: block.line,
            line_start: block.line_start,
        }
    }

    /// The char at an index of the text, whatever the end of the reader.
    pub fn t(&self, p: usize) -> char {
        self.src.text.get(p).copied().unwrap_or(EOT)
    }

    pub fn peek(&self, o: usize) -> char {
        let p = self.pos + o;
        if p < self.end { self.src.text[p] } else { EOT }
    }

    fn adv(&mut self) -> char {
        let c = self.t(self.pos);
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.line_start = self.pos;
        }
        c
    }

    fn col(&self) -> usize {
        self.pos - self.line_start + 1
    }

    pub fn ws(&mut self) {
        while self.pos < self.end && matches!(self.src.text[self.pos], ' ' | '\t' | '\r' | '\x0c' | '\x0b' | '\u{a0}') {
            self.pos += 1;
        }
    }

    pub fn eos(&mut self) -> bool {
        self.ws();
        matches!(self.peek(0), EOT | '\n' | ';' | ':')
    }

    fn to_eol(&mut self) {
        while self.pos < self.end && self.src.text[self.pos] != '\n' {
            self.pos += 1;
        }
    }

    fn save(&self) -> (usize, usize, usize) {
        (self.pos, self.line, self.line_start)
    }

    fn restore(&mut self, (pos, line, line_start): (usize, usize, usize)) {
        self.pos = pos;
        self.line = line;
        self.line_start = line_start;
    }

    fn keyword(&self, w: &str) -> bool {
        let n = w.chars().count();
        self.pos + n <= self.end
            && (0..n).all(|i| self.t(self.pos + i).to_ascii_lowercase() == w.chars().nth(i).unwrap())
            && !is_id_char(self.peek(n))
    }
}

// --------------------------------------------------------------------------------- symbols

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Kind {
    Equ,
    Label,
    Set,
}

#[derive(Debug)]
pub(super) struct Sym {
    pub name: String,
    pub value: Option<Val>,
    pub def_pass: u32,
    pub kind: Option<Kind>,
    pub file: Option<String>,
    pub line: usize,
    pub zone: String,
    pub used: bool,
    pub ever_undef: bool,
    pub is_set: bool,
    pub in_macro: bool,
    pub anon: bool,
}

pub(super) type SymRef = Rc<RefCell<Sym>>;

#[derive(Clone)]
struct Zone {
    id: u32,
    title: String,
}

struct Param {
    by_ref: bool,
    name: String,
}

struct Macro {
    params: Vec<Param>,
    block: Block,
    pass: u32,
}

#[derive(Clone)]
enum Convtab {
    Raw,
    Pet,
    Scr,
    Table(Rc<Vec<u8>>),
}

pub(super) struct LineRec {
    pub file: String,
    pub line: usize,
    pub addr: i64,
    pub real: i64,
    pub size: usize,
    pub in_macro: bool,
}

struct Cond {
    is_while: bool,
    block: Block,
}

enum MacroArg {
    Value(Val),
    Ref(String, String),
}

// --------------------------------------------------------------------------------- the assembler

pub(super) struct Asm {
    files: IndexMap<String, Vec<u8>>,
    lower_index: HashMap<String, String>,
    opts: Options,
    pub symbols: IndexMap<String, SymRef>,
    macros: HashMap<String, Macro>,
    pub texts: HashMap<String, Rc<Vec<char>>>,
    alias_keys: HashSet<String>,
    readers: Vec<Reader>,

    pub main: String,
    pub pass: u32,
    pub fin: bool,
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
    diag_seen: HashSet<String>,
    changed: bool,
    undef_count: usize,
    dry: u32,
    /// the highest address: 64K, 16 MB for the 68000
    pub top: i64,
    pub mem: Vec<u8>,
    pub written: Vec<bool>,
    pub min_addr: i64,
    pub max_addr: i64,
    pub realpc: Option<i64>,
    pc_offset: i64,
    seg_start: i64,
    segments: Vec<(i64, i64)>,
    convtab: Convtab,
    xor_val: i64,
    al: bool,
    rl: bool,
    pub initmem: u8,
    zone_counter: u32,
    cheap_counter: u32,
    zone: Zone,
    cheap: Zone,
    anon: HashMap<String, i64>,
    depth: usize,
    macro_depth: usize,
    addr_mode: u32,
    pub out_name: Option<String>,
    pub out_format: Option<String>,
    pub run_addr: Option<i64>,
    pub init_addr: Option<i64>,
    pub sl_name: Option<String>,
    pub svl_name: Option<String>,
    pub line_recs: Vec<LineRec>,
    emit_count: usize,
    pub source_order: Vec<String>,
    stale_uses: Vec<(SymRef, String, Loc)>,
    pub cpu_name: String,
    pub cpu_key: &'static str,
}

impl Asm {
    pub fn new(files: Vec<(String, Vec<u8>)>, opts: Options) -> Asm {
        let files: IndexMap<String, Vec<u8>> =
            files.into_iter().map(|(path, data)| (norm_path(&path), data)).collect();
        let mut lower_index = HashMap::new();
        for path in files.keys() {
            lower_index.entry(path.to_lowercase()).or_insert_with(|| path.clone());
        }
        Asm {
            files,
            lower_index,
            opts,
            symbols: IndexMap::new(),
            macros: HashMap::new(),
            texts: HashMap::new(),
            alias_keys: HashSet::new(),
            readers: Vec::new(),
            main: String::new(),
            pass: 0,
            fin: false,
            errors: Vec::new(),
            warnings: Vec::new(),
            diag_seen: HashSet::new(),
            changed: false,
            undef_count: 0,
            dry: 0,
            top: 0xffff,
            mem: Vec::new(),
            written: Vec::new(),
            min_addr: i64::MAX,
            max_addr: -1,
            realpc: None,
            pc_offset: 0,
            seg_start: 0,
            segments: Vec::new(),
            convtab: Convtab::Raw,
            xor_val: 0,
            al: false,
            rl: false,
            initmem: 0,
            zone_counter: 0,
            cheap_counter: 0,
            zone: Zone { id: 0, title: String::new() },
            cheap: Zone { id: 0, title: String::new() },
            anon: HashMap::new(),
            depth: 0,
            macro_depth: 0,
            addr_mode: 0,
            out_name: None,
            out_format: None,
            run_addr: None,
            init_addr: None,
            sl_name: None,
            svl_name: None,
            line_recs: Vec::new(),
            emit_count: 0,
            source_order: Vec::new(),
            stale_uses: Vec::new(),
            cpu_name: "6502".into(),
            cpu_key: "6502",
        }
    }

    pub fn opts(&self) -> &Options {
        &self.opts
    }

    // ------------------------------------------------------------------ the reader in front

    pub fn r(&self) -> &Reader {
        self.readers.last().expect("a reader")
    }

    pub fn rm(&mut self) -> &mut Reader {
        self.readers.last_mut().expect("a reader")
    }

    pub fn peek(&self, o: usize) -> char {
        self.r().peek(o)
    }

    pub fn bump(&mut self, n: usize) {
        self.rm().pos += n;
    }

    pub fn ws(&mut self) {
        self.rm().ws();
    }

    pub fn eos(&mut self) -> bool {
        self.rm().eos()
    }

    // ------------------------------------------------------------------ files

    fn find_file(&self, p: &str) -> Option<String> {
        let p = norm_path(p);
        if self.files.contains_key(&p) {
            return Some(p);
        }
        self.lower_index.get(&p.to_lowercase()).cloned()
    }

    fn resolve(&self, from_file: &str, name: &str, lib: bool) -> Option<String> {
        if lib {
            return self.find_file(&format!("lib/{name}"));
        }
        if name.starts_with('/') {
            return self.find_file(name);
        }
        let dir = dir_of(from_file);
        let rel = if dir.is_empty() { name.to_owned() } else { format!("{dir}/{name}") };
        self.find_file(&rel).or_else(|| self.find_file(name))
    }

    fn source_text(&mut self, path: &str) -> Rc<Vec<char>> {
        if let Some(text) = self.texts.get(path) {
            return text.clone();
        }
        let text = bytes_to_text(&self.files[path]).replace("\r\n", "\n").replace('\r', "\n");
        let text = Rc::new(text.chars().collect::<Vec<_>>());
        self.texts.insert(path.to_owned(), text.clone());
        text
    }

    // ------------------------------------------------------------------ diagnostics

    pub fn loc(&self) -> Loc {
        match self.readers.last() {
            Some(r) => Loc { file: r.src.file.clone(), line: r.line, col: r.col() },
            None => Loc { file: self.main.clone(), line: 1, col: 1 },
        }
    }

    pub fn err<T>(&self, msg: impl Into<String>) -> R<T> {
        Err(Flow::Error(msg.into(), self.loc()))
    }

    pub fn error(&mut self, msg: impl Into<String>) -> R<()> {
        let loc = self.loc();
        self.error_at(msg.into(), loc)
    }

    fn error_at(&mut self, msg: String, loc: Loc) -> R<()> {
        if !self.fin || self.dry > 0 {
            return Ok(());
        }
        let key = format!("{}|{}|{}|{}", loc.file, loc.line, loc.col, msg);
        if !self.diag_seen.insert(key) {
            return Ok(());
        }
        self.errors.push(Diagnostic::at(&loc, msg));
        if self.errors.len() >= MAX_ERRORS {
            self.errors.push(Diagnostic::at(&loc, "Too many errors, stopping.".into()));
            return Err(Flow::Stop);
        }
        Ok(())
    }

    pub fn warn(&mut self, msg: impl Into<String>) {
        let loc = self.loc();
        let msg = msg.into();
        if !self.fin || self.dry > 0 {
            return;
        }
        let key = format!("W{}|{}|{}|{}", loc.file, loc.line, loc.col, msg);
        if self.diag_seen.insert(key) {
            self.warnings.push(Diagnostic::at(&loc, msg));
        }
    }

    // ------------------------------------------------------------------ passes

    pub fn run(&mut self, main: &str) {
        self.main = norm_path(main);
        if let Some(found) = self.find_file(&self.main) {
            self.main = found;
        }
        let mut prev_undef = usize::MAX;
        let mut pass = 0;
        loop {
            pass += 1;
            self.run_pass(pass, false);
            if !self.changed && self.undef_count == 0 {
                break;
            }
            if pass >= MAX_PASSES {
                break;
            }
            if !self.changed && self.undef_count >= prev_undef {
                break;
            }
            prev_undef = self.undef_count;
        }
        self.run_pass(pass + 1, true);
    }

    fn run_pass(&mut self, pass: u32, fin: bool) {
        self.pass = pass;
        self.fin = fin;
        self.errors.clear();
        self.warnings.clear();
        self.diag_seen.clear();
        self.changed = false;
        self.undef_count = 0;
        self.dry = 0;
        self.top = 0xffff;
        self.mem = vec![0; 0x10000];
        self.written = vec![false; 0x10000];
        self.min_addr = i64::MAX;
        self.max_addr = -1;
        self.realpc = None;
        self.pc_offset = 0;
        self.seg_start = 0;
        self.segments.clear();
        self.convtab = Convtab::Raw;
        self.xor_val = 0;
        self.al = false;
        self.rl = false;
        self.initmem = 0;
        self.zone_counter = 0;
        self.cheap_counter = 0;
        self.zone = Zone { id: 0, title: String::new() };
        self.cheap = Zone { id: 0, title: String::new() };
        self.anon.clear();
        self.depth = 0;
        self.macro_depth = 0;
        self.addr_mode = 0;
        self.out_name = None;
        self.out_format = None;
        self.run_addr = self.opts.run;
        self.init_addr = self.opts.init;
        self.sl_name = None;
        self.svl_name = None;
        self.line_recs.clear();
        self.emit_count = 0;
        self.source_order.clear();
        self.stale_uses.clear();
        self.readers.clear();
        for key in self.alias_keys.drain() {
            self.symbols.shift_remove(&key);
        }

        let result = (|| -> R<()> {
            let cpu = self.opts.cpu.clone().unwrap_or_else(|| "6502".into()).to_lowercase();
            self.set_cpu(&cpu)?;
            for (name, value) in self.opts.defines.clone() {
                let value = match value {
                    Define::Str(s) => Val::S(s),
                    Define::Num(n) => Val::N(Num { v: n, f: n.fract() != 0., ..Default::default() }),
                };
                let key = self.sym_key(&name);
                self.define_key(key, &name, value, Kind::Set)?;
            }
            if !self.files.contains_key(&self.main) {
                return self.err(format!("Cannot open input file \"{}\".", self.main));
            }
            let main = self.main.clone();
            self.include_file(&main)
        })();
        if let Err(Flow::Error(msg, loc)) = result {
            self.error_at(msg, loc).ok();
        }
        self.close_segment();
        if fin {
            for (sym, name, loc) in std::mem::take(&mut self.stale_uses) {
                if sym.borrow().def_pass != self.pass && self.errors.len() < MAX_ERRORS {
                    self.error_at(format!("Symbol not defined ({name})."), loc).ok();
                }
            }
        }
    }

    fn include_file(&mut self, path: &str) -> R<()> {
        if self.depth >= MAX_DEPTH {
            return self.err("Too deeply nested. Recursive \"!source\"?");
        }
        let text = self.source_text(path);
        if self.fin && !self.source_order.iter().any(|p| p == path) {
            self.source_order.push(path.to_owned());
        }
        let len = text.len();
        let src = Rc::new(Src { file: path.to_owned(), text });
        self.depth += 1;
        let result = self.parse_statements(Reader { src, pos: 0, end: len, line: 1, line_start: 0 });
        self.depth -= 1;
        match result {
            Err(Flow::Eof) => Ok(()),
            other => other,
        }
    }

    fn run_block(&mut self, block: &Block) -> R<()> {
        self.parse_statements(Reader::of(block))
    }

    // ------------------------------------------------------------------ statements

    fn parse_statements(&mut self, reader: Reader) -> R<()> {
        self.readers.push(reader);
        let result = self.statements();
        self.readers.pop();
        result
    }

    fn statements(&mut self) -> R<()> {
        loop {
            self.ws();
            let r = self.r();
            if r.pos >= r.end {
                return Ok(());
            }
            match r.t(r.pos) {
                '\n' => {
                    self.rm().adv();
                    continue;
                }
                ';' => {
                    self.rm().to_eol();
                    continue;
                }
                ':' => {
                    self.bump(1);
                    continue;
                }
                _ => {}
            }
            let r = self.r();
            let line = r.line;
            let col = r.col();
            let first_on_line = (r.line_start..r.pos).all(|p| r.t(p).is_whitespace());
            let file = r.src.file.clone();
            let count0 = self.emit_count;
            let real0 = self.realpc;
            let pc0 = real0.map(|real| real + self.pc_offset);
            let result = (|| -> R<bool> {
                let is_block = self.statement(first_on_line && col > 1, false)?;
                self.ws();
                match self.peek(0) {
                    ';' => self.rm().to_eol(),
                    EOT | '\n' | ':' => {}
                    _ => return self.err("Garbage data at end of statement."),
                }
                Ok(is_block)
            })();
            let is_block = match result {
                Ok(is_block) => is_block,
                Err(Flow::Error(msg, loc)) => {
                    self.error_at(msg, loc)?;
                    self.skip_statement();
                    false
                }
                Err(other) => return Err(other),
            };
            if self.fin && self.emit_count > count0 && !is_block
                && let (Some(pc0), Some(real0)) = (pc0, real0)
            {
                self.record_line(file, line, pc0, real0, self.emit_count - count0);
            }
        }
    }

    fn record_line(&mut self, file: String, line: usize, addr: i64, real: i64, size: usize) {
        let in_macro = self.macro_depth > 0;
        if let Some(last) = self.line_recs.last_mut()
            && last.file == file
            && last.line == line
            && last.in_macro == in_macro
            && last.real + last.size as i64 == real
        {
            last.size += size;
            return;
        }
        self.line_recs.push(LineRec { file, line, addr, real, size, in_macro });
    }

    /// Past a string from its opening quote, as far as the line goes.
    fn skip_string(r: &mut Reader, quote: char) {
        r.pos += 1;
        while r.pos < r.end && r.t(r.pos) != quote && r.t(r.pos) != '\n' {
            if r.t(r.pos) == '\\' && r.t(r.pos + 1) != '\n' {
                r.pos += 1;
            }
            r.pos += 1;
        }
        if r.t(r.pos) == quote {
            r.pos += 1;
        }
    }

    fn skip_statement(&mut self) {
        loop {
            let r = self.rm();
            if r.pos >= r.end {
                return;
            }
            match r.t(r.pos) {
                '\n' => return,
                ';' => {
                    r.to_eol();
                    return;
                }
                c @ ('"' | '\'') => Self::skip_string(r, c),
                '{' => {
                    if self.scan_block().is_err() {
                        let r = self.rm();
                        r.pos = r.end;
                    }
                }
                _ => r.pos += 1,
            }
        }
    }

    fn scan_block(&mut self) -> R<Block> {
        self.ws();
        if self.peek(0) != '{' {
            return self.err("Missing '{'.");
        }
        let open = self.loc();
        let r = self.rm();
        r.pos += 1;
        let (start, line, line_start) = (r.pos, r.line, r.line_start);
        let mut depth = 1;
        while r.pos < r.end {
            match r.t(r.pos) {
                '\n' => {
                    r.adv();
                    continue;
                }
                c @ ('"' | '\'') => {
                    Self::skip_string(r, c);
                    continue;
                }
                ';' => {
                    r.to_eol();
                    continue;
                }
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = r.pos;
                        r.pos += 1;
                        return Ok(Block { src: r.src.clone(), start, end, line, line_start });
                    }
                }
                _ => {}
            }
            r.pos += 1;
        }
        Err(Flow::Error("Found end-of-file instead of '}'.".into(), open))
    }

    pub fn read_word(&mut self) -> String {
        let r = self.rm();
        let start = r.pos;
        while r.pos < r.end && is_id_char(r.t(r.pos)) {
            r.pos += 1;
        }
        r.src.text[start..r.pos].iter().collect()
    }

    pub fn read_sym_name(&mut self) -> R<String> {
        self.ws();
        let start = self.r().pos;
        let c = self.peek(0);
        if c == '.' || c == '@' {
            self.bump(1);
        } else if !is_id_start(c) {
            return self.err("Missing symbol name.");
        }
        let r = self.rm();
        while r.pos < r.end && is_id_char(r.t(r.pos)) {
            r.pos += 1;
        }
        if r.pos - start == 1 && (c == '.' || c == '@') {
            return self.err("Missing symbol name.");
        }
        let r = self.r();
        Ok(r.src.text[start..r.pos].iter().collect())
    }

    /// `"name"` (from the folder of the source), or `<name>` (from `lib/`).
    fn read_file_name(&mut self) -> R<(String, bool)> {
        self.ws();
        match self.peek(0) {
            '"' => {
                self.bump(1);
                Ok((self.read_quoted('"')?, false))
            }
            '<' => {
                self.bump(1);
                let r = self.rm();
                let start = r.pos;
                while r.pos < r.end && r.t(r.pos) != '>' && r.t(r.pos) != '\n' {
                    r.pos += 1;
                }
                if r.peek(0) != '>' {
                    return self.err("Quotes still open at end of line.");
                }
                let name = r.src.text[start..r.pos].iter().collect();
                r.pos += 1;
                Ok((name, true))
            }
            _ => self.err("File name quotes not found (\"\" or <>)."),
        }
    }

    fn read_quoted(&mut self, quote: char) -> R<String> {
        let mut s = String::new();
        loop {
            let r = self.r();
            if r.pos >= r.end || r.t(r.pos) == '\n' {
                return self.err("Quotes still open at end of line.");
            }
            let c = r.t(r.pos);
            self.bump(1);
            if c == quote {
                return Ok(s);
            }
            if c == '\\' {
                let r = self.r();
                let e = if r.pos < r.end { r.t(r.pos) } else { EOT };
                if r.pos < r.end {
                    self.bump(1);
                }
                s.push(match e {
                    '0' => '\0',
                    't' => '\t',
                    'n' => '\n',
                    'r' => '\r',
                    '\\' => '\\',
                    '"' => '"',
                    '\'' => '\'',
                    _ => return self.err("Unsupported backslash sequence."),
                });
            } else {
                s.push(c);
            }
        }
    }

    fn cpu_has(&self, mnemonic: &str) -> bool {
        match self.cpu_key {
            "z80" => z80::MNEMONICS.contains(&mnemonic),
            "m68k" => false,
            key => CPUS[key].table.contains_key(mnemonic),
        }
    }

    fn statement(&mut self, indented: bool, after_label: bool) -> R<bool> {
        let c = self.peek(0);
        if c == '!' {
            self.bump(1);
            let name = self.read_word().to_lowercase();
            if name.is_empty() {
                return self.err("Missing pseudo opcode.");
            }
            return self.pseudo(&name);
        }
        if c == '*' {
            self.bump(1);
            self.ws();
            if self.peek(0) != '=' {
                return self.err("Expected '=' after '*'.");
            }
            self.bump(1);
            self.set_pc()?;
            return Ok(false);
        }
        if c == '+' && (is_id_start(self.peek(1)) || self.peek(1) == '.') {
            return self.macro_call();
        }
        if c == '+' || c == '-' {
            let mut n = 0;
            while self.peek(0) == c {
                self.bump(1);
                n += 1;
            }
            if !matches!(self.peek(0), EOT | ' ' | '\t' | '\n' | ';' | ':' | '\r') {
                return self.err("Garbage data at end of statement.");
            }
            self.define_anon(&c.to_string().repeat(n))?;
            if self.eos() {
                return Ok(false);
            }
            return self.statement(false, true);
        }
        if c == '.' || c == '@' || is_id_start(c) {
            let name = self.read_sym_name()?;
            let lname = name.to_lowercase();
            let is_mnemonic = c != '.' && c != '@' && self.cpu_has(&lname);
            let mut force = 0;
            if is_mnemonic
                && self.cpu_key != "z80"
                && self.cpu_key != "m68k"
                && self.peek(0) == '+'
                && matches!(self.peek(1), '1' | '2' | '3')
                && !is_id_char(self.peek(2))
            {
                force = self.peek(1) as u8 - b'0';
                self.bump(2);
            }
            self.ws();
            if force == 0 && self.peek(0) == '=' && self.peek(1) != '=' {
                if after_label {
                    return self.err("Garbage data at end of statement.");
                }
                self.bump(1);
                let v = self.expr()?;
                self.define_sym(&name, v, Kind::Equ)?;
                return Ok(false);
            }
            if is_mnemonic {
                self.instruction(&lname, force)?;
                return Ok(false);
            }
            if after_label {
                return self.err("Garbage data at end of statement.");
            }
            if indented && self.peek(0) != ':' && ALL_MNEMONICS.contains_key(&lname) {
                return self.err(format!("Mnemonic \"{lname}\" is not available for CPU \"{}\".", self.cpu_name));
            }
            self.define_label(&name)?;
            if self.peek(0) == ':' {
                return Ok(false);
            }
            if indented {
                self.warn("Implicit label definition not in leftmost column.");
            }
            if self.eos() {
                return Ok(false);
            }
            return self.statement(false, true);
        }
        match c {
            '}' => self.err("Found '}' instead of end-of-file."),
            '{' => self.err("Found '{' without a pseudo opcode that takes a block."),
            _ => self.err("Syntax error."),
        }
    }

    // ------------------------------------------------------------------ program counter, output

    pub fn pc_val(&self) -> R<i64> {
        match self.realpc {
            Some(real) => Ok(real + self.pc_offset),
            None => self.err("Program counter undefined."),
        }
    }

    fn close_segment(&mut self) {
        if let Some(real) = self.realpc
            && real > self.seg_start
        {
            self.segments.push((self.seg_start, real));
        }
    }

    fn set_pc(&mut self) -> R<()> {
        let v = self.expr()?;
        let mut n = self.to_int(&v)?;
        let mut overlay = false;
        loop {
            self.ws();
            if self.peek(0) != ',' {
                break;
            }
            self.bump(1);
            self.ws();
            match self.read_word().to_lowercase().as_str() {
                "overlay" => overlay = true,
                "invisible" => {}
                _ => return self.err("Unknown \"*=\" segment modifier."),
            }
        }
        if v.is_undef() {
            n = 0.;
        }
        if n < 0. || n > self.top as f64 {
            return self.err("Program counter out of range.");
        }
        self.close_segment();
        let real = n as i64 - self.pc_offset;
        if self.fin && !overlay && self.segments.iter().any(|&(start, end)| real >= start && real < end) {
            self.warn("Segment starts inside another one, overwriting it.");
        }
        self.realpc = Some(real);
        self.seg_start = real;
        Ok(())
    }

    pub fn emit(&mut self, b: f64) -> R<()> {
        let Some(a) = self.realpc else {
            return self.err("Program counter undefined.");
        };
        if a > self.top || a < 0 {
            return self.err("Produced too much code.");
        }
        let x = ((int32(b) ^ self.xor_val) & 0xff) as u8;
        self.mem[a as usize] = x;
        self.written[a as usize] = true;
        self.min_addr = self.min_addr.min(a);
        self.max_addr = self.max_addr.max(a);
        self.realpc = Some(a + 1);
        self.emit_count += 1;
        Ok(())
    }

    /// A number of `size` bytes, little endian (or `be`); out of range is an error unless undefined.
    pub fn emit_val(&mut self, n: f64, size: u32, undef: bool, unsigned: bool, be: bool) -> R<()> {
        let max = 2f64.powi(8 * size as i32);
        let min = if unsigned { 0. } else { -(max / 2.) };
        if !undef && (n < min || n >= max) {
            self.error("Number out of range.")?;
        }
        let mut x = n.rem_euclid(4294967296.);
        let mut out = Vec::with_capacity(size as usize);
        for _ in 0..size {
            out.push(x % 256.);
            x = (x / 256.).floor();
        }
        if be {
            out.reverse();
        }
        for b in out {
            self.emit(b)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------ symbols

    fn new_zone(&mut self, title: String) -> Zone {
        self.zone_counter += 1;
        Zone { id: self.zone_counter, title }
    }

    fn new_cheap(&mut self, title: String) -> Zone {
        self.cheap_counter += 1;
        Zone { id: self.cheap_counter, title }
    }

    fn sym_key(&self, name: &str) -> String {
        match name.chars().next() {
            Some('.') => format!("L{}:{name}", self.zone.id),
            Some('@') => format!("C{}:{name}", self.cheap.id),
            _ => format!("G:{name}"),
        }
    }

    fn new_sym(key: &str, name: &str) -> SymRef {
        Rc::new(RefCell::new(Sym {
            name: name.to_owned(),
            value: None,
            def_pass: 0,
            kind: None,
            file: None,
            line: 0,
            zone: String::new(),
            used: false,
            ever_undef: false,
            is_set: false,
            in_macro: false,
            anon: key.starts_with('A'),
        }))
    }

    fn is_defined(&self, name: &str) -> bool {
        self.symbols.get(&self.sym_key(name)).is_some_and(|s| {
            let s = s.borrow();
            s.value.as_ref().is_some_and(|v| !v.is_undef()) && s.def_pass == self.pass
        })
    }

    fn define_sym(&mut self, name: &str, value: Val, kind: Kind) -> R<()> {
        let key = self.sym_key(name);
        self.define_key(key, name, value, kind)
    }

    fn define_key(&mut self, key: String, name: &str, mut value: Val, kind: Kind) -> R<()> {
        if self.dry > 0 {
            return Ok(());
        }
        if self.addr_mode > 0
            && let Val::N(n) = &mut value
        {
            n.a = 1;
        }
        let sym = self.symbols.entry(key.clone()).or_insert_with(|| Self::new_sym(&key, name)).clone();
        let (def_pass, is_set, old) = {
            let s = sym.borrow();
            (s.def_pass, s.is_set, s.value.clone())
        };
        if kind != Kind::Set && def_pass == self.pass && !is_set {
            if !old.as_ref().is_some_and(|old| val_equal(old, &value)) {
                self.error("Symbol already defined.")?;
            }
            return Ok(());
        }
        if kind != Kind::Set && def_pass != self.pass && old.as_ref().is_some_and(|old| !val_equal(old, &value)) {
            self.changed = true;
            if self.fin {
                self.error(format!("Symbol \"{name}\" changed value between passes (phase error)."))?;
            }
        }
        let (file, line) = match self.readers.last() {
            Some(r) => (Some(r.src.file.clone()), r.line),
            None => (None, 0),
        };
        let zone = match name.chars().next() {
            Some('.') => self.zone.title.clone(),
            Some('@') => self.cheap.title.clone(),
            _ => String::new(),
        };
        let mut s = sym.borrow_mut();
        s.value = Some(value);
        s.def_pass = self.pass;
        s.is_set = kind == Kind::Set;
        s.kind = Some(kind);
        s.file = file;
        s.line = line;
        s.zone = zone;
        s.in_macro = self.macro_depth > 0 && (name.starts_with('.') || name.starts_with('@'));
        Ok(())
    }

    fn define_label(&mut self, name: &str) -> R<()> {
        let v = Num { a: 1, ..Num::new(self.pc_val()? as f64) };
        self.define_sym(name, Val::N(v), Kind::Label)?;
        if !name.starts_with('.') && !name.starts_with('@') {
            self.cheap = self.new_cheap(name.to_owned());
        }
        Ok(())
    }

    fn define_anon(&mut self, run: &str) -> R<()> {
        let k = format!("{}:{run}", self.zone.id);
        let n = self.anon.get(&k).copied().unwrap_or(0);
        let key = if run.starts_with('-') {
            self.anon.insert(k.clone(), n + 1);
            format!("A{k}#{}", n + 1)
        } else {
            self.anon.insert(k.clone(), n + 1);
            format!("A{k}#{n}")
        };
        let v = Num { a: 1, ..Num::new(self.pc_val()? as f64) };
        self.define_key(key, run, Val::N(v), Kind::Label)
    }

    fn anon_ref(&mut self, run: &str) -> R<Val> {
        let k = format!("{}:{run}", self.zone.id);
        let key = format!("A{k}#{}", self.anon.get(&k).copied().unwrap_or(0));
        self.read_key(key, run)
    }

    fn read_key(&mut self, key: String, name: &str) -> R<Val> {
        let sym = self.symbols.get(&key).cloned();
        let defined = sym
            .as_ref()
            .and_then(|s| s.borrow().value.clone())
            .filter(|v| !v.is_undef());
        let Some(value) = defined else {
            if self.dry == 0 {
                let sym = sym.unwrap_or_else(|| {
                    let s = Self::new_sym(&key, name);
                    self.symbols.insert(key.clone(), s.clone());
                    s
                });
                sym.borrow_mut().ever_undef = true;
                self.undef_count += 1;
                if self.fin {
                    self.error(format!("Symbol not defined ({name})."))?;
                }
            }
            return Ok(Val::N(Num::undef()));
        };
        let sym = sym.unwrap();
        if self.dry == 0 {
            sym.borrow_mut().used = true;
            if self.fin && sym.borrow().def_pass != self.pass {
                let loc = self.loc();
                self.stale_uses.push((sym.clone(), name.to_owned(), loc));
            }
        }
        Ok(match value {
            Val::N(mut n) => {
                n.e = n.e || sym.borrow().ever_undef;
                Val::N(n)
            }
            other => other,
        })
    }

    // ------------------------------------------------------------------ expressions

    pub fn expr(&mut self) -> R<Val> {
        self.parse_bin(0)
    }

    fn parse_bin(&mut self, min_prec: u8) -> R<Val> {
        let mut left = self.parse_unary()?;
        loop {
            self.ws();
            let Some((op, prec, len)) = self.peek_op() else {
                return Ok(left);
            };
            if prec < min_prec {
                return Ok(left);
            }
            self.bump(len);
            let right = self.parse_bin(if prec == 11 { 11 } else { prec + 1 })?;
            left = self.binop(op, left, right)?;
        }
    }

    /// The binary operator at the reader: its name, precedence and length.
    fn peek_op(&self) -> Option<(&'static str, u8, usize)> {
        let (c, c1, c2) = (self.peek(0), self.peek(1), self.peek(2));
        Some(match c {
            '*' if c1 == '*' => ("^", 11, 2),
            '*' => ("*", 9, 1),
            '/' => ("/", 9, 1),
            '%' => ("%", 9, 1),
            '+' => ("+", 8, 1),
            '-' => ("-", 8, 1),
            '^' => ("^", 11, 1),
            '<' => match c1 {
                '<' => ("<<", 7, 2),
                '=' => ("<=", 5, 2),
                '>' => ("!=", 4, 2),
                _ => ("<", 5, 1),
            },
            '>' => match c1 {
                '>' if c2 == '>' => (">>>", 7, 3),
                '>' => (">>", 7, 2),
                '=' => (">=", 5, 2),
                '<' => ("!=", 4, 2),
                _ => (">", 5, 1),
            },
            '!' if c1 == '=' => ("!=", 4, 2),
            '!' => return None,
            '=' if c1 == '=' => ("=", 4, 2),
            '=' => ("=", 4, 1),
            '&' => ("&", 3, 1),
            '|' => ("|", 1, 1),
            c if is_id_start(c) => {
                let r = self.r();
                let mut e = r.pos;
                while e < r.end && is_id_char(r.t(e)) {
                    e += 1;
                }
                let word: String = r.src.text[r.pos..e].iter().collect::<String>().to_lowercase();
                let (op, prec) = match word.as_str() {
                    "div" => ("DIV", 9),
                    "mod" => ("%", 9),
                    "asl" | "lsl" => ("<<", 7),
                    "asr" => (">>", 7),
                    "lsr" => (">>>", 7),
                    "and" => ("&", 3),
                    "xor" | "eor" => ("XOR", 2),
                    "or" => ("|", 1),
                    _ => return None,
                };
                (op, prec, e - r.pos)
            }
            _ => return None,
        })
    }

    fn parse_unary(&mut self) -> R<Val> {
        self.ws();
        let c = self.peek(0);
        if c == '+' || c == '-' {
            let mut n = 0;
            while self.peek(n) == c {
                n += 1;
            }
            let r = self.r();
            let mut p = r.pos + n;
            while p < r.end && matches!(r.t(p), ' ' | '\t') {
                p += 1;
            }
            let next = if p < r.end { r.t(p) } else { EOT };
            if matches!(next, EOT | '\n' | ';' | ':' | ',' | ')' | ']' | '}' | '\r') {
                self.bump(n);
                return self.anon_ref(&c.to_string().repeat(n));
            }
            self.bump(1);
            let operand = self.parse_bin(11)?;
            let v = self.numify(operand)?;
            return Ok(Val::N(if c == '-' { Num { v: -v.v, a: -v.a, ..v } } else { v }));
        }
        if c == '!' {
            self.bump(1);
            let x = self.parse_bin(13)?;
            return self.unop("not", x);
        }
        if c == '<' || c == '>' || c == '^' {
            self.bump(1);
            let x = self.parse_bin(7)?;
            return self.unop(if c == '<' { "<" } else if c == '>' { ">" } else { "^" }, x);
        }
        if (c == 'n' || c == 'N') && self.r().keyword("not") {
            self.bump(3);
            let x = self.parse_bin(13)?;
            return self.unop("not", x);
        }
        let mut v = self.parse_primary()?;
        while self.peek(0) == '[' {
            self.bump(1);
            let i = self.expr()?;
            self.ws();
            if self.peek(0) != ']' {
                return self.err("Missing ']'.");
            }
            self.bump(1);
            v = self.index(v, &i)?;
        }
        Ok(v)
    }

    fn parse_primary(&mut self) -> R<Val> {
        self.ws();
        let c = self.peek(0);
        match c {
            '(' => {
                self.bump(1);
                let v = self.expr()?;
                self.ws();
                if self.peek(0) != ')' {
                    return self.err("Missing ')'.");
                }
                self.bump(1);
                Ok(v)
            }
            '[' => {
                self.bump(1);
                let mut list = Vec::new();
                self.ws();
                if self.peek(0) == ']' {
                    self.bump(1);
                    return Ok(Val::L(list));
                }
                loop {
                    list.push(self.expr()?);
                    self.ws();
                    match self.peek(0) {
                        ',' => self.bump(1),
                        ']' => {
                            self.bump(1);
                            return Ok(Val::L(list));
                        }
                        _ => return self.err("Missing ']'."),
                    }
                }
            }
            '"' => {
                self.bump(1);
                Ok(Val::S(self.read_quoted('"')?))
            }
            '\'' => {
                self.bump(1);
                let s = self.read_quoted('\'')?;
                let mut chars = s.chars();
                match (chars.next(), chars.next()) {
                    (Some(ch), None) if ch.len_utf16() == 1 => Ok(Val::N(Num::new(self.conv(ch as u32) as f64))),
                    _ => self.err("Character constant must contain exactly one character."),
                }
            }
            '$' => {
                self.bump(1);
                self.parse_digits(16)
            }
            '%' => {
                self.bump(1);
                self.parse_digits(2)
            }
            '&' => {
                self.bump(1);
                self.parse_digits(8)
            }
            '0'..='9' => {
                let c1 = self.peek(1);
                if c == '0' && (c1 == 'x' || c1 == 'X') {
                    self.bump(2);
                    return self.parse_digits(16);
                }
                if c == '0' && (c1 == 'b' || c1 == 'B') && matches!(self.peek(2), '0' | '1' | '.' | '#') {
                    self.bump(2);
                    return self.parse_digits(2);
                }
                let start = self.r().pos;
                while self.peek(0).is_ascii_digit() {
                    self.bump(1);
                }
                if self.peek(0) == '.' && self.peek(1).is_ascii_digit() {
                    self.bump(1);
                    while self.peek(0).is_ascii_digit() {
                        self.bump(1);
                    }
                    let text: String = self.r().src.text[start..self.r().pos].iter().collect();
                    return Ok(Val::N(Num::float(text.parse().unwrap_or(0.))));
                }
                if is_id_char(self.peek(0)) {
                    return self.err("Malformed number.");
                }
                let text: String = self.r().src.text[start..self.r().pos].iter().collect();
                Ok(Val::N(Num::new(text.parse().unwrap_or(0.))))
            }
            '*' => {
                self.bump(1);
                Ok(Val::N(Num { a: 1, ..Num::new(self.pc_val()? as f64) }))
            }
            c if c == '.' || c == '@' || is_id_start(c) => {
                let name = self.read_sym_name()?;
                if c != '.' && c != '@' {
                    let lname = name.to_lowercase();
                    if matches!(
                        lname.as_str(),
                        "sin" | "cos" | "tan" | "arcsin" | "arccos" | "arctan" | "int" | "float" | "addr"
                            | "address" | "is_number" | "is_list" | "is_string" | "len"
                    ) {
                        let saved = self.r().save();
                        self.ws();
                        if self.peek(0) == '(' {
                            self.bump(1);
                            let a = self.expr()?;
                            self.ws();
                            if self.peek(0) != ')' {
                                return self.err("Missing ')'.");
                            }
                            self.bump(1);
                            return self.func(&lname, a);
                        }
                        self.rm().restore(saved);
                    }
                }
                let key = self.sym_key(&name);
                self.read_key(key, &name)
            }
            EOT | '\n' | ';' | ':' => self.err("Value expected."),
            c => self.err(format!("Syntax error (unexpected \"{c}\").")),
        }
    }

    fn parse_digits(&mut self, base: u32) -> R<Val> {
        let mut digits = 0;
        let mut v = 0f64;
        loop {
            let c = self.peek(0);
            let d = match base {
                16 => match c.to_digit(16) {
                    Some(d) => d,
                    None => break,
                },
                2 => match c {
                    '0' | '.' => 0,
                    '1' | '#' => 1,
                    _ => break,
                },
                _ => match c {
                    '0'..='7' => c as u32 - '0' as u32,
                    _ => break,
                },
            };
            v = v * base as f64 + d as f64;
            digits += 1;
            self.bump(1);
        }
        if digits == 0 {
            return self.err("Malformed number.");
        }
        if base != 2 && is_id_char(self.peek(0)) {
            return self.err("Malformed number.");
        }
        let mut out = Num::new(v);
        if (base == 16 && digits > 2) || (base == 2 && digits > 8) {
            out.force = 2;
        }
        if (base == 16 && digits > 4) || (base == 2 && digits > 16) {
            out.force = 3;
        }
        Ok(Val::N(out))
    }

    pub fn conv(&self, code: u32) -> u32 {
        match &self.convtab {
            Convtab::Raw => code & 0xff,
            Convtab::Pet => conv_pet(code) & 0xff,
            Convtab::Scr => conv_scr(code) & 0xff,
            Convtab::Table(t) => t[(code & 0xff) as usize] as u32,
        }
    }

    pub fn numify(&self, v: Val) -> R<Num> {
        match v {
            Val::N(n) => Ok(n),
            Val::S(s) if s.encode_utf16().count() == 1 => {
                Ok(Num::new(self.conv(s.chars().next().unwrap() as u32) as f64))
            }
            Val::S(_) => self.err("String used where a number is expected (only single characters allowed)."),
            Val::L(_) => self.err("List used where a number is expected."),
        }
    }

    pub fn to_num(&self, v: &Val) -> R<f64> {
        Ok(self.numify(v.clone())?.v)
    }

    pub fn to_int(&self, v: &Val) -> R<f64> {
        Ok(self.to_num(v)?.trunc())
    }

    fn truthy(v: &Val) -> bool {
        match v {
            Val::N(n) => n.v != 0.,
            Val::S(s) => !s.is_empty(),
            Val::L(l) => !l.is_empty(),
        }
    }

    fn index(&self, v: Val, i: &Val) -> R<Val> {
        let n = self.to_int(i)? as i64;
        let len = match &v {
            Val::L(l) => l.len(),
            Val::S(s) => s.chars().count(),
            Val::N(_) => return self.err("Indexing is only possible for lists and strings."),
        } as i64;
        let k = if n < 0 { len + n } else { n };
        if k < 0 || k >= len {
            return self.err("Index out of range.");
        }
        Ok(match v {
            Val::L(l) => l[k as usize].clone(),
            Val::S(s) => Val::S(s.chars().nth(k as usize).unwrap().to_string()),
            Val::N(_) => unreachable!(),
        })
    }

    fn func(&self, name: &str, a: Val) -> R<Val> {
        let an = a.num();
        let fl = |x: f64| Val::N(Num { u: an.u, e: an.e, ..Num::float(x) });
        let range_check = |x: f64| -> R<()> {
            if (!(-1. ..=1.).contains(&x)) && !an.u {
                return self.err("Argument out of range.");
            }
            Ok(())
        };
        Ok(match name {
            "sin" => fl(self.numify(a)?.v.sin()),
            "cos" => fl(self.numify(a)?.v.cos()),
            "tan" => fl(self.numify(a)?.v.tan()),
            "arcsin" => {
                let x = self.numify(a)?.v;
                range_check(x)?;
                fl(x.asin())
            }
            "arccos" => {
                let x = self.numify(a)?.v;
                range_check(x)?;
                fl(x.acos())
            }
            "arctan" => fl(self.numify(a)?.v.atan()),
            "int" => {
                let x = self.numify(a)?;
                Val::N(Num { v: x.v.trunc(), f: false, ..x })
            }
            "float" => Val::N(Num { f: true, ..self.numify(a)? }),
            "addr" | "address" => Val::N(Num { a: 1, ..self.numify(a)? }),
            "is_number" => Val::bool(matches!(a, Val::N(_))),
            "is_list" => Val::bool(matches!(a, Val::L(_))),
            "is_string" => Val::bool(matches!(a, Val::S(_))),
            "len" => match a {
                Val::L(l) => Val::N(Num::new(l.len() as f64)),
                Val::S(s) => Val::N(Num::new(s.encode_utf16().count() as f64)),
                Val::N(_) => return self.err("len() needs a list or string."),
            },
            _ => return self.err("Unknown function."),
        })
    }

    fn unop(&self, op: &str, x: Val) -> R<Val> {
        let x = self.numify(x)?;
        let n = x.v.trunc();
        let v = match op {
            "not" => -n - 1.,
            "<" => bitop(n, 255., '&'),
            ">" => bitop((n / 256.).floor(), 255., '&'),
            _ => bitop((n / 65536.).floor(), 255., '&'),
        };
        Ok(Val::N(Num { v, u: x.u, e: x.e, ..Default::default() }))
    }

    fn binop(&self, op: &str, x: Val, y: Val) -> R<Val> {
        let (x, y) = match (x, y) {
            (Val::N(x), Val::N(y)) => (x, y),
            (Val::S(a), Val::S(b)) if matches!(op, "+" | "=" | "!=" | "<" | "<=" | ">" | ">=") => {
                return Ok(match op {
                    "+" => Val::S(a + &b),
                    "=" => Val::bool(a == b),
                    "!=" => Val::bool(a != b),
                    "<" => Val::bool(a < b),
                    "<=" => Val::bool(a <= b),
                    ">" => Val::bool(a > b),
                    _ => Val::bool(a >= b),
                });
            }
            (x @ Val::L(_), y) | (x, y @ Val::L(_)) => {
                if let (Val::L(a), Val::L(b)) = (&x, &y) {
                    match op {
                        "+" => return Ok(Val::L(a.iter().chain(b).cloned().collect())),
                        "=" => return Ok(Val::bool(val_equal(&x, &y))),
                        "!=" => return Ok(Val::bool(!val_equal(&x, &y))),
                        _ => {}
                    }
                }
                return self.err("Operation not supported for lists.");
            }
            (x, y) => (self.numify(x)?, self.numify(y)?),
        };
        let u = x.u || y.u;
        let e = x.e || y.e;
        let force = x.force.max(y.force);
        let is_f = x.f || y.f;
        let (a, b) = (x.v, y.v);
        let div_zero = || -> R<f64> {
            if !u {
                return self.err("Division by zero.");
            }
            Ok(0.)
        };
        let mut f = false;
        let mut addr = 0;
        let mut v = match op {
            "^" => {
                if !is_f && b < 0. {
                    if !u {
                        return self.err("Exponent is negative.");
                    }
                    0.
                } else {
                    f = is_f;
                    a.powf(b)
                }
            }
            "*" => {
                f = is_f;
                a * b
            }
            "/" => {
                if b == 0. {
                    div_zero()?
                } else if is_f {
                    f = true;
                    a / b
                } else {
                    (a / b).trunc()
                }
            }
            "DIV" => {
                if b == 0. {
                    div_zero()?
                } else {
                    (a / b).trunc()
                }
            }
            "%" => {
                if b == 0. {
                    div_zero()?
                } else {
                    f = is_f;
                    a % b
                }
            }
            "+" => {
                f = is_f;
                addr = x.a + y.a;
                a + b
            }
            "-" => {
                f = is_f;
                addr = x.a - y.a;
                a - b
            }
            "<<" => a.trunc() * 2f64.powf(b.trunc()),
            ">>" => (a.trunc() / 2f64.powf(b.trunc())).floor(),
            ">>>" => {
                let ua = if a.trunc() < 0. { a.trunc().rem_euclid(4294967296.) } else { a.trunc() };
                (ua / 2f64.powf(b.trunc())).floor()
            }
            "<" => (a < b) as u8 as f64,
            "<=" => (a <= b) as u8 as f64,
            ">" => (a > b) as u8 as f64,
            ">=" => (a >= b) as u8 as f64,
            "=" => (a == b) as u8 as f64,
            "!=" => (a != b) as u8 as f64,
            "&" => bitop(a, b, '&'),
            "|" => bitop(a, b, '|'),
            "XOR" => bitop(a, b, '^'),
            _ => return self.err("Unknown operator."),
        };
        if !f && !v.is_finite() {
            v = 0.;
        }
        Ok(Val::N(Num { v, f, u, e, a: addr, force }))
    }

    // ------------------------------------------------------------------ instructions

    pub fn set_cpu(&mut self, name: &str) -> R<()> {
        let Some(key) = tables::canonical_cpu(name) else {
            if name == "c64dtv2" {
                return self.err(format!(
                    "Unsupported CPU \"{name}\" (supported: 6502, nmos6502/6510, 65c02, r65c02, w65c02, 65816, 65ce02, 4502, m65, z80, 68000)."
                ));
            }
            return self.err(format!("Unknown processor \"{name}\"."));
        };
        if key == "m68k" {
            // TODO: port m68k.js
            return self.err("The 68000 is not supported by this version of NerdSuite yet.");
        }
        self.cpu_name = name.to_owned();
        self.cpu_key = key;
        Ok(())
    }

    /// Whether the operand in parentheses (or brackets) is all of it: `(zp),y`, not `(2+3)*4`.
    pub fn looks_indirect(&self) -> bool {
        let r = self.r();
        let mut p = r.pos;
        let mut depth = 0;
        while p < r.end {
            let c = r.t(p);
            match c {
                '\n' | ';' => return false,
                '"' | '\'' => {
                    p += 1;
                    while p < r.end && r.t(p) != c && r.t(p) != '\n' {
                        if r.t(p) == '\\' {
                            p += 1;
                        }
                        p += 1;
                    }
                }
                '(' | '[' => depth += 1,
                ')' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        p += 1;
                        while p < r.end && matches!(r.t(p), ' ' | '\t' | '\r') {
                            p += 1;
                        }
                        let n = if p < r.end { r.t(p) } else { EOT };
                        return matches!(n, EOT | '\n' | ';' | ':' | ',');
                    }
                }
                _ => {}
            }
            p += 1;
        }
        false
    }

    fn read_index(&mut self, want: &[&str]) -> R<String> {
        self.ws();
        let ix = self.read_word().to_lowercase();
        if !want.contains(&ix.as_str()) {
            return self.err(ILLEGAL_MODE);
        }
        Ok(ix)
    }

    fn expect(&mut self, c: char, msg: &str) -> R<()> {
        self.ws();
        if self.peek(0) != c {
            return self.err(msg);
        }
        self.bump(1);
        Ok(())
    }

    fn instruction(&mut self, mn: &str, force: u8) -> R<()> {
        if self.cpu_key == "z80" {
            return self.z80_instruction(mn);
        }
        let tab: &'static Modes = &CPUS[self.cpu_key].table[mn];
        let w816 = self.cpu_key == "65816";
        let ce02 = matches!(self.cpu_key, "65ce02" | "4502" | "m65");
        let mut v = Val::N(Num::default());
        let mut v2 = Val::N(Num::default());
        let mode;
        if self.eos() {
            mode = Parsed::Imp;
        } else {
            let c = self.peek(0);
            if c == '#' {
                self.bump(1);
                v = self.expr()?;
                mode = Parsed::Imm;
            } else if tab.contains_key(&Mode::Blk) {
                // mvn / mvp: the source bank, the target bank
                v = self.expr()?;
                self.ws();
                if self.peek(0) != ',' {
                    return self.err(ILLEGAL_MODE);
                }
                self.bump(1);
                v2 = self.expr()?;
                mode = Parsed::Blk;
            } else if (w816 || self.cpu_key == "m65") && c == '[' {
                // [dp], [dp],y, [abs] - the 45GS02: [zp],z and the quad's [zp]
                self.bump(1);
                v = self.expr()?;
                self.expect(']', "Missing ']'.")?;
                self.ws();
                if self.peek(0) == ',' {
                    self.bump(1);
                    if w816 {
                        self.read_index(&["y"])?;
                        mode = Parsed::Ildpy;
                    } else {
                        self.read_index(&["z"])?;
                        mode = Parsed::Izl;
                    }
                } else {
                    mode = if w816 { Parsed::Ildp } else { Parsed::Izlq };
                }
            } else if tab.contains_key(&Mode::Zprel) {
                v = self.expr()?;
                self.ws();
                if self.peek(0) != ',' {
                    return self.err(ILLEGAL_MODE);
                }
                self.bump(1);
                v2 = self.expr()?;
                mode = Parsed::Zprel;
            } else if c == '(' && self.looks_indirect() {
                self.bump(1);
                v = self.expr()?;
                self.ws();
                if self.peek(0) == ',' {
                    self.bump(1);
                    let ix = self.read_index(if w816 || ce02 { &["x", "s"] } else { &["x"] })?;
                    self.expect(')', "Missing ')'.")?;
                    if ix == "s" {
                        self.ws();
                        if self.peek(0) != ',' {
                            return self.err(ILLEGAL_MODE);
                        }
                        self.bump(1);
                        self.read_index(&["y"])?;
                        mode = Parsed::Sry;
                    } else {
                        mode = Parsed::Indx;
                    }
                } else {
                    if self.peek(0) != ')' {
                        return self.err("Missing ')'.");
                    }
                    self.bump(1);
                    self.ws();
                    if self.peek(0) == ',' {
                        self.bump(1);
                        let ix = self.read_index(if ce02 { &["y", "z"] } else { &["y"] })?;
                        mode = if ix == "z" { Parsed::Izpz } else { Parsed::Indy };
                    } else {
                        mode = Parsed::Ind;
                    }
                }
            } else {
                v = self.expr()?;
                self.ws();
                if self.peek(0) == ',' {
                    self.bump(1);
                    mode = match self.read_index(if w816 { &["x", "y", "s"] } else { &["x", "y"] })?.as_str() {
                        "x" => Parsed::X,
                        "y" => Parsed::Y,
                        _ => Parsed::Sr,
                    };
                } else {
                    mode = Parsed::Plain;
                }
            }
        }
        self.encode(mn, tab, mode, &v, &v2, force)
    }

    /// An opcode with its prefixes.
    fn emit_op(&mut self, op: u32) -> R<()> {
        for b in tables::op_bytes(op) {
            self.emit(b as f64)?;
        }
        Ok(())
    }

    fn encode(&mut self, mn: &str, tab: &Modes, mode: Parsed, v: &Val, v2: &Val, force: u8) -> R<()> {
        let op = |m: Mode| tab.get(&m).copied();
        match mode {
            Parsed::Imp => {
                let Some(o) = op(Mode::Imp) else { return self.err(ILLEGAL_MODE) };
                self.emit_op(o)
            }
            Parsed::Izpz | Parsed::Izl | Parsed::Izlq => {
                // (zp),z [zp],z [zp] - a quad takes [zp] and [zp],z
                let o = match mode {
                    Parsed::Izpz => op(Mode::Izpz),
                    Parsed::Izl => op(Mode::Izl).or(op(Mode::Izlq)),
                    _ => op(Mode::Izlq),
                };
                let Some(o) = o.filter(|_| force < 2) else { return self.err(ILLEGAL_MODE) };
                self.emit_op(o)?;
                let n = self.to_int(v)?;
                self.emit_val(n, 1, v.is_undef(), true, false)
            }
            Parsed::Imm => {
                if let Some(o) = op(Mode::Imm16) {
                    // phw #imm16
                    self.emit_op(o)?;
                    let n = self.to_int(v)?;
                    return self.emit_val(n, 2, v.is_undef(), false, false);
                }
                let Some(o) = op(Mode::Imm).filter(|_| force < 3) else { return self.err(ILLEGAL_MODE) };
                let n = self.to_int(v)?;
                let wide = self.cpu_key == "65816"
                    && ((IMM_A.contains(&mn) && self.al) || (IMM_X.contains(&mn) && self.rl));
                if force == 2 && !wide {
                    return self.err(ILLEGAL_MODE);
                }
                self.emit_op(o)?;
                self.emit_val(n, if wide { 2 } else { 1 }, v.is_undef(), false, false)
            }
            Parsed::Zprel => {
                let pc = self.pc_val()?;
                let zp = self.to_int(v)?;
                self.emit_op(op(Mode::Zprel).unwrap())?;
                self.emit_val(zp, 1, v.is_undef(), true, false)?;
                self.branch(v2, pc + 3)
            }
            Parsed::Plain => {
                if let Some(o) = op(Mode::Rel) {
                    let pc = self.pc_val()?;
                    self.emit_op(o)?;
                    return self.branch(v, pc + 2);
                }
                // the 65CE02's lbxx and bsr count from the third byte of the instruction; brl and per
                // from the next instruction
                for (m, from) in [(Mode::Rel16, 2), (Mode::Rell, 3)] {
                    if let Some(o) = op(m) {
                        let pc = self.pc_val()?;
                        let off = self.to_int(v)? - (pc + from) as f64;
                        if !v.is_undef() && !(-32768. ..=32767.).contains(&off) {
                            self.error(format!("Target out of range ({}).", js_number(off)))?;
                        }
                        self.emit_op(o)?;
                        return self.emit_val(off.rem_euclid(65536.), 2, v.is_undef(), true, false);
                    }
                }
                self.sized(op(Mode::Zp), op(Mode::Abs), v, force, op(Mode::Long))
            }
            Parsed::X => self.sized(op(Mode::Zpx), op(Mode::Absx), v, force, op(Mode::Longx)),
            Parsed::Y => self.sized(op(Mode::Zpy), op(Mode::Absy), v, force, None),
            Parsed::Sr | Parsed::Sry | Parsed::Ildpy => {
                let m = match mode {
                    Parsed::Sr => Mode::Sr,
                    Parsed::Sry => Mode::Sry,
                    _ => Mode::Ildpy,
                };
                let Some(o) = op(m) else { return self.err(ILLEGAL_MODE) };
                self.emit_op(o)?;
                let n = self.to_int(v)?;
                self.emit_val(n, 1, v.is_undef(), true, false)
            }
            Parsed::Ildp => {
                if let Some(o) = op(Mode::Ialong) {
                    self.emit_op(o)?;
                    let n = self.to_int(v)?;
                    return self.emit_val(n, 2, v.is_undef(), true, false);
                }
                let Some(o) = op(Mode::Ildp) else { return self.err(ILLEGAL_MODE) };
                self.emit_op(o)?;
                let n = self.to_int(v)?;
                self.emit_val(n, 1, v.is_undef(), true, false)
            }
            Parsed::Blk => {
                // the target bank first, as the CPU wants it
                self.emit_op(op(Mode::Blk).unwrap())?;
                let n2 = self.to_int(v2)?;
                self.emit_val(n2, 1, v2.is_undef(), true, false)?;
                let n = self.to_int(v)?;
                self.emit_val(n, 1, v.is_undef(), true, false)
            }
            Parsed::Ind => {
                if let Some(o) = op(Mode::Ind) {
                    let n = self.to_int(v)?;
                    if mn == "jmp"
                        && !v.is_undef()
                        && int32(n) & 0xff == 0xff
                        && matches!(self.cpu_key, "6502" | "nmos6502")
                    {
                        self.warn("Assembling buggy JMP($xxff) instruction");
                    }
                    return self.sized(None, Some(o), v, force, None);
                }
                self.sized(op(Mode::Izp), None, v, force, None)
            }
            Parsed::Indx => self.sized(op(Mode::Indx), op(Mode::Iax), v, force, None),
            Parsed::Indy => self.sized(op(Mode::Indy), None, v, force, None),
        }
    }

    fn sized(&mut self, zp_op: Option<u32>, abs_op: Option<u32>, v: &Val, force: u8, long_op: Option<u32>) -> R<()> {
        let n = self.to_int(v)?;
        let num = v.num();
        // 24 bits: asked for (+3, six hex digits), too big for 16 bits, or the only mode (jsl, jml)
        if let Some(long) = long_op
            && (force == 3
                || (force == 0 && (num.force == 3 || (!num.u && n > 65535.)))
                || (zp_op.is_none() && abs_op.is_none()))
        {
            self.emit_op(long)?;
            return self.emit_val(n, 3, num.u, true, false);
        }
        if zp_op.is_none() && abs_op.is_none() {
            return self.err(ILLEGAL_MODE);
        }
        if force == 3 {
            return self.err("24-bit addressing (\"+3\") is not supported by this CPU.");
        }
        let zp = match force {
            1 => true,
            2 => false,
            _ if zp_op.is_none() => false,
            _ if abs_op.is_none() => true,
            _ => !(num.u || num.e || num.force >= 2) && (0. ..=255.).contains(&n),
        };
        let (op, size) = if zp { (zp_op, 1) } else { (abs_op, 2) };
        let Some(op) = op else { return self.err(ILLEGAL_MODE) };
        self.emit_op(op)?;
        self.emit_val(n, size, num.u, true, false)
    }

    pub fn branch(&mut self, v: &Val, next: i64) -> R<()> {
        let t = self.to_int(v)?;
        let off = t - next as f64;
        if !v.is_undef() && !(-128. ..=127.).contains(&off) {
            let far = if off > 127. { off - 127. } else { -128. - off };
            self.error(format!("Target out of range ({}; {} too far).", js_number(off), js_number(far)))?;
        }
        self.emit((int32(off) & 0xff) as f64)
    }

    // ------------------------------------------------------------------ pseudo opcodes

    fn pseudo(&mut self, name: &str) -> R<bool> {
        match name {
            "to" => self.po_to(),
            "8" | "08" | "by" | "byte" => self.po_data(1, false),
            "16" | "wo" | "word" | "le16" => self.po_data(2, false),
            "be16" => self.po_data(2, true),
            "24" | "le24" => self.po_data(3, false),
            "be24" => self.po_data(3, true),
            "32" | "le32" => self.po_data(4, false),
            "be32" => self.po_data(4, true),
            "h" | "hex" => self.po_hex(),
            "fi" | "fill" => self.po_fill(),
            "skip" => self.po_skip(),
            "align" => self.po_align(),
            "tx" | "text" => self.po_text(None, 0),
            "pet" => self.po_text(Some(Convtab::Pet), 0),
            "raw" => self.po_text(Some(Convtab::Raw), 0),
            "scr" => self.po_text(Some(Convtab::Scr), 0),
            "scrxor" => {
                let v = self.expr()?;
                let x = int32(self.to_int(&v)?);
                self.ws();
                if self.peek(0) != ',' {
                    return self.err("Missing comma.");
                }
                self.bump(1);
                self.po_text(Some(Convtab::Scr), x)
            }
            "ct" | "convtab" => self.po_convtab(),
            "bi" | "bin" | "binary" => self.po_binary(),
            "src" | "source" => self.po_source(),
            "macro" => self.po_macro(),
            "if" => self.po_if(),
            "ifdef" => self.po_ifdef(true),
            "ifndef" => self.po_ifdef(false),
            "for" => self.po_for(),
            "do" => self.po_do(),
            "while" => self.po_while(),
            "set" => {
                let n = self.read_sym_name()?;
                self.ws();
                if self.peek(0) != '=' {
                    return self.err("Missing '='.");
                }
                self.bump(1);
                let v = self.expr()?;
                self.define_sym(&n, v, Kind::Set)?;
                Ok(false)
            }
            "zn" | "zone" => self.po_zone(),
            "sl" | "symbollist" => {
                self.sl_name = Some(self.read_file_name()?.0);
                Ok(false)
            }
            "svl" => {
                self.svl_name = Some(self.read_file_name()?.0);
                Ok(false)
            }
            "pseudopc" => self.po_pseudopc(),
            "realpc" => {
                self.pc_offset = 0;
                Ok(false)
            }
            "cpu" => self.po_cpu(),
            "al" | "as" | "rl" | "rs" => {
                if self.cpu_key != "65816" {
                    return self.err(format!("\"!{name}\" is only available for the 65816 CPU."));
                }
                let accu = name.starts_with('a');
                let wide = name.ends_with('l');
                self.with_block(
                    |asm| {
                        let flag = if accu { &mut asm.al } else { &mut asm.rl };
                        Ok(std::mem::replace(flag, wide))
                    },
                    |asm, old| *(if accu { &mut asm.al } else { &mut asm.rl }) = old,
                )
            }
            "initmem" => {
                let v = self.expr()?;
                let n = self.to_int(&v)?;
                if !v.is_undef() && !(-128. ..=255.).contains(&n) {
                    self.error("Number out of range.")?;
                }
                self.initmem = (int32(n) & 0xff) as u8;
                Ok(false)
            }
            "xor" => self.po_xor(),
            "warn" | "error" | "serious" => self.po_message(name),
            "eof" | "endoffile" => Err(Flow::Eof),
            "run" | "init" => {
                // nerdsuite's own: the entry point (xex RUNAD, amsdos exec) and the xex INITAD
                let v = self.expr()?;
                let n = if v.is_undef() { 0. } else { self.to_int(&v)? };
                if n < 0. || n > self.top as f64 {
                    return self.err("Number out of range.");
                }
                if name == "run" {
                    self.run_addr = Some(n as i64);
                } else {
                    self.init_addr = Some(n as i64);
                }
                Ok(false)
            }
            "addr" | "address" => self.po_address(),
            _ => self.err("Unknown pseudo opcode."),
        }
    }

    fn po_to(&mut self) -> R<bool> {
        let (name, _) = self.read_file_name()?;
        let mut format = None;
        self.ws();
        if self.peek(0) == ',' {
            self.bump(1);
            self.ws();
            let f = self.read_word().to_lowercase();
            if !OUTPUT_FORMATS.contains(&f.as_str()) {
                return self.err("Unknown output format.");
            }
            format = Some(f);
        } else {
            self.warn("Used \"!to\" without file format indicator. Defaulting to \"cbm\".");
        }
        if self.out_name.is_some() {
            self.warn("Output file already chosen, ignoring this \"!to\".");
        } else {
            self.out_name = Some(name);
            self.out_format = Some(format.unwrap_or_else(|| "cbm".into()));
        }
        Ok(false)
    }

    /// The values of a comma separated list, one by one.
    fn arg_list(&mut self, mut each: impl FnMut(&mut Self, Val) -> R<()>) -> R<()> {
        loop {
            let v = self.expr()?;
            each(self, v)?;
            self.ws();
            if self.peek(0) != ',' {
                return Ok(());
            }
            self.bump(1);
        }
    }

    fn po_data(&mut self, size: u32, be: bool) -> R<bool> {
        self.arg_list(|asm, v| match v {
            Val::S(s) if size == 1 => {
                for ch in s.chars() {
                    asm.emit(asm.conv(ch as u32) as f64)?;
                }
                Ok(())
            }
            Val::L(list) => {
                for x in list {
                    let n = asm.numify(x)?;
                    asm.emit_val(n.v.trunc(), size, n.u, false, be)?;
                }
                Ok(())
            }
            v => {
                let n = asm.numify(v)?;
                asm.emit_val(n.v.trunc(), size, n.u, false, be)
            }
        })?;
        Ok(false)
    }

    fn po_hex(&mut self) -> R<bool> {
        let mut any = false;
        while !self.eos() {
            let mut group = String::new();
            while self.peek(0).is_ascii_hexdigit() {
                group.push(self.peek(0));
                self.bump(1);
            }
            if group.is_empty() {
                return self.err("Only hex digits and spaces are allowed in \"!hex\".");
            }
            if group.len() % 2 == 1 {
                return self.err("Hex digits are not given in pairs.");
            }
            for ix in (0..group.len()).step_by(2) {
                self.emit(u8::from_str_radix(&group[ix..ix + 2], 16).unwrap() as f64)?;
            }
            any = true;
        }
        if !any {
            return self.err("Value expected.");
        }
        Ok(false)
    }

    fn po_fill(&mut self) -> R<bool> {
        let count = self.expr()?;
        let n = if count.is_undef() { 0. } else { self.to_int(&count)? };
        let mut fill = 0.;
        self.ws();
        if self.peek(0) == ',' {
            self.bump(1);
            let fv = self.expr()?;
            fill = self.to_int(&fv)?;
            if !fv.is_undef() && !(-128. ..=255.).contains(&fill) {
                self.error("Number out of range.")?;
            }
        }
        if n < 0. {
            return self.err("Negative size argument.");
        }
        if n > 65536. && n > (self.top + 1) as f64 {
            return self.err("Produced too much code.");
        }
        for _ in 0..n as i64 {
            self.emit(fill)?;
        }
        Ok(false)
    }

    fn po_skip(&mut self) -> R<bool> {
        let v = self.expr()?;
        let n = if v.is_undef() { 0. } else { self.to_int(&v)? };
        if n < 0. {
            return self.err("Negative size argument.");
        }
        self.pc_val()?;
        self.realpc = self.realpc.map(|pc| pc + n as i64);
        Ok(false)
    }

    fn po_align(&mut self) -> R<bool> {
        let v = self.expr()?;
        let and = int32(self.to_int(&v)?);
        self.ws();
        if self.peek(0) != ',' {
            return self.err("Missing comma.");
        }
        self.bump(1);
        let v = self.expr()?;
        let eq = int32(self.to_int(&v)?);
        let mut fill = if matches!(self.cpu_key, "z80" | "m68k") { 0. } else { 234. };
        self.ws();
        if self.peek(0) == ',' {
            self.bump(1);
            let v = self.expr()?;
            fill = self.to_int(&v)?;
        }
        if eq & !and != 0 {
            return self.err("\"!align\" can never be satisfied (equal value has bits outside the AND value).");
        }
        let mut guard = 0;
        while (int32(self.pc_val()? as f64) & and) != eq && guard < 0x10000 {
            guard += 1;
            self.emit(fill)?;
        }
        Ok(false)
    }

    fn po_text(&mut self, mode: Option<Convtab>, xor: i64) -> R<bool> {
        self.arg_list(|asm, v| {
            if let Val::S(s) = &v {
                for ch in s.chars() {
                    let c = ch as u32;
                    let code = match &mode {
                        Some(Convtab::Pet) => conv_pet(c),
                        Some(Convtab::Scr) => conv_scr(c),
                        Some(_) => c,
                        None => asm.conv(c),
                    };
                    asm.emit(((code as i64 & 0xff) ^ xor) as f64)?;
                }
                return Ok(());
            }
            let n = asm.numify(v)?;
            let x = n.v.trunc();
            if !n.u && !(-128. ..=255.).contains(&x) {
                asm.error("Number out of range.")?;
            }
            asm.emit((int32(x) ^ xor) as f64)
        })?;
        Ok(false)
    }

    /// A pseudo opcode for a block or for the rest of the file: `set` changes the state and gives
    /// the old one, which `undo` puts back after the block.
    fn with_block<S>(&mut self, set: impl FnOnce(&mut Self) -> R<S>, undo: impl FnOnce(&mut Self, S)) -> R<bool> {
        self.ws();
        if self.peek(0) == '{' {
            let block = self.scan_block()?;
            let old = set(self)?;
            let result = self.run_block(&block);
            undo(self, old);
            result?;
            return Ok(true);
        }
        set(self)?;
        Ok(false)
    }

    fn po_convtab(&mut self) -> R<bool> {
        self.ws();
        let tab = if matches!(self.peek(0), '"' | '<') {
            let (name, lib) = self.read_file_name()?;
            let from = self.r().src.file.clone();
            let Some(path) = self.resolve(&from, &name, lib) else {
                return self.err(format!("Cannot open input file \"{name}\"."));
            };
            let data = &self.files[&path];
            if data.len() < 256 {
                return self.err("Conversion table file is shorter than 256 bytes.");
            }
            Convtab::Table(Rc::new(data[..256].to_vec()))
        } else {
            match self.read_word().to_lowercase().as_str() {
                "pet" => Convtab::Pet,
                "raw" => Convtab::Raw,
                "scr" => Convtab::Scr,
                _ => return self.err("Unknown encoding."),
            }
        };
        self.with_block(
            |asm| Ok(std::mem::replace(&mut asm.convtab, tab)),
            |asm, old| asm.convtab = old,
        )
    }

    fn po_binary(&mut self) -> R<bool> {
        let (name, lib) = self.read_file_name()?;
        let mut size = None;
        let mut skip = 0.;
        self.ws();
        if self.peek(0) == ',' {
            self.bump(1);
            self.ws();
            if self.peek(0) != ',' && !self.eos() {
                let v = self.expr()?;
                size = Some(if v.is_undef() { 0. } else { self.to_int(&v)? });
            }
            self.ws();
            if self.peek(0) == ',' {
                self.bump(1);
                let v = self.expr()?;
                skip = if v.is_undef() { 0. } else { self.to_int(&v)? };
            }
        }
        if size.is_some_and(|s| s < 0.) || skip < 0. {
            return self.err("Negative size argument.");
        }
        let from = self.r().src.file.clone();
        let Some(path) = self.resolve(&from, &name, lib) else {
            return self.err(format!("Cannot open input file \"{name}\"."));
        };
        let data = self.files[&path].clone();
        let avail = &data[(skip as usize).min(data.len())..];
        let n = size.unwrap_or(avail.len() as f64);
        if n > 65536. && n > (self.top + 1) as f64 {
            return self.err("Produced too much code.");
        }
        for i in 0..n as usize {
            self.emit(avail.get(i).copied().unwrap_or(0) as f64)?;
        }
        Ok(false)
    }

    fn po_source(&mut self) -> R<bool> {
        let (name, lib) = self.read_file_name()?;
        let from = self.r().src.file.clone();
        let Some(path) = self.resolve(&from, &name, lib) else {
            return self.err(format!("Cannot open input file \"{name}\"."));
        };
        self.include_file(&path)?;
        Ok(true)
    }

    fn po_macro(&mut self) -> R<bool> {
        self.ws();
        let name = self.read_sym_name()?;
        let mut params = Vec::new();
        self.ws();
        while self.peek(0) != '{' && !self.eos() {
            let mut by_ref = false;
            if self.peek(0) == '~' {
                by_ref = true;
                self.bump(1);
                self.ws();
            }
            params.push(Param { by_ref, name: self.read_sym_name()? });
            self.ws();
            if self.peek(0) == ',' {
                self.bump(1);
                self.ws();
            } else {
                break;
            }
        }
        let block = self.scan_block()?;
        if let Some(old) = self.macros.get(&name)
            && old.pass == self.pass
            && !(old.block.src.file == block.src.file && old.block.start == block.start)
        {
            return self.err(format!("Macro already defined ({name})."));
        }
        self.macros.insert(name, Macro { params, block, pass: self.pass });
        Ok(true)
    }

    fn macro_call(&mut self) -> R<bool> {
        self.bump(1);
        let name = self.read_sym_name()?;
        let mut args = Vec::new();
        if !self.eos() {
            loop {
                self.ws();
                if self.peek(0) == '~' {
                    self.bump(1);
                    let sym_name = self.read_sym_name()?;
                    args.push(MacroArg::Ref(self.sym_key(&sym_name), sym_name));
                } else {
                    args.push(MacroArg::Value(self.expr()?));
                }
                self.ws();
                if self.peek(0) != ',' {
                    break;
                }
                self.bump(1);
            }
        }
        let Some(m) = self.macros.get(&name) else {
            self.undef_count += 1;
            return self.err(format!("Macro not defined ({name})."));
        };
        if m.params.len() != args.len() {
            return self.err(format!(
                "Wrong number of macro arguments for \"{name}\" (expected {}, got {}).",
                m.params.len(),
                args.len()
            ));
        }
        if self.depth >= MAX_DEPTH {
            return self.err("Too deeply nested. Recursive macro calls?");
        }
        let params = m.params.iter().map(|p| (p.by_ref, p.name.clone())).collect::<Vec<_>>();
        let block = m.block.clone();
        let ref_syms = args
            .iter()
            .map(|arg| match arg {
                MacroArg::Ref(key, sym_name) => Some(
                    self.symbols
                        .entry(key.clone())
                        .or_insert_with(|| Self::new_sym(key, sym_name))
                        .clone(),
                ),
                MacroArg::Value(_) => None,
            })
            .collect::<Vec<_>>();
        let (saved_zone, saved_cheap) = (self.zone.clone(), self.cheap.clone());
        self.zone = self.new_zone(name.clone());
        self.cheap = self.new_cheap(name.clone());
        self.depth += 1;
        self.macro_depth += 1;
        let result = (|| -> R<()> {
            for (i, ((by_ref, param), arg)) in params.iter().zip(args).enumerate() {
                let arg_by_ref = matches!(arg, MacroArg::Ref(..));
                if *by_ref != arg_by_ref {
                    return self.err(if *by_ref {
                        format!("Macro parameter {} of \"{name}\" is call-by-reference, use \"~\".", i + 1)
                    } else {
                        format!("Macro parameter {} of \"{name}\" is call-by-value, remove \"~\".", i + 1)
                    });
                }
                match arg {
                    MacroArg::Ref(..) => {
                        let key = self.sym_key(param);
                        self.symbols.insert(key.clone(), ref_syms[i].clone().unwrap());
                        self.alias_keys.insert(key);
                    }
                    MacroArg::Value(v) => self.define_sym(param, v, Kind::Set)?,
                }
            }
            self.run_block(&block)
        })();
        self.depth -= 1;
        self.macro_depth -= 1;
        self.zone = saved_zone;
        self.cheap = saved_cheap;
        result?;
        Ok(false)
    }

    fn dry_expr(&mut self) -> R<Val> {
        self.dry += 1;
        let result = self.expr();
        self.dry -= 1;
        result
    }

    fn else_chain(&mut self, mut done: bool) -> R<()> {
        loop {
            let saved = self.r().save();
            self.ws();
            if !self.r().keyword("else") {
                self.rm().restore(saved);
                return Ok(());
            }
            self.bump(4);
            self.ws();
            let mut cond = true;
            let mut has_cond = false;
            if self.r().keyword("if") {
                self.bump(2);
                has_cond = true;
                if done {
                    self.dry_expr()?;
                } else {
                    let v = self.expr()?;
                    cond = Self::truthy(&v);
                }
            } else if self.r().keyword("ifdef") || self.r().keyword("ifndef") {
                let want = self.r().keyword("ifdef");
                self.bump(if want { 5 } else { 6 });
                has_cond = true;
                let n = self.read_sym_name()?;
                cond = self.is_defined(&n) == want;
            }
            let block = self.scan_block()?;
            if !done && cond {
                self.run_block(&block)?;
                done = true;
            }
            if !has_cond {
                return Ok(());
            }
        }
    }

    fn po_if(&mut self) -> R<bool> {
        let v = self.expr()?;
        let cond = Self::truthy(&v);
        let block = self.scan_block()?;
        if cond {
            self.run_block(&block)?;
        }
        self.else_chain(cond)?;
        Ok(true)
    }

    fn po_ifdef(&mut self, want: bool) -> R<bool> {
        let name = self.read_sym_name()?;
        let cond = self.is_defined(&name) == want;
        self.ws();
        if self.peek(0) == '{' {
            let block = self.scan_block()?;
            if cond {
                self.run_block(&block)?;
            }
            self.else_chain(cond)?;
            return Ok(true);
        }
        if cond {
            if self.eos() {
                return Ok(false);
            }
            return self.statement(false, false);
        }
        self.skip_statement();
        Ok(false)
    }

    fn po_for(&mut self) -> R<bool> {
        let name = self.read_sym_name()?;
        self.ws();
        let mut values = None;
        let (mut start, mut end) = (0., 0.);
        if self.r().keyword("in") {
            self.bump(2);
            values = Some(match self.expr()? {
                Val::L(l) => l,
                Val::S(s) => s.chars().map(|c| Val::S(c.to_string())).collect(),
                v if v.is_undef() => Vec::new(),
                _ => return self.err("\"!for ... in\" needs a list or a string."),
            });
        } else {
            if self.peek(0) != ',' {
                return self.err("Missing comma.");
            }
            self.bump(1);
            let a = self.expr()?;
            self.ws();
            if self.peek(0) == ',' {
                self.bump(1);
                let b = self.expr()?;
                start = self.to_int(&a)?;
                end = self.to_int(&b)?;
            } else {
                self.warn("Found old \"!for\" syntax.");
                end = self.to_int(&a)?;
                let mut list = Vec::new();
                let mut i = 1.;
                while i <= end && i <= (LOOP_LIMIT + 1) as f64 {
                    list.push(Val::N(Num::new(i)));
                    i += 1.;
                }
                values = Some(list);
            }
        }
        let block = self.scan_block()?;
        let mut iterations = 0;
        let mut guard = |asm: &Self| -> R<()> {
            iterations += 1;
            if iterations > LOOP_LIMIT {
                return asm.err("Too many loop iterations.");
            }
            Ok(())
        };
        match values {
            Some(values) => {
                for v in values {
                    guard(self)?;
                    self.define_sym(&name, v, Kind::Set)?;
                    self.run_block(&block)?;
                }
            }
            None => {
                let step = if end >= start { 1. } else { -1. };
                let mut i = start;
                loop {
                    guard(self)?;
                    self.define_sym(&name, Val::N(Num::new(i)), Kind::Set)?;
                    self.run_block(&block)?;
                    if i == end {
                        break;
                    }
                    i += step;
                }
            }
        }
        Ok(true)
    }

    fn loop_cond(&mut self, forced_while: bool) -> R<Option<Cond>> {
        let saved = self.r().save();
        self.ws();
        let is_while = if forced_while {
            true
        } else if self.r().keyword("while") {
            self.bump(5);
            true
        } else if self.r().keyword("until") {
            self.bump(5);
            false
        } else {
            self.rm().restore(saved);
            return Ok(None);
        };
        let r = self.r();
        let block = Block { src: r.src.clone(), start: r.pos, end: r.end, line: r.line, line_start: r.line_start };
        self.dry_expr()?;
        Ok(Some(Cond { is_while, block }))
    }

    fn eval_cond(&mut self, cond: &Cond) -> R<bool> {
        self.readers.push(Reader::of(&cond.block));
        let result = self.expr();
        self.readers.pop();
        let t = Self::truthy(&result?);
        Ok(if cond.is_while { t } else { !t })
    }

    fn run_loop(&mut self, pre: Option<Cond>, block: Block, post: Option<Cond>) -> R<()> {
        let mut iterations = 0;
        loop {
            if let Some(pre) = &pre
                && !self.eval_cond(pre)?
            {
                return Ok(());
            }
            iterations += 1;
            if iterations > LOOP_LIMIT {
                return self.err("Too many loop iterations.");
            }
            self.run_block(&block)?;
            if let Some(post) = &post
                && !self.eval_cond(post)?
            {
                return Ok(());
            }
        }
    }

    fn po_do(&mut self) -> R<bool> {
        let pre = self.loop_cond(false)?;
        let block = self.scan_block()?;
        let post = self.loop_cond(false)?;
        self.run_loop(pre, block, post)?;
        Ok(true)
    }

    fn po_while(&mut self) -> R<bool> {
        let pre = self.loop_cond(true)?;
        let block = self.scan_block()?;
        self.run_loop(pre, block, None)?;
        Ok(true)
    }

    fn po_zone(&mut self) -> R<bool> {
        self.ws();
        let title = if is_id_start(self.peek(0)) {
            self.read_word()
        } else if self.peek(0) == '"' {
            self.bump(1);
            self.read_quoted('"')?
        } else {
            String::new()
        };
        self.with_block(
            |asm| {
                let zone = asm.new_zone(title);
                Ok(std::mem::replace(&mut asm.zone, zone))
            },
            |asm, old| asm.zone = old,
        )
    }

    fn po_pseudopc(&mut self) -> R<bool> {
        let v = self.expr()?;
        let n = if v.is_undef() { 0. } else { self.to_int(&v)? };
        if n < 0. || n > self.top as f64 {
            return self.err("Program counter out of range.");
        }
        self.with_block(
            |asm| {
                let Some(real) = asm.realpc else {
                    return asm.err("Program counter undefined.");
                };
                Ok(std::mem::replace(&mut asm.pc_offset, n as i64 - real))
            },
            |asm, old| asm.pc_offset = old,
        )
    }

    fn po_cpu(&mut self) -> R<bool> {
        self.ws();
        let name = self.read_word().to_lowercase();
        self.with_block(
            |asm| {
                let old = asm.cpu_name.clone();
                asm.set_cpu(&name)?;
                Ok(old)
            },
            |asm, old| {
                asm.set_cpu(&old).ok();
            },
        )
    }

    fn po_xor(&mut self) -> R<bool> {
        let v = self.expr()?;
        let n = self.to_int(&v)?;
        if !v.is_undef() && !(-128. ..=255.).contains(&n) {
            self.error("Number out of range.")?;
        }
        self.with_block(
            |asm| {
                let old = asm.xor_val;
                asm.xor_val ^= int32(n) & 0xff;
                Ok(old)
            },
            |asm, old| asm.xor_val = old,
        )
    }

    fn po_message(&mut self, kind: &str) -> R<bool> {
        let mut msg = String::new();
        if !self.eos() {
            self.arg_list(|_, v| {
                match v {
                    Val::S(s) => msg += &s,
                    Val::N(n) => msg += &if n.u { "<undefined>".into() } else { js_number(n.v) },
                    Val::L(l) => {
                        let items = l
                            .iter()
                            .map(|x| match x {
                                Val::S(s) => serde_json::to_string(s).unwrap(),
                                Val::N(n) => js_number(n.v),
                                Val::L(_) => "undefined".into(),
                            })
                            .collect::<Vec<_>>();
                        msg += &format!("[{}]", items.join(", "));
                    }
                }
                Ok(())
            })?;
        }
        match kind {
            "warn" => self.warn(msg),
            "error" => self.error(msg)?,
            _ if self.fin => {
                self.error(msg)?;
                return Err(Flow::Stop);
            }
            _ => {}
        }
        Ok(false)
    }

    fn po_address(&mut self) -> R<bool> {
        self.ws();
        if self.peek(0) == '{' {
            let block = self.scan_block()?;
            self.addr_mode += 1;
            let result = self.run_block(&block);
            self.addr_mode -= 1;
            result?;
            return Ok(true);
        }
        self.addr_mode += 1;
        let result = self.statement(false, false);
        self.addr_mode -= 1;
        result
    }
}

/// The addressing mode as the operand reads, before the table picks the opcode.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Parsed {
    Imp,
    Imm,
    Blk,
    Ildp,
    Ildpy,
    Izl,
    Izlq,
    Zprel,
    Indx,
    Sry,
    Izpz,
    Indy,
    Ind,
    Plain,
    X,
    Y,
    Sr,
}
