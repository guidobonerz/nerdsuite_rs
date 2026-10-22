//! An assembler compatible with ACME 0.97, ported from nerdsuite2's `acme.js`: the 6502 family
//! (6502, 6510 with the illegal opcodes, 65C02, R65C02, W65C02, 65816, 65CE02, 4502, the MEGA65's
//! 45GS02) and the Z80. The 68000 is not ported yet.
//!
//! ```text
//! assemble(main, files, opts) -> Assembly (never fails: the errors of the source are in it)
//! disassemble(bytes, addr, cpu) -> [Line]
//! ```

mod asm;
mod tables;
#[cfg(test)]
mod tests;
mod z80;

use std::collections::HashSet;

use asm::{Asm, Val, js_number};
pub use asm::{conv_pet, conv_scr};
pub use tables::{CPUS, Mode, op_bytes};

#[derive(Debug, Clone, PartialEq)]
pub enum Define {
    Num(f64),
    Str(String),
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Symbols defined before the source (`-D`).
    pub defines: Vec<(String, Define)>,
    /// The CPU before any `!cpu`, 6502 when none.
    pub cpu: Option<String>,
    /// The output format, before any `!to`: cbm, plain, apple, xex, amsdos, zx81, trs80, adf.
    pub format: Option<String>,
    /// The entry point (`!run`) and the xex INITAD (`!init`).
    pub run: Option<i64>,
    pub init: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub file: String,
    /// `None` for what belongs to no line (the output does not fit …).
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub msg: String,
}

impl Diagnostic {
    fn at(loc: &asm::Loc, msg: String) -> Diagnostic {
        Diagnostic { file: loc.file.clone(), line: Some(loc.line), col: Some(loc.col), msg }
    }

    fn of_file(file: &str, msg: String) -> Diagnostic {
        Diagnostic { file: file.to_owned(), line: None, col: None, msg }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub value: f64,
    pub file: Option<String>,
    pub line: usize,
    /// a label, or a value (`=`, `!set`)
    pub is_label: bool,
    /// the zone of a local (`.x`) or cheap (`@x`) symbol
    pub zone: Option<String>,
}

/// The bytes a line of a source made: the debugger's breakpoints and source lines.
#[derive(Debug, Clone, PartialEq)]
pub struct LineInfo {
    pub file: String,
    pub line: usize,
    pub addr: u32,
    pub size: usize,
    /// in the body of a macro
    pub in_macro: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Assembly {
    pub ok: bool,
    pub errors: Vec<Diagnostic>,
    pub warnings: Vec<Diagnostic>,
    pub output: Option<Vec<u8>>,
    /// the name of `!to`
    pub output_name: Option<String>,
    pub format: String,
    /// the lowest address written and the one after the highest
    pub start: u32,
    pub end: u32,
    pub symbols: Vec<Symbol>,
    pub lines: Vec<LineInfo>,
    /// VICE's label list (`al C:1000 .start`)
    pub vice_labels: String,
    pub listing: String,
    pub symbol_list: String,
    pub symbol_list_name: Option<String>,
    pub vice_labels_name: Option<String>,
    /// where the program starts: `!run`, else the lowest address
    pub exec: Option<u32>,
    pub cpu: String,
    pub passes: u32,
}

/// Assembles a project: `main` and the files it reads (`!source`, `!binary`), sources as UTF-8.
pub fn assemble(main: &str, files: Vec<(String, Vec<u8>)>, opts: &Options) -> Assembly {
    let format = opts.format.clone();
    let opts = opts.clone();
    let main = main.to_owned();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut asm = Asm::new(files, opts);
        asm.run(&main);
        result(&mut asm)
    }));
    result.unwrap_or_else(|panic| {
        let msg = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        Assembly {
            errors: vec![Diagnostic {
                file: main,
                line: Some(0),
                col: Some(0),
                msg: format!("Internal assembler error: {msg}"),
            }],
            format: format.unwrap_or_else(|| "cbm".into()),
            ..Default::default()
        }
    })
}

