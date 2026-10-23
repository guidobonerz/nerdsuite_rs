//! A 6502 for the import of .sid files, after nerdsuite2's `cpu6502.js`: it runs the init and the
//! play routine of a tune and the importer watches what they write to the SID. The legal opcodes,
//! without the decimal mode; an unknown opcode counts as a NOP (`unknown` counts them). An NMI the
//! importer asks for (`nmi_pending`) comes before the next instruction.

const N: u8 = 0x80;
const V: u8 = 0x40;
const B: u8 = 0x10;
const I: u8 = 0x04;
const Z: u8 = 0x02;
const C: u8 = 0x01;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Adc, And, Asl, Bcc, Bcs, Beq, Bmi, Bne, Bpl, Bvc, Bvs, Bit, Brk, Clc, Cld, Cli, Clv, Cmp, Cpx, Cpy,
    Dec, Dex, Dey, Eor, Inc, Inx, Iny, Jmp, Jsr, Lda, Ldx, Ldy, Lsr, Nop, Ora, Pha, Php, Pla, Plp,
    Rol, Ror, Rti, Rts, Sbc, Sec, Sed, Sei, Sta, Stx, Sty, Tax, Tay, Tsx, Txa, Txs, Tya,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Imp, Acc, Imm, Zp, Zpx, Zpy, Abs, Abx, Aby, Izx, Izy, Ind, Rel,
}

use Mode::*;

// the opcodes per instruction and addressing mode
#[rustfmt::skip]
const SPEC: &[(Op, &[(Mode, u8)])] = &[
    (Op::Adc, &[(Imm, 0x69), (Zp, 0x65), (Zpx, 0x75), (Abs, 0x6d), (Abx, 0x7d), (Aby, 0x79), (Izx, 0x61), (Izy, 0x71)]),
    (Op::And, &[(Imm, 0x29), (Zp, 0x25), (Zpx, 0x35), (Abs, 0x2d), (Abx, 0x3d), (Aby, 0x39), (Izx, 0x21), (Izy, 0x31)]),
    (Op::Asl, &[(Acc, 0x0a), (Zp, 0x06), (Zpx, 0x16), (Abs, 0x0e), (Abx, 0x1e)]),
    (Op::Bcc, &[(Rel, 0x90)]), (Op::Bcs, &[(Rel, 0xb0)]), (Op::Beq, &[(Rel, 0xf0)]), (Op::Bmi, &[(Rel, 0x30)]),
    (Op::Bne, &[(Rel, 0xd0)]), (Op::Bpl, &[(Rel, 0x10)]), (Op::Bvc, &[(Rel, 0x50)]), (Op::Bvs, &[(Rel, 0x70)]),
    (Op::Bit, &[(Zp, 0x24), (Abs, 0x2c)]), (Op::Brk, &[(Imp, 0x00)]),
    (Op::Clc, &[(Imp, 0x18)]), (Op::Cld, &[(Imp, 0xd8)]), (Op::Cli, &[(Imp, 0x58)]), (Op::Clv, &[(Imp, 0xb8)]),
    (Op::Cmp, &[(Imm, 0xc9), (Zp, 0xc5), (Zpx, 0xd5), (Abs, 0xcd), (Abx, 0xdd), (Aby, 0xd9), (Izx, 0xc1), (Izy, 0xd1)]),
    (Op::Cpx, &[(Imm, 0xe0), (Zp, 0xe4), (Abs, 0xec)]), (Op::Cpy, &[(Imm, 0xc0), (Zp, 0xc4), (Abs, 0xcc)]),
    (Op::Dec, &[(Zp, 0xc6), (Zpx, 0xd6), (Abs, 0xce), (Abx, 0xde)]), (Op::Dex, &[(Imp, 0xca)]), (Op::Dey, &[(Imp, 0x88)]),
    (Op::Eor, &[(Imm, 0x49), (Zp, 0x45), (Zpx, 0x55), (Abs, 0x4d), (Abx, 0x5d), (Aby, 0x59), (Izx, 0x41), (Izy, 0x51)]),
    (Op::Inc, &[(Zp, 0xe6), (Zpx, 0xf6), (Abs, 0xee), (Abx, 0xfe)]), (Op::Inx, &[(Imp, 0xe8)]), (Op::Iny, &[(Imp, 0xc8)]),
    (Op::Jmp, &[(Abs, 0x4c), (Ind, 0x6c)]), (Op::Jsr, &[(Abs, 0x20)]),
    (Op::Lda, &[(Imm, 0xa9), (Zp, 0xa5), (Zpx, 0xb5), (Abs, 0xad), (Abx, 0xbd), (Aby, 0xb9), (Izx, 0xa1), (Izy, 0xb1)]),
    (Op::Ldx, &[(Imm, 0xa2), (Zp, 0xa6), (Zpy, 0xb6), (Abs, 0xae), (Aby, 0xbe)]),
    (Op::Ldy, &[(Imm, 0xa0), (Zp, 0xa4), (Zpx, 0xb4), (Abs, 0xac), (Abx, 0xbc)]),
    (Op::Lsr, &[(Acc, 0x4a), (Zp, 0x46), (Zpx, 0x56), (Abs, 0x4e), (Abx, 0x5e)]), (Op::Nop, &[(Imp, 0xea)]),
    (Op::Ora, &[(Imm, 0x09), (Zp, 0x05), (Zpx, 0x15), (Abs, 0x0d), (Abx, 0x1d), (Aby, 0x19), (Izx, 0x01), (Izy, 0x11)]),
    (Op::Pha, &[(Imp, 0x48)]), (Op::Php, &[(Imp, 0x08)]), (Op::Pla, &[(Imp, 0x68)]), (Op::Plp, &[(Imp, 0x28)]),
    (Op::Rol, &[(Acc, 0x2a), (Zp, 0x26), (Zpx, 0x36), (Abs, 0x2e), (Abx, 0x3e)]),
    (Op::Ror, &[(Acc, 0x6a), (Zp, 0x66), (Zpx, 0x76), (Abs, 0x6e), (Abx, 0x7e)]),
    (Op::Rti, &[(Imp, 0x40)]), (Op::Rts, &[(Imp, 0x60)]),
    (Op::Sbc, &[(Imm, 0xe9), (Zp, 0xe5), (Zpx, 0xf5), (Abs, 0xed), (Abx, 0xfd), (Aby, 0xf9), (Izx, 0xe1), (Izy, 0xf1)]),
    (Op::Sec, &[(Imp, 0x38)]), (Op::Sed, &[(Imp, 0xf8)]), (Op::Sei, &[(Imp, 0x78)]),
    (Op::Sta, &[(Zp, 0x85), (Zpx, 0x95), (Abs, 0x8d), (Abx, 0x9d), (Aby, 0x99), (Izx, 0x81), (Izy, 0x91)]),
    (Op::Stx, &[(Zp, 0x86), (Zpy, 0x96), (Abs, 0x8e)]), (Op::Sty, &[(Zp, 0x84), (Zpx, 0x94), (Abs, 0x8c)]),
    (Op::Tax, &[(Imp, 0xaa)]), (Op::Tay, &[(Imp, 0xa8)]), (Op::Tsx, &[(Imp, 0xba)]),
    (Op::Txa, &[(Imp, 0x8a)]), (Op::Txs, &[(Imp, 0x9a)]), (Op::Tya, &[(Imp, 0x98)]),
];

