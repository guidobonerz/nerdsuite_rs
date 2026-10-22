//! The analysis of the Disassembler view, as nerdsuite2's `disasm.js`: which bytes of a binary are
//! code and which are data, the lines of the disassembly with labels, and the disassembly as ACME
//! source that assembles to the same bytes again.
//!
//! The user marks ranges as code or as data of a type. In auto mode the rest is found by tracing the
//! code from its entry points (the SYS of a BASIC line, the load address or the vectors of a
//! cartridge, the entries the user adds, the starts of the code regions): what the trace reaches is
//! code, the runs of printable bytes are text, the rest bytes. Without auto mode every byte that is
//! not marked is code. The addresses of the machine (`memmap`) name the operands - `sta VIC_BORDER` -
//! and a value stored to a register is told in words.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{
    acme::{self, Flags, conv_pet, conv_scr},
    basic, memmap,
    platform::{self, Family},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    Code,
    Bytes,
    Words,
    Text,
    Petscii,
    Screen,
    Chars,
    Sprites,
    Bitmap,
    Tiles,
    Basic,
}

impl DataType {
    /// The types the user can mark, in the order of the menu.
    pub const MARKS: [DataType; 10] = [
        DataType::Code,
        DataType::Bytes,
        DataType::Words,
        DataType::Text,
        DataType::Petscii,
        DataType::Screen,
        DataType::Chars,
        DataType::Sprites,
        DataType::Bitmap,
        DataType::Tiles,
    ];

    pub fn name(self) -> &'static str {
        match self {
            DataType::Code => "Code",
            DataType::Bytes => "Bytes",
            DataType::Words => "Words",
            DataType::Text => "Text (ASCII)",
            DataType::Petscii => "Text (PETSCII)",
            DataType::Screen => "Screen codes",
            DataType::Chars => "Chars",
            DataType::Sprites => "Sprites",
            DataType::Bitmap => "Bitmap",
            DataType::Tiles => "Tiles (NES, SNES)",
            DataType::Basic => "BASIC",
        }
    }

    /// The id in `nerdsuite.json` (the marks of the Disassembler).
    pub fn id(self) -> &'static str {
        match self {
            DataType::Code => "code",
            DataType::Bytes => "bytes",
            DataType::Words => "words",
            DataType::Text => "text",
            DataType::Petscii => "petscii",
            DataType::Screen => "screen",
            DataType::Chars => "chars",
            DataType::Sprites => "sprites",
            DataType::Bitmap => "bitmap",
            DataType::Tiles => "tiles",
            DataType::Basic => "basic",
        }
    }

    pub fn from_id(id: &str) -> Option<DataType> {
        [DataType::Basic].into_iter().chain(DataType::MARKS).find(|t| t.id() == id)
    }

    /// The bytes a line of the data holds.
    fn per(self) -> usize {
        match self {
            DataType::Text | DataType::Petscii | DataType::Screen => 32,
            DataType::Chars => 1,
            DataType::Sprites => 3,
            _ => 8,
        }
    }
}

/// A range the user marked: offsets into the bytes, the end included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub start: usize,
    pub end: usize,
    pub ty: DataType,
}

/// The regions with [a, b] marked as `ty` (`None`: unmarked): what overlaps is cut away, neighbours
/// of the same type are joined.
pub fn mark_range(regions: &[Region], a: usize, b: usize, ty: Option<DataType>) -> Vec<Region> {
    let (a, b) = if a > b { (b, a) } else { (a, b) };
    let mut out = Vec::new();
    for r in regions {
        if r.end < a || r.start > b {
            out.push(*r);
            continue;
        }
        if r.start < a {
            out.push(Region { end: a - 1, ..*r });
        }
        if r.end > b {
            out.push(Region { start: b + 1, ..*r });
        }
    }
    if let Some(ty) = ty {
        out.push(Region { start: a, end: b, ty });
    }
    out.sort_by_key(|r| r.start);
    let mut joined: Vec<Region> = Vec::new();
    for r in out {
        match joined.last_mut() {
            Some(last) if last.ty == r.ty && last.end + 1 == r.start => last.end = r.end,
            _ => joined.push(r),
        }
    }
    joined
}

// --- BASIC ----------------------------------------------------------------------------------------

/// The BASIC starts of the Commodores (the C128 with a graphics area at $4001).
const BASIC_STARTS: [u32; 6] = [0x0401, 0x0801, 0x1001, 0x1201, 0x1c01, 0x4001];

