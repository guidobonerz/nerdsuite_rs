//! The tests of nerdsuite2: basic.test.js, basic-listing.test.js and the ZX81, Level II and BBC
//! parts of platforms-zx81-trs80-bbc.test.js.

use super::listing::ATASCII_CODES;
use super::zx81::{PROGRAM, code_p, detokenize_zx81, float5, from_float5, tokenize_zx81};
use super::*;

const PRG_DIALECTS: [&str; 5] = ["v2", "v4", "v35", "v7", "simons"];

fn tok(src: &str, dialect: &str) -> Tokenized {
    tokenize(src, &TokenizeOptions::dialect(dialect)).unwrap()
}

fn detok(b: &[u8], dialect: &str) -> String {
    detokenize(b, dialect).unwrap()
}

fn prg(src: &str, dialect: &str) -> Vec<u8> {
    tok(src, dialect).prg.unwrap()
}

/// The tokenized content of the first line (without link, line number and terminator).
fn body(src: &str, dialect: &str) -> Vec<u8> {
    let r = tok(src, dialect);
    assert!(r.ok, "{:?}", r.errors);
    let p = r.prg.unwrap();
    p[6..p.len() - 3].to_vec()
}

fn error_lines(r: &Tokenized) -> Vec<usize> {
    r.errors.iter().map(|e| e.line).collect()
}

fn line(src: usize, number: u32, addr: usize) -> BasicLine {
    BasicLine { src, number, addr }
}

#[test]
fn exact_bytes_of_a_known_c64_program() {
    let r = tok("10 print \"{clr}hello\":goto 10", "v2");
    assert!(r.ok);
    assert_eq!(r.start, Some(0x0801));
    assert_eq!(
        r.prg.unwrap(),
        [
            0x01, 0x08, // load address
            0x15, 0x08, 0x0a, 0x00, // link -> $0815, line 10
            0x99, 0x20, 0x22, 0x93, 0x48, 0x45, 0x4c, 0x4c, 0x4f, 0x22, 0x3a, 0x89, 0x20, 0x31,
            0x30, 0x00, 0x00, 0x00, // end of program
        ]
    );
    assert_eq!(r.lines, [line(1, 10, 0x0801)]);
}

#[test]
fn multi_line_link_pointers_and_lines() {
    let r = tok("10 a=1\n\n20 ?a\n", "v2");
    assert!(r.ok);
    assert_eq!(
        r.prg.unwrap(),
        [
            0x01, 0x08, 0x09, 0x08, 0x0a, 0x00, 0x41, 0xb2, 0x31, 0x00, 0x10, 0x08, 0x14, 0x00,
            0x99, 0x41, 0x00, 0x00, 0x00,
        ]
    );
    assert_eq!(r.lines, [line(1, 10, 0x0801), line(3, 20, 0x0809)]);
}

#[test]
fn load_addresses_per_dialect_and_machine() {
    let start = |o: TokenizeOptions| tokenize("10 end", &o).unwrap().start.unwrap();
    let machine = |m| TokenizeOptions {
        machine: Some(m),
        ..Default::default()
    };
    assert_eq!(start(TokenizeOptions::default()), 0x0801);
    assert_eq!(start(machine("vic20")), 0x1001);
    assert_eq!(start(machine("vic20+3k")), 0x0401);
    assert_eq!(start(machine("vic20+8k")), 0x1201);
    assert_eq!(start(TokenizeOptions::dialect("v4")), 0x0401);
    assert_eq!(start(TokenizeOptions::dialect("v35")), 0x1001);
    assert_eq!(start(TokenizeOptions::dialect("v7")), 0x1c01);
    assert_eq!(start(TokenizeOptions::dialect("simons")), 0x0801);
    assert_eq!(
        start(TokenizeOptions {
            start: Some(0x4001),
            ..Default::default()
        }),
        0x4001
    );
    let r = tokenize("10 end", &machine("vic20")).unwrap();
    assert_eq!(r.prg.unwrap()[..4], [0x01, 0x10, 0x07, 0x10]);
    assert!(tokenize("10 end", &machine("nope")).is_err());
    assert!(tokenize("10 end", &TokenizeOptions::dialect("nope")).is_err());
}