const fn table() -> [Option<(Op, Mode)>; 256] {
    let mut t = [None; 256];
    let mut i = 0;
    while i < SPEC.len() {
        let (op, modes) = SPEC[i];
        let mut k = 0;
        while k < modes.len() {
            let (mode, code) = modes[k];
            t[code as usize] = Some((op, mode));
            k += 1;
        }
        i += 1;
    }
    t
}

static TABLE: [Option<(Op, Mode)>; 256] = table();

/// A call returns to here.
pub const SENTINEL: u16 = 0xfff0;

/// What the machine around the CPU does with its reads and writes.
pub trait Io {
    /// A read the machine answers; None: the memory.
    fn read(&mut self, _mem: &[u8], _addr: u16) -> Option<u8> {
        None
    }
    /// Sees every write, after it is in the memory; true: an NMI comes before the next instruction.
    fn write(&mut self, _mem: &[u8], _addr: u16, _value: u8) -> bool {
        false
    }
}

impl Io for () {}

/// The registers a [`Cpu::call`] starts with and how it ends.
#[derive(Clone, Copy, Debug)]
pub struct Call {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    /// the instructions at most
    pub max: usize,
    /// it returns with RTI: the return address and the flags are on the stack
    pub as_irq: bool,
    /// ... and A, X, Y of the Kernal's entry: $ea81 pulls them
    pub kernal: bool,
}

impl Default for Call {
    fn default() -> Self {
        Call { a: 0, x: 0, y: 0, max: 1_000_000, as_irq: false, kernal: false }
    }
}

pub struct Cpu<T: Io = ()> {
    /// 64K
    pub mem: Vec<u8>,
    pub io: T,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub s: u8,
    pub p: u8,
    pub pc: u16,
    /// the unknown opcodes it ran
    pub unknown: usize,
    pub nmi_pending: bool,
    broke: bool,
}