#[derive(Debug, Clone, PartialEq)]
pub struct BasicLine {
    pub off: usize,
    pub len: usize,
    pub number: u32,
    pub text: String,
    pub sys: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BasicProgram {
    /// the offset after the link of 0
    pub end: usize,
    pub lines: Vec<BasicLine>,
    /// the addresses of its SYS
    pub sys: Vec<u32>,
}

/// A BASIC program at the start of the data: the chain of its lines - each linked to the next, its
/// number higher than the one before, ended by a 0 just before the next line - up to the link of 0.
/// Looked for at the BASIC starts, and at any address on a Commodore (`any`).
pub fn detect_basic(data: &[u8], base: u32, dialect: &str, any: bool) -> Option<BasicProgram> {
    if !any && !BASIC_STARTS.contains(&base) {
        return None;
    }
    let mut lines = Vec::new();
    let mut sys = Vec::new();
    let mut at = 0usize;
    let mut last: i64 = -1;
    for _ in 0..4000 {
        if at + 2 > data.len() {
            return None;
        }
        let link = data[at] as u32 | (data[at + 1] as u32) << 8;
        if link == 0 {
            return (!lines.is_empty()).then_some(BasicProgram { end: at + 2, lines, sys });
        }
        let next = link as i64 - base as i64;
        let number = *data.get(at + 2)? as u32 | (*data.get(at + 3)? as u32) << 8;
        if next <= at as i64 + 4 || next > data.len() as i64 || data[next as usize - 1] != 0 || number as i64 <= last || number > 63999 {
            return None;
        }
        let next = next as usize;
        let mut own = Vec::new();
        for k in at + 4..next - 1 {
            if data[k] == 0 {
                return None; // no 0 inside a line
            }
            if data[k] != 0x9e {
                continue; // SYS 2061, SYS(2061)
            }
            let mut d = k + 1;
            while matches!(data.get(d), Some(0x20 | 0x28)) {
                d += 1;
            }
            let mut digits = String::new();
            while let Some(&c @ 0x30..=0x39) = data.get(d) {
                digits.push(c as char);
                d += 1;
            }
            if let Ok(v) = digits.parse::<u32>()
                && v < 0x10000
            {
                own.push(v);
            }
        }
        let mut prg = vec![base as u8, (base >> 8) as u8];
        prg.extend(&data[at..next]);
        prg.extend([0, 0]);
        let text = basic::detokenize(&prg, dialect).unwrap_or_default().trim().to_owned();
        sys.extend(&own);
        lines.push(BasicLine { off: at, len: next - at, number, text, sys: own });
        last = number as i64;
        at = next;
    }
    None
}

/// The address of the first SYS of a BASIC program at `base`.
pub fn basic_sys(data: &[u8], base: u32) -> Option<u32> {
    detect_basic(data, base, "v2", false)?.sys.first().copied()
}

// --- the layout of a file -------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// the bytes before the code: a load address, a cartridge header
    pub head: usize,
    /// the address of the first byte after them
    pub base: u32,
    /// where what the CPU sees ends (the CHR ROM of the NES, the banks of the SNES after the first)
    pub split: usize,
    /// from the vectors of a cartridge
    pub entries: Vec<u32>,
    pub cpu: Option<&'static str>,
    pub note: Option<String>,
    pub machine: &'static str,
    /// a .prg (its head is the load address)
    pub prg: bool,
}

/// The layout of a file by its name: a .prg, a .nes, a .sfc/.smc or an .a26.
pub fn file_layout(name: &str, bytes: &[u8]) -> Option<Layout> {
    let n = bytes.len();
    let word = |o: usize| bytes[o] as u32 | (bytes[o + 1] as u32) << 8;
    let lower = name.to_lowercase();
    if lower.ends_with(".prg") && n > 2 {
        let machine = match word(0) {
            0x1c01 => "c128",
            0x1001 | 0x1201 => "vic20",
            0x0401 => "pet",
            _ => "c64",
        };
        return Some(Layout { head: 2, base: word(0), split: n - 2, entries: vec![], cpu: None, note: None, machine, prg: true });
    }
    if lower.ends_with(".nes") && n > 16 && &bytes[..4] == b"NES\x1a" {
        let trainer = if bytes[6] & 4 != 0 { 512 } else { 0 };
        let prg = bytes[4] as usize * 0x4000;
        let head = 16 + trainer;
        let size = prg.min(0x8000).min(n - head);
        let base = 0x10000 - size as u32;
        let v = head + prg.min(n - head) - 6; // the vectors at the end of the last bank
        let mut entries = Vec::new();
        for e in [word(v), word(v + 2), word(v + 4)] {
            if e >= base && !entries.contains(&e) {
                entries.push(e);
            }
        }
        let note = format!(
            "iNES: {} × 16K PRG, {} × 8K CHR, mapper {}",
            bytes[4],
            bytes[5],
            (bytes[6] >> 4) | (bytes[7] & 0xf0)
        );
        return Some(Layout { head, base, split: size, entries, cpu: Some("6502"), note: Some(note), machine: "nes", prg: false });
    }
    if (lower.ends_with(".sfc") || lower.ends_with(".smc")) && n >= 0x8000 {
        let head = if n % 1024 == 512 { 512 } else { 0 }; // a copier header
        let v = head + 0x7fe0;
        let size = 0x8000.min(n - head);
        let mut entries = Vec::new();
        for e in [word(v + 0x1c), word(v + 0x0a), word(v + 0x0e)] {
            // reset, native NMI, native IRQ
            if e >= 0x8000 && !entries.contains(&e) {
                entries.push(e);
            }
        }
        let note = format!("LoROM, {}K - the first bank at $8000", (n - head) >> 10);
        return Some(Layout { head, base: 0x8000, split: size, entries, cpu: Some("65816"), note: Some(note), machine: "snes", prg: false });
    }
    if lower.ends_with(".a26") && (2048..=4096).contains(&n) {
        let base = 0x10000 - n as u32;
        let entry = base + (word(n - 4) & (n as u32 - 1));
        return Some(Layout { head: 0, base, split: n, entries: vec![entry], cpu: Some("6502"), note: None, machine: "vcs", prg: false });
    }
    None
}