#[test]
fn operators_functions_uppercase_input_question_mark() {
    assert_eq!(
        body("10 IF A<>B THEN PRINT CHR$(65)+\"X\"", "v2"),
        [
            0x8b, 0x20, 0x41, 0xb3, 0xb1, 0x42, 0x20, 0xa7, 0x20, 0x99, 0x20, 0xc7, 0x28, 0x36,
            0x35, 0x29, 0xaa, 0x22, 0xd8, 0x22
        ]
    );
    assert_eq!(body("10 ?\"a\"", "v2"), [0x99, 0x22, 0x41, 0x22]);
    assert_eq!(body("10 x=~*2", "v2"), [0x58, 0xb2, 0xff, 0xac, 0x32]);
    // keywords are tokenized everywhere outside strings (like the C64): "fort" -> FOR T
    assert_eq!(
        body("10 fort=1to2", "v2"),
        [0x81, 0x54, 0xb2, 0x31, 0xa4, 0x32]
    );
    assert_eq!(
        body("10 go to 10", "v2"),
        [0xcb, 0x20, 0xa4, 0x20, 0x31, 0x30]
    );
}

#[test]
fn strings_rem_and_data_are_not_tokenized() {
    assert_eq!(
        body("10 rem print goto", "v2"),
        [
            0x8f, 0x20, 0x50, 0x52, 0x49, 0x4e, 0x54, 0x20, 0x47, 0x4f, 0x54, 0x4f
        ]
    );
    assert_eq!(
        body("10 print \"print\"", "v2"),
        [0x99, 0x20, 0x22, 0x50, 0x52, 0x49, 0x4e, 0x54, 0x22]
    );
    // DATA ends at ':' outside quotes
    assert_eq!(
        body("10 data to,\"a:to\":to", "v2"),
        [
            0x83, 0x20, 0x54, 0x4f, 0x2c, 0x22, 0x41, 0x3a, 0x54, 0x4f, 0x22, 0x3a, 0xa4
        ]
    );
    // ? stays literal in strings/REM
    assert_eq!(body("10 rem ?", "v2"), [0x8f, 0x20, 0x3f]);
    // shifted letters inside strings/REM (petcat: uppercase ASCII -> $C1-$DA)
    assert_eq!(body("10 rem Ab", "v2"), [0x8f, 0x20, 0xc1, 0x42]);
}

#[test]
fn control_codes_in_strings() {
    assert_eq!(
        body(
            "10 print \"{clr}{home}{down}{rvs on}{wht}{f1}{$ff}{147}{shift-a}{cbm-a}{rvof}\"",
            "v2"
        ),
        [
            0x99, 0x20, 0x22, 0x93, 0x13, 0x11, 0x12, 0x05, 0x85, 0xff, 0x93, 0xc1, 0xb0, 0x92,
            0x22
        ]
    );
    assert_eq!(
        body("10 print \"{3 down}{right*2}{2left}\"", "v2"),
        [
            0x99, 0x20, 0x22, 0x11, 0x11, 0x11, 0x1d, 0x1d, 0x9d, 0x9d, 0x22
        ]
    );
    assert_eq!(body("10 rem {CLR}{Red}", "v2"), [0x8f, 0x20, 0x93, 0x1c]);
    assert_eq!(body("10 {p}r{i}nt", "v2"), [0x50, 0x52, 0x49, 0x4e, 0x54]);
    assert_eq!(lookup_control_code("blu", "v2"), Some(0x1f));
    assert_eq!(lookup_control_code("flon", "v35"), Some(0x82));
    assert_eq!(lookup_control_code("flon", "v2"), None);
    assert!(
        control_codes()
            .iter()
            .any(|c| c.name == "clr" && c.code == 0x93)
    );
    for d in PRG_DIALECTS {
        for c in control_codes_for(d) {
            assert_eq!(
                lookup_control_code(c.name, d),
                Some(c.code),
                "{d} {}",
                c.name
            );
        }
    }
    let r = tok("10 print \"{nosuchcode}\"", "v2");
    assert!(!r.ok);
    assert_eq!(r.errors[0].line, 1);
    assert!(r.errors[0].msg.contains("unknown control code"));
}