fn hex4(n: f64) -> String {
    format!("{:04x}", n.rem_euclid(4294967296.) as u64)
}

fn global_symbols(asm: &Asm) -> Vec<asm::SymRef> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (key, sym) in &asm.symbols {
        if !key.starts_with("G:") || !seen.insert(std::rc::Rc::as_ptr(sym)) {
            continue;
        }
        let s = sym.borrow();
        if s.def_pass != asm.pass || !matches!(&s.value, Some(Val::N(n)) if !n.u) {
            continue;
        }
        drop(s);
        out.push(sym.clone());
    }
    out.sort_by(|a, b| a.borrow().name.cmp(&b.borrow().name));
    out
}

fn num_of(sym: &asm::SymRef) -> asm::Num {
    sym.borrow().value.as_ref().unwrap().num()
}

fn vice_labels(asm: &Asm) -> String {
    let syms: Vec<_> = global_symbols(asm).into_iter().filter(|s| !num_of(s).f).collect();
    let line = |s: &asm::SymRef| format!("al C:{} .{}\n", hex4(num_of(s).v), s.borrow().name);
    let is_addr = |s: &asm::SymRef| num_of(s).a == 1;
    let mut out = String::new();
    for s in syms.iter().filter(|s| !is_addr(s) && !s.borrow().used) {
        out += &line(s);
    }
    out.push('\n');
    for s in syms.iter().filter(|s| !is_addr(s) && s.borrow().used) {
        out += &line(s);
    }
    out.push('\n');
    for s in syms.iter().filter(|s| is_addr(s)) {
        out += &line(s);
    }
    out
}

fn symbol_list(asm: &Asm) -> String {
    let mut out = String::new();
    for s in global_symbols(asm) {
        let v = num_of(&s);
        let value = if v.f {
            js_number(v.v)
        } else if v.a == 1 {
            format!("${}", hex4(v.v))
        } else {
            js_number(v.v)
        };
        let s = s.borrow();
        out += &format!("{}\t= {value}{}\n", s.name, if s.used { "" } else { "\t; ?" });
    }
    out
}

fn listing(asm: &Asm) -> String {
    // per file and line: the address, the first bytes and all bytes of the line
    let mut by_file: std::collections::HashMap<&str, indexmap::IndexMap<usize, (i64, i64, usize, usize)>> =
        Default::default();
    for l in asm.line_recs.iter().filter(|l| !l.in_macro) {
        let lines = by_file.entry(&l.file).or_default();
        lines
            .entry(l.line)
            .and_modify(|e| e.3 += l.size)
            .or_insert((l.addr, l.real, l.size, l.size));
    }
    let mut out = String::new();
    for file in &asm.source_order {
        out += &format!("\n; ******** Source: {file}\n");
        let text: String = asm.texts[file].iter().collect();
        let mut lines: Vec<&str> = text.split('\n').collect();
        if lines.len() > 1 && lines.last() == Some(&"") {
            lines.pop();
        }
        let recs = by_file.get(file.as_str());
        for (i, txt) in lines.iter().enumerate() {
            let (mut addr, mut hx) = ("    ".to_owned(), String::new());
            if let Some(&(a, real, size, total)) = recs.and_then(|m| m.get(&(i + 1))) {
                addr = format!("{:04x}", a);
                let n = size.min(8);
                for k in 0..n {
                    hx += &format!("{:02x}", asm.mem[((real + k as i64) & asm.top) as usize]);
                }
                if total > n {
                    hx += "...";
                }
            }
            out += &format!("{:>6}  {addr} {hx:<19} {txt}\n", i + 1);
        }
    }
    out
}

