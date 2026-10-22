//! The tests of nerdsuite2's `disasm.test.js` (but those of its starter programs, not ported).

use super::*;
use crate::acme::{Options as AsmOptions, assemble};

/// Assembles `src` (6510 unless `cpu`) to plain bytes.
fn bytes(src: &str, cpu: &str) -> Vec<u8> {
    let opts = AsmOptions { cpu: Some(cpu.into()), format: Some("plain".into()), ..Default::default() };
    let r = assemble("main.asm", vec![("main.asm".into(), src.as_bytes().to_vec())], &opts);
    assert_eq!(r.errors, vec![], "{src}");
    r.output.unwrap()
}

/// The disassembly of `data` assembles to `data` again.
fn round_trip(data: &[u8], opts: &Options) -> String {
    let src = to_source(&disassemble_all(data, opts), opts, "", None, None);
    assert_eq!(bytes(&src, &opts.cpu), data, "{src}");
    src
}

fn region(start: usize, end: usize, ty: DataType) -> Region {
    Region { start, end, ty }
}

#[test]
fn marking_cuts_and_joins() {
    let r = mark_range(&[], 0, 9, Some(DataType::Code));
    let r = mark_range(&r, 4, 5, Some(DataType::Bytes));
    assert_eq!(r, [region(0, 3, DataType::Code), region(4, 5, DataType::Bytes), region(6, 9, DataType::Code)]);
    let r = mark_range(&r, 4, 5, Some(DataType::Code));
    assert_eq!(r, [region(0, 9, DataType::Code)]);
    assert_eq!(mark_range(&r, 0, 9, None), []);
}

#[test]
fn the_sys_of_a_basic_line_is_the_entry() {
    let data = bytes("* = $0801\n!word next, 10\n!byte $9e\n!text \"2064\"\n!byte 0\nnext !word 0\n* = $0810\nrts", "6510");
    assert_eq!(basic_sys(&data, 0x0801), Some(0x0810));
    let c = classify(&data, &Options::new(0x0801, "6510"));
    assert_eq!(c.types[0x0f], DataType::Code);
    assert_eq!(c.types[0], DataType::Basic);
}

#[test]
fn auto_mode_traces_the_code() {
    let src = "* = $c000\nstart jsr sub\n jmp start\ntable !byte 1,2,3,4\nsub ldx #3\nloop lda table,x\n dex\n bpl loop\n rts\nmsg !text \"HELLO WORLD\"\n";
    let data = bytes(src, "6510");
    let opts = Options::new(0xc000, "6510");
    let r = disassemble_all(&data, &opts);
    let text = r.lines.iter().map(|l| format!("{} {}", l.label.clone().unwrap_or_default(), l.text)).collect::<Vec<_>>().join("\n");
    assert!(text.contains("jsr l_c00a"), "{text}");
    assert!(text.contains("d_c006 !byte $01, $02, $03, $04"), "{text}");
    assert!(text.contains("lda d_c006,x"), "{text}");
    assert!(text.contains("!text \"HELLO WORLD\""), "{text}");
    round_trip(&data, &opts);
}

#[test]
fn the_marks_win_over_the_trace() {
    let data = bytes("* = $1000\n lda $1006\n sta $1007\n rts\n!byte $a9, $00, $60, $40, $41, $42, $43\n", "6510");
    let opts = Options {
        regions: vec![region(7, 9, DataType::Code), region(10, 13, DataType::Petscii)],
        ..Options::new(0x1000, "6510")
    };
    let r = disassemble_all(&data, &opts);
    assert_eq!(r.types()[7], DataType::Code);
    assert!(r.lines.iter().any(|l| l.text.starts_with("!pet")));
    assert!(r.lines.iter().any(|l| l.text == "sta l_1007"));
    round_trip(&data, &opts);
}

#[test]
fn every_data_type_assembles_to_its_bytes() {
    let data: Vec<u8> = (0..256u32).map(|k| ((k * 37 + 11) & 0xff) as u8).collect();
    for ty in [
        DataType::Bytes,
        DataType::Words,
        DataType::Text,
        DataType::Petscii,
        DataType::Screen,
        DataType::Chars,
        DataType::Sprites,
        DataType::Bitmap,
    ] {
        round_trip(&data, &Options { regions: vec![region(0, 255, ty)], ..Options::new(0x2000, "6510") });
    }
}

#[test]
fn without_auto_everything_unmarked_is_code() {
    let data: Vec<u8> = (0..512u32).map(|k| ((k * 97 + 3) & 0xff) as u8).collect();
    let opts = Options { auto: false, ..Options::new(0x4000, "6510") };
    let r = disassemble_all(&data, &opts);
    assert!(r.types().iter().all(|t| *t == DataType::Code));
    round_trip(&data, &opts);
}

#[test]
fn z80_calls_jumps_returns() {
    let data = bytes("!cpu z80\n* = $4000\nstart call func\n jr start\nfunc ld hl,data\n ld a,(hl)\n ret\ndata !byte 1,2,3\n", "z80");
    let opts = Options::new(0x4000, "z80");
    let r = disassemble_all(&data, &opts);
    assert_eq!(*r.types().last().unwrap(), DataType::Bytes);
    assert!(r.lines.iter().any(|l| l.text == "ld hl,d_400a"));
    round_trip(&data, &opts);
}

