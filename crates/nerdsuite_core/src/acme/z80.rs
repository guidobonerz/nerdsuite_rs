//! The Z80 in Zilog's syntax (`!cpu z80`): the CPC, the ZX81 and the TRS-80. The disassembly
//! assembles back to the same bytes.

use super::{
    Line,
    asm::{Asm, ILLEGAL_MODE, Num, R, Val, is_id_start},
};

const SIMPLE: &[(&str, &[u8])] = &[
    ("nop", &[0x00]),
    ("halt", &[0x76]),
    ("di", &[0xf3]),
    ("ei", &[0xfb]),
    ("exx", &[0xd9]),
    ("daa", &[0x27]),
    ("cpl", &[0x2f]),
    ("scf", &[0x37]),
    ("ccf", &[0x3f]),
    ("rla", &[0x17]),
    ("rra", &[0x1f]),
    ("rlca", &[0x07]),
    ("rrca", &[0x0f]),
    ("neg", &[0xed, 0x44]),
    ("reti", &[0xed, 0x4d]),
    ("retn", &[0xed, 0x45]),
    ("rld", &[0xed, 0x6f]),
    ("rrd", &[0xed, 0x67]),
    ("ldi", &[0xed, 0xa0]),
    ("ldir", &[0xed, 0xb0]),
    ("ldd", &[0xed, 0xa8]),
    ("lddr", &[0xed, 0xb8]),
    ("cpi", &[0xed, 0xa1]),
    ("cpir", &[0xed, 0xb1]),
    ("cpd", &[0xed, 0xa9]),
    ("cpdr", &[0xed, 0xb9]),
    ("ini", &[0xed, 0xa2]),
    ("inir", &[0xed, 0xb2]),
    ("ind", &[0xed, 0xaa]),
    ("indr", &[0xed, 0xba]),
    ("outi", &[0xed, 0xa3]),
    ("otir", &[0xed, 0xb3]),
    ("outd", &[0xed, 0xab]),
    ("otdr", &[0xed, 0xbb]),
];
const ALU: &[(&str, u8)] = &[("add", 0), ("adc", 1), ("sub", 2), ("sbc", 3), ("and", 4), ("xor", 5), ("or", 6), ("cp", 7)];
const ROT: &[(&str, u8)] = &[
    ("rlc", 0),
    ("rrc", 1),
    ("rl", 2),
    ("rr", 3),
    ("sla", 4),
    ("sra", 5),
    ("sll", 6),
    ("sl1", 6),
    ("srl", 7),
];
const BIT: &[(&str, u8)] = &[("bit", 0x40), ("res", 0x80), ("set", 0xc0)];
const CC: [&str; 8] = ["nz", "z", "nc", "c", "po", "pe", "p", "m"];
const R8: [&str; 8] = ["b", "c", "d", "e", "h", "l", "", "a"];
const REGS: &[&str] = &[
    "a", "b", "c", "d", "e", "h", "l", "i", "r", "f", "af", "bc", "de", "hl", "sp", "ix", "iy", "ixh", "ixl", "iyh",
    "iyl", "nz", "z", "nc", "po", "pe", "p", "m",
];

/// The mnemonics of the Z80, for the statements and the help.
pub const MNEMONICS: &[&str] = &[
    "nop", "halt", "di", "ei", "exx", "daa", "cpl", "scf", "ccf", "rla", "rra", "rlca", "rrca", "neg", "reti", "retn",
    "rld", "rrd", "ldi", "ldir", "ldd", "lddr", "cpi", "cpir", "cpd", "cpdr", "ini", "inir", "ind", "indr", "outi",
    "otir", "outd", "otdr", "add", "adc", "sub", "sbc", "and", "xor", "or", "cp", "rlc", "rrc", "rl", "rr", "sla",
    "sra", "sll", "sl1", "srl", "bit", "res", "set", "ld", "push", "pop", "ex", "inc", "dec", "jp", "jr", "djnz",
    "call", "ret", "rst", "im", "in", "out",
];

fn lookup<T: Copy>(table: &[(&str, T)], key: &str) -> Option<T> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

#[derive(Clone)]
enum Op {
    /// `(bc)` `(de)` `(hl)` `(sp)` `(c)`
    Ind(String),
    /// `(ix+d)`: the prefix, the displacement; `plain`: `(ix)` without one
    Idx { p: u8, d: Val, plain: bool },
    /// `(nn)`
    Mem(Val),
    Reg(String),
    Imm(Val),
}

