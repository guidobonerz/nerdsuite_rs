//! The opcodes of the 6502 family, by CPU: `{ mnemonic: { mode: opcode } }`, built from the same
//! tables as nerdsuite2's `acme.js`.

use std::{collections::HashMap, sync::LazyLock};

use indexmap::IndexMap;

/// An addressing mode of the 6502 family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Imp,
    Imm,
    Zp,
    Zpx,
    Zpy,
    Abs,
    Absx,
    Absy,
    Ind,
    Indx,
    Indy,
    Izp,
    Iax,
    Rel,
    Zprel,
    Sr,
    Sry,
    Ildp,
    Ildpy,
    Long,
    Longx,
    Ialong,
    Rell,
    Blk,
    Izpz,
    Imm16,
    Rel16,
    Izl,
    Izlq,
}

impl Mode {
    fn parse(name: &str) -> Mode {
        match name {
            "imp" => Mode::Imp,
            "imm" => Mode::Imm,
            "zp" => Mode::Zp,
            "zpx" => Mode::Zpx,
            "zpy" => Mode::Zpy,
            "abs" => Mode::Abs,
            "absx" => Mode::Absx,
            "absy" => Mode::Absy,
            "ind" => Mode::Ind,
            "indx" => Mode::Indx,
            "indy" => Mode::Indy,
            "izp" => Mode::Izp,
            "iax" => Mode::Iax,
            "rel" => Mode::Rel,
            "zprel" => Mode::Zprel,
            "sr" => Mode::Sr,
            "sry" => Mode::Sry,
            "ildp" => Mode::Ildp,
            "ildpy" => Mode::Ildpy,
            "long" => Mode::Long,
            "longx" => Mode::Longx,
            "ialong" => Mode::Ialong,
            "rell" => Mode::Rell,
            "blk" => Mode::Blk,
            "izpz" => Mode::Izpz,
            "imm16" => Mode::Imm16,
            "rel16" => Mode::Rel16,
            "izl" => Mode::Izl,
            "izlq" => Mode::Izlq,
            _ => panic!("unknown mode {name}"),
        }
    }

    /// The bytes of an instruction in this mode, the opcode included (its prefixes not).
    pub fn len(self) -> usize {
        match self {
            Mode::Imp => 1,
            Mode::Imm
            | Mode::Zp
            | Mode::Zpx
            | Mode::Zpy
            | Mode::Indx
            | Mode::Indy
            | Mode::Izp
            | Mode::Rel
            | Mode::Sr
            | Mode::Sry
            | Mode::Ildp
            | Mode::Ildpy
            | Mode::Izpz
            | Mode::Izl
            | Mode::Izlq => 2,
            Mode::Abs
            | Mode::Absx
            | Mode::Absy
            | Mode::Ind
            | Mode::Iax
            | Mode::Zprel
            | Mode::Ialong
            | Mode::Rell
            | Mode::Blk
            | Mode::Imm16
            | Mode::Rel16 => 3,
            Mode::Long | Mode::Longx => 4,
        }
    }

    /// The operand as the help of the editor writes it.
    pub fn syntax(self) -> &'static str {
        match self {
            Mode::Imp => "",
            Mode::Imm => "#imm",
            Mode::Zp => "zp",
            Mode::Zpx => "zp,x",
            Mode::Zpy => "zp,y",
            Mode::Abs => "abs",
            Mode::Absx => "abs,x",
            Mode::Absy => "abs,y",
            Mode::Ind => "(abs)",
            Mode::Indx => "(zp,x)",
            Mode::Indy => "(zp),y",
            Mode::Izp => "(zp)",
            Mode::Iax => "(abs,x)",
            Mode::Rel => "rel",
            Mode::Zprel => "zp,rel",
            Mode::Sr => "sr,s",
            Mode::Sry => "(sr,s),y",
            Mode::Ildp => "[dp]",
            Mode::Ildpy => "[dp],y",
            Mode::Long => "long",
            Mode::Longx => "long,x",
            Mode::Ialong => "[abs]",
            Mode::Rell => "rel16",
            Mode::Blk => "src,dst",
            Mode::Izpz => "(zp),z",
            Mode::Imm16 => "#imm16",
            Mode::Rel16 => "rel16",
            Mode::Izl => "[zp],z",
            Mode::Izlq => "[zp]",
        }
    }
}

/// The modes of a mnemonic and their opcodes; an opcode above $ff carries prefixes in its high
/// bytes (the 45GS02: `0x4242eab2` is `$42 $42 $ea $b2`).
pub type Modes = IndexMap<Mode, u32>;
pub type Table = IndexMap<String, Modes>;