// --- one instruction ------------------------------------------------------------------------------

struct Ins {
    len: usize,
    text: String,
    mn: String,
    arg: String,
    /// the addresses of the operand: 16 bits, or 24 bits in bank $00 / $80
    addrs: Vec<u32>,
    jumps: Vec<u32>,
    stop: bool,
    before: Flags,
    after: Flags,
}

fn is_z80(cpu: &str) -> bool {
    cpu.eq_ignore_ascii_case("z80")
}

/// The `$hex` operands of a text: (start, end, `#` before it, the digits).
fn hex_operands(text: &str) -> Vec<(usize, usize, bool, &str)> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' {
            let mut k = i + 1;
            while k < b.len() && b[k].is_ascii_hexdigit() {
                k += 1;
            }
            if k > i + 1 {
                let hash = i > 0 && b[i - 1] == b'#';
                out.push((if hash { i - 1 } else { i }, k, hash, &text[i + 1..k]));
                i = k;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn decode(data: &[u8], off: usize, base: u32, cpu: &str, flags: Flags) -> Option<Ins> {
    let end = (off + 5).min(data.len());
    let ins = acme::disassemble(&data[off..end], (base + off as u32) & 0xffff, cpu, flags).into_iter().next()?;
    if ins.text.starts_with("!byte") {
        return None;
    }
    let text = ins.text;
    let mn = text.split(' ').next().unwrap_or_default().to_lowercase();
    let arg = text[mn.len()..].trim().to_owned();
    let addrs: Vec<u32> = hex_operands(&text)
        .into_iter()
        .filter(|(_, _, hash, h)| !hash && h.len() >= 4)
        .filter_map(|(_, _, _, h)| {
            let v = u32::from_str_radix(h, 16).ok()?;
            if h.len() == 6 { ((v >> 16) & 0x7f == 0).then_some(v & 0xffff) } else { Some(v) }
        })
        .collect();
    let target = addrs.last().copied();
    let mut stop = false;
    let mut jumps = Vec::new();
    let is = |set: &[&str]| set.contains(&mn.as_str());
    if is_z80(cpu) {
        let cond = arg.contains(',');
        match mn.as_str() {
            "jp" => {
                if arg.contains('(') {
                    stop = true;
                } else {
                    jumps.extend(target);
                    stop = !cond;
                }
            }
            "jr" | "djnz" | "call" => {
                jumps.extend(target);
                stop = mn == "jr" && !cond;
            }
            "ret" => stop = arg.is_empty(),
            "reti" | "retn" => stop = true,
            _ => {}
        }
    } else if is(&["bcc", "bcs", "beq", "bmi", "bne", "bpl", "bvc", "bvs", "jsr", "jsl", "bsr", "lbcc", "lbcs", "lbeq", "lbmi", "lbne", "lbpl", "lbvc", "lbvs"])
        || ((mn.starts_with("bbr") || mn.starts_with("bbs")) && mn.len() == 4 && mn.as_bytes()[3].is_ascii_digit())
    {
        jumps.extend(target);
    } else if is(&["jmp", "jml"]) {
        if !arg.starts_with(['(', '[']) {
            jumps.extend(target);
        }
        stop = true;
    } else if is(&["bra", "brl", "lbra"]) {
        jumps.extend(target);
        stop = true;
    } else if is(&["rts", "rti", "rtl", "rtn", "brk", "stp"]) {
        stop = true;
    }
    // the 65816 follows rep and sep
    let mut after = flags;
    if cpu == "65816" && let Some(&p) = ins.bytes.get(1) {
        if mn == "rep" {
            after.m8 &= p & 0x20 == 0;
            after.x8 &= p & 0x10 == 0;
        }
        if mn == "sep" {
            after.m8 |= p & 0x20 != 0;
            after.x8 |= p & 0x10 != 0;
        }
    }
    Some(Ins { len: ins.bytes.len(), text, mn, arg, addrs, jumps, stop, before: flags, after })
}

// --- the classification ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub base: u32,
    pub cpu: String,
    pub machine: Option<String>,
    pub regions: Vec<Region>,
    pub entries: Vec<u32>,
    pub auto: bool,
    /// where what the CPU sees ends (the length of the data when `None`)
    pub split: Option<usize>,
    /// the base is no entry (a cartridge: its vectors are)
    pub no_base: bool,
    /// the names the user gave addresses
    pub names: HashMap<u32, String>,
}

impl Options {
    pub fn new(base: u32, cpu: &str) -> Options {
        Options { base, cpu: cpu.to_owned(), auto: true, ..Default::default() }
    }
}

#[derive(Debug, Clone)]
pub struct Classified {
    pub types: Vec<DataType>,
    pub marked: Vec<bool>,
    /// the offsets where the trace found instructions, with the 65816 flags there
    pub starts: HashMap<usize, Flags>,
    /// the addresses the trace started from
    pub entries: Vec<u32>,
    pub basic: Option<BasicProgram>,
}