/// Atari XEX: $ffff, a segment per written area, then the INITAD and RUNAD segments.
fn build_xex(asm: &Asm) -> Vec<u8> {
    let mut out = vec![0xff, 0xff];
    let mut a = asm.min_addr;
    while a <= asm.max_addr {
        if !asm.written[a as usize] {
            a += 1;
            continue;
        }
        let mut e = a;
        while e < asm.max_addr && asm.written[(e + 1) as usize] {
            e += 1;
        }
        out.extend([a as u8, (a >> 8) as u8, e as u8, (e >> 8) as u8]);
        out.extend(&asm.mem[a as usize..=e as usize]);
        a = e + 1;
    }
    if let Some(init) = asm.init_addr {
        out.extend([0xe2, 0x02, 0xe3, 0x02, init as u8, (init >> 8) as u8]);
    }
    if let Some(run) = asm.run_addr {
        out.extend([0xe0, 0x02, 0xe1, 0x02, run as u8, (run >> 8) as u8]);
    }
    out
}

/// TRS-80 /CMD: a load block ($01, length + 2, address) per 252 bytes of each written area,
/// then the entry ($02 $02 address).
fn build_trs80(asm: &Asm) -> Vec<u8> {
    let mut out = Vec::new();
    let mut a = asm.min_addr;
    while a <= asm.max_addr {
        if !asm.written[a as usize] {
            a += 1;
            continue;
        }
        let mut e = a;
        while e < asm.max_addr && asm.written[(e + 1) as usize] && e + 1 - a < 252 {
            e += 1;
        }
        out.extend([0x01, (e - a + 3) as u8, a as u8, (a >> 8) as u8]);
        out.extend(&asm.mem[a as usize..=e as usize]);
        a = e + 1;
    }
    let exec = asm.run_addr.unwrap_or(asm.min_addr);
    out.extend([0x02, 0x02, exec as u8, (exec >> 8) as u8]);
    out
}

/// An Amstrad CPC AMSDOS binary: a header of 128 bytes, then the data.
fn build_amsdos(asm: &Asm, data: &[u8], start: i64) -> Vec<u8> {
    let mut h = [0u8; 128];
    let base = asm
        .out_name
        .clone()
        .unwrap_or_else(|| if asm.main.is_empty() { "NONAME".into() } else { asm.main.clone() });
    let base = base.rsplit('/').next().unwrap().to_uppercase();
    let (name, ext) = match base.rfind('.') {
        Some(dot) if dot > 0 => (&base[..dot], &base[dot + 1..]),
        _ => (base.as_str(), "BIN"),
    };
    let pad = |s: &str, n: usize| format!("{:<n$}", s.chars().take(n).collect::<String>());
    for (i, c) in pad(name, 8).chars().take(8).enumerate() {
        h[1 + i] = (c as u32 & 0x7f) as u8;
    }
    for (i, c) in pad(ext, 3).chars().take(3).enumerate() {
        h[9 + i] = (c as u32 & 0x7f) as u8;
    }
    let len = data.len();
    let exec = asm.run_addr.unwrap_or(start);
    h[0x12] = 2;
    h[0x13] = len as u8;
    h[0x14] = (len >> 8) as u8;
    h[0x15] = start as u8;
    h[0x16] = (start >> 8) as u8;
    h[0x18] = len as u8;
    h[0x19] = (len >> 8) as u8;
    h[0x1a] = exec as u8;
    h[0x1b] = (exec >> 8) as u8;
    h[0x40] = len as u8;
    h[0x41] = (len >> 8) as u8;
    h[0x42] = (len >> 16) as u8;
    let sum: u32 = h[..=66].iter().map(|&b| b as u32).sum();
    h[0x43] = sum as u8;
    h[0x44] = (sum >> 8) as u8;
    let mut out = h.to_vec();
    out.extend(data);
    out
}