impl<T: Io> Cpu<T> {
    pub fn new(mut mem: Vec<u8>, io: T) -> Self {
        mem.resize(0x10000, 0);
        Cpu { mem, io, a: 0, x: 0, y: 0, s: 0xff, p: 0x24, pc: 0, unknown: 0, nmi_pending: false, broke: false }
    }

    fn read(&mut self, a: u16) -> u8 {
        self.io.read(&self.mem, a).unwrap_or(self.mem[a as usize])
    }

    fn write(&mut self, a: u16, v: u8) {
        self.mem[a as usize] = v;
        if self.io.write(&self.mem, a, v) {
            self.nmi_pending = true;
        }
    }

    fn push(&mut self, v: u8) {
        self.mem[0x100 + self.s as usize] = v;
        self.s = self.s.wrapping_sub(1);
    }

    fn pull(&mut self) -> u8 {
        self.s = self.s.wrapping_add(1);
        self.mem[0x100 + self.s as usize]
    }

    fn nz(&mut self, v: u8) -> u8 {
        self.p = (self.p & !(N | Z)) | (v & N) | if v == 0 { Z } else { 0 };
        v
    }

    fn flag(&mut self, f: u8, on: bool) {
        self.p = if on { self.p | f } else { self.p & !f };
    }

    /// Runs the routine at addr until it returns (RTS, or RTI with `as_irq`) or `max` instructions
    /// passed; false when it did not return.
    pub fn call(&mut self, addr: u16, o: Call) -> bool {
        self.a = o.a;
        self.x = o.x;
        self.y = o.y;
        self.s = 0xff;
        if o.as_irq {
            self.push((SENTINEL >> 8) as u8);
            self.push(SENTINEL as u8);
            self.push(self.p);
            if o.kernal {
                self.push(0);
                self.push(0);
                self.push(0);
            }
        } else {
            self.push(((SENTINEL - 1) >> 8) as u8);
            self.push((SENTINEL - 1) as u8);
        }
        self.pc = addr;
        self.broke = false;
        for _ in 0..o.max {
            // the return of the routine: at the mark, with the stack as it was - a routine that sets
            // the stack anew (TXS) and comes by the mark later has not returned
            if self.pc == SENTINEL && (self.s == 0xff || self.broke) {
                return true;
            }
            if self.nmi_pending {
                self.nmi_pending = false;
                self.nmi();
            }
            self.step();
        }
        false
    }

    /// The NMI: the return address and the flags on the stack, on to the vector at $fffa.
    pub fn nmi(&mut self) {
        self.push((self.pc >> 8) as u8);
        self.push(self.pc as u8);
        self.push((self.p & !B) | 0x20);
        self.flag(I, true);
        self.pc = self.mem[0xfffa] as u16 | (self.mem[0xfffb] as u16) << 8;
    }

