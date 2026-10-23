//! The shell of nerdsuite2's `index.html` and `app.js`: the top bar, the rail of the views, the
//! files of the project as a tree, the view in front and a status bar.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use gpui::{
    App, Context, Entity, FocusHandle, Focusable, FontWeight, KeyBinding, PathPromptOptions,
    SharedString, Window, actions, div, prelude::*, px,
};
use nerdsuite_core::{
    build::{self as nsbuild, BUILD_DIR, DebugInfo, Problem},
    hex::CharSet,
    run::{
        Runner,
        debug::{Debugger, Snapshot},
        tools::ToolSettings,
    },
    project::{FileKind, Project},
    syntax::Language,
};

use crate::{
    code_view::{CodeView, CodeViewEvent},
    bitmap_view::{BitmapEvent, BitmapView},
    composer_view::{ComposerEvent, ComposerView},
    chars_view::{CharsEvent, CharsView},
    screen_view::{ScreenEvent, ScreenView},
    sprites_view::{SpritesEvent, SpritesView},
    disasm_view::{DisasmEvent, DisasmView},
    hex_view::HexView,
    theme::theme,
};

actions!(
    workspace,
    [
        Save,
        OpenFolder,
        Build,
        Run,
        Debug,
        DebugContinue,
        DebugPauseOrContinue,
        StepOver,
        StepInto,
        StepOut,
        StopDebugging,
        Quit
    ]
);

mod debug;

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(secondary!("s"), Save, None),
        KeyBinding::new(secondary!("b"), Build, None),
        KeyBinding::new("f5", Run, None),
        KeyBinding::new("f6", Debug, None),
        KeyBinding::new("f8", DebugPauseOrContinue, None),
        KeyBinding::new("f10", StepOver, None),
        KeyBinding::new("f11", StepInto, None),
        KeyBinding::new("shift-f11", StepOut, None),
        KeyBinding::new("shift-f5", StopDebugging, None),
        KeyBinding::new(secondary!("o"), OpenFolder, None),
        KeyBinding::new(secondary!("q"), Quit, None),
    ]);
}

/// The views of the rail, in nerdsuite2's order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Code,
    Debugger,
    Disassembler,
    Hex,
    Screen,
    Sprites,
    Chars,
    Tiles,
    Bitmap,
    Composer,
}

impl View {
    const ALL: [View; 10] = [
        View::Code,
        View::Debugger,
        View::Disassembler,
        View::Hex,
        View::Screen,
        View::Sprites,
        View::Chars,
        View::Tiles,
        View::Bitmap,
        View::Composer,
    ];

    fn label(self) -> &'static str {
        match self {
            View::Code => "Code",
            View::Debugger => "Debugger",
            View::Disassembler => "Disassembler",
            View::Hex => "Hex",
            View::Screen => "Screen",
            View::Sprites => "Sprites",
            View::Chars => "Chars",
            View::Tiles => "Tiles",
            View::Bitmap => "Bitmap",
            View::Composer => "Composer",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            View::Code => "</>",
            View::Debugger => "⚙",
            View::Disassembler => "≡",
            View::Hex => "#",
            View::Screen => "▣",
            View::Sprites => "☺",
            View::Chars => "▦",
            View::Tiles => "▩",
            View::Bitmap => "✎",
            View::Composer => "♫",
        }
    }

    /// What the view of nerdsuite2 does, for the ones not ported yet.
    fn description(self) -> &'static str {
        match self {
            View::Code | View::Hex => "",
            View::Debugger => {
                "The program in VICE through its binary remote monitor, or in mimic: breakpoints, stepping, registers, memory, disassembly, watches."
            }
            View::Disassembler => {
                "A program as code and data: traced from its entry points, labels, the registers of the machine named, ACME source that assembles to the same bytes."
            }
            View::Screen => "Char screens of 25 rows, 40 to 120 columns: charsets, multicolor, ECM, layers and their animations.",
            View::Sprites => "Sprites with frames, layers and animations, hires and multicolor.",
            View::Chars => "The 256 chars of a charset, tiles of chars, char animations.",
            View::Tiles => "Tilesets of 2 × 2 or 3 × 3 chars.",
            View::Bitmap => "Koala (multicolor) and Art Studio (hires) pictures.",
            View::Composer => "A SID tracker with GoatTracker's tables; GoatTracker songs, SID tunes and MIDI files imported.",
        }
    }
}