/// The bytes of an opcode with its prefixes, in the order they are written.
pub fn op_bytes(mut op: u32) -> Vec<u8> {
    let mut out = Vec::new();
    while op > 0xff {
        out.insert(0, (op % 256) as u8);
        op /= 256;
    }
    out.insert(0, op as u8);
    out
}

fn parse_table(src: &str) -> Table {
    let mut table = Table::new();
    for line in src.lines() {
        let mut parts = line.split_whitespace();
        let Some(mnemonic) = parts.next() else { continue };
        let modes = parts
            .map(|part| {
                let (mode, hex) = part.split_once('=').unwrap();
                (Mode::parse(mode), u32::from_str_radix(hex, 16).unwrap())
            })
            .collect();
        table.insert(mnemonic.to_owned(), modes);
    }
    table
}

const BASE: &str = "
adc imm=69 zp=65 zpx=75 abs=6d absx=7d absy=79 indx=61 indy=71
and imm=29 zp=25 zpx=35 abs=2d absx=3d absy=39 indx=21 indy=31
asl imp=0a zp=06 zpx=16 abs=0e absx=1e
bcc rel=90
bcs rel=b0
beq rel=f0
bit zp=24 abs=2c
bmi rel=30
bne rel=d0
bpl rel=10
brk imp=00
bvc rel=50
bvs rel=70
clc imp=18
cld imp=d8
cli imp=58
clv imp=b8
cmp imm=c9 zp=c5 zpx=d5 abs=cd absx=dd absy=d9 indx=c1 indy=d1
cpx imm=e0 zp=e4 abs=ec
cpy imm=c0 zp=c4 abs=cc
dec zp=c6 zpx=d6 abs=ce absx=de
dex imp=ca
dey imp=88
eor imm=49 zp=45 zpx=55 abs=4d absx=5d absy=59 indx=41 indy=51
inc zp=e6 zpx=f6 abs=ee absx=fe
inx imp=e8
iny imp=c8
jmp abs=4c ind=6c
jsr abs=20
lda imm=a9 zp=a5 zpx=b5 abs=ad absx=bd absy=b9 indx=a1 indy=b1
ldx imm=a2 zp=a6 zpy=b6 abs=ae absy=be
ldy imm=a0 zp=a4 zpx=b4 abs=ac absx=bc
lsr imp=4a zp=46 zpx=56 abs=4e absx=5e
nop imp=ea
ora imm=09 zp=05 zpx=15 abs=0d absx=1d absy=19 indx=01 indy=11
pha imp=48
php imp=08
pla imp=68
plp imp=28
rol imp=2a zp=26 zpx=36 abs=2e absx=3e
ror imp=6a zp=66 zpx=76 abs=6e absx=7e
rti imp=40
rts imp=60
sbc imm=e9 zp=e5 zpx=f5 abs=ed absx=fd absy=f9 indx=e1 indy=f1
sec imp=38
sed imp=f8
sei imp=78
sta zp=85 zpx=95 abs=8d absx=9d absy=99 indx=81 indy=91
stx zp=86 zpy=96 abs=8e
sty zp=84 zpx=94 abs=8c
tax imp=aa
tay imp=a8
tsx imp=ba
txa imp=8a
txs imp=9a
tya imp=98
";

// the undocumented opcodes of the NMOS 6502 / 6510, ACME's mnemonics
const NMOS: &str = "
slo zp=07 zpx=17 abs=0f absx=1f absy=1b indx=03 indy=13
rla zp=27 zpx=37 abs=2f absx=3f absy=3b indx=23 indy=33
sre zp=47 zpx=57 abs=4f absx=5f absy=5b indx=43 indy=53
rra zp=67 zpx=77 abs=6f absx=7f absy=7b indx=63 indy=73
sax zp=87 zpy=97 abs=8f indx=83
lax imm=ab zp=a7 zpy=b7 abs=af absy=bf indx=a3 indy=b3
dcp zp=c7 zpx=d7 abs=cf absx=df absy=db indx=c3 indy=d3
isc zp=e7 zpx=f7 abs=ef absx=ff absy=fb indx=e3 indy=f3
anc imm=0b
asr imm=4b
arr imm=6b
sbx imm=cb
ane imm=8b
lxa imm=ab
sha absy=9f indy=93
shx absy=9e
shy absx=9c
tas absy=9b
las absy=bb
dop imm=80 zp=04 zpx=14
top abs=0c absx=1c
jam imp=02
";
const NOPX: &str = "nop imm=80 zp=04 zpx=14 abs=0c absx=1c";

