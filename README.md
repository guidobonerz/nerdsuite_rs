# NerdSuite (Rust)

The IDE for programs of 8 bit machines of [nerdsuite2](../nerdsuite2), in Rust, with
[gpui](https://github.com/zed-industries/zed/tree/main/crates/gpui) - Zed's GPU-accelerated UI framework -
in place of the browser and the Go server: one native program, no browser window.

```sh
cargo run -p nerdsuite -- examples/demo   # the demo project
cargo run -p nerdsuite                    # the welcome page: open a folder, or the demo
cargo test --workspace                    # the tests
cargo build --release                     # target/release/nerdsuite(.exe)
```

gpui comes from Zed's repository at a fixed commit (`Cargo.toml`, `[workspace.dependencies]`), with
`gpui_platform` for the windows of Windows, macOS and Linux. The first build fetches and compiles it (a few minutes).

## What there is

| | |
|---|---|
| **Shell** | the top bar, the rail of the ten views of nerdsuite2, the files of the project as a tree, a status bar (machine, CPU, main program, caret, messages); light or dark after the system, in nerdsuite2's colours |
| **Project** | a folder with `nerdsuite.json` (keys this version does not know are kept); Open folder (Ctrl+O), Save (Ctrl+S) |
| **Code** | the sources in tabs (middle button or × closes, ● marks unsaved changes); an editor with line numbers and the colours of ACME and BASIC; the caret and selection by keys and mouse (double click: a word), undo and redo, copy, cut, paste, the input methods of the system (IME); Enter keeps the indent (BASIC: proposes the next line number), Tab and Shift+Tab indent, Ctrl+/ comments lines in or out (`;`, BASIC: `rem`) |
| **Hex** | any other file of the project as bytes: offsets (a .prg at its load address), hex, ASCII, PETSCII or screen codes |
| **Build** | Ctrl+B or the button: the source in the editor (not an include), else the main program, with the texts not saved yet. An assembler source with the ACME compatible assembler (`!cpu` 6502 … 65816, the MEGA65's 45GS02, the Z80; the output of the machine without `!to`: .prg, .xex, AMSDOS, .p, /CMD, .adf, plain), a BASIC program with the tokenizer of its dialect. The program, the VICE label list (`.lbl`) and the listing (`.lst`) go into `build/`; the errors and warnings into the problems below the editor (a click goes to the line) and as marks on the lines |

| **Run** | F5 or the button: builds, then starts the program in the emulator of the settings - nerdsuite2's `~/.nerdsuite/settings.json` (`preferred` per platform, `paths` of the tools; or the file `NERDSUITE_SETTINGS` names), else mimic when it runs the machine, else VICE or the first emulator found. mimic gets the program through its debug protocol (a .prg, a .xex, a 2600 cartridge, an Amiga ADF; a mimic of the same machine stays and takes the next one); VICE autostarts the .prg; Altirra, Stella, Mesen, FCEUX, bsnes, Snes9x, EightyOne, ZEsarUX, trs80gp, Xemu, WinUAE, FS-UAE and MAME get nerdsuite2's command lines. The emulator ends with NerdSuite |

| **Debug** | F6 or the Debugger view: builds and starts the program in mimic with the breakpoints of the sources (a click on a line number sets one, a red dot). The Debugger view: continue, pause, step over, step into, step out, stop; the registers and the flags, the code from the PC with the labels and the lines of the sources, the stack, the memory (zero page, stack, screen, program, PC; ASCII, PETSCII or screen codes), the log. The line of the CPU is marked in the editor. Keys of nerdsuite2: F5/F8 continue, F8 pause, F10 step over, F11 step into, Shift+F11 step out, Shift+F5 stop. A .prg, a .xex or a 2600 cartridge |

| **Disassembler** | a binary of the project (a .prg, a .nes, .sfc, .a26 with their header and vectors, any file of bytes): the disassembly on the left, the bytes on the right coloured by their type. Auto traces the code from its entry points (the SYS of the BASIC line, the load address, the vectors); a click selects a line or a byte (Shift extends), the buttons mark the selection as code or as data (bytes, words, ASCII, PETSCII, screen codes, chars, sprites, a bitmap, tiles) or unmark it; undo, redo. The registers and ROM routines of the machine name the operands (`sta VIC_BORDER`, `jsr CHROUT`) and a value stored to a register is told in words; Follow (or a double click) goes where an operand points, Back returns. Copy or save the disassembly as ACME source that assembles to the same bytes. The marks are kept in `nerdsuite.json` (`disassembler`), as nerdsuite2 keeps them |

| **Chars** | a charset of the project (a .chr of 2048 bytes, a load address before them kept): the character in work large - the left button draws, the right one erases, a drag paints - and all 256 in a grid to pick from; hires or multicolor (the bit pairs: the background, $d022, $d023, the cell color), the colors from the VIC-II palette; clear, invert, flip, rotate, shift, copy and paste; undo. Save (Ctrl+S) writes the file |

| **Sprites** | a costume of the project (a `.spr` of nerdsuite2 and yopaca: frames of w x h sprites of 24 x 21 pixels, hires or multicolor; its layers and the tag of the editor that wrote it are kept, an unchanged file is written as it was): the frame in work large - the left button draws, the right one erases - and all frames small to pick from; the pens of multicolor ($d025, the sprite color, $d026); new, duplicate and delete frames; clear, flip, shift; undo. Save (Ctrl+S) writes the file |

| **Screen** | a screen of the project (a `.scr` of nerdsuite2 and yopaca, `YSCR` 1 to 3: the characters and their colors, its patches - a door open, closed … - its look, the tag of the editor; an unchanged file is written as it was) as the VIC-II shows it - hires, multicolor by the cell color, extended color mode - in its charset: a .chr of the project (the one in the Chars view with its changes not saved yet), else the C64's ROM font, read from VICE or mimic (NerdSuite has none of its own). The left button puts the character and color chosen, the right one picks them up; a patch is edited in its place over the screen; fill, clear, a grid, the background and border colors; undo. Save (Ctrl+S) writes the file |

| **Bitmap** | a picture of the project - Koala (.kla, multicolor) or Art Studio (.art, hires), written again byte for byte - with the color rules of the VIC-II's bitmap modes: in hires every cell two colors; in multicolor every cell three of its own and the background of the picture - a color takes the slot of the cell that holds it, a free one or the one it paints over, a fourth one over the background is a color clash, counted. The left button paints with the color, the right one with the background; zoom 2x to 4x; undo. Save (Ctrl+S) writes the file |

| **Composer** | a tracker for the SID: a song of the project (`.sng`, `YSNG` 1 to 5, written as the lowest version that holds it, the tag of the editor kept) - the orders (positions, patterns, the loop), the pattern of a position (a cell a voice: note, instrument, command and value), the song (tempo, rows, the octave and step of the keys), up to 15 instruments (waveform, sync and ring, ADSR, pulse width and sweep, vibrato, arpeggio, filter with cutoff, resonance and sweep, the rows of GoatTracker's four tables they start at; ready ones by group) and the wave, pulse, filter and speed tables, their bytes chosen by name in popups. Notes are typed like on a piano by the place of the keys (QWERTZ and QWERTY alike) and sound while held; the popup of a cell (double click, right button, Enter) has every note, the instruments, the commands. Space plays the song (Shift: the pattern), voices are muted, the game's three voices chosen; selections, copy, cut, paste, transpose, undo. It plays on nerdsuite2's model of the SID and its player through the sound output of the system (cpal) - a SID for every three voices of a song of more. Import… (or a .sid, .mid of the project) brings in GoatTracker 2 songs, SID tunes (run on a 6502 and transcribed) and MIDI files as a new song. Save (Ctrl+S) writes the file; a new song goes into `audio/music` |

The Tiles editor has its place in the rail
and is not ported yet; nor are: the confirmations before a position, an instrument or the other voices of a song are removed (undo brings them back), the storyboard's songs and sounds; the tools of nerdsuite2's Chars editor beyond the pencil (lines, shapes,
fills, brushes, selections, the ROM font traced over, tiles of chars, ECM), the preview of the Disassembler (chars, sprites, a bitmap, tiles), its
names of labels and its keys, the files from outside the project; debugging in VICE (its binary monitor),
watchpoints, conditions, watches, the screen of the emulator in the Debugger view and the editing of
registers and memory; breakpoints that move with the lines edited; in mimic the programs that
are typed in (a CPC binary or listing, a ZX81 .p, an Atari BASIC listing); the disc images of the launcher
(CPC .dsk, BBC .ssd); the 68000 of the assembler (the Amiga); the BASIC preprocessor (`#define`, `@if`,
`DATA @embed`) and the label mode; KickAssembler, the toolchains, the Ultimate 64, the wizard,
export/import and the handbook.

`MIMIC_TEST=1 cargo test -p nerdsuite_core` also builds the demo and runs it in `mimic-headless`: its
text on the screen (Run), a breakpoint, a step, continue and pause (Debug).

## Files

| file | content |
|---|---|
| `crates/nerdsuite_core` | what needs no window, tested on its own: the machines (`platform.rs`, from `platforms.js`), the project on disk (`project.rs`), the rows of the Hex view (`hex.rs`), the colours of the sources (`syntax.rs`) |
| `crates/nerdsuite_core/src/acme` | the assembler (`acme.js`): tables of the 6502 family, passes, expressions, macros, pseudo ops, the Z80, the output formats, the disassembler; the tests of nerdsuite2 |
| `crates/nerdsuite_core/src/basic` | the BASIC tokenizer and detokenizer of every dialect (`basic/*.js`, `zx81.js`): Commodore .prg, Atari/CPC/TRS-80/BBC listings, ZX81 .p files |
| `crates/nerdsuite_core/src/build.rs` | the build of a program (`nsbuild.js`) |
| `crates/nerdsuite_core/src/disasm.rs` | the analysis of the Disassembler (`disasm.js`): marks, the trace, BASIC, the layouts of files, the lines, the source |
| `crates/nerdsuite_core/src/memmap` | the addresses of every machine and the bits of their registers (`memmap.js`); `data.rs` is generated from it |
| `crates/nerdsuite/src/disasm_view.rs` | the Disassembler view |
| `crates/nerdsuite_core/src/gfx.rs` | the VIC-II palette, charsets, the sprites of a `.spr` (YSPR), the screens of a `.scr` (YSCR) and how the VIC-II shows them, the ROM font of an emulator, the bitmaps of Koala and Art Studio |
| `crates/nerdsuite/src/pixels.rs`, `chars_view.rs`, `sprites_view.rs`, `screen_view.rs`, `bitmap_view.rs` | the picture element the graphics editors draw with (quads, runs of a color as one); the Chars view |
| `crates/nerdsuite_core/src/song.rs`, `sid.rs`, `songimport.rs`, `cpu6502.rs` | the song and its file (`song.js`), the model of the SID, the player and the engine of every three voices (`sid.js`, `sidplayer.js`), the imports (`songimport.js`) and the 6502 SID tunes run on (`cpu6502.js`) |
| `crates/nerdsuite/src/composer_view.rs`, `audio.rs` | the Composer view; the sound output the engine renders into |
| `crates/nerdsuite_core/src/run` | Run: where the tools are and the settings (`tools.rs`, `backend/core`), mimic and its debug protocol (`mimic.rs`, `backend/mimic`), the choice of the emulator and the launcher's command lines (`mod.rs`, `backend/emu`) |
| `crates/nerdsuite/src/main.rs` | the program: the window, the keys, the theme |
| `crates/nerdsuite/src/workspace.rs` | the shell: top bar, rail, explorer, views, status bar, open and save |
| `crates/nerdsuite/src/code_view.rs`, `editor.rs`, `text.rs` | the Code view, its editor surface (a gpui `Element` painting the lines in sight) and the text it edits |
| `crates/nerdsuite/src/hex_view.rs` | the Hex view |
| `crates/nerdsuite/src/workspace/debug.rs`, `debug_panel.rs` | Debug in the window: the session, the breakpoints, the Debugger view and its panels |
| `crates/nerdsuite_core/src/run/debug.rs` | the debugger: a thread with mimic, its commands and what it saw (`Snapshot`) |
| `crates/nerdsuite/src/theme.rs` | the colours of `m3.css` and `nerdsuite.css` |
| `examples/demo` | the demo project of nerdsuite2 |