#[test]
fn the_demo_round_trips() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo");
    let project = crate::project::Project::open(root).unwrap();
    let b = crate::build::build_program(&project, "src/main.asm", &Default::default());
    let out = b.file.unwrap().bytes;
    let base = out[0] as u32 | (out[1] as u32) << 8;
    round_trip(&out[2..], &Options::new(base, "6510"));
    // as the view reads the .prg: its layout, a C64 by the load address, the source with !to
    let lay = file_layout("demo.prg", &out).unwrap();
    assert_eq!((lay.base, lay.machine), (0x0801, "c64"));
    let opts = Options { machine: Some(lay.machine.into()), ..Options::new(lay.base, "6510") };
    let r = disassemble_all(&out[lay.head..], &opts);
    assert!(r.lines.iter().any(|l| l.text == "sta VIC_BORDER"), "{:?}", r.lines.iter().map(|l| &l.text).collect::<Vec<_>>());
    let src = to_source(&r, &opts, "demo.prg", None, Some(("prg", "cbm")));
    let asm = assemble("main.asm", vec![("main.asm".into(), src.clone().into_bytes())], &AsmOptions::default());
    assert_eq!(asm.errors, vec![], "{src}");
    assert_eq!(asm.output.unwrap(), out);
}

#[test]
fn c64_registers_by_name_and_values_in_words() {
    let data = bytes("* = $c000\n lda #$1b\n sta $d011\n lda #6\n sta $d020\n jsr $ffd2\n rts\n", "6510");
    let opts = Options { machine: Some("c64".into()), ..Options::new(0xc000, "6510") };
    let r = disassemble_all(&data, &opts);
    let by = |t: &str| r.lines.iter().find(|l| l.text == t).unwrap_or_else(|| panic!("{t}")).clone();
    assert!(by("sta VIC_CTRL1").comment.unwrap().contains("y scroll 3 · rows 25 · screen on"));
    assert!(by("sta VIC_BORDER").comment.unwrap().contains("color blue"));
    by("jsr CHROUT");
    round_trip(&data, &opts);
}

#[test]
fn vcs_tia_in_the_zero_page() {
    let data = bytes("* = $f000\n sta $02\n lda $0c\n sta $09\n rts\n", "6502");
    let opts = Options { machine: Some("vcs".into()), ..Options::new(0xf000, "6502") };
    let r = disassemble_all(&data, &opts);
    assert_eq!(r.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["sta WSYNC", "lda INPT4", "sta COLUBK", "rts"]);
    round_trip(&data, &opts);
}

#[test]
fn a_prg_and_its_machine() {
    let prg = [0x01, 0x08, 0x0b, 0x08, 0x0a, 0x00, 0x9e, 0x32, 0x30, 0x36, 0x31, 0x00, 0x00, 0x00, 0xa9, 0x00, 0x60];
    let lay = file_layout("lines.prg", &prg).unwrap();
    assert_eq!((lay.machine, lay.base), ("c64", 0x0801));
    assert_eq!(file_layout("x.prg", &[0x01, 0x1c, 0]).unwrap().machine, "c128");
    let r = disassemble_all(&prg[2..], &Options { machine: Some("c64".into()), ..Options::new(0x0801, "6510") });
    assert!(r.lines.iter().any(|l| l.text == "lda #$00" && l.comment.as_deref() == Some("entry")));
}

#[test]
fn basic_by_the_chain_of_its_lines() {
    let src = "* = $0801\n!word l20, 10\n!byte $99, $22\n!text \"HI\"\n!byte $22, 0\nl20 !word end, 20\n!byte $9e\n!text \"2072\"\n!byte 0\nend !word 0\n* = $0818\n lda #1\n rts\n";
    let data = bytes(src, "6510");
    let b = detect_basic(&data, 0x0801, "v2", false).unwrap();
    assert_eq!(b.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["10 print\"hi\"", "20 sys2072"]);
    assert_eq!(b.sys, [0x0818]);
    assert_eq!(b.end, 22);
    // not at a BASIC start: only on a Commodore
    let moved = bytes(&src.replacen("$0801", "$2001", 1).replace("2072", "8216").replace("$0818", "$2018"), "6510");
    assert!(detect_basic(&moved, 0x2001, "v2", false).is_none());
    assert!(detect_basic(&moved, 0x2001, "v2", true).is_some());
    let r = disassemble_all(&data, &Options { machine: Some("c64".into()), ..Options::new(0x0801, "6510") });
    let views: Vec<_> = r.lines.iter().filter(|l| l.ty == DataType::Basic).map(|l| l.view.clone()).collect();
    assert_eq!(views, [Some("10 PRINT\"HI\"".into()), Some("20 SYS2072".into()), None]);
    assert_eq!(r.classified.entries, [0x0818]);
    assert!(r.lines.iter().any(|l| l.text == "lda #$01" && l.comment.as_deref() == Some("entry")));
}