fn printable(b: u8) -> bool {
    (0x20..0x7f).contains(&b)
}

/// The type of every byte: code, or a type of data.
pub fn classify(data: &[u8], opts: &Options) -> Classified {
    let n = data.len();
    let split = opts.split.unwrap_or(n).min(n);
    let base = opts.base;
    let cpu = opts.cpu.as_str();
    let mut types: Vec<Option<DataType>> = vec![None; n];
    let mut marked = vec![false; n];
    for r in &opts.regions {
        for i in r.start..=r.end.min(n.saturating_sub(1)) {
            if i < n {
                types[i] = Some(r.ty);
                marked[i] = true;
            }
        }
    }
    for t in types.iter_mut().skip(split) {
        if *t == Some(DataType::Code) || t.is_none() {
            *t = if *t == Some(DataType::Code) || !opts.auto { Some(DataType::Bytes) } else { None };
        }
    }
    let mut starts = HashMap::new();
    // a BASIC program at the start (a Commodore's): its lines, its SYS the entries; no code at the load address
    let p = opts.machine.as_deref().and_then(platform::platform);
    let commodore = p.is_some_and(|p| p.family == Family::Commodore);
    let dialect = p.filter(|_| commodore).and_then(|p| p.basic.first().copied()).unwrap_or("v2");
    let basic = (!is_z80(cpu) && cpu != "65816").then(|| detect_basic(data, base, dialect, commodore)).flatten();
    if let Some(b) = &basic {
        for t in types.iter_mut().take(b.end) {
            t.get_or_insert(DataType::Basic);
        }
    }
    let sys: Vec<u32> = basic
        .as_ref()
        .map(|b| b.sys.iter().copied().filter(|&a| a >= base + b.end as u32 && a < base + split as u32).collect())
        .unwrap_or_default();
    let mut found: Vec<u32> = Vec::new();
    let base_entry = (basic.is_none() && !opts.no_base).then_some(base);
    for a in opts.entries.iter().copied().chain(sys).chain(base_entry) {
        if !found.contains(&a) {
            found.push(a);
        }
    }
    if !opts.auto {
        let types = types.into_iter().map(|t| t.unwrap_or(DataType::Code)).collect();
        return Classified { types, marked, starts, entries: found, basic };
    }
    let mut seen = vec![false; n];
    let mut queue: Vec<(i64, Flags)> = found.iter().map(|&a| (a as i64 - base as i64, Flags::default())).collect();
    queue.extend(opts.regions.iter().filter(|r| r.ty == DataType::Code).map(|r| (r.start as i64, Flags::default())));
    while let Some((mut off, mut flags)) = queue.pop() {
        while off >= 0 && (off as usize) < split && !starts.contains_key(&(off as usize)) {
            let o = off as usize;
            let Some(ins) = decode(data, o, base, cpu, flags) else { break };
            if o + ins.len > split {
                break;
            }
            if (0..ins.len).any(|k| marked[o + k] && types[o + k] != Some(DataType::Code)) {
                break;
            }
            starts.insert(o, ins.before);
            for k in 0..ins.len {
                seen[o + k] = true;
            }
            for &t in &ins.jumps {
                queue.push((t as i64 - base as i64, ins.after));
            }
            if ins.stop {
                break;
            }
            flags = ins.after;
            off += ins.len as i64;
        }
    }
    for i in 0..n {
        if !marked[i] && seen[i] && types[i] != Some(DataType::Basic) {
            types[i] = Some(DataType::Code);
        }
    }
    // the rest: runs of 6 printable bytes and more are text, else bytes
    let end = basic.as_ref().map_or(0, |b| b.end);
    let mut i = 0;
    while i < n {
        if types[i].is_some() {
            i += 1;
            continue;
        }
        let mut k = i;
        while k < n && types[k].is_none() && printable(data[k]) && (k != split || k == i) {
            k += 1;
        }
        let run = k - i >= 6 && i >= end && i < split;
        for t in types.iter_mut().take(k.max(i + 1)).skip(i) {
            *t = Some(if run { DataType::Text } else { DataType::Bytes });
        }
        i = k.max(i + 1);
    }
    let types = types.into_iter().map(|t| t.unwrap_or(DataType::Bytes)).collect();
    Classified { types, marked, starts, entries: found, basic }
}

// --- the lines ------------------------------------------------------------------------------------

/// A register or area of the machine an operand names.
#[derive(Debug, Clone, PartialEq)]
pub struct SymRef {
    pub text: String,
    pub name: String,
    pub desc: String,
    pub addr: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub off: usize,
    pub len: usize,
    pub addr: u32,
    pub ty: DataType,
    pub label: Option<String>,
    pub text: String,
    pub comment: Option<String>,
    /// the offset an operand (or a word, or a SYS) points to
    pub reference: Option<usize>,
    /// the register of the machine it names
    pub sym: Option<SymRef>,
    /// what is stored there
    pub value: Option<u32>,
    /// the 65816 flags it was read with
    pub flags: Option<Flags>,
    /// a BASIC line as the machine lists it
    pub view: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Disassembly {
    pub lines: Vec<Line>,
    pub labels: BTreeMap<u32, String>,
    /// the names of the machine it uses: name -> (address, description)
    pub symbols: BTreeMap<String, (u32, String)>,
    pub split: usize,
    pub classified: Classified,
}

impl Disassembly {
    pub fn types(&self) -> &[DataType] {
        &self.classified.types
    }