const C02: &str = "
adc izp=72
and izp=32
cmp izp=d2
eor izp=52
lda izp=b2
ora izp=12
sbc izp=f2
sta izp=92
bit imm=89 zpx=34 absx=3c
dec imp=3a
inc imp=1a
jmp iax=7c
bra rel=80
phx imp=da
phy imp=5a
plx imp=fa
ply imp=7a
stz zp=64 zpx=74 abs=9c absx=9e
trb zp=14 abs=1c
tsb zp=04 abs=0c
";
const W02: &str = "wai imp=cb\nstp imp=db";

// the 65816: the 65C02 without the Rockwell bit opcodes, and its own modes and instructions
const W816: &str = "
ora sr=03 ildp=07 long=0f sry=13 ildpy=17 longx=1f
and sr=23 ildp=27 long=2f sry=33 ildpy=37 longx=3f
eor sr=43 ildp=47 long=4f sry=53 ildpy=57 longx=5f
adc sr=63 ildp=67 long=6f sry=73 ildpy=77 longx=7f
sta sr=83 ildp=87 long=8f sry=93 ildpy=97 longx=9f
lda sr=a3 ildp=a7 long=af sry=b3 ildpy=b7 longx=bf
cmp sr=c3 ildp=c7 long=cf sry=d3 ildpy=d7 longx=df
sbc sr=e3 ildp=e7 long=ef sry=f3 ildpy=f7 longx=ff
cop imm=02
wdm imm=42
jsl long=22
jml long=5c ialong=dc
jmp long=5c ialong=dc
jsr iax=fc
brl rell=82
per rell=62
pea abs=f4
pei izp=d4
mvp blk=44
mvn blk=54
phb imp=8b
phd imp=0b
phk imp=4b
plb imp=ab
pld imp=2b
rep imm=c2
sep imm=e2
rtl imp=6b
tcd imp=5b
tcs imp=1b
tdc imp=7b
tsc imp=3b
txy imp=9b
tyx imp=bb
wai imp=cb
stp imp=db
xba imp=eb
xce imp=fb
";

// the 65CE02: the R65C02 with a Z register, 16 bit branches (ACME: lbxx, bsr), (zp) is (zp),z
const CE02: &str = "
ora izpz=12
and izpz=32
eor izpz=52
adc izpz=72
sta izpz=92 sry=82
lda izpz=b2 sry=e2
cmp izpz=d2
sbc izpz=f2
cle imp=02
see imp=03
tsy imp=0b
inz imp=1b
tys imp=2b
dez imp=3b
neg imp=42
asr imp=43 zp=44 zpx=54
taz imp=4b
tab imp=5b
tza imp=6b
tba imp=7b
phz imp=db
plz imp=fb
jsr ind=22 iax=23
rtn imm=62
bsr rel16=63
lbpl rel16=13
lbmi rel16=33
lbvc rel16=53
lbvs rel16=73
lbra rel16=83
lbcc rel16=93
lbcs rel16=b3
lbne rel16=d3
lbeq rel16=f3
sty absx=8b
stx absy=9b
ldz imm=a3 abs=ab absx=bb
cpz imm=c2 zp=d4 abs=dc
dew zp=c3
inw zp=e3
asw abs=cb
row abs=eb
phw imm16=f4 abs=fc
";
const AUG: &str = "aug imp=5c";
// the 4502 (the C65, the MEGA65): aug is map, nop is eom too
const MAP: &str = "map imp=5c\neom imp=ea";

fn r02() -> String {
    let mut src = String::new();
    for n in 0..8 {
        src += &format!("rmb{n} zp={n}7\nsmb{n} zp={:x}7\n", n + 8);
        src += &format!("bbr{n} zprel={n}f\nbbs{n} zprel={:x}f\n", n + 8);
    }
    src
}

// the 45GS02 (the MEGA65, ACME's m65): [zp],z with 32 bit pointers ($ea), the quad instructions on
// Q = A X Y Z ($42 $42), both ($42 $42 $ea)
fn m65() -> String {
    let mut src = String::new();
    for (m, q, op) in [
        ("ora", "orq", 0x00),
        ("and", "andq", 0x20),
        ("eor", "eorq", 0x40),
        ("adc", "adcq", 0x60),
        ("sta", "stq", 0x80),
        ("lda", "ldq", 0xa0),
        ("cmp", "cpq", 0xc0),
        ("sbc", "sbcq", 0xe0),
    ] {
        let x = |n: u32| format!("{:02x}", op + n);
        src += &format!(
            "{m} izl=ea{}\n{q} zp=4242{} abs=4242{} izp=4242{} izlq=4242ea{}\n",
            x(0x12),
            x(5),
            x(0x0d),
            x(0x12),
            x(0x12)
        );
    }
    for (q, op) in [("aslq", 0x00), ("rolq", 0x20), ("lsrq", 0x40), ("rorq", 0x60)] {
        let x = |n: u32| format!("{:02x}", op + n);
        src += &format!(
            "{q} imp=4242{} zp=4242{} zpx=4242{} abs=4242{} absx=4242{}\n",
            x(0x0a),
            x(6),
            x(0x16),
            x(0x0e),
            x(0x1e)
        );
    }
    src += "
cmpq zp=4242c5 abs=4242cd izp=4242d2 izlq=4242ead2
inq imp=42421a zp=4242e6 zpx=4242f6 abs=4242ee absx=4242fe
deq imp=42423a zp=4242c6 zpx=4242d6 abs=4242ce absx=4242de
bitq zp=424224 abs=42422c
asrq imp=424243 zp=424244 zpx=424254
";
    src
}