/// An Amiga disk (ADF, 880K): the code in the boot block from its byte 12, then the root and
/// bitmap blocks of an empty OFS disk.
fn build_adf(asm: &mut Asm, data: &[u8]) -> Option<Vec<u8>> {
    const BLOCK: usize = 512;
    const BLOCKS: usize = 1760;
    const ROOT: usize = 880;
    let mut img = vec![0u8; BLOCKS * BLOCK];
    if data.len() > img.len() - 12 - 2 * BLOCK {
        asm.errors.push(Diagnostic::of_file(&asm.main, "The code does not fit the disk.".into()));
        return None;
    }
    if data.len() > 1012 {
        asm.warnings.push(Diagnostic::of_file(
            &asm.main,
            format!(
                "Kickstart loads 1012 bytes of the boot block's code, the program has {}: it loads the rest itself.",
                data.len()
            ),
        ));
    }
    let get = |img: &[u8], at: usize| u32::from_be_bytes(img[at..at + 4].try_into().unwrap());
    let set = |img: &mut [u8], at: usize, v: u32| img[at..at + 4].copy_from_slice(&v.to_be_bytes());
    img[0..4].copy_from_slice(b"DOS\0");
    set(&mut img, 8, ROOT as u32);
    img[12..12 + data.len()].copy_from_slice(data);
    // the boot block's checksum: the sum of its longs with the carries added back in, inverted
    let mut sum: u64 = 0;
    for i in (0..1024).step_by(4) {
        sum += get(&img, i) as u64;
        if sum > 0xffff_ffff {
            sum -= 0xffff_ffff;
        }
    }
    set(&mut img, 4, !(sum as u32));
    let block_sum = |img: &mut [u8], at: usize, field: usize| {
        let s = (0..BLOCK).step_by(4).fold(0u32, |s, i| s.wrapping_add(get(img, at + i)));
        set(img, at + field, 0u32.wrapping_sub(s));
    };
    let root = ROOT * BLOCK;
    set(&mut img, root, 2); // T_HEADER
    set(&mut img, root + 12, 72); // the size of the hash table
    set(&mut img, root + 312, 0xffff_ffff); // the bitmap is valid
    set(&mut img, root + 316, ROOT as u32 + 1); // the bitmap block
    let out_name = asm.out_name.clone().unwrap_or_else(|| "nerdsuite".into());
    let mut name = out_name.rsplit('/').next().unwrap().to_owned();
    if name.to_lowercase().ends_with(".adf") {
        name.truncate(name.len() - 4);
    }
    let mut name: String = name.chars().take(30).collect();
    if name.is_empty() {
        name = "nerdsuite".into();
    }
    img[root + 432] = name.chars().count() as u8;
    for (i, c) in name.chars().enumerate() {
        img[root + 433 + i] = c as u32 as u8;
    }
    set(&mut img, root + 508, 1); // ST_ROOT
    block_sum(&mut img, root, 20);
    // the bitmap: a set bit is a free block (from block 2); the boot code, the root and the bitmap are used
    let bm = (ROOT + 1) * BLOCK;
    let mut used: HashSet<usize> = [ROOT, ROOT + 1].into();
    let mut b = 2;
    while b * BLOCK < 12 + data.len() {
        used.insert(b);
        b += 1;
    }
    for b in 2..BLOCKS {
        if !used.contains(&b) {
            img[bm + 4 + ((b - 2) >> 5) * 4 + 3 - (((b - 2) & 31) >> 3)] |= 1 << ((b - 2) & 7);
        }
    }
    block_sum(&mut img, bm, 0);
    Some(img)
}