    /// The line that holds an offset.
    pub fn line_at(&self, off: usize) -> Option<usize> {
        let ix = self.lines.partition_point(|l| l.off <= off);
        ix.checked_sub(1)
    }
}

fn hex2(n: u32) -> String {
    format!("{:02x}", n & 0xff)
}

fn hex4(n: u32) -> String {
    format!("{:04x}", n)
}

/// The chars ACME writes for a byte in a text, the quote and the backslash left out.
fn text_maps() -> [HashMap<u8, char>; 3] {
    let invert = |conv: fn(u32) -> u32| {
        let mut map = HashMap::new();
        for c in 0x20u32..0x7f {
            if c != 0x22 && c != 0x5c {
                map.entry((conv(c) & 0xff) as u8).or_insert(char::from_u32(c).unwrap());
            }
        }
        map
    };
    let ascii = (0x20u8..0x7f).filter(|&c| c != 0x22 && c != 0x5c).map(|c| (c, c as char)).collect();
    [ascii, invert(conv_pet), invert(conv_scr)]
}

/// Bytes as an argument list of ACME: "the text", $0d, "more".
fn text_args(bytes: &[u8], map: &HashMap<u8, char>) -> String {
    let mut parts = Vec::new();
    let mut s = String::new();
    for b in bytes {
        if let Some(c) = map.get(b) {
            s.push(*c);
            continue;
        }
        if !s.is_empty() {
            parts.push(format!("\"{s}\""));
            s.clear();
        }
        parts.push(format!("${:02x}", b));
    }
    if !s.is_empty() {
        parts.push(format!("\"{s}\""));
    }
    parts.join(", ")
}

/// The values the registers hold, as far as the straight code shows them.
#[derive(Default, Clone, Copy)]
struct Regs {
    a: Option<u32>,
    x: Option<u32>,
    y: Option<u32>,
}

fn track(regs: &mut Regs, mn: &str, arg: &str, width: u32) {
    let v = arg.strip_prefix("#$").and_then(|h| u32::from_str_radix(h, 16).ok());
    match mn {
        "lda" => regs.a = v,
        "ldx" => regs.x = v,
        "ldy" => regs.y = v,
        "tax" => regs.x = regs.a,
        "tay" => regs.y = regs.a,
        "txa" => regs.a = regs.x,
        "tya" => regs.a = regs.y,
        "txy" => regs.y = regs.x,
        "tyx" => regs.x = regs.y,
        "inx" | "dex" => regs.x = regs.x.map(|x| (if mn == "inx" { x.wrapping_add(1) } else { x.wrapping_sub(1) }) & width),
        "iny" | "dey" => regs.y = regs.y.map(|y| (if mn == "iny" { y.wrapping_add(1) } else { y.wrapping_sub(1) }) & width),
        "ora" => regs.a = regs.a.zip(v).map(|(a, v)| a | v),
        "and" => regs.a = regs.a.zip(v).map(|(a, v)| a & v),
        "eor" => regs.a = regs.a.zip(v).map(|(a, v)| a ^ v),
        "jsr" | "jsl" | "brk" | "cop" => *regs = Regs::default(),
        "asl" | "lsr" | "rol" | "ror" | "inc" | "dec" => {
            if arg.is_empty() {
                regs.a = None;
            }
        }
        "adc" | "sbc" | "pla" | "xba" | "tdc" | "tsc" => regs.a = None,
        "tsx" | "plx" => regs.x = None,
        "ply" => regs.y = None,
        _ => {}
    }
}

const WRITES: &[&str] = &["sta", "stx", "sty", "stz", "inc", "dec", "asl", "lsr", "rol", "ror", "trb", "tsb"];
const NO_SYMBOLS: &[&str] = &["mvn", "mvp", "rep", "sep", "cop", "wdm", "brk", "pea", "pei", "per"];

/// The disassembly of `data`.
pub fn disassemble_all(data: &[u8], opts: &Options) -> Disassembly {
    let n = data.len();
    let base = opts.base;
    let cpu = opts.cpu.as_str();
    let split = opts.split.unwrap_or(n).min(n);
    let machine = opts.machine.as_deref();
    let cls = classify(data, opts);
    let types = &cls.types;
    let syms = machine.map(memmap::symbols_for);
    let inside = |a: u32| a >= base.max(0x100) && a < base + split as u32;

    // the code: the instructions of the runs of code, and the addresses they point to
    struct Code {
        off: usize,
        len: usize,
        ins: Option<Ins>,
    }
    let mut code: Vec<Code> = Vec::new();
    let mut refs: Vec<u32> = cls.entries.iter().copied().filter(|&a| inside(a)).collect();
    let mut flags = Flags::default();
    let mut i = 0;
    while i < n {
        if types[i] != DataType::Code {
            i += 1;
            flags = Flags::default();
            continue;
        }
        flags = cls.starts.get(&i).copied().unwrap_or(flags);
        let ins = decode(data, i, base, cpu, flags);
        let mut len = ins.as_ref().map_or(1, |x| x.len);
        if ins.is_some() && (1..len).any(|k| i + k >= n || types[i + k] != DataType::Code || i + k >= split) {
            len = 0;
        }
        let Some(ins) = ins.filter(|_| len > 0) else {
            code.push(Code { off: i, len: 1, ins: None });
            i += 1;
            continue;
        };
        refs.extend(ins.addrs.iter().copied().filter(|&a| inside(a)));
        flags = ins.after;
        code.push(Code { off: i, len, ins: Some(ins) });
        i += len;
    }
    // the words that point into the bytes
    let mut i = 0;
    while i + 1 < split {
        if types[i] == DataType::Words && types[i + 1] == DataType::Words {
            let w = data[i] as u32 | (data[i + 1] as u32) << 8;
            if inside(w) {
                refs.push(w);
            }
            i += 1;
        }
        i += 1;
    }
    // a name the user gave an address: a label there, whether something points to it or not
    refs.extend(opts.names.keys().copied().filter(|&a| a >= base && a < base + split as u32));
    let ref_offs: HashSet<usize> = refs.iter().map(|&a| (a - base) as usize).collect();

    // the lines: the code as decoded, the data in lines of its type, broken where a label is; BASIC by its lines
    let basic_at: HashMap<usize, &BasicLine> =
        cls.basic.iter().flat_map(|b| b.lines.iter().map(|l| (l.off, l))).collect();
    struct Raw<'a> {
        off: usize,
        len: usize,
        ty: DataType,
        ins: Option<Ins>,
        basic: Option<&'a BasicLine>,
    }
    let mut raw: Vec<Raw> = Vec::new();
    let mut code = code.into_iter();
    let mut i = 0;
    while i < n {
        let t = types[i];
        if t == DataType::Code {
            let x = code.next().unwrap();
            raw.push(Raw { off: x.off, len: x.len, ty: t, ins: x.ins, basic: None });
            i += x.len;
            continue;
        }
        if t == DataType::Basic
            && let Some(bl) = basic_at.get(&i).filter(|bl| bl.len <= n - i)
        {
            raw.push(Raw { off: i, len: bl.len, ty: t, ins: None, basic: Some(bl) });
            i += bl.len;
            continue;
        }
        let per = t.per();
        let mut k = i + 1;
        while k < n && types[k] == t && k - i < per && !ref_offs.contains(&k) && k != split {
            k += 1;
        }
        if t == DataType::Words && (k - i) % 2 == 1 && k - i > 1 {
            k -= 1; // whole words, the odd byte by itself
        }
        raw.push(Raw { off: i, len: k - i, ty: t, ins: None, basic: None });
        i = k;
    }