/// An 8 bit operand: its register code, the prefix of ixh/ixl/(ix+d), the displacement.
struct R8Op {
    c: u8,
    p: Option<u8>,
    d: Option<Val>,
    half: bool,
    mem: bool,
}

/// A 16 bit register pair: bc=0 de=1 hl/ix/iy=2 sp|af=3.
struct Rp {
    i: u8,
    p: Option<u8>,
}

enum Item {
    B(u8),
    N(Val),
    NN(Val),
    D(Val),
    Rel(Val),
}

fn is_reg(o: Option<&Op>, name: &str) -> bool {
    matches!(o, Some(Op::Reg(n)) if n == name)
}

fn is_ind(o: Option<&Op>, name: &str) -> bool {
    matches!(o, Some(Op::Ind(n)) if n == name)
}

fn cc(o: Option<&Op>) -> Option<u8> {
    match o {
        Some(Op::Reg(n)) => CC.iter().position(|c| c == n).map(|i| i as u8),
        _ => None,
    }
}

fn r8(o: Option<&Op>) -> Option<R8Op> {
    match o? {
        Op::Reg(name) => {
            if let Some(c) = R8.iter().position(|r| !r.is_empty() && r == name) {
                return Some(R8Op { c: c as u8, p: None, d: None, half: false, mem: false });
            }
            let (p, c) = match name.as_str() {
                "ixh" => (0xdd, 4),
                "ixl" => (0xdd, 5),
                "iyh" => (0xfd, 4),
                "iyl" => (0xfd, 5),
                _ => return None,
            };
            Some(R8Op { c, p: Some(p), d: None, half: true, mem: false })
        }
        Op::Ind(name) if name == "hl" => Some(R8Op { c: 6, p: None, d: None, half: false, mem: true }),
        Op::Idx { p, d, .. } => Some(R8Op { c: 6, p: Some(*p), d: Some(d.clone()), half: false, mem: true }),
        _ => None,
    }
}

fn rp(o: Option<&Op>, af: bool) -> Option<Rp> {
    let Some(Op::Reg(n)) = o else { return None };
    Some(match n.as_str() {
        "bc" => Rp { i: 0, p: None },
        "de" => Rp { i: 1, p: None },
        "hl" => Rp { i: 2, p: None },
        "ix" => Rp { i: 2, p: Some(0xdd) },
        "iy" => Rp { i: 2, p: Some(0xfd) },
        n if n == if af { "af" } else { "sp" } => Rp { i: 3, p: None },
        _ => return None,
    })
}

/// The displacement of an `(ix+d)` operand.
fn disp(x: &R8Op) -> Option<Item> {
    (x.mem && x.p.is_some()).then(|| Item::D(x.d.clone().unwrap()))
}

impl Asm {
    fn z80_operand(&mut self) -> R<Op> {
        self.ws();
        let saved = (self.r().pos, self.r().line, self.r().line_start);
        if self.peek(0) == '(' && self.looks_indirect() {
            self.bump(1);
            self.ws();
            let saved2 = self.r().pos;
            if is_id_start(self.peek(0)) {
                let w = self.read_word().to_lowercase();
                self.ws();
                if matches!(w.as_str(), "bc" | "de" | "hl" | "sp" | "c") && self.peek(0) == ')' {
                    self.bump(1);
                    return Ok(Op::Ind(w));
                }
                if w == "ix" || w == "iy" {
                    let p = if w == "ix" { 0xdd } else { 0xfd };
                    if self.peek(0) == ')' {
                        self.bump(1);
                        return Ok(Op::Idx { p, d: Val::N(Num::new(0.)), plain: true });
                    }
                    if matches!(self.peek(0), '+' | '-') {
                        let d = self.expr()?;
                        self.ws();
                        if self.peek(0) != ')' {
                            return self.err("Missing ')'.");
                        }
                        self.bump(1);
                        return Ok(Op::Idx { p, d, plain: false });
                    }
                }
                self.rm().pos = saved2;
            }
            let v = self.expr()?;
            self.ws();
            if self.peek(0) != ')' {
                return self.err("Missing ')'.");
            }
            self.bump(1);
            return Ok(Op::Mem(v));
        }
        if is_id_start(self.peek(0)) {
            let w = self.read_word().to_lowercase();
            if REGS.contains(&w.as_str()) {
                if w == "af" && self.peek(0) == '\'' {
                    self.bump(1);
                    return Ok(Op::Reg("af'".into()));
                }
                return Ok(Op::Reg(w));
            }
            let r = self.rm();
            (r.pos, r.line, r.line_start) = saved;
        }
        Ok(Op::Imm(self.expr()?))
    }