fn result(asm: &mut Asm) -> Assembly {
    let ok = asm.errors.is_empty();
    let format = asm
        .opts()
        .format
        .clone()
        .or_else(|| asm.out_format.clone())
        .unwrap_or_else(|| "cbm".into());
    let mut output = None;
    let (mut start, mut end) = (0i64, 0i64);
    if asm.max_addr >= asm.min_addr {
        start = asm.min_addr;
        end = asm.max_addr + 1;
        if ok {
            let len = (end - start) as usize;
            let data: Vec<u8> = (0..len)
                .map(|i| {
                    let a = start as usize + i;
                    if asm.written[a] { asm.mem[a] } else { asm.initmem }
                })
                .collect();
            output = match format.as_str() {
                "xex" => Some(build_xex(asm)),
                "amsdos" => Some(build_amsdos(asm, &data, start)),
                "trs80" => Some(build_trs80(asm)),
                "adf" => build_adf(asm, &data),
                "zx81" => {
                    // the code in the REM line of a .p file: *= $4082
                    let exec = asm.run_addr.unwrap_or(start);
                    match crate::basic::zx81::code_p(&data, start as u16, exec as u16) {
                        Ok(p) => Some(p),
                        Err(msg) => {
                            asm.errors.push(Diagnostic::of_file(&asm.main, msg));
                            None
                        }
                    }
                }
                _ => {
                    let mut out = match format.as_str() {
                        "cbm" => vec![start as u8, (start >> 8) as u8],
                        "apple" => vec![start as u8, (start >> 8) as u8, len as u8, (len >> 8) as u8],
                        _ => Vec::new(),
                    };
                    out.extend(data);
                    Some(out)
                }
            };
        }
    }
    let exec = asm.run_addr.or((end > start).then_some(start));

    let mut seen = HashSet::new();
    let mut symbols = Vec::new();
    for sym in asm.symbols.values() {
        if !seen.insert(std::rc::Rc::as_ptr(sym)) {
            continue;
        }
        let s = sym.borrow();
        let Some(Val::N(v)) = &s.value else { continue };
        if s.anon || s.in_macro || s.def_pass != asm.pass || v.u {
            continue;
        }
        symbols.push(Symbol {
            name: s.name.clone(),
            value: v.v,
            file: s.file.clone(),
            line: s.line,
            is_label: s.kind == Some(asm::Kind::Label),
            zone: (s.name.starts_with('.') || s.name.starts_with('@')).then(|| s.zone.clone()),
        });
    }
    symbols.sort_by(|a, b| {
        a.name.cmp(&b.name).then_with(|| {
            a.zone.clone().unwrap_or_default().cmp(&b.zone.clone().unwrap_or_default())
        })
    });

    let lines = asm
        .line_recs
        .iter()
        .map(|l| LineInfo {
            file: l.file.clone(),
            line: l.line,
            addr: l.addr as u32,
            size: l.size,
            in_macro: l.in_macro,
        })
        .collect();

    Assembly {
        ok,
        errors: asm.errors.clone(),
        warnings: asm.warnings.clone(),
        output,
        output_name: asm.out_name.clone(),
        format,
        start: start as u32,
        end: end as u32,
        symbols,
        lines,
        vice_labels: vice_labels(asm),
        listing: listing(asm),
        symbol_list: symbol_list(asm),
        symbol_list_name: asm.sl_name.clone(),
        vice_labels_name: asm.svl_name.clone(),
        exec: exec.map(|e| e as u32),
        cpu: asm.cpu_name.clone(),
        passes: asm.pass,
    }
}

// ------------------------------------------------------------------------------ disassembler

/// The 65816's register sizes: 8 bit accumulator (`m8`) and index registers (`x8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    pub m8: bool,
    pub x8: bool,
}

impl Default for Flags {
    fn default() -> Self {
        Flags { m8: true, x8: true }
    }
}

/// A disassembled instruction (or `!byte` for none).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub addr: u32,
    pub bytes: Vec<u8>,
    pub text: String,
    /// the 65816: the sizes it was read with
    pub flags: Option<Flags>,
}