    // the labels: at the start of a line, else the label of its line and the distance
    let starts: Vec<usize> = raw.iter().map(|l| l.off).collect();
    let mut line_labels: Vec<Option<String>> = vec![None; raw.len()];
    let mut labels = BTreeMap::new();
    let mut symbols = BTreeMap::new();
    let line_of = |off: usize| starts.partition_point(|&s| s <= off).saturating_sub(1);
    let name_of = |off: usize| {
        let addr = (base + off as u32) & 0xffff;
        opts.names
            .get(&addr)
            .cloned()
            .unwrap_or_else(|| format!("{}_{}", if types[off] == DataType::Code { 'l' } else { 'd' }, hex4(addr)))
    };
    let expr = |a: u32, line_labels: &mut Vec<Option<String>>, labels: &mut BTreeMap<u32, String>| -> String {
        let off = (a - base) as usize;
        let l = line_of(off);
        let label = line_labels[l].get_or_insert_with(|| {
            let name = name_of(starts[l]);
            labels.insert((base + starts[l] as u32) & 0xffff, name.clone());
            name
        });
        if starts[l] == off { label.clone() } else { format!("{label}+{}", off - starts[l]) }
    };
    for &off in &ref_offs {
        expr(base + off as u32, &mut line_labels, &mut labels);
    }
    // a register or area of the machine: its name, and the name kept for the equates of the source
    let symbol = |a: u32, read: bool, small: bool, symbols: &mut BTreeMap<String, (u32, String)>| -> Option<SymRef> {
        let hit = syms?.at(a, read)?;
        if small && hit.sym.size > 16 {
            return None; // the zero page: registers and small areas only
        }
        symbols.insert(hit.name.to_owned(), (hit.sym.addr, hit.desc.to_owned()));
        Some(SymRef {
            text: if hit.offset > 0 { format!("{}+{}", hit.name, hit.offset) } else { hit.name.to_owned() },
            name: hit.name.to_owned(),
            desc: hit.desc.to_owned(),
            addr: a,
            offset: hit.offset,
        })
    };