    pub(super) fn z80_instruction(&mut self, mn: &str) -> R<()> {
        let mut ops = Vec::new();
        if !self.eos() {
            loop {
                ops.push(self.z80_operand()?);
                self.ws();
                if self.peek(0) != ',' {
                    break;
                }
                self.bump(1);
            }
        }
        self.z80_encode(mn, &ops)
    }

    fn z80_emit(&mut self, items: Vec<Option<Item>>) -> R<()> {
        let items: Vec<Item> = items.into_iter().flatten().collect();
        let pc = self.pc_val()?;
        let len: i64 = items.iter().map(|it| if matches!(it, Item::NN(_)) { 2 } else { 1 }).sum();
        for item in items {
            match item {
                Item::B(b) => self.emit(b as f64)?,
                Item::N(v) => {
                    let n = self.to_int(&v)?;
                    self.emit_val(n, 1, v.is_undef(), false, false)?;
                }
                Item::NN(v) => {
                    let n = self.to_int(&v)?;
                    self.emit_val(n, 2, v.is_undef(), false, false)?;
                }
                Item::D(v) => {
                    let d = self.to_int(&v)?;
                    if !v.is_undef() && !(-128. ..=127.).contains(&d) {
                        self.error("Index displacement out of range.")?;
                    }
                    self.emit((d as i64 & 0xff) as f64)?;
                }
                Item::Rel(v) => self.branch(&v, pc + len)?,
            }
        }
        Ok(())
    }