/// Disassembles bytes from `addr`; the 65816 follows `rep` and `sep` from `flags`.
pub fn disassemble(bytes: &[u8], addr: u32, cpu: &str, mut flags: Flags) -> Vec<Line> {
    let key = tables::canonical_cpu(&cpu.to_lowercase()).unwrap_or("6502");
    if key == "z80" {
        return z80::disassemble(bytes, addr as i64);
    }
    let Some(table) = CPUS.get(key) else {
        // TODO: the 68000
        return bytes
            .iter()
            .enumerate()
            .map(|(i, &b)| Line { addr: addr + i as u32, bytes: vec![b], text: format!("!byte ${b:02x}"), flags: None })
            .collect();
    };
    let h2 = |n: u8| format!("${n:02x}");
    let h4 = |n: i64| format!("${:04x}", n & 0xffff);
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let a = (addr as i64 + i as i64) & 0xffff;
        let op = bytes[i];
        let mut entry = table.reverse[op as usize].clone();
        let mut prefixes = 0;
        // the 45GS02: $42 $42 ($ea) and $ea before an opcode - the longest prefix that makes an instruction
        if !table.prefixed.is_empty() {
            for n in [3, 2, 1] {
                if i + n >= bytes.len() {
                    continue;
                }
                let code = (0..=n).fold(0u32, |code, k| code * 256 + bytes[i + k] as u32);
                if let Some(e) = table.prefixed.get(&code) {
                    entry = Some(e.clone());
                    prefixes = n;
                    break;
                }
            }
        }
        let wide = key == "65816"
            && entry.as_ref().is_some_and(|(m, mode)| {
                *mode == Mode::Imm
                    && ((tables::IMM_A.contains(&m.as_str()) && !flags.m8)
                        || (tables::IMM_X.contains(&m.as_str()) && !flags.x8))
            });
        let len = match &entry {
            Some(_) if wide => 3,
            Some((_, mode)) => mode.len() + prefixes,
            None => 1,
        };
        let Some((m, mode)) = entry.filter(|_| i + len <= bytes.len()) else {
            out.push(Line { addr: a as u32, bytes: vec![op], text: format!("!byte {}", h2(op)), flags: None });
            i += 1;
            continue;
        };
        let all = bytes[i..i + len].to_vec();
        let b = &all[prefixes..];
        let b1 = b.get(1).copied().unwrap_or(0);
        let b2 = b.get(2).copied().unwrap_or(0);
        let w = b1 as i64 | (b2 as i64) << 8;
        let long = || format!("${:06x}", w | (b[3] as i64) << 16);
        let rel = |o: u8, base: i64| h4(base + o as i8 as i64);
        let rel16 = |base: i64| h4(base + if w < 32768 { w } else { w - 65536 });
        let operand = match mode {
            Mode::Imp => String::new(),
            Mode::Imm if wide => format!("#{}", h4(w)),
            Mode::Imm => format!("#{}", h2(b1)),
            Mode::Zp => h2(b1),
            Mode::Zpx => format!("{},x", h2(b1)),
            Mode::Zpy => format!("{},y", h2(b1)),
            Mode::Abs => h4(w),
            Mode::Absx => format!("{},x", h4(w)),
            Mode::Absy => format!("{},y", h4(w)),
            Mode::Ind => format!("({})", h4(w)),
            Mode::Indx => format!("({},x)", h2(b1)),
            Mode::Indy => format!("({}),y", h2(b1)),
            Mode::Izp => format!("({})", h2(b1)),
            Mode::Iax => format!("({},x)", h4(w)),
            Mode::Rel => rel(b1, a + 2),
            Mode::Zprel => format!("{},{}", h2(b1), rel(b2, a + 3)),
            Mode::Sr => format!("{},s", h2(b1)),
            Mode::Sry => format!("({},s),y", h2(b1)),
            Mode::Ildp => format!("[{}]", h2(b1)),
            Mode::Ildpy => format!("[{}],y", h2(b1)),
            Mode::Long => long(),
            Mode::Longx => format!("{},x", long()),
            Mode::Ialong => format!("[{}]", h4(w)),
            Mode::Rell => rel16(a + 3),
            Mode::Blk => format!("{},{}", h2(b2), h2(b1)),
            Mode::Izpz => format!("({}),z", h2(b1)),
            Mode::Imm16 => format!("#{}", h4(w)),
            Mode::Rel16 => rel16(a + 2),
            Mode::Izl => format!("[{}],z", h2(b1)),
            Mode::Izlq => format!("[{}]", h2(b1)),
        };
        let text = if operand.is_empty() { m.clone() } else { format!("{m} {operand}") };
        let mut line = Line { addr: a as u32, bytes: all.clone(), text, flags: None };
        if key == "65816" {
            line.flags = Some(flags);
            if m == "rep" {
                flags.m8 &= b1 & 0x20 == 0;
                flags.x8 &= b1 & 0x10 == 0;
            }
            if m == "sep" {
                flags.m8 |= b1 & 0x20 != 0;
                flags.x8 |= b1 & 0x10 != 0;
            }
        }
        out.push(line);
        i += len;
    }
    out
}