    fn byte(&mut self) -> u8 {
        let v = self.mem[self.pc as usize];
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn word(&mut self) -> u16 {
        let lo = self.byte() as u16;
        lo | (self.byte() as u16) << 8
    }

    fn ptr(&self, z: u8) -> u16 {
        self.mem[z as usize] as u16 | (self.mem[z.wrapping_add(1) as usize] as u16) << 8
    }

    pub fn step(&mut self) {
        let code = self.mem[self.pc as usize];
        self.pc = self.pc.wrapping_add(1);
        let Some((op, mode)) = TABLE[code as usize] else {
            self.unknown += 1;
            return;
        };
        let addr: u16 = match mode {
            Imp | Acc => 0,
            Imm => {
                let a = self.pc;
                self.pc = self.pc.wrapping_add(1);
                a
            }
            Zp => self.byte() as u16,
            Zpx => self.byte().wrapping_add(self.x) as u16,
            Zpy => self.byte().wrapping_add(self.y) as u16,
            Abs => self.word(),
            Abx => self.word().wrapping_add(self.x as u16),
            Aby => self.word().wrapping_add(self.y as u16),
            Izx => {
                let z = self.byte().wrapping_add(self.x);
                self.ptr(z)
            }
            Izy => {
                let z = self.byte();
                self.ptr(z).wrapping_add(self.y as u16)
            }
            Ind => {
                let p = self.word();
                self.mem[p as usize] as u16 | (self.mem[((p & 0xff00) | (p.wrapping_add(1) & 255)) as usize] as u16) << 8
            }
            Rel => {
                let d = self.byte() as i8;
                self.pc.wrapping_add(d as u16)
            }
        };
        let value = |cpu: &mut Self| if mode == Acc { cpu.a } else { cpu.read(addr) };
        let store = |cpu: &mut Self, v: u8| if mode == Acc { cpu.a = v } else { cpu.write(addr, v) };
        let branch = |cpu: &mut Self, on: bool| {
            if on {
                cpu.pc = addr;
            }
        };
        let compare = |cpu: &mut Self, r: u8, v: u8| {
            cpu.flag(C, r >= v);
            cpu.nz(r.wrapping_sub(v));
        };
        let add = |cpu: &mut Self, v: u8| {
            let sum = cpu.a as u16 + v as u16 + (cpu.p & C) as u16;
            cpu.flag(V, !(cpu.a ^ v) & (cpu.a ^ sum as u8) & 0x80 != 0);
            cpu.flag(C, sum > 255);
            cpu.a = cpu.nz(sum as u8);
        };
        let p = self.p;
        match op {
            Op::Adc => {
                let v = value(self);
                add(self, v);
            }
            Op::Sbc => {
                let v = value(self);
                add(self, v ^ 255);
            }
            Op::And => {
                let v = self.a & value(self);
                self.a = self.nz(v);
            }
            Op::Ora => {
                let v = self.a | value(self);
                self.a = self.nz(v);
            }
            Op::Eor => {
                let v = self.a ^ value(self);
                self.a = self.nz(v);
            }
            Op::Asl => {
                let v = value(self);
                self.flag(C, v & 0x80 != 0);
                let r = self.nz(v << 1);
                store(self, r);
            }
            Op::Lsr => {
                let v = value(self);
                self.flag(C, v & 1 != 0);
                let r = self.nz(v >> 1);
                store(self, r);
            }
            Op::Rol => {
                let (v, c) = (value(self), self.p & C);
                self.flag(C, v & 0x80 != 0);
                let r = self.nz((v << 1) | c);
                store(self, r);
            }
            Op::Ror => {
                let (v, c) = (value(self), self.p & C);
                self.flag(C, v & 1 != 0);
                let r = self.nz((v >> 1) | (c << 7));
                store(self, r);
            }
            Op::Bit => {
                let v = value(self);
                self.flag(Z, self.a & v == 0);
                self.flag(N, v & N != 0);
                self.flag(V, v & V != 0);
            }
            Op::Bcc => branch(self, p & C == 0),
            Op::Bcs => branch(self, p & C != 0),
            Op::Beq => branch(self, p & Z != 0),
            Op::Bne => branch(self, p & Z == 0),
            Op::Bmi => branch(self, p & N != 0),
            Op::Bpl => branch(self, p & N == 0),
            Op::Bvc => branch(self, p & V == 0),
            Op::Bvs => branch(self, p & V != 0),
            // a tune does not break: the call ends
            Op::Brk => {
                self.pc = SENTINEL;
                self.broke = true;
            }
            Op::Clc => self.flag(C, false),
            Op::Sec => self.flag(C, true),
            Op::Cli => self.flag(I, false),
            Op::Sei => self.flag(I, true),
            Op::Clv => self.flag(V, false),
            Op::Cld | Op::Sed | Op::Nop => {}
            Op::Cmp => {
                let (r, v) = (self.a, value(self));
                compare(self, r, v);
            }
            Op::Cpx => {
                let (r, v) = (self.x, value(self));
                compare(self, r, v);
            }
            Op::Cpy => {
                let (r, v) = (self.y, value(self));
                compare(self, r, v);
            }
            Op::Dec => {
                let v = value(self).wrapping_sub(1);
                let r = self.nz(v);
                store(self, r);
            }
            Op::Inc => {
                let v = value(self).wrapping_add(1);
                let r = self.nz(v);
                store(self, r);
            }
            Op::Dex => self.x = self.nz(self.x.wrapping_sub(1)),
            Op::Dey => self.y = self.nz(self.y.wrapping_sub(1)),
            Op::Inx => self.x = self.nz(self.x.wrapping_add(1)),
            Op::Iny => self.y = self.nz(self.y.wrapping_add(1)),
            Op::Jmp => self.pc = addr,
            Op::Jsr => {
                let r = self.pc.wrapping_sub(1);
                self.push((r >> 8) as u8);
                self.push(r as u8);
                self.pc = addr;
            }
            Op::Rts => {
                let lo = self.pull() as u16;
                let hi = self.pull() as u16;
                self.pc = (lo | hi << 8).wrapping_add(1);
            }
            Op::Rti => {
                self.p = (self.pull() & !B) | 0x20;
                let lo = self.pull() as u16;
                let hi = self.pull() as u16;
                self.pc = lo | hi << 8;
            }
            Op::Lda => {
                let v = value(self);
                self.a = self.nz(v);
            }
            Op::Ldx => {
                let v = value(self);
                self.x = self.nz(v);
            }
            Op::Ldy => {
                let v = value(self);
                self.y = self.nz(v);
            }
            Op::Sta => self.write(addr, self.a),
            Op::Stx => self.write(addr, self.x),
            Op::Sty => self.write(addr, self.y),
            Op::Pha => self.push(self.a),
            Op::Php => self.push(self.p | B),
            Op::Pla => {
                let v = self.pull();
                self.a = self.nz(v);
            }
            Op::Plp => self.p = (self.pull() & !B) | 0x20,
            Op::Tax => self.x = self.nz(self.a),
            Op::Tay => self.y = self.nz(self.a),
            Op::Tsx => self.x = self.nz(self.s),
            Op::Txa => self.a = self.nz(self.x),
            Op::Txs => self.s = self.x,
            Op::Tya => self.a = self.nz(self.y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A CPU with the code at $1000, called there.
    fn run(code: &[u8]) -> Cpu {
        let mut mem = vec![0; 0x10000];
        mem[0x1000..0x1000 + code.len()].copy_from_slice(code);
        let mut cpu = Cpu::new(mem, ());
        assert!(cpu.call(0x1000, Call::default()));
        cpu
    }

    #[test]
    fn adc_and_sbc_flags() {
        // CLC; LDA #$50; ADC #$50; RTS: an overflow into the sign
        let cpu = run(&[0x18, 0xa9, 0x50, 0x69, 0x50, 0x60]);
        assert_eq!(cpu.a, 0xa0);
        assert_eq!(cpu.p & (N | V | C | Z), N | V);
        // CLC; LDA #$ff; ADC #$01: a carry, zero
        let cpu = run(&[0x18, 0xa9, 0xff, 0x69, 0x01, 0x60]);
        assert_eq!(cpu.a, 0);
        assert_eq!(cpu.p & (N | V | C | Z), C | Z);
        // SEC; LDA #0; SBC #1: a borrow
        let cpu = run(&[0x38, 0xa9, 0x00, 0xe9, 0x01, 0x60]);
        assert_eq!(cpu.a, 0xff);
        assert_eq!(cpu.p & (N | V | C | Z), N);
        // SEC; LDA #$80; SBC #1: an overflow out of the sign
        let cpu = run(&[0x38, 0xa9, 0x80, 0xe9, 0x01, 0x60]);
        assert_eq!(cpu.a, 0x7f);
        assert_eq!(cpu.p & (N | V | C | Z), V | C);
    }

    #[test]
    fn branches_and_subroutines() {
        // LDX #5; LDY #0; loop: INY; DEX; BNE loop; JSR sub; RTS; sub: LDA #42; STA $2000; RTS
        let cpu = run(&[0xa2, 0x05, 0xa0, 0x00, 0xc8, 0xca, 0xd0, 0xfc, 0x20, 0x0c, 0x10, 0x60, 0xa9, 42, 0x8d, 0x00, 0x20, 0x60]);
        assert_eq!((cpu.x, cpu.y, cpu.a), (0, 5, 42));
        assert_eq!(cpu.mem[0x2000], 42);
        assert_eq!(cpu.s, 0xff);
        // CMP: LDA #3; CMP #5; BCS +2; LDX #1; RTS
        let cpu = run(&[0xa9, 0x03, 0xc9, 0x05, 0xb0, 0x02, 0xa2, 0x01, 0x60]);
        assert_eq!(cpu.x, 1);
    }

    #[test]
    fn unknown_opcodes_are_nops() {
        // $02 (a JAM) and $ff are not known: they pass
        let cpu = run(&[0x02, 0xff, 0xa9, 0x07, 0x60]);
        assert_eq!((cpu.a, cpu.unknown), (7, 2));
        // a routine that never returns
        let mut mem = vec![0; 0x10000];
        mem[0x1000..0x1003].copy_from_slice(&[0x4c, 0x00, 0x10]);
        let mut cpu = Cpu::new(mem, ());
        assert!(!cpu.call(0x1000, Call { max: 1000, ..Default::default() }));
    }

    #[test]
    fn irq_returns_with_rti() {
        // INC $2000; RTI
        let mut mem = vec![0; 0x10000];
        mem[0x1000..0x1004].copy_from_slice(&[0xee, 0x00, 0x20, 0x40]);
        let mut cpu = Cpu::new(mem, ());
        assert!(cpu.call(0x1000, Call { as_irq: true, ..Default::default() }));
        assert_eq!(cpu.mem[0x2000], 1);
    }
}