    fn z80_encode(&mut self, mn: &str, ops: &[Op]) -> R<()> {
        use Item::*;
        let a = ops.first();
        let b = ops.get(1);
        let n_ops = |asm: &Self, k: usize| -> R<()> {
            if ops.len() != k {
                return asm.err(ILLEGAL_MODE);
            }
            Ok(())
        };
        let p = |x: Option<u8>| x.map(B);
        let imm = |o: Option<&Op>| match o {
            Some(Op::Imm(v)) => Some(v.clone()),
            _ => None,
        };
        let mem = |o: Option<&Op>| match o {
            Some(Op::Mem(v)) => Some(v.clone()),
            _ => None,
        };

        if let Some(bytes) = lookup(SIMPLE, mn) {
            n_ops(self, 0)?;
            return self.z80_emit(bytes.iter().map(|&b| Some(B(b))).collect());
        }

        if let Some(alu) = lookup(ALU, mn) {
            if ops.len() == 2 && (is_reg(a, "hl") || is_reg(a, "ix") || is_reg(a, "iy")) {
                let dst = rp(a, false).unwrap();
                let Some(src) = rp(b, false).filter(|src| !(src.i == 2 && src.p != dst.p)) else {
                    return self.err(ILLEGAL_MODE);
                };
                if mn == "add" {
                    return self.z80_emit(vec![p(dst.p), Some(B(0x09 | (src.i << 4)))]);
                }
                if dst.p.is_some() {
                    return self.err(ILLEGAL_MODE);
                }
                match mn {
                    "adc" => return self.z80_emit(vec![Some(B(0xed)), Some(B(0x4a | (src.i << 4)))]),
                    "sbc" => return self.z80_emit(vec![Some(B(0xed)), Some(B(0x42 | (src.i << 4)))]),
                    _ => return self.err(ILLEGAL_MODE),
                }
            }
            let src = if ops.len() == 2 {
                if !is_reg(a, "a") {
                    return self.err(ILLEGAL_MODE);
                }
                b
            } else {
                n_ops(self, 1)?;
                a
            };
            if let Some(x) = r8(src) {
                return self.z80_emit(vec![p(x.p), Some(B(0x80 | (alu << 3) | x.c)), disp(&x)]);
            }
            if let Some(v) = imm(src) {
                return self.z80_emit(vec![Some(B(0xc6 | (alu << 3))), Some(N(v))]);
            }
            return self.err(ILLEGAL_MODE);
        }

        if let Some(rot) = lookup(ROT, mn) {
            n_ops(self, 1)?;
            let Some(x) = r8(a).filter(|x| !x.half) else { return self.err(ILLEGAL_MODE) };
            let op = (rot << 3) | x.c;
            return match x.p {
                Some(prefix) => self.z80_emit(vec![Some(B(prefix)), Some(B(0xcb)), disp(&x), Some(B(op))]),
                None => self.z80_emit(vec![Some(B(0xcb)), Some(B(op))]),
            };
        }

        if let Some(base) = lookup(BIT, mn) {
            n_ops(self, 2)?;
            let Some(bit) = imm(a) else { return self.err(ILLEGAL_MODE) };
            let bit = self.to_int(&bit)?;
            if !(0. ..=7.).contains(&bit) {
                return self.err("Bit number out of range (0..7).");
            }
            let Some(x) = r8(b).filter(|x| !x.half) else { return self.err(ILLEGAL_MODE) };
            let op = base | ((bit as u8) << 3) | x.c;
            return match x.p {
                Some(prefix) => self.z80_emit(vec![Some(B(prefix)), Some(B(0xcb)), disp(&x), Some(B(op))]),
                None => self.z80_emit(vec![Some(B(0xcb)), Some(B(op))]),
            };
        }

        match mn {
            "ld" => {
                n_ops(self, 2)?;
                let (x, y) = (r8(a), r8(b));
                if let (Some(x), Some(y)) = (&x, &y) {
                    let hl = |z: &R8Op| z.p.is_none() && (z.c == 4 || z.c == 5);
                    if (x.mem && y.mem)
                        || (x.p.is_some() && y.p.is_some() && (x.p != y.p || x.mem || y.mem))
                        || (x.half && hl(y))
                        || (y.half && hl(x))
                    {
                        return self.err(ILLEGAL_MODE);
                    }
                    let m = if x.mem { Some(x) } else if y.mem { Some(y) } else { None };
                    if m.is_some() && (x.half || y.half) {
                        return self.err(ILLEGAL_MODE);
                    }
                    return self.z80_emit(vec![p(x.p.or(y.p)), Some(B(0x40 | (x.c << 3) | y.c)), m.and_then(disp)]);
                }
                if let (Some(x), Some(v)) = (&x, imm(b)) {
                    return self.z80_emit(vec![p(x.p), Some(B(0x06 | (x.c << 3))), disp(x), Some(N(v))]);
                }
                if is_reg(a, "a") {
                    if is_ind(b, "bc") {
                        return self.z80_emit(vec![Some(B(0x0a))]);
                    }
                    if is_ind(b, "de") {
                        return self.z80_emit(vec![Some(B(0x1a))]);
                    }
                    if let Some(v) = mem(b) {
                        return self.z80_emit(vec![Some(B(0x3a)), Some(NN(v))]);
                    }
                    if is_reg(b, "i") {
                        return self.z80_emit(vec![Some(B(0xed)), Some(B(0x57))]);
                    }
                    if is_reg(b, "r") {
                        return self.z80_emit(vec![Some(B(0xed)), Some(B(0x5f))]);
                    }
                }
                if is_reg(b, "a") {
                    if is_ind(a, "bc") {
                        return self.z80_emit(vec![Some(B(0x02))]);
                    }
                    if is_ind(a, "de") {
                        return self.z80_emit(vec![Some(B(0x12))]);
                    }
                    if let Some(v) = mem(a) {
                        return self.z80_emit(vec![Some(B(0x32)), Some(NN(v))]);
                    }
                    if is_reg(a, "i") {
                        return self.z80_emit(vec![Some(B(0xed)), Some(B(0x47))]);
                    }
                    if is_reg(a, "r") {
                        return self.z80_emit(vec![Some(B(0xed)), Some(B(0x4f))]);
                    }
                }
                if let Some(ra) = rp(a, false) {
                    if let Some(v) = imm(b) {
                        return self.z80_emit(vec![p(ra.p), Some(B(0x01 | (ra.i << 4))), Some(NN(v))]);
                    }
                    if let Some(v) = mem(b) {
                        return if ra.i == 2 {
                            self.z80_emit(vec![p(ra.p), Some(B(0x2a)), Some(NN(v))])
                        } else {
                            self.z80_emit(vec![Some(B(0xed)), Some(B(0x4b | (ra.i << 4))), Some(NN(v))])
                        };
                    }
                    if ra.i == 3
                        && let Some(s) = rp(b, false).filter(|s| s.i == 2)
                    {
                        return self.z80_emit(vec![p(s.p), Some(B(0xf9))]);
                    }
                }
                if let (Some(v), Some(rb)) = (mem(a), rp(b, false)) {
                    return if rb.i == 2 {
                        self.z80_emit(vec![p(rb.p), Some(B(0x22)), Some(NN(v))])
                    } else {
                        self.z80_emit(vec![Some(B(0xed)), Some(B(0x43 | (rb.i << 4))), Some(NN(v))])
                    };
                }
                self.err(ILLEGAL_MODE)
            }
            "push" | "pop" => {
                n_ops(self, 1)?;
                let Some(x) = rp(a, true) else { return self.err(ILLEGAL_MODE) };
                let op = if mn == "push" { 0xc5 } else { 0xc1 };
                self.z80_emit(vec![p(x.p), Some(B(op | (x.i << 4)))])
            }
            "ex" => {
                n_ops(self, 2)?;
                if is_reg(a, "de") && is_reg(b, "hl") {
                    return self.z80_emit(vec![Some(B(0xeb))]);
                }
                if is_reg(a, "af") && is_reg(b, "af'") {
                    return self.z80_emit(vec![Some(B(0x08))]);
                }
                if is_ind(a, "sp")
                    && let Some(x) = rp(b, false).filter(|x| x.i == 2)
                {
                    return self.z80_emit(vec![p(x.p), Some(B(0xe3))]);
                }
                self.err(ILLEGAL_MODE)
            }
            "inc" | "dec" => {
                n_ops(self, 1)?;
                let inc = mn == "inc";
                if let Some(x) = r8(a) {
                    let op = if inc { 0x04 } else { 0x05 } | (x.c << 3);
                    return self.z80_emit(vec![p(x.p), Some(B(op)), disp(&x)]);
                }
                if let Some(y) = rp(a, false) {
                    let op = if inc { 0x03 } else { 0x0b } | (y.i << 4);
                    return self.z80_emit(vec![p(y.p), Some(B(op))]);
                }
                self.err(ILLEGAL_MODE)
            }
            "jp" => {
                if ops.len() == 1 {
                    if is_ind(a, "hl") {
                        return self.z80_emit(vec![Some(B(0xe9))]);
                    }
                    if let Some(Op::Idx { p: prefix, plain: true, .. }) = a {
                        return self.z80_emit(vec![Some(B(*prefix)), Some(B(0xe9))]);
                    }
                    if let Some(v) = imm(a) {
                        return self.z80_emit(vec![Some(B(0xc3)), Some(NN(v))]);
                    }
                    return self.err(ILLEGAL_MODE);
                }
                n_ops(self, 2)?;
                let (Some(c), Some(v)) = (cc(a), imm(b)) else { return self.err(ILLEGAL_MODE) };
                self.z80_emit(vec![Some(B(0xc2 | (c << 3))), Some(NN(v))])
            }
            "call" => {
                if ops.len() == 1 {
                    let Some(v) = imm(a) else { return self.err(ILLEGAL_MODE) };
                    return self.z80_emit(vec![Some(B(0xcd)), Some(NN(v))]);
                }
                n_ops(self, 2)?;
                let (Some(c), Some(v)) = (cc(a), imm(b)) else { return self.err(ILLEGAL_MODE) };
                self.z80_emit(vec![Some(B(0xc4 | (c << 3))), Some(NN(v))])
            }
            "jr" => {
                if ops.len() == 1 {
                    let Some(v) = imm(a) else { return self.err(ILLEGAL_MODE) };
                    return self.z80_emit(vec![Some(B(0x18)), Some(Rel(v))]);
                }
                n_ops(self, 2)?;
                let (Some(c), Some(v)) = (cc(a).filter(|c| *c <= 3), imm(b)) else {
                    return self.err(ILLEGAL_MODE);
                };
                self.z80_emit(vec![Some(B(0x20 | (c << 3))), Some(Rel(v))])
            }
            "djnz" => {
                n_ops(self, 1)?;
                let Some(v) = imm(a) else { return self.err(ILLEGAL_MODE) };
                self.z80_emit(vec![Some(B(0x10)), Some(Rel(v))])
            }
            "ret" => {
                if ops.is_empty() {
                    return self.z80_emit(vec![Some(B(0xc9))]);
                }
                n_ops(self, 1)?;
                let Some(c) = cc(a) else { return self.err(ILLEGAL_MODE) };
                self.z80_emit(vec![Some(B(0xc0 | (c << 3)))])
            }
            "rst" => {
                n_ops(self, 1)?;
                let Some(v) = imm(a) else { return self.err(ILLEGAL_MODE) };
                let n = self.to_int(&v)? as i64;
                if !v.is_undef() && n & !0x38 != 0 {
                    return self.err("Illegal RST address (must be $00, $08, ... $38).");
                }
                self.z80_emit(vec![Some(B(0xc7 | (n & 0x38) as u8))])
            }
            "im" => {
                n_ops(self, 1)?;
                let Some(v) = imm(a) else { return self.err(ILLEGAL_MODE) };
                let op = match self.to_int(&v)? as i64 {
                    0 => 0x46,
                    1 => 0x56,
                    2 => 0x5e,
                    _ => return self.err("Illegal interrupt mode (0, 1 or 2)."),
                };
                self.z80_emit(vec![Some(B(0xed)), Some(B(op))])
            }
            "in" => {
                if ops.len() == 1 && is_ind(a, "c") {
                    return self.z80_emit(vec![Some(B(0xed)), Some(B(0x70))]);
                }
                n_ops(self, 2)?;
                if is_reg(a, "a")
                    && let Some(v) = mem(b)
                {
                    return self.z80_emit(vec![Some(B(0xdb)), Some(N(v))]);
                }
                if is_ind(b, "c") {
                    if is_reg(a, "f") {
                        return self.z80_emit(vec![Some(B(0xed)), Some(B(0x70))]);
                    }
                    if let Some(x) = r8(a).filter(|x| x.p.is_none() && !x.mem) {
                        return self.z80_emit(vec![Some(B(0xed)), Some(B(0x40 | (x.c << 3)))]);
                    }
                }
                self.err(ILLEGAL_MODE)
            }
            "out" => {
                n_ops(self, 2)?;
                if let Some(v) = mem(a)
                    && is_reg(b, "a")
                {
                    return self.z80_emit(vec![Some(B(0xd3)), Some(N(v))]);
                }
                if is_ind(a, "c") {
                    if let Some(v) = imm(b)
                        && self.to_int(&v)? == 0.
                    {
                        return self.z80_emit(vec![Some(B(0xed)), Some(B(0x71))]);
                    }
                    if let Some(x) = r8(b).filter(|x| x.p.is_none() && !x.mem) {
                        return self.z80_emit(vec![Some(B(0xed)), Some(B(0x41 | (x.c << 3)))]);
                    }
                }
                self.err(ILLEGAL_MODE)
            }
            _ => self.err(ILLEGAL_MODE),
        }
    }
}