pub struct Workspace {
    project: Option<Project>,
    view: View,
    expanded: HashSet<String>,
    selected_file: Option<String>,
    code: Entity<CodeView>,
    hex: Entity<HexView>,
    disasm: Entity<DisasmView>,
    chars: Entity<CharsView>,
    sprites: Entity<SpritesView>,
    screen: Entity<ScreenView>,
    bitmap: Entity<BitmapView>,
    composer: Entity<ComposerView>,
    /// the C64 ROM font, read once from VICE or mimic
    rom_font: Option<Option<Vec<u8>>>,
    message: Option<(SharedString, bool)>,
    /// the errors and warnings of the last build
    problems: Vec<Problem>,
    /// the emulator of the last Run
    runner: Arc<Mutex<Runner>>,
    /// an emulator is being started
    starting: bool,
    /// the debug session in mimic
    debugger: Option<Debugger>,
    debug_version: u64,
    snapshot: Option<Snapshot>,
    /// the last build that succeeded, and its addresses and lines
    last_build: Option<nsbuild::Build>,
    debug_info: Option<DebugInfo>,
    /// the breakpoints of the sources: file -> lines (from 1)
    breakpoints: HashMap<String, BTreeSet<usize>>,
    memory_charset: CharSet,
    focus_handle: FocusHandle,
}

