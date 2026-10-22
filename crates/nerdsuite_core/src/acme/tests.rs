//! The tests of nerdsuite2's `acme.test.js`, `acme-m65.test.js` and `acme-z80.test.js`.

use super::*;

fn asm_with(src: &str, opts: &Options, extra: &[(&str, &[u8])]) -> Assembly {
    let mut files = vec![("main.asm".to_owned(), src.as_bytes().to_vec())];
    files.extend(extra.iter().map(|(p, d)| (p.to_string(), d.to_vec())));
    assemble("main.asm", files, opts)
}

fn asm(src: &str) -> Assembly {
    asm_with(src, &Options::default(), &[])
}

fn msgs(r: &Assembly) -> Vec<(usize, String)> {
    r.errors.iter().map(|e| (e.line.unwrap_or(0), e.msg.clone())).collect()
}

/// Assembles, asserts success and gives the bytes without the load address.
fn bytes_with(src: &str, opts: &Options, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let r = asm_with(src, opts, extra);
    assert_eq!(r.errors, vec![], "unexpected errors in {src}");
    assert!(r.ok);
    r.output.unwrap()[2..].to_vec()
}

fn bytes(src: &str) -> Vec<u8> {
    bytes_with(src, &Options::default(), &[])
}

fn h(s: &str) -> Vec<u8> {
    s.split_whitespace().map(|x| u8::from_str_radix(x, 16).unwrap()).collect()
}

fn plain(cpu: &str) -> Options {
    Options { cpu: Some(cpu.into()), format: Some("plain".into()), ..Default::default() }
}

// ------------------------------------------------------------------------------ opcodes

const LEGAL: &str = "
adc imm=69 zp=65 zpx=75 abs=6d absx=7d absy=79 indx=61 indy=71
and imm=29 zp=25 zpx=35 abs=2d absx=3d absy=39 indx=21 indy=31
asl imp=0a zp=06 zpx=16 abs=0e absx=1e
bcc rel=90
bcs rel=b0
beq rel=f0
bmi rel=30
bne rel=d0
bpl rel=10
bvc rel=50
bvs rel=70
bit zp=24 abs=2c
brk imp=00
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

/// The operand of a test instruction in a mode, and the bytes it makes after the opcode.
fn operand(mode: Mode) -> (&'static str, Vec<u8>) {
    match mode {
        Mode::Imp => ("", vec![]),
        Mode::Imm => ("#$12", vec![0x12]),
        Mode::Zp => ("$12", vec![0x12]),
        Mode::Zpx => ("$12,x", vec![0x12]),
        Mode::Zpy => ("$12,y", vec![0x12]),
        Mode::Abs => ("$1234", vec![0x34, 0x12]),
        Mode::Absx => ("$1234,x", vec![0x34, 0x12]),
        Mode::Absy => ("$1234,y", vec![0x34, 0x12]),
        Mode::Ind => ("($1234)", vec![0x34, 0x12]),
        Mode::Indx => ("($12,x)", vec![0x12]),
        Mode::Indy => ("($12),y", vec![0x12]),
        Mode::Izp => ("($12)", vec![0x12]),
        Mode::Iax => ("($1234,x)", vec![0x34, 0x12]),
        Mode::Rel => ("* + $12", vec![0x10]),
        Mode::Zprel => ("$12, * + $13", vec![0x12, 0x10]),
        Mode::Sr => ("$12,s", vec![0x12]),
        Mode::Sry => ("($12,s),y", vec![0x12]),
        Mode::Ildp => ("[$12]", vec![0x12]),
        Mode::Ildpy => ("[$12],y", vec![0x12]),
        Mode::Long => ("$123456", vec![0x56, 0x34, 0x12]),
        Mode::Longx => ("$123456,x", vec![0x56, 0x34, 0x12]),
        Mode::Ialong => ("[$1234]", vec![0x34, 0x12]),
        Mode::Rell => ("* + $1234", vec![0x31, 0x12]),
        Mode::Blk => ("$01, $02", vec![0x02, 0x01]),
        Mode::Izpz => ("($12),z", vec![0x12]),
        Mode::Imm16 => ("#$1234", vec![0x34, 0x12]),
        Mode::Rel16 => ("* + $1234", vec![0x32, 0x12]),
        Mode::Izl => ("[$12],z", vec![0x12]),
        Mode::Izlq => ("[$12]", vec![0x12]),
    }
}

#[test]
fn all_151_documented_6502_opcodes() {
    let mut count = 0;
    for line in LEGAL.lines().filter(|l| !l.trim().is_empty()) {
        let mut parts = line.split_whitespace();
        let mn = parts.next().unwrap();
        for part in parts {
            let (mode, op) = part.split_once('=').unwrap();
            let op = u8::from_str_radix(op, 16).unwrap();
            let table_mode = *CPUS["6502"].table[mn].iter().find(|(_, o)| **o == op as u32).unwrap().0;
            let (text, tail) = operand(table_mode);
            let mut want = vec![op];
            want.extend(tail);
            assert_eq!(bytes(&format!("*= $1000\n {mn} {text}\n")), want, "{mn} {mode}");
            count += 1;
        }
    }
    assert_eq!(count, 151);
    // the table has no more than these
    assert_eq!(CPUS["6502"].table.values().map(|m| m.len()).sum::<usize>(), 151);
}

#[test]
fn mnemonics_are_case_insensitive() {
    assert_eq!(bytes("*=$1000\n LDA #$01\n Sta $D020\n"), h("a9 01 8d 20 d0"));
}