// ------------------------------------------------------------------------------ disassembler

const ZR: [&str; 8] = ["b", "c", "d", "e", "h", "l", "(hl)", "a"];
const ZRP: [&str; 4] = ["bc", "de", "hl", "sp"];
const ZRP2: [&str; 4] = ["bc", "de", "hl", "af"];
const ZALU: [&str; 8] = ["add a,", "adc a,", "sub ", "sbc a,", "and ", "xor ", "or ", "cp "];
const ZROT: [&str; 8] = ["rlc", "rrc", "rl", "rr", "sla", "sra", "sll", "srl"];
const ZBLOCK: [[&str; 4]; 4] = [
    ["ldi", "cpi", "ini", "outi"],
    ["ldd", "cpd", "ind", "outd"],
    ["ldir", "cpir", "inir", "otir"],
    ["lddr", "cpdr", "indr", "otdr"],
];

fn h2(v: u8) -> String {
    format!("${v:02x}")
}

fn h4(v: i64) -> String {
    format!("${:04x}", v & 0xffff)
}

fn sd(d: u8) -> i64 {
    d as i8 as i64
}

/// One instruction at `bytes[0..]`: its length and text, `None` for no instruction (or too few bytes).
fn decode(bytes: &[u8], a: i64) -> Option<(usize, String)> {
    let get = |k: usize| bytes.get(k).copied();
    let op0 = get(0)?;
    if op0 == 0xcb {
        let op = get(1)?;
        let (x, y, z) = (op >> 6, (op >> 3) & 7, op & 7);
        let text = if x == 0 {
            format!("{} {}", ZROT[y as usize], ZR[z as usize])
        } else {
            format!("{} {y},{}", ["", "bit", "res", "set"][x as usize], ZR[z as usize])
        };
        return Some((2, text));
    }
    if op0 == 0xed {
        let op = get(1)?;
        let (x, y, z) = (op >> 6, (op >> 3) & 7, op & 7);
        let (q, p) = (y & 1, (y >> 1) as usize);
        let nn = || -> Option<String> { Some(h4(get(2)? as i64 | (get(3)? as i64) << 8)) };
        if x == 1 {
            return Some(match z {
                0 => (2, if y == 6 { "in (c)".into() } else { format!("in {},(c)", ZR[y as usize]) }),
                1 => (2, if y == 6 { "out (c),0".into() } else { format!("out (c),{}", ZR[y as usize]) }),
                2 => (2, format!("{} hl,{}", if q == 1 { "adc" } else { "sbc" }, ZRP[p])),
                3 => {
                    if p == 2 {
                        return None;
                    }
                    let nn = nn()?;
                    (4, if q == 1 { format!("ld {},({nn})", ZRP[p]) } else { format!("ld ({nn}),{}", ZRP[p]) })
                }
                4 if y == 0 => (2, "neg".into()),
                5 if y == 0 => (2, "retn".into()),
                5 if y == 1 => (2, "reti".into()),
                6 => match y {
                    0 => (2, "im 0".into()),
                    2 => (2, "im 1".into()),
                    3 => (2, "im 2".into()),
                    _ => return None,
                },
                7 if y < 6 => (2, ["ld i,a", "ld r,a", "ld a,i", "ld a,r", "rrd", "rld"][y as usize].into()),
                _ => return None,
            });
        }
        if x == 2 && z <= 3 && y >= 4 {
            return Some((2, ZBLOCK[(y - 4) as usize][z as usize].into()));
        }
        return None;
    }
    let mut ix: Option<&str> = None;
    let mut pos = 0;
    if op0 == 0xdd || op0 == 0xfd {
        let name = if op0 == 0xdd { "ix" } else { "iy" };
        ix = Some(name);
        pos = 1;
        if get(1) == Some(0xcb) {
            let d = get(2)?;
            let op = get(3)?;
            let (x, y, z) = (op >> 6, (op >> 3) & 7, op & 7);
            if z != 6 {
                return None;
            }
            let m = format!("({name}{}{})", if sd(d) < 0 { '-' } else { '+' }, h2(sd(d).unsigned_abs() as u8));
            let text = if x == 0 {
                format!("{} {m}", ZROT[y as usize])
            } else {
                format!("{} {y},{m}", ["", "bit", "res", "set"][x as usize])
            };
            return Some((4, text));
        }
    }
    let op = get(pos)?;
    pos += 1;
    if ix.is_some() && matches!(op, 0xdd | 0xfd | 0xed | 0xcb) {
        return None;
    }

    struct Cur<'a> {
        bytes: &'a [u8],
        pos: usize,
        ix: Option<&'a str>,
        used: bool,
        disp: Option<i64>,
    }
    impl Cur<'_> {
        fn d(&mut self) -> Option<String> {
            if self.disp.is_none() {
                let d = *self.bytes.get(self.pos)?;
                self.pos += 1;
                self.disp = Some(sd(d));
            }
            let d = self.disp.unwrap();
            Some(format!("({}{}{})", self.ix.unwrap(), if d < 0 { '-' } else { '+' }, h2(d.unsigned_abs() as u8)))
        }
        fn r(&mut self, k: u8, plain: bool) -> Option<String> {
            if let Some(ix) = self.ix {
                if k == 6 {
                    self.used = true;
                    return self.d();
                }
                if !plain && (k == 4 || k == 5) {
                    self.used = true;
                    return Some(format!("{ix}{}", if k == 4 { 'h' } else { 'l' }));
                }
            }
            Some(ZR[k as usize].into())
        }
        fn hl(&mut self) -> String {
            match self.ix {
                Some(ix) => {
                    self.used = true;
                    ix.into()
                }
                None => "hl".into(),
            }
        }
        fn rp(&mut self, k: usize) -> String {
            if k == 2 { self.hl() } else { ZRP[k].into() }
        }
        fn rp2(&mut self, k: usize) -> String {
            if k == 2 { self.hl() } else { ZRP2[k].into() }
        }
        fn n(&mut self) -> Option<String> {
            let v = *self.bytes.get(self.pos)?;
            self.pos += 1;
            Some(h2(v))
        }
        fn nn(&mut self) -> Option<String> {
            let lo = *self.bytes.get(self.pos)?;
            let hi = *self.bytes.get(self.pos + 1)?;
            self.pos += 2;
            Some(h4(lo as i64 | (hi as i64) << 8))
        }
        fn rel(&mut self, a: i64) -> Option<String> {
            let d = *self.bytes.get(self.pos)?;
            self.pos += 1;
            Some(h4(a + self.pos as i64 + sd(d)))
        }
    }

    let mut c = Cur { bytes, pos, ix, used: false, disp: None };
    let (x, y, z) = (op >> 6, (op >> 3) & 7, op & 7);
    let (q, p) = (y & 1, (y >> 1) as usize);
    let text = match x {
        0 => match z {
            0 => match y {
                0 => "nop".into(),
                1 => "ex af,af'".into(),
                2 => format!("djnz {}", c.rel(a)?),
                3 => format!("jr {}", c.rel(a)?),
                _ => format!("jr {},{}", CC[(y - 4) as usize], c.rel(a)?),
            },
            1 => {
                if q == 1 {
                    let hl = c.hl();
                    format!("add {hl},{}", c.rp(p))
                } else {
                    let rp = c.rp(p);
                    format!("ld {rp},{}", c.nn()?)
                }
            }
            2 => match (q, p) {
                (0, 0) => "ld (bc),a".into(),
                (0, 1) => "ld (de),a".into(),
                (0, 2) => {
                    let nn = c.nn()?;
                    format!("ld ({nn}),{}", c.hl())
                }
                (0, _) => format!("ld ({}),a", c.nn()?),
                (_, 0) => "ld a,(bc)".into(),
                (_, 1) => "ld a,(de)".into(),
                (_, 2) => {
                    let hl = c.hl();
                    format!("ld {hl},({})", c.nn()?)
                }
                _ => format!("ld a,({})", c.nn()?),
            },
            3 => format!("{} {}", if q == 1 { "dec" } else { "inc" }, c.rp(p)),
            4 => format!("inc {}", c.r(y, false)?),
            5 => format!("dec {}", c.r(y, false)?),
            6 => {
                let r = c.r(y, false)?;
                format!("ld {r},{}", c.n()?)
            }
            _ => ["rlca", "rrca", "rla", "rra", "daa", "cpl", "scf", "ccf"][y as usize].into(),
        },
        1 => {
            if y == 6 && z == 6 {
                "halt".into()
            } else {
                let d = c.r(y, z == 6)?;
                format!("ld {d},{}", c.r(z, y == 6)?)
            }
        }
        2 => format!("{}{}", ZALU[y as usize], c.r(z, false)?),
        _ => match z {
            0 => format!("ret {}", CC[y as usize]),
            1 => match (q, p) {
                (0, _) => format!("pop {}", c.rp2(p)),
                (_, 0) => "ret".into(),
                (_, 1) => "exx".into(),
                (_, 2) => format!("jp ({})", c.hl()),
                _ => format!("ld sp,{}", c.hl()),
            },
            2 => format!("jp {},{}", CC[y as usize], c.nn()?),
            3 => match y {
                0 => format!("jp {}", c.nn()?),
                2 => format!("out ({}),a", c.n()?),
                3 => format!("in a,({})", c.n()?),
                4 => format!("ex (sp),{}", c.hl()),
                5 => "ex de,hl".into(),
                6 => "di".into(),
                _ => "ei".into(),
            },
            4 => format!("call {},{}", CC[y as usize], c.nn()?),
            5 => {
                if q == 0 {
                    format!("push {}", c.rp2(p))
                } else {
                    format!("call {}", c.nn()?)
                }
            }
            6 => format!("{}{}", ZALU[y as usize], c.n()?),
            _ => format!("rst {}", h2(y * 8)),
        },
    };
    if c.ix.is_some() && !c.used {
        return None;
    }
    Some((c.pos, text))
}

pub(super) fn disassemble(bytes: &[u8], addr: i64) -> Vec<Line> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let a = (addr + i as i64) & 0xffff;
        match decode(&bytes[i..], a) {
            Some((len, text)) => {
                out.push(Line { addr: a as u32, bytes: bytes[i..i + len].to_vec(), text, flags: None });
                i += len;
            }
            None => {
                out.push(Line { addr: a as u32, bytes: vec![bytes[i]], text: format!("!byte ${:02x}", bytes[i]), flags: None });
                i += 1;
            }
        }
    }
    out
}