impl Workspace {
    pub fn new(project: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let code = cx.new(CodeView::new);
        let disasm = cx.new(|_| DisasmView::new());
        let chars = cx.new(|_| CharsView::new());
        let sprites = cx.new(|_| SpritesView::new());
        let screen = cx.new(|_| ScreenView::new());
        let bitmap = cx.new(|_| BitmapView::new());
        let composer = cx.new(ComposerView::new);
        cx.subscribe(&composer, |this, _, event: &ComposerEvent, cx| {
            if let ComposerEvent::OpenFile(path) = event {
                this.open_song(path, cx);
            }
            cx.notify();
        })
        .detach();
        cx.subscribe(&bitmap, |this, _, event: &BitmapEvent, cx| {
            if let BitmapEvent::OpenFile(path) = event {
                this.open_bitmap(path, cx);
            }
            cx.notify();
        })
        .detach();
        cx.subscribe(&screen, |this, _, event: &ScreenEvent, cx| {
            if let ScreenEvent::OpenFile(path) = event {
                this.open_screen(path, cx);
            }
            cx.notify();
        })
        .detach();
        cx.subscribe(&sprites, |this, _, event: &SpritesEvent, cx| {
            if let SpritesEvent::OpenFile(path) = event {
                this.open_sprites(path, cx);
            }
            cx.notify();
        })
        .detach();
        cx.subscribe(&chars, |this, _, event: &CharsEvent, cx| {
            if let CharsEvent::OpenFile(path) = event {
                this.open_charset(path, cx);
            }
            cx.notify();
        })
        .detach();
        cx.subscribe(&disasm, |this, _, event: &DisasmEvent, cx| this.on_disasm_event(event, cx)).detach();
        cx.subscribe(&code, |this, _, event: &CodeViewEvent, cx| {
            if let CodeViewEvent::BreakpointToggled(path, line) = event {
                this.toggle_breakpoint(path, *line, cx);
            }
            cx.notify();
        })
        .detach();
        // the debugger is asked what it saw, ten times a second
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
                if this.update_in(cx, |this, window, cx| this.poll_debugger(window, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        // the emulator of the last Run ends with the program
        cx.on_app_quit(|this: &mut Self, _| {
            if let Ok(mut runner) = this.runner.lock() {
                runner.stop();
            }
            this.debugger = None;
            async {}
        })
        .detach();
        let mut workspace = Workspace {
            project: None,
            view: View::Code,
            expanded: HashSet::new(),
            selected_file: None,
            code,
            hex: cx.new(|_| HexView::default()),
            disasm,
            chars,
            sprites,
            screen,
            bitmap,
            composer,
            rom_font: None,
            message: None,
            problems: Vec::new(),
            runner: Default::default(),
            starting: false,
            debugger: None,
            debug_version: 0,
            snapshot: None,
            last_build: None,
            debug_info: None,
            breakpoints: HashMap::new(),
            memory_charset: CharSet::Petscii,
            focus_handle: cx.focus_handle(),
        };
        if let Some(path) = project {
            workspace.open_project(path, window, cx);
        }
        workspace
    }

    pub fn open_project(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.code.read(cx).has_unsaved(cx) {
            self.message = Some(("Save the open sources before another project is opened (Ctrl+S).".into(), true));
            cx.notify();
            return;
        }
        match Project::open(&path) {
            Ok(project) => {
                window.set_window_title(&format!("{} - NerdSuite", project.name()));
                self.code.update(cx, |code, cx| code.close_all(cx));
                self.hex.update(cx, |hex, cx| hex.close(cx));
                self.expanded = ["src".to_owned()].into();
                self.selected_file = None;
                self.message = Some((format!("Opened {}", project.root.display()).into(), false));
                let main = project.settings.main.clone();
                self.project = Some(project);
                self.view = View::Code;
                if let Some(main) = main {
                    self.open_file(&main, window, cx);
                }
            }
            Err(error) => self.message = Some((format!("{error:#}").into(), true)),
        }
        cx.notify();
    }

    fn open_folder(&mut self, _: &OpenFolder, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open project".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update_in(cx, |this, window, cx| this.open_project(path, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// A text file into the Code view, any other into the Hex view (the editors of the graphics
    /// and the composer are not ported yet).
    fn open_file(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.as_ref() else { return };
        self.selected_file = Some(path.to_owned());
        let bytes = match project.read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.message = Some((format!("{error:#}").into(), true));
                cx.notify();
                return;
            }
        };
        let kind = FileKind::of(path.as_ref());
        if kind.is_text() {
            self.view = View::Code;
            self.code.update(cx, |code, cx| {
                code.open(path, Language::of(kind), || String::from_utf8_lossy(&bytes).into_owned(), window, cx)
            });
            self.sync_editor(path, cx);
        } else if kind == FileKind::Bitmap {
            self.open_bitmap(path, cx);
        } else if kind == FileKind::Screen {
            self.open_screen(path, cx);
        } else if kind == FileKind::Sprite {
            self.open_sprites(path, cx);
        } else if kind == FileKind::Charset {
            self.open_charset(path, cx);
        } else if kind == FileKind::Song {
            self.open_song(path, cx);
        } else if is_foreign_song(path) {
            // a SID tune, a MIDI file: imported as a new song
            self.view = View::Composer;
            self.composer.update(cx, |c, cx| c.import(path.to_owned(), bytes, true, cx));
        } else if self.view == View::Disassembler && is_binary(path) {
            self.open_in_disasm(path, bytes, cx);
        } else {
            self.view = View::Hex;
            self.hex.update(cx, |hex, cx| hex.open(path.to_owned(), bytes, cx));
        }
        cx.notify();
    }

    /// The files of the project of a kind.
    fn files_of(&self, kind: FileKind) -> Vec<String> {
        self.project
            .as_ref()
            .map(|p| p.files.iter().filter(|f| !f.is_dir && FileKind::of(f.path.as_ref()) == kind).map(|f| f.path.clone()).collect())
            .unwrap_or_default()
    }

    fn open_bitmap(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(project) = self.project.as_ref() else { return };
        if self.bitmap.read(cx).is_dirty() && self.bitmap.read(cx).path() != Some(path) {
            self.message = Some(("Save the picture before another one is opened (Ctrl+S).".into(), true));
            cx.notify();
            return;
        }
        match project.read(path) {
            Ok(bytes) => {
                self.selected_file = Some(path.to_owned());
                self.view = View::Bitmap;
                self.bitmap.update(cx, |b, cx| b.open(path.to_owned(), &bytes, cx));
            }
            Err(e) => self.message = Some((format!("{e:#}").into(), true)),
        }
        cx.notify();
    }

    fn open_screen(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(project) = self.project.as_ref() else { return };
        if self.screen.read(cx).is_dirty() && self.screen.read(cx).path() != Some(path) {
            self.message = Some(("Save the screen before another one is opened (Ctrl+S).".into(), true));
            cx.notify();
            return;
        }
        match project.read(path) {
            Ok(bytes) => {
                self.selected_file = Some(path.to_owned());
                self.view = View::Screen;
                self.screen.update(cx, |s, cx| s.open(path.to_owned(), &bytes, cx));
            }
            Err(e) => self.message = Some((format!("{e:#}").into(), true)),
        }
        cx.notify();
    }

    /// The charset of the screen in front: the one of the Chars view when it is open there (its changes
    /// show at once), else its file, else the ROM font.
    fn screen_charset(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.screen.read(cx).charset_path() else { return };
        let bytes = if name.is_empty() {
            let rom = self.rom_font.get_or_insert_with(nerdsuite_core::gfx::c64_rom_font);
            rom.as_ref().map(|r| r[..2048].to_vec())
        } else if self.chars.read(cx).path() == Some(name.as_str()) {
            Some(self.chars.read(cx).bytes().to_vec())
        } else {
            self.project.as_ref().and_then(|p| p.read(&name).ok()).map(|b| nerdsuite_core::gfx::Charset::from_file(&b).bytes)
        };
        let bytes = bytes.unwrap_or_else(|| vec![0; 2048]);
        self.screen.update(cx, |s, cx| s.set_charset(&bytes, &name, cx));
    }

    fn open_sprites(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(project) = self.project.as_ref() else { return };
        if self.sprites.read(cx).is_dirty() && self.sprites.read(cx).path() != Some(path) {
            self.message = Some(("Save the sprites before others are opened (Ctrl+S).".into(), true));
            cx.notify();
            return;
        }
        match project.read(path) {
            Ok(bytes) => {
                self.selected_file = Some(path.to_owned());
                self.view = View::Sprites;
                self.sprites.update(cx, |s, cx| s.open(path.to_owned(), &bytes, cx));
            }
            Err(e) => self.message = Some((format!("{e:#}").into(), true)),
        }
        cx.notify();
    }

    fn open_charset(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(project) = self.project.as_ref() else { return };
        if self.chars.read(cx).is_dirty() && self.chars.read(cx).path() != Some(path) {
            self.message = Some(("Save the charset before another one is opened (Ctrl+S).".into(), true));
            cx.notify();
            return;
        }
        match project.read(path) {
            Ok(bytes) => {
                self.selected_file = Some(path.to_owned());
                self.view = View::Chars;
                self.chars.update(cx, |c, cx| c.open(path.to_owned(), &bytes, cx));
            }
            Err(e) => self.message = Some((format!("{e:#}").into(), true)),
        }
        cx.notify();
    }

    fn open_song(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(project) = self.project.as_ref() else { return };
        if self.composer.read(cx).is_dirty() && self.composer.read(cx).path() != Some(path) {
            self.message = Some(("Save the song before another one is opened (Ctrl+S).".into(), true));
            cx.notify();
            return;
        }
        match project.read(path) {
            Ok(bytes) => {
                self.selected_file = Some(path.to_owned());
                self.view = View::Composer;
                self.composer.update(cx, |c, cx| c.open(path.to_owned(), &bytes, cx));
            }
            Err(e) => self.message = Some((format!("{e:#}").into(), true)),
        }
        cx.notify();
    }

    /// The binaries of the project the Disassembler offers.
    fn binaries(&self) -> Vec<String> {
        self.project
            .as_ref()
            .map(|p| p.files.iter().filter(|f| !f.is_dir && is_binary(&f.path)).map(|f| f.path.clone()).collect())
            .unwrap_or_default()
    }

    fn open_in_disasm(&mut self, path: &str, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let Some(project) = self.project.as_ref() else { return };
        let kept = project.settings.extra.get("disassembler").and_then(|d| d.get(path)).cloned();
        let machine = project.settings.machine.clone();
        let cpu = project.settings.cpu.clone().unwrap_or_else(|| project.platform().cpu.id().to_owned());
        self.selected_file = Some(path.to_owned());
        self.view = View::Disassembler;
        self.disasm.update(cx, |d, cx| d.open(path.to_owned(), bytes, kept.as_ref(), &machine, &cpu, cx));
    }

    fn on_disasm_event(&mut self, event: &DisasmEvent, cx: &mut Context<Self>) {
        let Some(project) = self.project.as_mut() else { return };
        let result = match event {
            DisasmEvent::OpenFile(path) => {
                match project.read(path) {
                    Ok(bytes) => self.open_in_disasm(path, bytes, cx),
                    Err(e) => self.message = Some((format!("{e:#}").into(), true)),
                }
                Ok(())
            }
            // the marks of a file into nerdsuite.json (`disassembler`), as nerdsuite2 keeps them
            DisasmEvent::ConfigChanged(path, cfg) => {
                let all = project
                    .settings
                    .extra
                    .entry("disassembler")
                    .or_insert_with(|| serde_json::Value::Object(Default::default()));
                if let Some(all) = all.as_object_mut() {
                    all.insert(path.clone(), cfg.to_json());
                }
                project.save_settings()
            }
            DisasmEvent::SaveSource { path, text } => {
                let r = project.write(path, text.as_bytes());
                project.rescan().ok();
                if r.is_ok() {
                    self.message = Some((format!("Saved {path}").into(), false));
                }
                r
            }
        };
        if let Err(e) = result {
            self.message = Some((format!("{e:#}").into(), true));
        }
        cx.notify();
    }

    /// What Build makes: the source in the editor (not an include), else the main program.
    fn build_target(&self, cx: &App) -> Option<String> {
        let project = self.project.as_ref()?;
        let is_program = |path: &str| {
            matches!(FileKind::of(path.as_ref()), FileKind::Assembler | FileKind::Basic)
                && !path.to_lowercase().ends_with(".inc")
        };
        let current = self.code.read(cx).active_editor().map(|e| e.read(cx).path.clone());
        current
            .filter(|path| is_program(path))
            .or_else(|| project.settings.main.clone())
            .or_else(|| project.files.iter().find(|f| !f.is_dir && is_program(&f.path)).map(|f| f.path.clone()))
    }

    /// Builds the program into `build/`: the build when it succeeded.
    fn build_now(&mut self, cx: &mut Context<Self>) -> Option<nsbuild::Build> {
        let Some(main) = self.build_target(cx) else {
            self.message = Some(("No program to build: open a source or set the main program.".into(), true));
            cx.notify();
            return None;
        };
        let texts = self.code.read(cx).texts(cx);
        let project = self.project.as_mut()?;
        let build = nsbuild::build_program(project, &main, &texts);
        let warnings = build.problems.iter().filter(|p| p.warning).count();
        let message: (SharedString, bool) = match (&build.file, build.ok) {
            (Some(file), true) => {
                let written = build
                    .outputs()
                    .into_iter()
                    .try_for_each(|(name, bytes)| project.write(&format!("{BUILD_DIR}/{name}"), &bytes));
                project.rescan().ok();
                match written {
                    Ok(()) => {
                        let from = build.start.map(|s| format!(" from ${s:04x}")).unwrap_or_default();
                        let warnings = match warnings {
                            0 => String::new(),
                            1 => " · 1 warning".into(),
                            n => format!(" · {n} warnings"),
                        };
                        let size = file.bytes.len();
                        (format!("Built {BUILD_DIR}/{}: {size} bytes{from}{warnings}", file.name).into(), false)
                    }
                    Err(error) => (format!("{error:#}").into(), true),
                }
            }
            _ => {
                let msg = match build.errors().next() {
                    Some(p) => format!(
                        "Build failed: {}{} {}",
                        p.file.clone().unwrap_or_default(),
                        p.line.map(|l| format!(":{l}")).unwrap_or_default(),
                        p.msg
                    ),
                    None => "Build failed: no program".into(),
                };
                (msg.into(), true)
            }
        };
        self.message = Some(message);
        self.code.update(cx, |code, cx| code.set_problems(&build.problems, cx));
        self.problems = build.problems.clone();
        cx.notify();
        if !build.ok {
            return None;
        }
        self.debug_info = Some(DebugInfo::new(&build));
        self.last_build = Some(build.clone());
        Some(build)
    }

    fn build(&mut self, _: &Build, _: &mut Window, cx: &mut Context<Self>) {
        self.build_now(cx);
    }

    /// F5: builds, then runs the program in the emulator of the settings (mimic, VICE, Altirra …).
    fn run(&mut self, _: &Run, window: &mut Window, cx: &mut Context<Self>) {
        if self.starting {
            return;
        }
        if self.debugger.is_some() {
            // F5 in a debug session: on
            return self.debug_continue(&DebugContinue, window, cx);
        }
        let Some(build) = self.build_now(cx) else { return };
        let Some(project) = self.project.as_ref() else { return };
        let platform = project.platform();
        self.starting = true;
        self.message = Some((format!("Starting {}…", build.file.as_ref().map_or("", |f| f.name.as_str())).into(), false));
        cx.notify();
        let runner = self.runner.clone();
        let task = cx.background_executor().spawn(async move {
            let settings = ToolSettings::load();
            runner.lock().unwrap().run(platform, &build, &settings)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.starting = false;
                this.message = Some(match result {
                    Ok(started) => (started.into(), false),
                    Err(error) => (format!("Run: {error:#}").into(), true),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Opens the file of a problem at its line.
    fn go_to_problem(&mut self, problem: &Problem, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = problem.file.clone() else { return };
        self.open_file(&file, window, cx);
        self.code.update(cx, |code, cx| code.set_problems(&self.problems, cx));
        if let Some(editor) = self.code.read(cx).active_editor().cloned()
            && editor.read(cx).path == file
        {
            let (line, col) = (problem.line.unwrap_or(1), problem.col.unwrap_or(1));
            editor.update(cx, |editor, cx| editor.go_to(line, col, cx));
            window.focus(&editor.focus_handle(cx), cx);
        }
    }

    fn render_problems(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let errors = self.problems.iter().filter(|p| !p.warning).count();
        let warnings = self.problems.len() - errors;
        div()
            .flex()
            .flex_col()
            .flex_none()
            .max_h(px(180.))
            .border_t_1()
            .border_color(theme.outline_variant)
            .bg(theme.container_low)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.on_surface_variant)
                    .child("PROBLEMS")
                    .child(format!("{errors} errors · {warnings} warnings"))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("problems-close")
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|el| el.bg(theme.container_highest))
                            .child("×")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.problems.clear();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("problems")
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .pb_1()
                    .children(self.problems.iter().enumerate().map(|(ix, problem)| {
                        let p = problem.clone();
                        let place = format!(
                            "{}{}",
                            p.file.clone().unwrap_or_default(),
                            p.line.map(|l| format!(":{l}")).unwrap_or_default()
                        );
                        div()
                            .id(("problem", ix))
                            .flex()
                            .gap_3()
                            .px_4()
                            .text_sm()
                            .cursor_pointer()
                            .hover(|el| el.bg(theme.container))
                            .child(
                                div()
                                    .w(px(14.))
                                    .text_color(if p.warning { theme.warning } else { theme.error })
                                    .child(if p.warning { "▲" } else { "●" }),
                            )
                            .child(div().text_color(theme.on_surface_variant).child(place))
                            .child(p.msg.clone())
                            .on_click(cx.listener(move |this, _, window, cx| this.go_to_problem(&p, window, cx)))
                    })),
            )
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.project.as_ref() else { return };
        let dirty = self
            .code
            .read(cx)
            .editors()
            .filter(|editor| editor.read(cx).is_dirty())
            .cloned()
            .collect::<Vec<_>>();
        let mut saved = 0;
        // the charset of the Chars view, the costume of the Sprites view
        let graphics = [
            self.chars.read(cx).unsaved().map(|f| (f, 0)),
            self.sprites.read(cx).unsaved().map(|f| (f, 1)),
            self.screen.read(cx).unsaved().map(|f| (f, 2)),
            self.bitmap.read(cx).unsaved().map(|f| (f, 3)),
        ];
        for ((path, bytes), which) in graphics.into_iter().flatten() {
            match project.write(&path, &bytes) {
                Ok(()) => {
                    match which {
                        0 => self.chars.update(cx, |c, cx| c.mark_saved(cx)),
                        1 => self.sprites.update(cx, |s, cx| s.mark_saved(cx)),
                        2 => self.screen.update(cx, |s, cx| s.mark_saved(cx)),
                        _ => self.bitmap.update(cx, |b, cx| b.mark_saved(cx)),
                    }
                    saved += 1;
                }
                Err(error) => {
                    self.message = Some((format!("{error:#}").into(), true));
                    cx.notify();
                    return;
                }
            }
        }
        // the song of the Composer: a new one gets its file
        if let Some((path, bytes)) = self.composer.read(cx).unsaved() {
            let new = !project.files.iter().any(|f| f.path == path);
            match project.write(&path, &bytes) {
                Ok(()) => {
                    self.composer.update(cx, |c, cx| c.mark_saved(path, cx));
                    saved += 1;
                    if new && let Some(project) = self.project.as_mut() {
                        project.rescan().ok();
                    }
                }
                Err(error) => {
                    self.message = Some((format!("{error:#}").into(), true));
                    cx.notify();
                    return;
                }
            }
        }
        let Some(project) = self.project.as_ref() else { return };
        for editor in dirty {
            let (path, text) = {
                let editor = editor.read(cx);
                (editor.path.clone(), editor.text().to_owned())
            };
            match project.write(&path, text.as_bytes()) {
                Ok(()) => {
                    editor.update(cx, |editor, cx| editor.mark_saved(cx));
                    saved += 1;
                }
                Err(error) => {
                    self.message = Some((format!("{error:#}").into(), true));
                    cx.notify();
                    return;
                }
            }
        }
        let message: SharedString = match saved {
            0 => "Nothing to save".into(),
            1 => "Saved 1 file".into(),
            n => format!("Saved {n} files").into(),
        };
        self.message = Some((message, false));
        cx.notify();
    }

    fn set_view(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        self.view = view;
        if view == View::Code {
            window.focus(&self.code.focus_handle(cx), cx);
        }
        if view == View::Composer {
            window.focus(&self.composer.focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn toggle_dir(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_owned());
        }
        cx.notify();
    }

    // --- the parts of the window -----------------------------------------------------------------

    fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let dirty = self.code.read(cx).has_unsaved(cx) || self.chars.read(cx).is_dirty() || self.sprites.read(cx).is_dirty() || self.screen.read(cx).is_dirty() || self.bitmap.read(cx).is_dirty() || self.composer.read(cx).is_dirty();
        let project = self.project.as_ref();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .h(px(52.))
            .px_4()
            .border_b_1()
            .border_color(theme.outline_variant)
            .bg(theme.surface)
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("NerdSuite"),
            )
            .child(
                div()
                    .text_color(theme.on_surface_variant)
                    .child(match project {
                        Some(project) => format!("{} · {}", project.name(), project.platform().name),
                        None => "no project".to_owned(),
                    }),
            )
            .child(div().flex_1())
            .child(button("open-folder", "Open folder…", theme).on_click(cx.listener(
                |this, _, window, cx| this.open_folder(&OpenFolder, window, cx),
            )))
            .when(project.is_some(), |el| {
                el.child(
                    button("build", "Build", theme)
                        .on_click(cx.listener(|this, _, window, cx| this.build(&Build, window, cx))),
                )
                .child(
                    button("run", if self.starting { "Starting…" } else { "Run" }, theme)
                        .on_click(cx.listener(|this, _, window, cx| this.run(&Run, window, cx))),
                )
            })
            .when(project.is_some(), |el| {
                el.child(
                    button("save", if dirty { "Save ●" } else { "Save" }, theme)
                        .on_click(cx.listener(|this, _, window, cx| this.save(&Save, window, cx))),
                )
            })
    }

    fn render_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(88.))
            .py_2()
            .gap_1()
            .border_r_1()
            .border_color(theme.outline_variant)
            .bg(theme.surface)
            .children(View::ALL.into_iter().map(|view| {
                let active = view == self.view;
                div()
                    .id(view.label())
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_0p5()
                    .py_1()
                    .cursor_pointer()
                    .child(
                        div()
                            .w(px(56.))
                            .h(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .when(active, |el| el.bg(theme.container_highest))
                            .hover(|el| el.bg(theme.container))
                            .child(view.icon()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .when(active, |el| el.font_weight(FontWeight::SEMIBOLD))
                            .when(!active, |el| el.text_color(theme.on_surface_variant))
                            .child(view.label()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.set_view(view, window, cx)))
            }))
    }

    fn render_explorer(&self, project: &Project, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        // a file is in sight when every folder above it is open
        let mut hidden_below: Option<usize> = None;
        let mut rows = Vec::new();
        for entry in &project.files {
            if let Some(depth) = hidden_below {
                if entry.depth > depth {
                    continue;
                }
                hidden_below = None;
            }
            let expanded = entry.is_dir && self.expanded.contains(&entry.path);
            if entry.is_dir && !expanded {
                hidden_below = Some(entry.depth);
            }
            let selected = self.selected_file.as_deref() == Some(entry.path.as_str());
            let is_main = project.settings.main.as_deref() == Some(entry.path.as_str());
            let path = entry.path.clone();
            let is_dir = entry.is_dir;
            rows.push(
                div()
                    .id(SharedString::from(format!("file-{}", entry.path)))
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(24.))
                    .pl(px(8. + 14. * entry.depth as f32))
                    .pr_2()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .when(selected, |el| el.bg(theme.container_highest))
                    .hover(|el| el.bg(theme.container))
                    .child(
                        div()
                            .w(px(12.))
                            .text_color(theme.on_surface_variant)
                            .child(match (is_dir, expanded) {
                                (true, true) => "▾",
                                (true, false) => "▸",
                                _ => "",
                            }),
                    )
                    .child(entry.name.clone())
                    .when(is_main, |el| {
                        el.child(div().text_xs().text_color(theme.tab_active).child("main"))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if is_dir {
                            this.toggle_dir(&path, cx);
                        } else {
                            this.open_file(&path, window, cx);
                        }
                    })),
            );
        }
        div()
            .id("explorer")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(240.))
            .p_2()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(theme.outline_variant)
            .bg(theme.container_low)
            .child(
                div()
                    .px_2()
                    .pb_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.on_surface_variant)
                    .child(project.name().to_uppercase()),
            )
            .children(rows)
    }

    fn render_main(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = *theme(cx);
        match self.view {
            View::Code => div()
                .size_full()
                .flex()
                .flex_col()
                .child(div().flex_1().min_h_0().child(self.code.clone()))
                .when(!self.problems.is_empty(), |el| el.child(self.render_problems(cx)))
                .into_any_element(),
            View::Hex => self.hex.clone().into_any_element(),
            View::Screen => self.screen.clone().into_any_element(),
            View::Bitmap => self.bitmap.clone().into_any_element(),
            View::Composer => {
                let files = self.files_of(FileKind::Song);
                let first = files.first().cloned();
                self.composer.update(cx, |c, cx| c.set_files(files, cx));
                if !self.composer.read(cx).has_song()
                    && let Some(first) = first
                    && let Some(project) = &self.project
                    && let Ok(bytes) = project.read(&first)
                {
                    self.composer.update(cx, |c, cx| c.open(first, &bytes, cx));
                }
                self.composer.clone().into_any_element()
            }
            View::Sprites => {
                let files = self.files_of(FileKind::Sprite);
                let first = files.first().cloned();
                self.sprites.update(cx, |s, cx| s.set_files(files, cx));
                if self.sprites.read(cx).path().is_none()
                    && let Some(first) = first
                    && let Some(project) = &self.project
                    && let Ok(bytes) = project.read(&first)
                {
                    self.sprites.update(cx, |s, cx| s.open(first, &bytes, cx));
                }
                self.sprites.clone().into_any_element()
            }
            View::Chars => {
                let files = self.files_of(FileKind::Charset);
                let first = files.first().cloned();
                self.chars.update(cx, |c, cx| c.set_files(files, cx));
                if self.chars.read(cx).path().is_none()
                    && let Some(first) = first
                    && let Some(project) = &self.project
                    && let Ok(bytes) = project.read(&first)
                {
                    self.chars.update(cx, |c, cx| c.open(first, &bytes, cx));
                }
                self.chars.clone().into_any_element()
            }
            View::Disassembler => {
                let files = self.binaries();
                self.disasm.update(cx, |d, cx| d.set_files(files, cx));
                self.disasm.clone().into_any_element()
            }
            View::Debugger => self.render_debugger(cx),
            view => div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .p_8()
                .child(div().text_2xl().child(view.icon()))
                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(view.label()))
                .child(
                    div()
                        .max_w(px(520.))
                        .text_center()
                        .text_color(theme.on_surface_variant)
                        .child(view.description()),
                )
                .child(
                    div()
                        .pt_2()
                        .text_sm()
                        .text_color(theme.on_surface_variant)
                        .child("Not ported to the Rust version yet."),
                )
                .into_any_element(),
        }
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let mut parts: Vec<String> = Vec::new();
        if let Some(project) = self.project.as_ref() {
            let platform = project.platform();
            parts.push(platform.short.to_owned());
            parts.push(project.settings.cpu.clone().unwrap_or_else(|| platform.cpu.id().to_owned()));
            if let Some(main) = project.settings.main.as_ref() {
                parts.push(format!("main: {main}"));
            }
        }
        if self.view == View::Code
            && let Some(editor) = self.code.read(cx).active_editor()
        {
            let (line, column) = editor.read(cx).caret_position();
            parts.push(format!("Ln {line}, Col {column}"));
        }
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_4()
            .h(px(26.))
            .px_4()
            .text_xs()
            .border_t_1()
            .border_color(theme.outline_variant)
            .bg(theme.container_low)
            .text_color(theme.on_surface_variant)
            .children(parts)
            .child(div().flex_1())
            .when_some(self.message.clone(), |el, (message, error)| {
                el.child(div().when(error, |el| el.text_color(theme.error)).child(message))
            })
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let demo = crate::demo_project();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(div().text_2xl().font_weight(FontWeight::SEMIBOLD).child("NerdSuite"))
            .child(
                div()
                    .text_color(theme.on_surface_variant)
                    .child("An IDE for programs of 8 bit machines."),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .pt_2()
                    .child(button("welcome-open", "Open folder…", theme).on_click(cx.listener(
                        |this, _, window, cx| this.open_folder(&OpenFolder, window, cx),
                    )))
                    .when_some(demo, |el, demo| {
                        el.child(button("welcome-demo", "Open the demo", theme).on_click(cx.listener(
                            move |this, _, window, cx| this.open_project(demo.clone(), window, cx),
                        )))
                    }),
            )
    }
}