#[test]
fn line_errors() {
    let r = tok(
        "10 print\nprint \"x\"\n20 a\n20 b\n15 c\n70000 d\n30 {clr\n40\n50 ok",
        "v2",
    );
    assert!(!r.ok);
    assert_eq!(error_lines(&r), [2, 4, 5, 6, 7, 8]);
    assert!(r.errors[0].msg.contains("missing line number"));
    assert!(r.errors[1].msg.contains("duplicate"));
    assert!(r.errors[2].msg.contains("not greater"));
    assert!(r.errors[3].msg.contains("out of range"));
    assert!(r.errors[4].msg.contains("unterminated"));
    // valid lines are still emitted
    assert_eq!(
        r.lines.iter().map(|l| l.number).collect::<Vec<_>>(),
        [10, 20, 50]
    );
    let long = tok(&format!("10 rem {}", "x".repeat(300)), "v2");
    assert!(!long.ok);
    assert!(long.errors[0].msg.contains("too long"));
    assert!(tok("63999 end", "v2").ok);
    assert!(!tok("64000 end", "v2").ok);
}

#[test]
fn src_lines_map_the_errors_and_lines() {
    let src = [7, 9, 12];
    let r = tokenize(
        "10 a\nx\n20 b",
        &TokenizeOptions {
            src_lines: Some(&src),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(error_lines(&r), [9]);
    assert_eq!(r.lines.iter().map(|l| l.src).collect::<Vec<_>>(), [7, 12]);
}

#[test]
fn basic_4_tokens() {
    assert_eq!(
        body("10 dopen#1,\"x\":directory:concat", "v4"),
        [
            0xcd, 0x23, 0x31, 0x2c, 0x22, 0x58, 0x22, 0x3a, 0xda, 0x3a, 0xcc
        ]
    );
    // not keywords in plain V2
    assert_eq!(body("10 directory", "v2")[..3], [0x44, 0x49, 0x52]);
}

#[test]
fn basic_35_tokens() {
    assert_eq!(
        body("10 graphic 1:color 0,1:x=rlum(1):do:loop until x", "v35"),
        [
            0xde, 0x20, 0x31, 0x3a, 0xe7, 0x20, 0x30, 0x2c, 0x31, 0x3a, 0x58, 0xb2, 0xce, 0x28,
            0x31, 0x29, 0x3a, 0xeb, 0x3a, 0xec, 0x20, 0xfc, 0x20, 0x58
        ]
    );
}

#[test]
fn basic_7_two_byte_tokens() {
    assert_eq!(
        body(
            "10 bank 15:fast:slow:x=pot(1)+xor(1,2):sprite 1,1:dopen#1,\"a\":do",
            "v7"
        ),
        [
            0xfe, 0x02, 0x20, 0x31, 0x35, 0x3a, 0xfe, 0x25, 0x3a, 0xfe, 0x26, 0x3a, 0x58, 0xb2,
            0xce, 0x02, 0x28, 0x31, 0x29, 0xaa, 0xce, 0x08, 0x28, 0x31, 0x2c, 0x32, 0x29, 0x3a,
            0xfe, 0x07, 0x20, 0x31, 0x2c, 0x31, 0x3a, 0xfe, 0x0d, 0x23, 0x31, 0x2c, 0x22, 0x41,
            0x22, 0x3a, 0xeb
        ]
    );
    assert_eq!(
        body("10 stash:fetch:swap:pointer(a)", "v7"),
        [
            0xfe, 0x1f, 0x3a, 0xfe, 0x21, 0x3a, 0xfe, 0x23, 0x3a, 0xce, 0x0a, 0x28, 0x41, 0x29
        ]
    );
    // rlum is not a 7.0 keyword ($CE is the prefix there)
    assert_eq!(body("10 rlum", "v7"), [0x52, 0x4c, 0x55, 0x4d]);
    assert_eq!(
        detok(
            &[
                0x01, 0x1c, 0x08, 0x1c, 0x0a, 0x00, 0xfe, 0x17, 0x00, 0x00, 0x00
            ],
            "v7"
        ),
        "10 collision\n"
    );
}

#[test]
fn simons_basic_tokens() {
    assert_eq!(
        body("10 hires 0,1:plot 1,2,1:hi col:end proc:print", "simons"),
        [
            0x64, 0x01, 0x20, 0x30, 0x2c, 0x31, 0x3a, 0x64, 0x02, 0x20, 0x31, 0x2c, 0x32, 0x2c,
            0x31, 0x3a, 0x64, 0x0c, 0x3a, 0x64, 0x34, 0x3a, 0x99
        ]
    );
    // Simons' keywords take precedence over V2 ones ("on error" vs "on", "end proc" vs "end")
    assert_eq!(body("10 on error:goto 9", "simons")[..2], [0x64, 0x43]);
    assert_eq!(
        body("10 print at(1,2)\"x\"", "simons")[..4],
        [0x99, 0x20, 0x64, 0x28]
    );
    assert_eq!(body("10 hires", "v2"), [0x48, 0x49, 0x52, 0x45, 0x53]);
}

const SAMPLES: [(&str, &str); 5] = [
    (
        "v2",
        r###"10 print "{clr}{wht}hello world{$a0}{rvs on}x{rvof}":goto 10
20 for i=1 to 10 step 2:a$=left$("abc",2)+chr$(13):next i
30 if a>=1 and not b<>2 then poke 53280,peek(53281)or 1
40 data 1,2,"x:y",abc: rem trailing ? stuff {cbm-a}{shift-z}
50 open 1,8,15,"i0":print#1,"x":close 1:x=~*2-3/4^5
60 rem Shifted Text in REM and "{cbm-*}"
70 def fn f(x)=x*2:print fn f(3);tab(5);spc(2);pos(0)
80 get a$:input "name";n$:sys 64738:wait 198,1
90 a=rnd(-ti):b=sqr(abs(int(sgn(-2))))
100 on x goto 10,20:on y gosub 30:return:stop:end
"###,
    ),
    (
        "v4",
        r###"10 dopen#1,"data",d0,w:print#1,"x":dclose#1
20 directory:catalog d1:header "disk",d0,i01
30 scratch "old":rename "a" to "b":copy "x" to "y":concat "a" to "b"
40 dsave "prog":dload "prog":append#2,"log":record#2,5
50 collect:backup d0 to d1:print "{clr}ok"
"###,
    ),
    (
        "v35",
        r###"10 graphic 1,1:color 1,2,5:draw 1,0,0 to 159,99
20 circle 1,80,50,20:box 1,10,10,40,40:paint 1,15,15
30 char 1,0,0,"hello":scnclr:sound 1,500,60:vol 8
40 do:get a$:loop until a$<>"":x=joy(1):y=rlum(2):z=rclr(1)
50 trap 100:print hex$(dec("ff"));err$(1);instr("abc","b")
60 key 1,"list"+chr$(13):pudef " ":print using "##.#";1.5
70 if x then print "a":else print "b"
80 print "{flon}{help}{dblu}{pink}{blgn}"
90 restore 10:monitor:renumber:delete 10-20:auto 10
100 resume next
"###,
    ),
    (
        "v7",
        r###"10 graphic 1,1:color 1,2:draw 1,0,0 to 319,199
20 sprite 1,1,2:movspr 1,100,100:sprcolor 3,4:collision 1,100
30 bank 15:fast:slow:x=pot(1)+bump(1)+pen(0)+xor(1,2)
40 play "cdefg":tempo 10:envelope 1,5,5,5,5,1:filter 1000,1
50 dopen#1,"x",w:print#1,"a":dclose#1:bload "spr",b0,p3584
60 if a then begin
70 print "x":bend:sleep 1:window 0,0,39,24
80 stash 100,4096,0,0:fetch 100,4096,0,0:swap 100,4096,0,0
90 x=rsppos(1,0)+rsprite(1,1)+rspcolor(1)+rwindow(2):p=pointer(a)
100 do while x<10:x=x+1:loop:print rgr(0);rclr(1);rdot(0)
110 boot "x":sprdef:sprsav 1,a$:rreg a,x,y,s:dclear:dverify "x"
120 catalog:append#1,"y":record#1,1:concat "a" to "b":width 2
"###,
    ),
    (
        "simons",
        r###"10 hires 0,1:plot 10,10,1:line 0,0,319,199,1:rec 10,10,50,50,1
20 circle 160,100,50,50,1:paint 160,100,1:block 1,1,5,5,1
30 colour 0,0:print at(5,5)"hello":centre "title"
40 proc start:exec start:end proc:call start
50 repeat:x=x+1:until x>10:loop:exit if x>20:end loop
60 on error:goto 100:no error:on key "abc":goto 200
70 print mod(10,3);div(10,3);frac(1.5);exor(5,3);dup("ab",3)
80 mob set 1,13,2,0,0:mmob 1,100,100,0,0:mob off 1:low col 1,2,3:hi col
90 music 5,"a":play 1:vol 15:wave 1,32:envelope 1,0,0,15,0
100 print inkey;lin;penx;peny;pot(1):x=joy:print insert("a","b",1)
110 rem secure 0 is not tokenized here
120 disapa:secure 0:trace 1:retrace:dump:find print:option 10
"###,
    ),
];

#[test]
fn round_trip_tokenize_detokenize_tokenize() {
    for (dialect, src) in SAMPLES {
        let a = tok(src, dialect);
        assert!(a.ok, "{dialect}: {:?}", a.errors);
        let text = detok(a.prg.as_ref().unwrap(), dialect);
        let b = tok(&text, dialect);
        assert!(b.ok, "{dialect}: {:?}", b.errors);
        assert_eq!(b.prg, a.prg, "{dialect}: {text}");
        // the text is stable as well
        assert_eq!(detok(b.prg.as_ref().unwrap(), dialect), text);
    }
}

#[test]
fn detokenize_output_format_petcat_style() {
    let src = "10 print \"{clr}hello\":goto 10\n20 rem Abc\n";
    assert_eq!(detok(&prg(src, "v2"), "v2"), src);
    assert_eq!(
        detok(&prg("10 rem  x  ", "v2"), "v2"),
        "10 rem  x{$20}{$20}\n"
    );
    assert_eq!(detok(&prg("10 {$20}x", "v2"), "v2"), "10 {$20}x\n");
}

#[test]
fn detokenize_escapes_text_that_would_be_tokenized() {
    // untokenized "print", "+" and "?" in program text ($C1 outside strings is the ATN token)
    let p = [
        0x01, 0x08, 0x0e, 0x08, 0x0a, 0x00, 0x50, 0x52, 0x49, 0x4e, 0x54, 0x2b, 0x3f, 0xc1, 0x00,
        0x00, 0x00,
    ];
    let text = detok(&p, "v2");
    assert_eq!(text, "10 {p}r{i}nt{+}{$3f}atn\n");
    assert_eq!(prg(&text, "v2"), p);
}

#[test]
fn detokenize_never_produces_text_that_tokenizes_differently() {
    // GO + "SUB" would become GOSUB, "I" + FRE would become IF + "RE"
    let go_sub = [
        0x01, 0x08, 0x0a, 0x08, 0x0a, 0x00, 0xcb, 0x53, 0x55, 0x42, 0x00, 0x00, 0x00,
    ];
    assert_eq!(detok(&go_sub, "v2"), "10 {$cb}sub\n");
    let i_fre = [
        0x01, 0x08, 0x08, 0x08, 0x0a, 0x00, 0x49, 0xb8, 0x00, 0x00, 0x00,
    ];
    assert_eq!(detok(&i_fre, "v2"), "10 {i}fre\n");
    // deterministic fuzz over random token/char soup (the LCG of the JS test, its doubles included)
    let mut seed: i64 = 12345;
    let mut rnd = |n: i64| {
        seed = ((seed as f64 * 1103515245.0 + 12345.0) as i64) & 0x7fffffff;
        seed % n
    };
    let pool = [
        0x20, 0x22, 0x3a, 0x8f, 0x83, 0xfe, 0xce, 0x64, 0x99, 0xcb, 0xa4,
    ];
    for dialect in PRG_DIALECTS {
        for _ in 0..500 {
            let len = 1 + rnd(50) as usize;
            let mut b = Vec::new();
            for _ in 0..len {
                let r = rnd(10);
                b.push(if r < 3 {
                    pool[rnd(pool.len() as i64) as usize]
                } else if r < 6 {
                    0x41 + rnd(26) as u8
                } else {
                    1 + rnd(255) as u8
                });
            }
            let next = 0x0801 + len + 5;
            let mut p = vec![0x01, 0x08, next as u8, (next >> 8) as u8, 0x0a, 0x00];
            p.extend(&b);
            p.extend([0, 0, 0]);
            let text = detok(&p, dialect);
            let r = tokenize(
                &text,
                &TokenizeOptions {
                    dialect,
                    start: Some(0x0801),
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(r.ok, "{dialect}: {text} {:?}", r.errors);
            assert_eq!(r.prg.unwrap(), p, "{dialect}: {text}");
        }
    }
}

#[test]
fn detokenize_unknown_tokens_and_end_marker() {
    let p = [
        0x01, 0x08, 0x07, 0x08, 0x0a, 0x00, 0xcc, 0x00, 0x00, 0x00, 0xea, 0xea,
    ];
    assert_eq!(detok(&p, "v2"), "10 {$cc}\n");
    assert_eq!(prg(&detok(&p, "v2"), "v2"), p[..10]);
    assert_eq!(detok(&[0x01, 0x08, 0x00, 0x00], "v2"), "");
    // the simons placeholder token ">>" ($64 $22) is kept byte-exact
    let s = [
        0x01, 0x08, 0x08, 0x08, 0x0a, 0x00, 0x64, 0x22, 0x00, 0x00, 0x00,
    ];
    assert_eq!(detok(&s, "simons"), "10 {$64}{$22}\n");
    assert_eq!(prg(&detok(&s, "simons"), "simons"), s);
    assert!(detokenize(&s, "nope").is_err());
}

#[test]
fn dialect_tables_are_well_formed() {
    for id in PRG_DIALECTS {
        let d = dialect(id).unwrap();
        assert_eq!(d.output, Output::Prg, "{id}");
        assert!(
            !d.name.is_empty() && d.start.is_some() && d.machine(d.machine).is_some(),
            "{id}"
        );
        let mut seen = Vec::new();
        for kw in d.keywords {
            assert!(
                !kw.name.is_empty() && !kw.syntax.is_empty() && !kw.desc.is_empty(),
                "{id} {}",
                kw.name
            );
            assert!(
                !seen.contains(&kw.token),
                "{id}: duplicate token {:?}",
                kw.token
            );
            seen.push(kw.token);
        }
    }
    let pairs = |id| {
        dialect(id)
            .unwrap()
            .keywords
            .iter()
            .filter(|k| matches!(k.token, Some(KeywordToken::Pair(..))))
            .count()
    };
    assert_eq!(dialect("v2").unwrap().keywords.len(), 77);
    assert_eq!(pairs("v7"), 9 + 35);
    assert_eq!(pairs("simons"), 116); // 127 slots minus 11 unused ">>" placeholders
    assert_eq!(
        dialects().iter().map(|d| d.id).collect::<Vec<_>>(),
        [
            "v2", "v4", "v35", "v7", "v65", "simons", "atari", "turbo", "loco", "zx81", "trs80",
            "bbc"
        ]
    );
}

// ------------------------------------------------------------------ listings

fn listing(r: &Tokenized) -> &[u8] {
    r.listing.as_deref().unwrap()
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

#[test]
fn dialect_metadata_output_formats_and_keyword_lists() {
    assert_eq!(dialect("atari").unwrap().output, Output::AtasciiLst);
    assert_eq!(dialect("turbo").unwrap().output, Output::AtasciiLst);
    assert_eq!(dialect("loco").unwrap().output, Output::CpcAscii);
    for id in ["atari", "turbo", "loco"] {
        let d = dialect(id).unwrap();
        assert!(!d.name.is_empty() && d.machine(d.machine).is_some(), "{id}");
        let mut names = Vec::new();
        for kw in d.keywords {
            assert!(
                !kw.name.is_empty() && !kw.syntax.is_empty() && !kw.desc.is_empty(),
                "{id} {}",
                kw.name
            );
            assert!(!names.contains(&kw.name), "{id}: duplicate {}", kw.name);
            names.push(kw.name);
        }
    }
    assert!(
        ATASCII_CODES
            .iter()
            .any(|c| c.name == "clear" && c.code == Some(0x7d))
    );
}

#[test]
fn atari_enter_able_atascii_listing() {
    let r = tok(
        "10 print \"Hello\":goto 10\n20 rem lower case stays\n30 data a,b:c\n",
        "atari",
    );
    assert!(r.ok, "{:?}", r.errors);
    assert_eq!(r.prg, None);
    assert_eq!(r.format.id(), "atascii-lst");
    assert_eq!(
        listing(&r),
        cat(&[
            b"10 PRINT \"Hello\":GOTO 10",
            &[0x9b],
            b"20 REM lower case stays",
            &[0x9b],
            b"30 DATA a,b:c",
            &[0x9b]
        ])
    );
    assert_eq!(r.lines, [line(1, 10, 0), line(2, 20, 25), line(3, 30, 49)]);
}

#[test]
fn atari_escapes_and_inverse_video() {
    let r = tok(
        "10 ? \"{clear}{down}{bell}{inv}Ab{/inv}c{$9c}\":if x then rem ok",
        "atari",
    );
    assert!(r.ok, "{:?}", r.errors);
    assert_eq!(
        listing(&r),
        cat(&[
            b"10 ? \"",
            &[0x7d, 0x1d, 0xfd, 0xc1, 0xe2, 0x63, 0x9c],
            b"\":IF X THEN REM ok",
            &[0x9b]
        ])
    );
    let text = detok(listing(&r), "atari");
    assert_eq!(
        text,
        "10 ? \"{clear}{down}{bell}{inv}Ab{/inv}c{delline}\":IF X THEN REM ok\n"
    );
    assert_eq!(tok(&text, "atari").listing, r.listing);
}

#[test]
fn atari_line_number_and_character_errors() {
    let r = tok(
        "0 rem ok\n32768 end\n20 a=1\n10 b=2\n20 c=3\nx=1\n30 ? \"{nope}\"\n40 ? \"{$9b}\"",
        "atari",
    );
    assert!(!r.ok);
    assert_eq!(error_lines(&r), [2, 4, 5, 6, 7, 8]);
    assert!(tok("32767 END", "atari").ok);
    assert!(!tok(&format!("10 REM {}", "x".repeat(120)), "atari").ok);
}

#[test]
fn turbo_basic_same_listing_format() {
    let r = tok("10 repeat:dpoke 88,0:until stick(0)<>15", "turbo");
    assert!(r.ok);
    assert_eq!(
        listing(&r),
        cat(&[b"10 REPEAT:DPOKE 88,0:UNTIL STICK(0)<>15", &[0x9b]])
    );
}

#[test]
fn loco_cpc_ascii_file() {
    let r = tok(
        "10 mode 1:print chr$(65);name$\n20 for i=1 to 10:next ' a comment\n30 rem keep print\n40 data print,\"x\":locate 1,1",
        "loco",
    );
    assert!(r.ok, "{:?}", r.errors);
    assert_eq!(r.prg, None);
    assert_eq!(r.format, Output::CpcAscii);
    assert_eq!(
        listing(&r),
        cat(&[
            b"10 MODE 1:PRINT CHR$(65);name$\r\n",
            b"20 FOR i=1 TO 10:NEXT ' a comment\r\n",
            b"30 REM keep print\r\n",
            b"40 DATA print,\"x\":LOCATE 1,1\r\n",
            &[0x1a],
        ])
    );
    let text = detok(listing(&r), "loco");
    assert_eq!(
        text.split('\n').next().unwrap(),
        "10 MODE 1:PRINT CHR$(65);name$"
    );
    assert_eq!(tok(&text, "loco").listing, r.listing);
}

#[test]
fn loco_escapes_and_errors() {
    let r = tok("10 print \"{$18}x{$7b}\"", "loco");
    assert_eq!(
        listing(&r),
        cat(&[b"10 PRINT \"", &[0x18, 0x78, 0x7b, 0x22, 0x0d, 0x0a, 0x1a]])
    );
    assert_eq!(detok(listing(&r), "loco"), "10 PRINT \"{$18}x{$7b}\"\n");
    let e = tok("0 cls\n65535 end\n65536 end\n100 a=1\n{clear}", "loco");
    assert_eq!(error_lines(&e), [1, 3, 4, 5]);
    assert_eq!(listing(&tok("", "loco")).len(), 1); // just the EOF marker
}

#[test]
fn level_ii_and_bbc_basic_ascii_listings_with_cr() {
    let t = tok("10 print \"hi\" ' a remark\n20 goto 10", "trs80");
    assert!(t.ok);
    assert_eq!(listing(&t), b"10 PRINT \"hi\" ' a remark\r20 GOTO 10\r");
    let b = tok("10 for i%=1 to 3:print '\"x\";i%:next\n20 PROCmain", "bbc");
    assert_eq!(
        listing(&b),
        b"10 FOR i%=1 TO 3:PRINT '\"x\";i%:NEXT\r20 PROCmain\r"
    );
    assert_eq!(
        detok(listing(&b), "bbc"),
        "10 FOR i%=1 TO 3:PRINT '\"x\";i%:NEXT\n20 PROCmain\n"
    );
}

// ------------------------------------------------------------------ ZX81

fn at(p: &[u8], addr: usize) -> u8 {
    p[addr - 0x4009]
}

fn word(p: &[u8], addr: usize) -> usize {
    at(p, addr) as usize | (at(p, addr + 1) as usize) << 8
}

const PROG: usize = PROGRAM as usize - 0x4009;

#[test]
fn float5_the_5_byte_numbers_of_the_zx81() {
    assert_eq!(float5(0.0).unwrap(), [0, 0, 0, 0, 0]);
    assert_eq!(float5(1.0).unwrap(), [0x81, 0, 0, 0, 0]);
    assert_eq!(float5(10.0).unwrap(), [0x84, 0x20, 0, 0, 0]);
    assert_eq!(float5(-0.5).unwrap(), [0x80, 0x80, 0, 0, 0]);
    assert_eq!(float5(16514.0).unwrap(), [0x8f, 0x01, 0x04, 0, 0]);
    for v in [3.25, -1234.5, 0.1, 65535.0] {
        assert!((from_float5(float5(v).unwrap()) - v).abs() < 1e-6 * v.abs());
    }
    assert!(float5(f64::INFINITY).is_err());
}

#[test]
fn tokenize_zx81_keywords_numbers_and_a_p_file_that_runs() {
    let r = tokenize_zx81("10 PRINT \"HI\"\n20 GOTO 10");
    assert_eq!(r.errors, []);
    let p = r.bytes.unwrap();
    // line 10 at $407D: number (big endian), length, PRINT "HI" newline
    assert_eq!(
        p[PROG..PROG + 10],
        [0, 10, 6, 0, 0xf5, 0x0b, 0x2d, 0x2e, 0x0b, 0x76]
    );
    // line 20: GOTO 1 0 $7E and 10 as a float, newline (10 bytes)
    assert_eq!(
        p[PROG + 10..PROG + 24],
        [
            0, 20, 10, 0, 0xec, 0x1d, 0x1c, 0x7e, 0x84, 0x20, 0, 0, 0, 0x76
        ]
    );
    assert_eq!(word(&p, 0x4029), PROGRAM as usize); // NXTLIN: it runs from its first line
    let dfile = word(&p, 0x400c);
    assert_eq!(at(&p, dfile), 0x76);
    assert_eq!(word(&p, 0x4014), word(&p, 0x4010) + 1); // E_LINE after the variables
    assert_eq!(p.len(), word(&p, 0x4014) - 0x4009); // the file ends at E_LINE
}

#[test]
fn tokenize_zx81_names_strings_errors() {
    let p = tokenize_zx81("10 LET TOTAL=A1+2**3\n20 PRINT \"SAY \"\"HI\"\"\"\n30 REM 12 GOTO")
        .bytes
        .unwrap();
    assert_eq!(
        detokenize_zx81(&p),
        "10 LET TOTAL=A1+2**3\n20 PRINT \"SAY \"\"HI\"\"\"\n30 REM 12 GOTO\n"
    );
    assert!(
        tokenize_zx81("10 PRINT \"HI!\"").errors[0]
            .msg
            .contains("no character \"!\"")
    );
    assert!(
        tokenize_zx81("20 CLS\n10 CLS").errors[0]
            .msg
            .contains("not greater")
    );
    let r = tok("10 CLS", "zx81");
    assert_eq!(r.start, Some(PROGRAM));
    assert_eq!(detok(listing(&r), "zx81"), "10 CLS\n");
}

#[test]
fn code_p_machine_code_in_1_rem_run_by_2_rand_usr() {
    let p = code_p(&[0x3e, 0x01, 0xc9], 0x4082, 0x4082).unwrap();
    assert_eq!(
        p[PROG..PROG + 9],
        [0, 1, 5, 0, 0xea, 0x3e, 0x01, 0xc9, 0x76]
    );
    assert!(detokenize_zx81(&p).contains("2 RAND USR 16514"));
    assert_eq!(word(&p, 0x4029), PROGRAM as usize + 9); // NXTLIN: line 2
    assert!(
        code_p(&[0xc9], 0x4000, 0x4000)
            .unwrap_err()
            .contains("$4082")
    );
}