/// The immediates that are 16 bits with `!al` (the accumulator) or `!rl` (the index registers).
pub const IMM_A: &[&str] = &["lda", "adc", "and", "bit", "cmp", "eor", "ora", "sbc"];
pub const IMM_X: &[&str] = &["ldx", "ldy", "cpx", "cpy"];

pub struct Cpu {
    pub table: Table,
    /// opcode -> (mnemonic, mode), the first part that has it wins
    pub reverse: Vec<Option<(String, Mode)>>,
    /// the opcodes with prefixes (the 45GS02)
    pub prefixed: IndexMap<u32, (String, Mode)>,
}

/// The tables of the 6502 family by their canonical names, in the order of `acme.js`.
pub static CPUS: LazyLock<IndexMap<&'static str, Cpu>> = LazyLock::new(|| {
    let base = parse_table(BASE);
    let nmos = parse_table(NMOS);
    let nopx = parse_table(NOPX);
    let c02 = parse_table(C02);
    let r02 = parse_table(&r02());
    let w02 = parse_table(W02);
    let w816 = parse_table(W816);
    let ce02 = parse_table(CE02);
    let aug = parse_table(AUG);
    let map = parse_table(MAP);
    let m65 = parse_table(&m65());
    // the 65C02 without (zp): the 65CE02 has (zp),z there
    let c02_ce: Table = c02
        .iter()
        .map(|(m, modes)| {
            let modes: Modes = modes
                .iter()
                .filter(|(mode, _)| **mode != Mode::Izp)
                .map(|(mode, op)| (*mode, *op))
                .collect();
            (m.clone(), modes)
        })
        .filter(|(_, modes)| !modes.is_empty())
        .collect();

    let parts: [(&str, Vec<&Table>); 9] = [
        ("6502", vec![&base]),
        ("nmos6502", vec![&base, &nmos, &nopx]),
        ("65c02", vec![&base, &c02]),
        ("r65c02", vec![&base, &c02, &r02]),
        ("w65c02", vec![&base, &c02, &r02, &w02]),
        ("65816", vec![&base, &c02, &w816]),
        ("65ce02", vec![&base, &c02_ce, &r02, &ce02, &aug]),
        ("4502", vec![&base, &c02_ce, &r02, &ce02, &map]),
        ("m65", vec![&base, &c02_ce, &r02, &ce02, &map, &m65]),
    ];
    parts
        .into_iter()
        .map(|(name, parts)| {
            let mut table = Table::new();
            let mut reverse = vec![None; 256];
            let mut prefixed = IndexMap::new();
            for part in parts {
                for (m, modes) in part {
                    let merged = table.entry(m.clone()).or_default();
                    for (mode, op) in modes {
                        merged.insert(*mode, *op);
                        if *op > 0xff {
                            prefixed.entry(*op).or_insert_with(|| (m.clone(), *mode));
                        } else if reverse[*op as usize].is_none() {
                            reverse[*op as usize] = Some((m.clone(), *mode));
                        }
                    }
                }
            }
            (name, Cpu { table, reverse, prefixed })
        })
        .collect()
});

/// The names `!cpu` takes, and their canonical table (`z80` and `m68k` have no table here).
pub fn canonical_cpu(name: &str) -> Option<&'static str> {
    Some(match name {
        "6502" => "6502",
        "nmos6502" | "6510" => "nmos6502",
        "65c02" => "65c02",
        "r65c02" => "r65c02",
        "w65c02" => "w65c02",
        "65816" => "65816",
        "65ce02" => "65ce02",
        "4502" => "4502",
        "m65" => "m65",
        "z80" => "z80",
        "68000" | "m68k" | "mc68000" => "m68k",
        _ => return None,
    })
}

/// Every mnemonic of the 6502 family, for "not available for CPU".
pub static ALL_MNEMONICS: LazyLock<HashMap<String, ()>> = LazyLock::new(|| {
    CPUS.values()
        .flat_map(|cpu| cpu.table.keys().cloned())
        .map(|m| (m, ()))
        .collect()
});