    let [ascii, pet, scr] = text_maps();
    let mut regs = Regs::default();
    let mut lines = Vec::with_capacity(raw.len());
    for (ix, r) in raw.into_iter().enumerate() {
        let addr = (base + r.off as u32) & 0xffff;
        let bytes = &data[r.off..r.off + r.len];
        let mut line = Line {
            off: r.off,
            len: r.len,
            addr,
            ty: r.ty,
            label: None,
            text: String::new(),
            comment: None,
            reference: None,
            sym: None,
            value: None,
            flags: None,
            view: None,
        };
        let region_start = || {
            let mut o = r.off;
            while o > 0 && types[o - 1] == r.ty {
                o -= 1;
            }
            o
        };
        match r.ty {
            DataType::Code => {
                if line_labels[ix].is_some() {
                    regs = Regs::default(); // something jumps here: nothing known
                }
                let Some(ins) = r.ins else {
                    line.text = format!("!byte ${}", hex2(bytes[0] as u32));
                    line.comment = Some("not an instruction".into());
                    regs = Regs::default();
                    lines.push(line);
                    continue;
                };
                line.flags = Some(ins.before);
                let write = WRITES.contains(&ins.mn.as_str()) && !ins.arg.is_empty();
                let mut sym: Option<SymRef> = None;
                let mut text = String::new();
                let mut last = 0;
                for (start, end, hash, h) in hex_operands(&ins.text) {
                    text.push_str(&ins.text[last..start]);
                    last = end;
                    let original = &ins.text[start..end];
                    if hash || (is_z80(cpu) && h.len() < 4) || NO_SYMBOLS.contains(&ins.mn.as_str()) || ins.arg.contains(",s") {
                        text.push_str(original);
                        continue;
                    }
                    let v = u32::from_str_radix(h, 16).unwrap_or(0);
                    let replaced = match h.len() {
                        6 => ((ins.mn == "jsl" || ins.mn == "jml") && v >> 16 == 0 && inside(v))
                            .then(|| expr(v, &mut line_labels, &mut labels)),
                        4 if inside(v) => Some(expr(v, &mut line_labels, &mut labels)),
                        4 if v < 0x100 => None, // an absolute address in the zero page: as it is
                        4 => {
                            if sym.is_none() {
                                sym = symbol(v, !write, false, &mut symbols);
                            }
                            sym.as_ref().filter(|s| s.addr == v).map(|s| s.text.clone())
                        }
                        _ => {
                            if sym.is_none() {
                                sym = symbol(v, !write, true, &mut symbols); // the zero page
                            }
                            sym.as_ref().filter(|s| s.addr == v).map(|s| s.text.clone())
                        }
                    };
                    text.push_str(replaced.as_deref().unwrap_or(original));
                }
                text.push_str(&ins.text[last..]);
                line.text = text;
                if let Some(to) = ins.addrs.iter().copied().find(|&a| inside(a)) {
                    line.reference = Some((to - base) as usize);
                }
                let mut notes = Vec::new();
                if cls.entries.contains(&addr) {
                    notes.push("entry".to_owned());
                }
                if let Some(s) = &sym {
                    let store = match ins.mn.as_str() {
                        "sta" => regs.a,
                        "stx" => regs.x,
                        "sty" => regs.y,
                        "stz" => Some(0),
                        _ => None,
                    };
                    match store.filter(|_| s.offset == 0) {
                        Some(store) => {
                            line.value = Some(store);
                            let words = machine.and_then(|m| memmap::decode_bits(m, &s.name, store));
                            notes.push(match words {
                                Some(words) => format!("${}: {words}", hex2(store)),
                                None => s.desc.clone(),
                            });
                        }
                        None => notes.push(s.desc.clone()),
                    }
                    line.sym = Some(s.clone());
                }
                if !notes.is_empty() {
                    line.comment = Some(notes.join(" · "));
                }
                if !is_z80(cpu) {
                    track(&mut regs, &ins.mn, &ins.arg, if cpu == "65816" { 0xffff } else { 0xff });
                }
                if ins.stop {
                    regs = Regs::default();
                }
            }
            DataType::Words => {
                let mut ws = Vec::new();
                for k in (0..bytes.len().saturating_sub(1)).step_by(2) {
                    let w = bytes[k] as u32 | (bytes[k + 1] as u32) << 8;
                    let s = (!inside(w) && r.off < split && w >= 0x100).then(|| symbol(w, true, false, &mut symbols)).flatten();
                    ws.push(if inside(w) && r.off < split {
                        expr(w, &mut line_labels, &mut labels)
                    } else if let Some(s) = s.filter(|s| s.offset == 0) {
                        s.text
                    } else {
                        format!("${}", hex4(w))
                    });
                }
                line.text = if ws.is_empty() { format!("!byte ${}", hex2(bytes[0] as u32)) } else { format!("!word {}", ws.join(", ")) };
                if bytes.len() > 1 {
                    let w0 = bytes[0] as u32 | (bytes[1] as u32) << 8;
                    if inside(w0) {
                        line.reference = Some((w0 - base) as usize);
                    }
                }
            }
            DataType::Basic => {
                line.text = format!("!byte {}", bytes.iter().map(|b| format!("${b:02x}")).collect::<Vec<_>>().join(", "));
                if let Some(bl) = r.basic {
                    // a SYS in hex too, and the line leads to the code there
                    line.view = Some(bl.text.to_uppercase());
                    let note = (!bl.sys.is_empty())
                        .then(|| format!("SYS {}", bl.sys.iter().map(|a| format!("${}", hex4(*a))).collect::<Vec<_>>().join(", ")));
                    line.comment = Some(match &note {
                        Some(note) => format!("{} · {note}", bl.text),
                        None => bl.text.clone(),
                    });
                    if let Some(to) = bl.sys.iter().copied().find(|&a| inside(a)) {
                        line.reference = Some((to - base) as usize);
                    }
                } else if cls.basic.as_ref().is_some_and(|b| r.off + 2 == b.end) {
                    line.comment = Some("the end of the BASIC program".into());
                }
            }
            DataType::Text => line.text = format!("!text {}", text_args(bytes, &ascii)),
            DataType::Petscii => line.text = format!("!pet {}", text_args(bytes, &pet)),
            DataType::Screen => line.text = format!("!scr {}", text_args(bytes, &scr)),
            DataType::Chars | DataType::Sprites => {
                line.text = format!("!byte {}", bytes.iter().map(|b| format!("%{b:08b}")).collect::<Vec<_>>().join(", "));
                let size = if r.ty == DataType::Chars { 8 } else { 64 };
                let at = r.off - region_start();
                if at % size == 0 {
                    line.comment = Some(format!("{} {}", if r.ty == DataType::Chars { "char" } else { "sprite" }, at / size));
                }
            }
            _ => {
                line.text = format!("!byte {}", bytes.iter().map(|b| format!("${b:02x}")).collect::<Vec<_>>().join(", "));
                let at = r.off - region_start();
                if r.ty == DataType::Bitmap && at % 320 == 0 {
                    line.comment = Some(format!("row {}", at / 320));
                }
                if r.ty == DataType::Tiles && at % 16 == 0 {
                    line.comment = Some(format!("tile {}", at / 16));
                }
            }
        }
        lines.push(line);
    }
    for (line, label) in lines.iter_mut().zip(line_labels) {
        line.label = label;
    }
    Disassembly { lines, labels, symbols, split, classified: cls }
}