/// A file of bytes the Disassembler takes.
fn is_binary(path: &str) -> bool {
    let lower = path.to_lowercase();
    ["prg", "bin", "raw", "dat", "a26", "xex", "nes", "sfc", "smc", "rom"]
        .iter()
        .any(|ext| lower.ends_with(&format!(".{ext}")))
}

fn button(id: &'static str, label: &'static str, theme: crate::theme::Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(theme.outline)
        .cursor_pointer()
        .text_sm()
        .hover(|el| el.bg(theme.container))
        .child(label)
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // the colours follow the system's light or dark
        let appearance_theme = crate::theme::Theme::for_appearance(window.appearance());
        if appearance_theme.dark != theme(cx).dark {
            cx.set_global(appearance_theme);
        }
        // another view: the song stops
        if self.view != View::Composer {
            self.composer.update(cx, |c, cx| c.leave(cx));
        }
        // the Bitmap view: its files, the first one when none is open
        if self.view == View::Bitmap {
            let files = self.files_of(FileKind::Bitmap);
            let first = files.first().cloned();
            self.bitmap.update(cx, |b, cx| b.set_files(files, cx));
            if self.bitmap.read(cx).path().is_none()
                && let Some(first) = first
                && let Some(project) = &self.project
                && let Ok(bytes) = project.read(&first)
            {
                self.bitmap.update(cx, |b, cx| b.open(first, &bytes, cx));
            }
        }
        // the Screen view: its files, the first one when none is open, its charset
        if self.view == View::Screen {
            let files = self.files_of(FileKind::Screen);
            let first = files.first().cloned();
            self.screen.update(cx, |s, cx| s.set_files(files, cx));
            if self.screen.read(cx).path().is_none()
                && let Some(first) = first
                && let Some(project) = &self.project
                && let Ok(bytes) = project.read(&first)
            {
                self.screen.update(cx, |s, cx| s.open(first, &bytes, cx));
            }
            self.screen_charset(cx);
        }
        let theme = *theme(cx);

        let body = match self.project.as_ref() {
            Some(project) => div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(self.render_rail(cx))
                .child(self.render_explorer(project, cx))
                .child(div().flex_1().min_w_0().child(self.render_main(cx)))
                .into_any_element(),
            None => self.render_welcome(cx).into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .key_context("Workspace")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::build))
            .on_action(cx.listener(Self::run))
            .on_action(cx.listener(Self::debug))
            .on_action(cx.listener(Self::debug_continue))
            .on_action(cx.listener(Self::debug_pause_or_continue))
            .on_action(cx.listener(Self::step_over))
            .on_action(cx.listener(Self::step_into))
            .on_action(cx.listener(Self::step_out))
            .on_action(cx.listener(Self::stop_debugging))
            .on_action(cx.listener(Self::open_folder))
            .bg(theme.surface)
            .text_color(theme.on_surface)
            .text_size(px(14.))
            .child(self.render_top_bar(cx))
            .child(body)
            .child(self.render_status_bar(cx))
    }
}

/// A song of another program the Composer imports: a SID tune, a MIDI file.
fn is_foreign_song(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".sid", ".mid", ".midi", ".rmi"].iter().any(|e| lower.ends_with(e))
}