#[test]
fn undocumented_nmos_opcodes() {
    let src = "*= $1000
!cpu 6510
  slo $12
  rla $1234,x
  sre ($12),y
  rra ($12,x)
  sax $12,y
  lax $1234,y
  dcp $12,x
  isc $1234
  anc #$01
  asr #$02
  arr #$03
  sbx #$04
  ane #$05
  lxa #$06
  sha $1234,y
  shx $1234,y
  shy $1234,x
  tas $1234,y
  las $1234,y
  dop #$07
  top $1234
  jam
  nop $12
";
    assert_eq!(
        bytes(src),
        h("07 12  3f 34 12  53 12  63 12  97 12  bf 34 12  d7 12  ef 34 12
      0b 01  4b 02  6b 03  cb 04  8b 05  ab 06  9f 34 12  9e 34 12  9c 34 12  9b 34 12  bb 34 12
      80 07  0c 34 12  02  04 12")
    );
    let r = asm("*=$1000\n  lax $12\n");
    assert!(!r.ok);
    assert!(r.errors[0].msg.contains("not available for CPU \"6502\""));
}

#[test]
fn cmos_extensions() {
    let src = "*= $1000
!cpu 65c02 {
  phx
  ply
  stz $12
  stz $1234,x
  bra *
  lda ($12)
  jmp ($1234,x)
  inc
  bit #$01
  tsb $12
}
!cpu r65c02 {
  rmb3 $12
  smb7 $12
  bbr0 $12, *
  bbs7 $12, *
}
!cpu w65c02 { wai : stp }
";
    assert_eq!(
        bytes(src),
        h("da 7a 64 12 9e 34 12 80 fe b2 12 7c 34 12 1a 89 01 04 12
      37 12 f7 12 0f 12 fd ff 12 fd cb db")
    );
}

#[test]
fn unsupported_cpus() {
    let r = asm("!cpu c64dtv2\n");
    assert!(!r.ok);
    assert!(r.errors[0].msg.contains("Unsupported CPU \"c64dtv2\""));
    assert!(!asm("*=$1000\n!al\n").ok);
    let opts = Options { cpu: Some("c64dtv2".into()), ..Default::default() };
    assert!(!asm_with("*=$1000\nnop", &opts, &[]).ok);
}

#[test]
fn size_postfixes_and_indirect() {
    assert_eq!(bytes("*=$1000\n lda+2 $12\n lda+1 $12\n sta+2 $fe,x\n"), h("ad 12 00 a5 12 9d fe 00"));
    assert!(!asm("*=$1000\n lda+1 $1234\n").ok);
    assert_eq!(bytes("*=$1000\n lda (2+3)*4\n lda (2+3)*400,x\n jmp ($fffc)\n"), h("a5 14 bd d0 07 6c fc ff"));
    let r = asm("*=$1000\n jmp ($10ff)\n");
    assert!(r.ok);
    assert!(r.warnings[0].msg.contains("buggy JMP"));
}

// ------------------------------------------------------------------------------ symbols, passes

#[test]
fn forward_references_and_zero_page() {
    let src = "*= $1000
  lda fwd        ; forward ref -> absolute (like ACME)
  lda+1 fwd      ; forced zp
fwd = $fb
  lda fwd        ; symbol was undefined in pass 1 -> stays absolute
back = $fc
  lda back       ; known -> zero page
  lda $00fc      ; leading zeros force absolute
  jmp later
  ldx back,y
  stx back,y
  sta back,y     ; no zp,y for sta -> absolute
later rts
";
    assert_eq!(bytes(src), h("ad fb 00 a5 fb ad fb 00 a5 fc ad fc 00 4c 17 10 b6 fc 96 fc 99 fc 00 60"));
}

#[test]
fn labels_anonymous_zones_cheap() {
    assert_eq!(bytes("*=$1000\na: nop : b nop\nc\n jmp a : jmp b : jmp c\n"), h("ea ea 4c 00 10 4c 01 10 4c 02 10"));
    let src = "*= $1000
--  nop
-   dex
    bne -
    bne --
    beq +
    beq ++
+   nop
++  rts
";
    assert_eq!(bytes(src), h("ea ca d0 fd d0 fa f0 02 f0 01 ea 60"));
    let src = "*= $1000
!zone a
.l  nop
    jmp .l
!zone b
.l  nop
    jmp .l
!zone c {
.l  rts
}
    jmp .l
!zn
    !byte <.l2
.l2
";
    let r = asm(src);
    assert!(r.ok, "{:?}", r.errors);
    assert_eq!(r.output.as_ref().unwrap()[2..], h("ea 4c 00 10 ea 4c 04 10 60 4c 04 10 0d"));
    let mut locals: Vec<_> = r
        .symbols
        .iter()
        .filter(|s| s.name == ".l")
        .map(|s| (s.zone.clone().unwrap(), s.value))
        .collect();
    locals.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(locals, vec![("a".into(), 4096.), ("b".into(), 4100.), ("c".into(), 4104.)]);
    assert_eq!(
        bytes("*= $1000\nfirst\n@l  nop\n    jmp @l\nsecond\n@l  nop\n    jmp @l\n"),
        h("ea 4c 00 10 ea 4c 04 10")
    );
}

#[test]
fn symbol_definitions() {
    let r = asm("*=$1000\nfoo nop\nfoo nop\n");
    assert!(!r.ok);
    assert_eq!(msgs(&r), vec![(3, "Symbol already defined.".into())]);
    assert_eq!(bytes("*=$1000\n!set x = 1\n!byte x\n!set x = x + 1\n!byte x\ny = 3\ny = 3\n!byte y"), vec![1, 2, 3]);
    let r = asm("*=$1000\n nop\n lda missing\n");
    assert!(!r.ok);
    assert_eq!(r.output, None);
    assert_eq!(msgs(&r), vec![(3, "Symbol not defined (missing).".into())]);
}

#[test]
fn defines_and_ifdef() {
    let src = "*=$1000\n!ifdef DEBUG { !byte DEBUG } else { !byte 0 }\n!ifndef DEBUG !byte $ff\n";
    let opts = Options { defines: vec![("DEBUG".into(), Define::Num(7.))], ..Default::default() };
    assert_eq!(bytes_with(src, &opts, &[]), vec![7]);
    assert_eq!(bytes(src), vec![0, 0xff]);
    let lib = b"!ifndef LIB_A { LIB_A = 1\nlibfn rts\n}\n";
    let src = "*=$1000\n!src \"lib.a\"\n!src \"lib.a\"\n jsr libfn\n";
    assert_eq!(bytes_with(src, &Options::default(), &[("lib.a", lib)]), h("60 20 00 10"));
}

#[test]
fn symbols_output() {
    let r = asm("*=$1000\nstart nop\nCONST = 5\n!zone z\n.loc rts\n");
    let by = |name: &str| r.symbols.iter().find(|s| s.name == name).unwrap().clone();
    assert_eq!(
        by("start"),
        Symbol { name: "start".into(), value: 4096., file: Some("main.asm".into()), line: 2, is_label: true, zone: None }
    );
    assert_eq!(
        by("CONST"),
        Symbol { name: "CONST".into(), value: 5., file: Some("main.asm".into()), line: 3, is_label: false, zone: None }
    );
    assert_eq!(by(".loc").zone.as_deref(), Some("z"));
}

// ------------------------------------------------------------------------------ expressions

#[test]
fn expressions() {
    let src = "*=$1000
!byte <$1234+1, >$1234, ^$123456
!byte 2+3*4, 2^3, (-2^2) & $ff, 7 DIV 2, 7 % 3, 7 MOD 4, 1 << 3, $f0 >> 4, $80 >>> 7
!byte %..##, %0101, &17, 0x10, 0b11, 'A', \"B\", 10
!byte 3 = 3, 3 == 3, 3 != 4, 3 <> 3, 2 < 3, 3 <= 2, 4 > 3, 4 >= 5
!byte $0f XOR $ff, $0f EOR $f0, $f0 OR $0f, $f0 | 1, $f0 AND $3c, $f0 & $3c, NOT 0 & $ff, !$f0 & $ff
!byte 1 ASL 2, 8 LSR 1, 2 ** 3
!byte int(1.5 * 3), int(sin(0)), len([1,2,3]), len(\"abcd\"), is_number(1), is_string(\"x\"), is_list([1])
!byte [10, 20, 30][1], \"xyz\"[2]
!word int(1000.7), float(2) * 1.5
!byte *-$1000
";
    assert_eq!(
        bytes(src),
        vec![
            0x35, 0x12, 0x12, 14, 8, 0xfc, 3, 1, 3, 8, 0x0f, 1, 3, 5, 15, 16, 3, 0x41, 0x42, 10, 1, 1, 1, 0, 1, 0, 1, 0,
            0xf0, 0xff, 0xff, 0xf1, 0x30, 0x30, 0xff, 0x0f, 4, 4, 8, 4, 0, 3, 4, 1, 1, 1, 20, 0x7a, 0xe8, 0x03, 3, 0, 52,
        ]
    );
    assert_eq!(bytes("*=$c000\n lda #<tab\n ldx #>tab\ntab !byte 0\n"), h("a9 04 a2 c0 00"));
    let r = asm("*=$1000\n!byte 1/0\n");
    assert_eq!(r.errors[0].msg, "Division by zero.");
}

// ------------------------------------------------------------------------------ macros

#[test]
fn macros() {
    let src = "*= $1000
!macro poke .addr, .val {
    lda #.val
    sta .addr
}
!macro poke2 .a, .v {
    +poke .a, .v
    +poke .a + 1, .v
}
!macro setref ~.sym, .val {
    .sym = .val
}
!macro wait {
-   dex
    bne -
.l  bne .l
}
    +poke $d020, 0
    +poke2 $d020, 1
    +setref ~result, 42
    !byte result
    +wait
    +wait
";
    assert_eq!(
        bytes(src),
        h("a9 00 8d 20 d0  a9 01 8d 20 d0 a9 01 8d 21 d0  2a
      ca d0 fd d0 fe  ca d0 fd d0 fe")
    );
    assert_eq!(bytes("*=$1000\n +m 5\n jmp end\n!macro m .x { !byte .x }\nend rts\n"), h("05 4c 04 10 60"));
    assert_eq!(bytes("*=$1000\n!macro inc ~.v { !set .v = .v + 1 }\n!set n = 1\n+inc ~n\n+inc ~n\n!byte n\n"), vec![3]);
    assert!(asm("*=$1000\n+nothere\n").errors[0].msg.contains("Macro not defined"));
    assert!(asm("*=$1000\n!macro m .a { nop }\n+m\n").errors[0].msg.contains("Wrong number of macro arguments"));
    assert!(asm("*=$1000\n!macro rec { +rec }\n+rec\n").errors[0].msg.contains("Too deeply nested"));
}

// ------------------------------------------------------------------------------ flow control

#[test]
fn flow_control() {
    let src = "*= $1000
!for i, 1, 4 { !byte i }
!for i, 3, 1 { !byte i }
!for v in [5, 6, 7] { !byte v }
!for c in \"ab\" { !byte c }
!set c = 0
!do { !byte c : !set c = c + 1 } while c < 3
!set c = 0
!do until c = 2 { !byte $10 + c : !set c = c + 1 }
!set c = 3
!while c > 0 { !byte c : !set c = c - 1 }
!if 1 { !byte $aa } else { !byte $bb }
!if 0 { !byte $aa } else if 1 { !byte $cc } else { !byte $bb }
!if 0 {
  !byte 1
} else if 0 {
  !byte 2
} else {
  !byte $dd
}
!ifdef UNDEFINED_X { !byte 1 } else { !byte 2 }
!ifndef UNDEFINED_X { !byte 3 }
";
    assert_eq!(bytes(src), h("01 02 03 04 03 02 01 05 06 07 61 62 00 01 02 10 11 03 02 01 aa cc dd 02 03"));
    assert_eq!(bytes("*=$1000\n!if MODE = 2 { !byte 2 } else { !byte 1 }\nMODE = 2\n"), vec![2]);
}

#[test]
fn eof_warn_error_serious() {
    assert_eq!(bytes("*=$1000\n!byte 1\n!eof\n!byte 2\n"), vec![1]);
    let r = asm("*=$1000\n!warn \"value is \", 5\n");
    assert!(r.ok);
    assert_eq!(
        r.warnings,
        vec![Diagnostic { file: "main.asm".into(), line: Some(2), col: Some(21), msg: "value is 5".into() }]
    );
    let r = asm("*=$1000\n!error \"bad\"\n!byte 1\n!error \"worse\"\n");
    assert_eq!(msgs(&r), vec![(2, "bad".into()), (4, "worse".into())]);
    let r = asm("*=$1000\n!serious \"stop\"\n!error \"not reached\"\n");
    assert_eq!(r.errors.iter().map(|e| e.msg.as_str()).collect::<Vec<_>>(), vec!["stop"]);
}

// ------------------------------------------------------------------------------ data

#[test]
fn data_pseudo_opcodes() {
    let src = "*= $1000
!by 1 : !08 2 : !8 3
!wo $1234 : !16 $5678 : !le16 $9abc : !be16 $1234
!24 $123456 : !be24 $123456
!32 $12345678 : !be32 $12345678
!hex 01 02 ff
!h 0a0b
!fill 3, $aa
!fi 2
!skip 2
!byte $ee
!align 3, 0
!byte $01
";
    assert_eq!(
        bytes(src),
        h("01 02 03  34 12 78 56 bc 9a 12 34  56 34 12 12 34 56  78 56 34 12 12 34 56 78
      01 02 ff 0a 0b aa aa aa 00 00 00 00 ee ea ea 01")
    );
    assert_eq!(bytes("*=$1000\n!initmem $ff\n!byte 1\n!skip 2\n!byte 2\n"), h("01 ff ff 02"));
    let r = asm("*=$1000\n!byte 256\n lda #$100\n!word $10000\n!byte -128, -129\n");
    let out_of_range = "Number out of range.".to_owned();
    assert_eq!(
        msgs(&r),
        vec![(2, out_of_range.clone()), (3, out_of_range.clone()), (4, out_of_range.clone()), (5, out_of_range)]
    );
}

#[test]
fn texts() {
    let src = "*= $1000
!text \"AbC@\", 13
!pet \"AbC@z\"
!raw \"AbC\"
!scr \"AbC@[az\"
!scrxor $80, \"a@\"
!ct scr { !tx \"ab\" : lda #'a' }
!ct pet
!tx \"a\"
lda #'a'
!ct raw
!tx \"a\"
!byte \"AB\"
!text \"a\\\"b\\\\\"
";
    assert_eq!(
        bytes(src),
        h("41 62 43 40 0d  c1 42 c3 40 5a  41 62 43  41 02 43 00 1b 01 1a  81 80  01 02 a9 01
      41 a9 41  61  41 42  61 22 62 5c")
    );
    let tab: Vec<u8> = (0..=255u8).map(|i| 255 - i).collect();
    assert_eq!(
        bytes_with("*=$1000\n!ct \"tab.bin\" { !text \"A\" }\n!text \"A\"", &Options::default(), &[("tab.bin", &tab)]),
        vec![0xbe, 0x41]
    );
    assert_eq!(bytes("*=$1000\n!xor $ff { !byte 0, 1 }\n!byte 0\n!xor $0f\n!byte $f0\n"), h("ff fe 00 ff"));
}

#[test]
fn binary_and_source() {
    let data = [1u8, 2, 3, 4, 5];
    let src = "*=$1000\n!bin \"data.bin\"\n!binary \"data.bin\", 2, 1\n!bi \"data.bin\",,3\n!bin \"data.bin\", 7\n";
    assert_eq!(
        bytes_with(src, &Options::default(), &[("data.bin", &data)]),
        h("01 02 03 04 05  02 03  04 05  01 02 03 04 05 00 00")
    );
    let r = asm("*=$1000\n!bin \"nope.bin\"\n");
    assert_eq!(msgs(&r), vec![(2, "Cannot open input file \"nope.bin\".".into())]);

    let files: Vec<(String, Vec<u8>)> = [
        ("src/main.asm", b"*= $1000\n!src \"inc/a.asm\"\n!source \"common.asm\"\n!src <std/macros.a>\n+m\n".to_vec()),
        ("src/inc/a.asm", b"!byte 1\n!src \"b.asm\"\n".to_vec()),
        ("src/inc/b.asm", b"!byte 2\n!bin \"../data/x.bin\"\n".to_vec()),
        ("src/data/x.bin", vec![9]),
        ("common.asm", b"!byte 3\n".to_vec()),
        ("lib/std/macros.a", b"!macro m { !byte 4 }\n".to_vec()),
    ]
    .into_iter()
    .map(|(p, d)| (p.to_owned(), d))
    .collect();
    let r = assemble("src/main.asm", files, &Options::default());
    assert_eq!(r.errors, vec![]);
    assert_eq!(r.output.as_ref().unwrap()[2..], [1, 2, 9, 3, 4]);
    assert_eq!(
        r.lines.iter().map(|l| (l.file.as_str(), l.line, l.addr)).collect::<Vec<_>>(),
        vec![
            ("src/inc/a.asm", 1, 0x1000),
            ("src/inc/b.asm", 1, 0x1001),
            ("src/inc/b.asm", 2, 0x1002),
            ("common.asm", 1, 0x1003),
            ("lib/std/macros.a", 1, 0x1004),
            ("src/main.asm", 5, 0x1004),
        ]
    );
    let files = vec![
        ("main.asm".to_owned(), b"*=$1000\n!src \"inc.asm\"\n".to_vec()),
        ("inc.asm".to_owned(), b"nop\n\n lda #\n".to_vec()),
    ];
    let r = assemble("main.asm", files, &Options::default());
    assert_eq!(r.errors[0].file, "inc.asm");
    assert_eq!(r.errors[0].line, Some(3));
}

#[test]
fn pseudopc_and_segments() {
    let src = "*= $1000
    lda #0
!pseudopc $c000 {
reloc   jmp reloc
        nop
}
after   jmp after
!pseudopc $2000
x2      jmp x2
!realpc
x3      jmp x3
";
    let r = asm(src);
    assert_eq!(r.errors, vec![]);
    assert_eq!(r.output.as_ref().unwrap()[2..], h("a9 00 4c 00 c0 ea 4c 06 10 4c 00 20 4c 0c 10"));
    assert_eq!(r.lines.iter().find(|l| l.line == 4).unwrap().addr, 0xc000);
    assert_eq!((r.start, r.end), (0x1000, 0x100f));
    let r = asm("*=$1000\n!fill 4\n*=$1002\n!byte 1\n");
    assert!(r.ok);
    assert!(r.warnings[0].msg.contains("Segment starts inside another one"));
    assert_eq!(r.output.unwrap()[2..], [0, 0, 1, 0]);
}

// ------------------------------------------------------------------------------ output

#[test]
fn output_formats() {
    let r = asm("!to \"game.prg\", cbm\n*= $c000\n rts\n");
    assert_eq!(r.output_name.as_deref(), Some("game.prg"));
    assert_eq!(r.format, "cbm");
    assert_eq!(r.output.unwrap(), [0x00, 0xc0, 0x60]);
    let r = asm("!to \"game.bin\", plain\n*= $c000\n rts\n");
    assert_eq!(r.format, "plain");
    assert_eq!(r.output.unwrap(), [0x60]);
    let r = asm("!to \"game\", apple\n*= $0803\n rts : rts\n");
    assert_eq!(r.output.unwrap(), [0x03, 0x08, 0x02, 0x00, 0x60, 0x60]);
    let r = asm_with("*= $c000\n rts\n", &Options { format: Some("plain".into()), ..Default::default() }, &[]);
    assert_eq!(r.output_name, None);
    assert_eq!(r.output.unwrap(), [0x60]);
    let r = asm("!to \"x.prg\"\n*= $c000\n rts\n");
    assert!(r.warnings[0].msg.contains("without file format"));
    assert!(asm("!to \"x\", tap\n").errors[0].msg.contains("Unknown output format"));
    assert_eq!(asm("*= $c000\n rts\n").exec, Some(0xc000));
}

#[test]
fn label_lists_and_listing() {
    let r = asm("*= $1000\nstart  lda #CONST\nCONST = 5\nunused = 7\nloop  jmp loop\n");
    assert_eq!(r.vice_labels, "al C:0007 .unused\n\nal C:0005 .CONST\n\nal C:1002 .loop\nal C:1000 .start\n");
    let r = asm("!sl \"syms.txt\"\n!svl \"labels.txt\"\n*= $1000\nstart rts\nv = 3\n");
    assert_eq!(r.symbol_list_name.as_deref(), Some("syms.txt"));
    assert_eq!(r.vice_labels_name.as_deref(), Some("labels.txt"));
    assert_eq!(r.symbol_list, "start\t= $1000\t; ?\nv\t= 3\t; ?\n");
    let r = asm("*= $1000\n  lda #1 ; comment\n");
    assert_eq!(
        r.listing,
        "\n; ******** Source: main.asm\n     1                           *= $1000\n     2  1000 a901                  lda #1 ; comment\n"
    );
}

#[test]
fn lines_with_macros() {
    let src = "*= $1000
  lda #1
!macro m {
  nop
  nop
}
  +m
!for i, 0, 1 { !byte i }
";
    let r = asm(src);
    let line = |line, addr, size, in_macro| LineInfo { file: "main.asm".into(), line, addr, size, in_macro };
    assert_eq!(
        r.lines,
        vec![
            line(2, 0x1000, 2, false),
            line(4, 0x1002, 1, true),
            line(5, 0x1003, 1, true),
            line(7, 0x1002, 2, false),
            line(8, 0x1004, 2, false),
        ]
    );
}

// ------------------------------------------------------------------------------ errors

#[test]
fn errors_carry_lines_and_assembly_goes_on() {
    let src = "*= $1000
  nop
  lda #$100
  foo bar
  lda undefined_sym
  !byte 1,
  !unknownop
  sta
  lda (1,y)
  nop
";
    let r = asm(src);
    assert!(!r.ok);
    let illegal = "Illegal combination of command and addressing mode.".to_owned();
    assert_eq!(
        msgs(&r),
        vec![
            (3, "Number out of range.".into()),
            (4, "Garbage data at end of statement.".into()),
            (5, "Symbol not defined (undefined_sym).".into()),
            (6, "Value expected.".into()),
            (7, "Unknown pseudo opcode.".into()),
            (8, illegal.clone()),
            (9, illegal),
        ]
    );
    assert!(r.errors.iter().all(|e| e.file == "main.asm"));
}

#[test]
fn branches_and_blocks() {
    let r = asm("*= $1000\n  bne far\n  !fill 200\nfar rts\n  beq back\nback\n");
    assert_eq!(msgs(&r), vec![(2, "Target out of range (200; 73 too far).".into())]);
    assert!(asm("*=$1000\n bne *+129\n bne *-126\n").ok);
    assert!(!asm("*=$1000\n bne *+130\n").ok);
    let r = asm("*=$1000\n!if 1 {\n nop\n");
    assert_eq!(msgs(&r), vec![(2, "Found end-of-file instead of '}'.".into())]);
    let r = asm("*=$1000\n nop\n}\n");
    assert_eq!(msgs(&r), vec![(3, "Found '}' instead of end-of-file.".into())]);
}

#[test]
fn never_panics() {
    assert_eq!(asm(" nop\n").errors[0].msg, "Program counter undefined.");
    let r = assemble("nope.asm", vec![], &Options::default());
    assert!(!r.ok);
    assert!(r.errors[0].msg.contains("Cannot open input file"));
    for junk in ["\"", "!for", "!macro", "+", "-", "*=", "((((", "!if {", "\u{0}\u{1}", "!byte [1,", "}}}{{{"] {
        let r = asm(&format!("*=$1000\n{junk}\n"));
        assert!(!r.errors.iter().any(|e| e.msg.starts_with("Internal")), "{junk}: {:?}", r.errors);
    }
    let r = asm("*=$1000\n  foo\n  nop\n");
    assert!(r.ok);
    assert!(r.warnings[0].msg.contains("Implicit label definition not in leftmost column"));
}

// ------------------------------------------------------------------------------ disassembler

fn text_for(mn: &str, mode: Mode) -> String {
    let op = match mode {
        Mode::Imp => "",
        Mode::Imm => "#$12",
        Mode::Zp => "$12",
        Mode::Zpx => "$12,x",
        Mode::Zpy => "$12,y",
        Mode::Abs => "$1234",
        Mode::Absx => "$1234,x",
        Mode::Absy => "$1234,y",
        Mode::Ind => "($1234)",
        Mode::Indx => "($12,x)",
        Mode::Indy => "($12),y",
        Mode::Izp => "($12)",
        Mode::Iax => "($1234,x)",
        Mode::Rel => "$1012",
        Mode::Zprel => "$12,$1013",
        Mode::Sr => "$12,s",
        Mode::Sry => "($12,s),y",
        Mode::Ildp => "[$12]",
        Mode::Ildpy => "[$12],y",
        Mode::Long => "$123456",
        Mode::Longx => "$123456,x",
        Mode::Ialong => "[$1234]",
        Mode::Rell => "$2234",
        Mode::Blk => "$01,$02",
        Mode::Izpz => "($12),z",
        Mode::Imm16 => "#$1234",
        Mode::Rel16 => "$2234",
        Mode::Izl => "[$12],z",
        Mode::Izlq => "[$12]",
    };
    if op.is_empty() { mn.into() } else { format!("{mn} {op}") }
}

#[test]
fn round_trip_of_every_opcode() {
    for (cpu, table) in CPUS.iter() {
        for (mn, modes) in &table.table {
            for (&mode, &op) in modes {
                let (text, _) = operand(mode);
                let r = asm(&format!("!cpu {cpu}\n*= $1000\n {mn} {text}\n"));
                assert_eq!(r.errors, vec![], "{cpu} {mn} {text}");
                let code = &r.output.as_ref().unwrap()[2..];
                let ops = op_bytes(op);
                assert_eq!(code.len(), mode.len() + ops.len() - 1, "{cpu} {mn} {text}");
                assert_eq!(code[..ops.len()], ops[..]);
                let d = disassemble(code, 0x1000, cpu, Flags::default());
                assert_eq!(d.len(), 1, "{cpu} {mn} {text}: {d:?}");
                assert_eq!(d[0].addr, 0x1000);
                assert_eq!(d[0].bytes, code);
                if d[0].text.split(' ').next() == Some(mn) {
                    assert_eq!(d[0].text, text_for(mn, mode));
                }
                let r2 = asm(&format!("!cpu {cpu}\n*= $1000\n {}\n", d[0].text));
                assert_eq!(r2.output.as_ref().unwrap()[2..], *code, "{}", d[0].text);
            }
        }
    }
}

#[test]
fn w65816_wide_immediates() {
    let r = asm(
        "!cpu 65816\n*= $8000\n rep #$30\n!al\n!rl\n lda #$1234\n ldx #$5678\n sep #$20\n!as\n lda #$12\n ldy #$9abc\n jsl $018000\n lda $7e0010,x\n mvn $7e, $7f\n brl * + 3\n",
    );
    assert_eq!(r.errors, vec![]);
    let code = &r.output.as_ref().unwrap()[2..];
    assert_eq!(code, h("c2 30 a9 34 12 a2 78 56 e2 20 a9 12 a0 bc 9a 22 00 80 01 bf 10 00 7e 54 7f 7e 82 00 00"));
    let texts: Vec<_> = disassemble(code, 0x8000, "65816", Flags::default()).into_iter().map(|l| l.text).collect();
    assert_eq!(
        texts,
        [
            "rep #$30", "lda #$1234", "ldx #$5678", "sep #$20", "lda #$12", "ldy #$9abc", "jsl $018000",
            "lda $7e0010,x", "mvn $7e,$7f", "brl $801d"
        ]
    );
}

#[test]
fn unknown_and_truncated_bytes() {
    let d = disassemble(&[0x02, 0xad, 0x34], 0x2000, "6502", Flags::default());
    assert_eq!(
        d.iter().map(|l| (l.addr, l.bytes.clone(), l.text.as_str())).collect::<Vec<_>>(),
        vec![
            (0x2000, vec![0x02], "!byte $02"),
            (0x2001, vec![0xad], "!byte $ad"),
            (0x2002, vec![0x34], "!byte $34"),
        ]
    );
    assert_eq!(disassemble(&[0x02], 0, "6510", Flags::default())[0].text, "jam");
}

// ------------------------------------------------------------------------------ a C64 program

#[test]
fn c64_raster_program() {
    let src = "; raster bar demo
!to \"raster.prg\", cbm
!cpu 6510

VIC_RASTER = $d012
BORDER     = $d020

*= $0801
        !byte $0b, $08, $0a, $00, $9e, $32, $30, $36, $31, $00, $00, $00   ; 10 SYS2061
*= $080d
start   sei
        lda #$7f
        sta $dc0d               ; disable CIA interrupts
        lda #$01
        sta $d01a               ; enable raster IRQ
        lda #<irq
        sta $0314
        lda #>irq
        sta $0315
        lda #$80
        sta VIC_RASTER
        lda $d011
        and #$7f
        sta $d011
        cli
-       jmp -

irq     inc $d019               ; acknowledge
        inc BORDER
        jmp $ea31
";
    let r = asm(src);
    assert_eq!(r.errors, vec![]);
    assert_eq!(r.warnings, vec![]);
    assert_eq!(r.output_name.as_deref(), Some("raster.prg"));
    assert_eq!((r.start, r.end), (0x0801, 0x083c));
    assert_eq!(
        r.output.unwrap(),
        h("01 08
      0b 08 0a 00 9e 32 30 36 31 00 00 00
      78 a9 7f 8d 0d dc a9 01 8d 1a d0 a9 33 8d 14 03 a9 08 8d 15 03
      a9 80 8d 12 d0 ad 11 d0 29 7f 8d 11 d0 58 4c 30 08
      ee 19 d0 ee 20 d0 4c 31 ea")
    );
    assert!(r.vice_labels.contains("al C:0833 .irq\n"));
    assert!(r.vice_labels.contains("al C:080d .start\n"));
}

#[test]
fn the_demo_project_assembles() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo");
    let files = ["src/main.asm", "src/c64.inc", "gfx/charset/chars.chr"]
        .map(|p| (p.to_owned(), std::fs::read(root.join(p)).unwrap()))
        .to_vec();
    let r = assemble("src/main.asm", files, &Options::default());
    assert_eq!(r.errors, vec![]);
    let out = r.output.unwrap();
    assert_eq!(out[..2], [0x01, 0x08]);
    assert_eq!(r.output_name.as_deref(), Some("demo.prg"));
}

// ------------------------------------------------------------------------------ the 65CE02, 4502, 45GS02

fn m65_bytes(src: &str, cpu: &str) -> Vec<u8> {
    let r = asm_with(&format!("!cpu {cpu}\n*= $2000\n{src}"), &Options { format: Some("plain".into()), ..Default::default() }, &[]);
    assert_eq!(r.errors, vec![], "{src}");
    r.output.unwrap()
}

fn m65_ok(src: &str, cpu: &str) -> bool {
    asm_with(&format!("!cpu {cpu}\n*= $2000\n{src}"), &Options { format: Some("plain".into()), ..Default::default() }, &[]).ok
}

#[test]
fn ce02_instructions() {
    for (src, out) in [
        ("lda ($fb),z", "b2 fb"), ("sta ($fb),z", "92 fb"), ("ldz #3", "a3 03"), ("ldz $1234", "ab 34 12"),
        ("ldz $1234,x", "bb 34 12"), ("cpz #1", "c2 01"), ("cpz $10", "d4 10"), ("cpz $1234", "dc 34 12"),
        ("inz", "1b"), ("dez", "3b"), ("phz", "db"), ("plz", "fb"), ("taz", "4b"), ("tza", "6b"), ("tab", "5b"),
        ("tba", "7b"), ("tsy", "0b"), ("tys", "2b"), ("cle", "02"), ("see", "03"), ("neg", "42"), ("asr", "43"),
        ("asr $10", "44 10"), ("asr $10,x", "54 10"), ("inw $10", "e3 10"), ("dew $10", "c3 10"),
        ("asw $1234", "cb 34 12"), ("row $1234", "eb 34 12"), ("phw #$1234", "f4 34 12"), ("phw $1234", "fc 34 12"),
        ("rtn #2", "62 02"), ("jsr ($1234)", "22 34 12"), ("jsr ($1234,x)", "23 34 12"), ("sty $1234,x", "8b 34 12"),
        ("stx $1234,y", "9b 34 12"), ("lda (2,s),y", "e2 02"), ("sta (2,s),y", "82 02"), ("aug", "5c"),
        ("rmb3 $10", "37 10"),
    ] {
        assert_eq!(m65_bytes(src, "65ce02"), h(out), "{src}");
    }
    assert!(!m65_ok("lda ($fb)", "65ce02"));
    assert_eq!(m65_bytes("l: lbne l", "m65"), h("d3 fe ff"));
    assert_eq!(m65_bytes("bsr t\nt: rts", "m65"), h("63 01 00 60"));
    assert_eq!(m65_bytes("lbra t\n!fill 300, 0\nt: rts", "m65")[..3], h("83 2d 01"));
    assert_eq!(m65_bytes("map\neom\nnop", "4502"), h("5c ea ea"));
    assert!(!m65_ok("ldq $10", "4502"));
}

#[test]
fn gs02_quads() {
    for (src, out) in [
        ("lda [$fb],z", "ea b2 fb"), ("sta [$fb],z", "ea 92 fb"), ("cmp [$fb],z", "ea d2 fb"),
        ("ldq $fb", "42 42 a5 fb"), ("ldq $1234", "42 42 ad 34 12"), ("ldq ($fb)", "42 42 b2 fb"),
        ("ldq [$fb]", "42 42 ea b2 fb"), ("stq $fb", "42 42 85 fb"), ("adcq $10", "42 42 65 10"),
        ("sbcq $10", "42 42 e5 10"), ("andq $10", "42 42 25 10"), ("orq $10", "42 42 05 10"),
        ("eorq $10", "42 42 45 10"), ("cpq $10", "42 42 c5 10"), ("cmpq $10", "42 42 c5 10"), ("aslq", "42 42 0a"),
        ("lsrq $10", "42 42 46 10"), ("rolq $10,x", "42 42 36 10"), ("rorq $1234,x", "42 42 7e 34 12"),
        ("inq", "42 42 1a"), ("deq $1234", "42 42 ce 34 12"), ("bitq $1234", "42 42 2c 34 12"), ("asrq", "42 42 43"),
        ("asrq $10,x", "42 42 54 10"),
    ] {
        assert_eq!(m65_bytes(src, "m65"), h(out), "{src}");
    }
    for src in ["lda [$fb]", "ldq #1", "stq ($fb),z"] {
        assert!(!m65_ok(src, "m65"), "{src}");
    }
    let src = "l: ldq [$fb]\nlda [$fb],z\nstq $1234\ninq\nlda ($fb),z\nphw #$1234\nlbne l\nnop\nneg";
    let texts: Vec<_> =
        disassemble(&m65_bytes(src, "m65"), 0x2000, "m65", Flags::default()).into_iter().map(|l| l.text).collect();
    assert_eq!(
        texts,
        ["ldq [$fb]", "lda [$fb],z", "stq $1234", "inq", "lda ($fb),z", "phw #$1234", "lbne $2000", "nop", "neg"]
    );
}

// ------------------------------------------------------------------------------ the Z80

const Z80_CASES: &str = "
nop                 | 00
ld bc,$1234         | 01 34 12
ld (bc),a           | 02
inc bc              | 03
ld b,$12            | 06 12
ex af,af'           | 08
add hl,bc           | 09
ld a,(de)           | 1a
djnz *              | 10 fe
jr *                | 18 fe
jr nz,*             | 20 fe
jr c,*+4            | 38 02
ld hl,($1234)       | 2a 34 12
ld ($1234),hl       | 22 34 12
ld ($1234),a        | 32 34 12
ld a,($1234)        | 3a 34 12
ld sp,$c000         | 31 00 c0
ld a,(hl)           | 7e
ld (hl),b           | 70
ld (hl),$aa         | 36 aa
ld e,h              | 5c
halt                | 76
add a,b             | 80
adc a,$01           | ce 01
sub c               | 91
sbc a,(hl)          | 9e
and $0f             | e6 0f
xor a               | af
or e                | b3
cp (hl)             | be
cp $10              | fe 10
inc (hl)            | 34
dec a               | 3d
dec sp              | 3b
ret                 | c9
ret nz              | c0
ret m               | f8
pop af              | f1
push de             | d5
jp $1234            | c3 34 12
jp pe,$1234         | ea 34 12
call $1234          | cd 34 12
call z,$1234        | cc 34 12
rst $38             | ff
rst 0               | c7
ex de,hl            | eb
ex (sp),hl          | e3
jp (hl)             | e9
ld sp,hl            | f9
di                  | f3
ei                  | fb
exx                 | d9
daa                 | 27
cpl                 | 2f
scf                 | 37
ccf                 | 3f
rlca                | 07
rrca                | 0f
rla                 | 17
rra                 | 1f
out ($fe),a         | d3 fe
in a,($fe)          | db fe
rlc b               | cb 00
rrc c               | cb 09
rl (hl)             | cb 16
rr d                | cb 1a
sla e               | cb 23
sra h               | cb 2c
sll c               | cb 31
srl a               | cb 3f
bit 7,a             | cb 7f
bit 0,(hl)          | cb 46
res 0,(hl)          | cb 86
set 7,b             | cb f8
in b,(c)            | ed 40
in a,(c)            | ed 78
in (c)              | ed 70
out (c),a           | ed 79
out (c),0           | ed 71
sbc hl,de           | ed 52
adc hl,sp           | ed 7a
ld ($1234),de       | ed 53 34 12
ld sp,($1234)       | ed 7b 34 12
neg                 | ed 44
im 0                | ed 46
im 1                | ed 56
im 2                | ed 5e
ld a,i              | ed 57
ld a,r              | ed 5f
ld i,a              | ed 47
ld r,a              | ed 4f
rld                 | ed 6f
rrd                 | ed 67
reti                | ed 4d
retn                | ed 45
ldi                 | ed a0
ldir                | ed b0
ldd                 | ed a8
lddr                | ed b8
cpir                | ed b1
inir                | ed b2
otir                | ed b3
outd                | ed ab
ld ix,$1234         | dd 21 34 12
ld iy,($1234)       | fd 2a 34 12
ld ($1234),ix       | dd 22 34 12
ld (ix+5),a         | dd 77 05
ld (ix-1),$55       | dd 36 ff 55
ld a,(iy+$7f)       | fd 7e 7f
ld h,(ix+2)         | dd 66 02
add a,(ix+3)        | dd 86 03
or (ix+5)           | dd b6 05
inc (iy-128)        | fd 34 80
add ix,ix           | dd 29
add iy,bc           | fd 09
inc ix              | dd 23
push ix             | dd e5
pop iy              | fd e1
jp (ix)             | dd e9
jp (iy)             | fd e9
ex (sp),ix          | dd e3
ld sp,iy            | fd f9
set 3,(ix+2)        | dd cb 02 de
rl (iy+1)           | fd cb 01 16
bit 1,(ix+0)        | dd cb 00 4e
ld ixh,$12          | dd 26 12
ld b,ixl            | dd 45
ld ixh,ixl          | dd 65
inc iyh             | fd 24
sub iyl             | fd 95
";

fn z80(src: &str) -> Vec<u8> {
    let r = asm_with(&format!("!cpu z80\n*= $4000\n{src}\n"), &Options { format: Some("plain".into()), ..Default::default() }, &[]);
    assert_eq!(r.errors, vec![], "{src}");
    r.output.unwrap()
}

#[test]
fn z80_encodings() {
    for line in Z80_CASES.lines().filter(|l| !l.trim().is_empty()) {
        let (src, want) = line.split_once('|').unwrap();
        assert_eq!(z80(&format!(" {}", src.trim())), h(want), "{src}");
    }
    let src = "
!macro fill .val {
        ld a,.val
        ld (hl),a
}
start   LD HL,screen
        LD B,4
-       +fill <start
        INC HL
        DJNZ -
        jr .done
.done   ret
screen  !fill 2, $ff
!for n, 0, 1 { ld a,n }
";
    assert_eq!(z80(src), h("21 0e 40 06 04 3e 00 77 23 10 fa 18 00 c9 ff ff 3e 00 3e 01"));
    assert_eq!(z80(" ld a,1\n!align 3,0\n halt"), h("3e 01 00 00 76"));
}

#[test]
fn z80_errors() {
    for (src, want) in [
        ("jr far\n!fill 200\nfar nop", "Target out of range"),
        ("djnz *-200", "Target out of range"),
        ("ld (hl),(hl)", "Illegal combination"),
        ("ld ixh,h", "Illegal combination"),
        ("ld ixh,(ix+1)", "Illegal combination"),
        ("jr po,*", "Illegal combination"),
        ("ld a,(ix+200)", "displacement out of range"),
        ("rst 3", "Illegal RST"),
        ("im 3", "Illegal interrupt mode"),
        ("bit 8,a", "Bit number"),
        ("ld a", "Illegal combination"),
        ("push sp", "Illegal combination"),
    ] {
        let r = asm(&format!("!cpu z80\n*= $4000\n {src}\n"));
        assert!(!r.ok, "{src}");
        assert!(r.errors[0].msg.contains(want), "{src}: {}", r.errors[0].msg);
        assert_eq!(r.errors[0].line, Some(3), "{src}");
    }
    assert!(!asm("!cpu z80\n*=$4000\n lda #1\n").ok);
    assert!(!asm("*=$4000\n ld a,1\n").ok);
}

#[test]
fn z80_disassembler() {
    let d = disassemble(&h("3e 05 dd 77 fd 18 fe 08 fd cb fe 7e ed 79 ed 00"), 0x4000, "z80", Flags::default());
    assert_eq!(
        d.iter().map(|l| (l.addr, l.text.as_str())).collect::<Vec<_>>(),
        vec![
            (0x4000, "ld a,$05"),
            (0x4002, "ld (ix-$03),a"),
            (0x4005, "jr $4005"),
            (0x4007, "ex af,af'"),
            (0x4008, "bit 7,(iy-$02)"),
            (0x400c, "out (c),a"),
            (0x400e, "!byte $ed"),
            (0x400f, "nop"),
        ]
    );
    let prefixes: [&[u8]; 7] = [&[], &[0xcb], &[0xed], &[0xdd], &[0xfd], &[0xdd, 0xcb, 0x05], &[0xfd, 0xcb, 0xfb]];
    let mut n = 0;
    for pre in prefixes {
        for op in 0..=255u8 {
            let mut code = pre.to_vec();
            code.extend([op, 0x05, 0x12, 0x34]);
            let d = &disassemble(&code, 0x4000, "z80", Flags::default())[0];
            if d.text.starts_with("!byte") {
                continue;
            }
            let r = asm_with(&format!("!cpu z80\n*= $4000\n {}\n", d.text), &plain("z80"), &[]);
            assert_eq!(r.errors, vec![], "{}", d.text);
            assert_eq!(r.output.unwrap(), d.bytes, "{}", d.text);
            n += 1;
        }
    }
    assert!(n > 780, "only {n} instructions round-tripped");
}

#[test]
fn xex_amsdos_outputs() {
    let src = "!to \"game.xex\", xex
*= $2000
start   lda #0
        rts
*= $3000
        !byte 1, 2
!init start
!run start
";
    let r = asm(src);
    assert_eq!(r.errors, vec![]);
    assert_eq!(r.format, "xex");
    assert_eq!(r.exec, Some(0x2000));
    assert_eq!(
        r.output.unwrap(),
        h("ff ff 00 20 02 20 a9 00 60 00 30 01 30 01 02 e2 02 e3 02 00 20 e0 02 e1 02 00 20")
    );
    let xex = Options { format: Some("xex".into()), ..Default::default() };
    assert_eq!(asm_with("*= $0600\n rts\n", &xex, &[]).output.unwrap(), h("ff ff 00 06 00 06 60"));
    let r = asm_with("*= $0600\n rts\n", &Options { run: Some(0x600), ..xex }, &[]);
    assert_eq!(r.output.unwrap(), h("ff ff 00 06 00 06 60 e0 02 e1 02 00 06"));
    assert_eq!(r.exec, Some(0x600));

    let src = "!cpu z80
!to \"hello.bin\", amsdos
*= $4000
        nop
start   ld a,1
        call $bb5a
        ret
!run start
";
    let r = asm(src);
    assert_eq!(r.errors, vec![]);
    assert_eq!(r.format, "amsdos");
    assert_eq!(r.exec, Some(0x4001));
    let o = r.output.unwrap();
    assert_eq!(o.len(), 128 + 7);
    assert_eq!(o[0], 0);
    assert_eq!(&o[1..12], b"HELLO   BIN");
    assert_eq!(o[0x12], 2);
    assert_eq!(o[0x13..0x17], [7, 0, 0x00, 0x40]);
    assert_eq!(o[0x18..0x1c], [7, 0, 0x01, 0x40]);
    assert_eq!(o[0x40..0x43], [7, 0, 0]);
    let sum: u32 = o[..=66].iter().map(|&b| b as u32).sum();
    assert_eq!(o[0x43] as u32 | (o[0x44] as u32) << 8, sum);
    assert_eq!(o[128..], h("00 3e 01 cd 5a bb c9"));

    let files = vec![("src/game.asm".to_owned(), b"!cpu z80\n*= $8000\n ret\n".to_vec())];
    let r = assemble("src/game.asm", files, &Options { format: Some("amsdos".into()), ..Default::default() });
    let o = r.output.unwrap();
    assert_eq!(&o[1..12], b"GAME    ASM");
    assert_eq!(o[0x1a..0x1c], [0x00, 0x80]);
    assert_eq!(r.exec, Some(0x8000));
}