/// The disassembly as ACME source: the names of the machine it uses, the header of a cartridge as
/// bytes, what the CPU sees at its addresses (in `!pseudopc` when there is more), the rest after it.
/// `format`: a `!to` of the extension and the format (`("prg", "cbm")`).
pub fn to_source(result: &Disassembly, opts: &Options, name: &str, head: Option<&[u8]>, format: Option<(&str, &str)>) -> String {
    let cpu = opts.cpu.as_str();
    let mut out = vec![format!("; {}disassembled by NerdSuite", if name.is_empty() { String::new() } else { format!("{name} - ") }), String::new()];
    let mut syms: Vec<_> = result.symbols.iter().collect();
    syms.sort_by_key(|(_, (addr, _))| *addr);
    if !syms.is_empty() {
        out.push(format!("; the addresses of the {} it uses (all of them: hardware.inc)", opts.machine.as_deref().unwrap_or("machine")));
        for (n, (addr, desc)) in syms {
            let a = if *addr < 0x100 { hex2(*addr) } else { hex4(*addr) };
            out.push(format!("{n:<20}= ${a}    ; {desc}"));
        }
        out.push(String::new());
    }
    out.push(format!("        !cpu {}", if is_z80(cpu) { "z80" } else { cpu }));
    if let Some((ext, kind)) = format {
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        out.push(format!("        !to \"{stem}.{ext}\", {kind}"));
    }
    let head = head.filter(|h| !h.is_empty());
    let wrapped = head.is_some() || result.split < result.types().len();
    if wrapped {
        out.push("        * = $0000".into());
        out.push(String::new());
        if let Some(head) = head {
            out.push("; the header".into());
            for chunk in head.chunks(16) {
                out.push(format!("        !byte {}", chunk.iter().map(|b| format!("${b:02x}")).collect::<Vec<_>>().join(", ")));
            }
            out.push(String::new());
        }
        out.push(format!("        !pseudopc ${} {{", hex4(opts.base)));
    } else {
        out.push(format!("        * = ${}", hex4(opts.base)));
    }
    out.push(String::new());
    let mut last: Option<DataType> = None;
    let (mut al, mut rl, mut closed) = (false, false, false);
    for l in &result.lines {
        if wrapped && !closed && l.off >= result.split {
            out.push("        }".into());
            out.push(String::new());
            out.push("; after what the CPU sees at these addresses".into());
            closed = true;
        }
        if last.is_some_and(|t| t != DataType::Code) && l.ty == DataType::Code {
            out.push(String::new());
        }
        if let Some(label) = &l.label {
            out.push(label.clone());
        }
        if cpu == "65816"
            && let Some(f) = l.flags
        {
            if !f.m8 != al {
                al = !f.m8;
                out.push(format!("        !a{}", if al { 'l' } else { 's' }));
            }
            if !f.x8 != rl {
                rl = !f.x8;
                out.push(format!("        !r{}", if rl { 'l' } else { 's' }));
            }
        }
        out.push(match &l.comment {
            Some(c) => format!("        {}    ; {c}", l.text),
            None => format!("        {}", l.text),
        });
        last = Some(l.ty);
    }
    if wrapped && !closed {
        out.push("        }".into());
    }
    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests;
