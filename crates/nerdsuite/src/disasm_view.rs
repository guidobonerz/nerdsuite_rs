//! The Disassembler view, as nerdsuite2's `disasmview.js`: a binary of the project, the disassembly
//! on the left and the bytes on the right. In auto mode the code is found by tracing it from its
//! entry points; the user selects bytes or lines and marks them as code or as data of a type. The
//! marks, the load address and the entry points of a file are kept in `nerdsuite.json`
//! (`disassembler`, the format of nerdsuite2). A .prg, .nes, .sfc and .a26 bring their layout: the
//! load address or the header, the vectors as entry points, and their machine.

use std::collections::HashMap;

use gpui::{
    ClipboardItem, Context, EventEmitter, FontWeight, Hsla, MouseButton, ScrollStrategy, SharedString, UniformListScrollHandle,
    Window, div, prelude::*, px, uniform_list,
};
use nerdsuite_core::{
    disasm::{self, DataType, Disassembly, Layout, Options, Region},
    hex::{self, CharSet},
};
use serde_json::{Value, json};

use crate::theme::{Theme, mono_font, theme};

/// What the view keeps of a file (`nerdsuite.json` › `disassembler` › its path).
#[derive(Debug, Clone, PartialEq)]
pub struct FileConfig {
    pub base: u32,
    pub auto: bool,
    pub regions: Vec<Region>,
    pub entries: Vec<u32>,
    pub machine: Option<String>,
    /// address (as 4 hex digits) -> name
    pub names: HashMap<u32, String>,
    /// the source it was saved as
    pub source: Option<String>,
}

impl Default for FileConfig {
    fn default() -> Self {
        FileConfig { base: 0, auto: true, regions: vec![], entries: vec![], machine: None, names: HashMap::new(), source: None }
    }
}

impl FileConfig {
    pub fn from_json(v: &Value) -> FileConfig {
        let mut c = FileConfig::default();
        if let Some(b) = v["base"].as_u64() {
            c.base = b as u32;
        }
        if let Some(a) = v["auto"].as_bool() {
            c.auto = a;
        }
        c.regions = v["regions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                Some(Region {
                    start: r["start"].as_u64()? as usize,
                    end: r["end"].as_u64()? as usize,
                    ty: DataType::from_id(r["type"].as_str()?)?,
                })
            })
            .collect();
        c.entries = v["entries"].as_array().into_iter().flatten().filter_map(|e| e.as_u64().map(|e| e as u32)).collect();
        c.machine = v["machine"].as_str().map(str::to_owned);
        if let Some(names) = v["names"].as_object() {
            c.names = names
                .iter()
                .filter_map(|(k, v)| Some((u32::from_str_radix(k, 16).ok()?, v.as_str()?.to_owned())))
                .collect();
        }
        c.source = v["source"].as_str().map(str::to_owned);
        c
    }

    pub fn to_json(&self) -> Value {
        let mut v = json!({
            "base": self.base,
            "auto": self.auto,
            "regions": self.regions.iter().map(|r| json!({ "start": r.start, "end": r.end, "type": r.ty.id() })).collect::<Vec<_>>(),
            "entries": self.entries,
        });
        if let Some(m) = &self.machine {
            v["machine"] = json!(m);
        }
        if !self.names.is_empty() {
            v["names"] = self.names.iter().map(|(a, n)| (format!("{a:04x}"), json!(n))).collect::<serde_json::Map<_, _>>().into();
        }
        if let Some(s) = &self.source {
            v["source"] = json!(s);
        }
        v
    }
}

/// The view asks the workspace: keep the settings of a file, save a source.
pub enum DisasmEvent {
    /// a binary of the project chosen in the bar
    OpenFile(String),
    ConfigChanged(String, FileConfig),
    SaveSource { path: String, text: String },
}

impl EventEmitter<DisasmEvent> for DisasmView {}

struct File {
    path: String,
    bytes: Vec<u8>,
    layout: Option<Layout>,
    head: usize,
    cfg: FileConfig,
    /// the machine and CPU of the project
    project_machine: String,
    project_cpu: String,
}

pub struct DisasmView {
    /// the binaries of the project
    files: Vec<String>,
    file: Option<File>,
    result: Option<Disassembly>,
    cursor: usize,
    anchor: Option<usize>,
    charset: CharSet,
    undo: Vec<FileConfig>,
    redo: Vec<FileConfig>,
    back: Vec<usize>,
    dis_scroll: UniformListScrollHandle,
    hex_scroll: UniformListScrollHandle,
    message: Option<String>,
}

impl DisasmView {
    pub fn new() -> Self {
        DisasmView {
            files: vec![],
            file: None,
            result: None,
            cursor: 0,
            anchor: None,
            charset: CharSet::Petscii,
            undo: vec![],
            redo: vec![],
            back: vec![],
            dis_scroll: UniformListScrollHandle::new(),
            hex_scroll: UniformListScrollHandle::new(),
            message: None,
        }
    }

    pub fn set_files(&mut self, files: Vec<String>, cx: &mut Context<Self>) {
        if self.files != files {
            self.files = files;
            cx.notify();
        }
    }

    pub fn path(&self) -> Option<&str> {
        self.file.as_ref().map(|f| f.path.as_str())
    }

    /// Opens a binary with what was kept of it; `machine` and `cpu` are the project's.
    pub fn open(&mut self, path: String, bytes: Vec<u8>, kept: Option<&Value>, machine: &str, cpu: &str, cx: &mut Context<Self>) {
        let layout = disasm::file_layout(&path, &bytes);
        let mut cfg = kept.map(FileConfig::from_json).unwrap_or_default();
        if let Some(l) = &layout {
            cfg.base = l.base; // a .prg or a cartridge knows where it goes
        }
        let head = layout.as_ref().map_or(0, |l| l.head);
        self.file = Some(File { path, bytes, layout, head, cfg, project_machine: machine.into(), project_cpu: cpu.into() });
        self.cursor = 0;
        self.anchor = None;
        self.undo.clear();
        self.redo.clear();
        self.back.clear();
        self.message = None;
        self.dis_scroll = UniformListScrollHandle::new();
        self.hex_scroll = UniformListScrollHandle::new();
        self.analyze();
        cx.notify();
    }

    /// The machine of the file: its own setting, else the project's for a .prg of a Commodore, else
    /// that of a cartridge, else the project's.
    fn machine(f: &File) -> String {
        if let Some(m) = &f.cfg.machine {
            return m.clone();
        }
        let own = nerdsuite_core::platform::platform_or_default(&f.project_machine);
        match &f.layout {
            Some(l) if l.prg && own.family == nerdsuite_core::platform::Family::Commodore => f.project_machine.clone(),
            Some(l) => l.machine.into(),
            None => f.project_machine.clone(),
        }
    }

    /// The CPU of that machine: the project's when it is the project's machine, else the machine's.
    fn cpu(f: &File) -> String {
        if let (Some(cpu), None) = (f.layout.as_ref().and_then(|l| l.cpu), &f.cfg.machine) {
            return cpu.into();
        }
        let machine = Self::machine(f);
        let m = nerdsuite_core::platform::platform_or_default(&machine);
        if m.id == f.project_machine { f.project_cpu.clone() } else { m.cpu.id().into() }
    }

    fn options(f: &File) -> Options {
        let lay = f.layout.as_ref();
        let data_len = f.bytes.len() - f.head;
        Options {
            base: f.cfg.base,
            cpu: Self::cpu(f),
            machine: Some(Self::machine(f)),
            regions: f.cfg.regions.clone(),
            entries: lay.map(|l| l.entries.clone()).unwrap_or_default().into_iter().chain(f.cfg.entries.iter().copied()).collect(),
            auto: f.cfg.auto,
            split: Some(lay.map_or(data_len, |l| l.split)),
            no_base: lay.is_some_and(|l| !l.entries.is_empty()),
            names: f.cfg.names.clone(),
        }
    }

    fn analyze(&mut self) {
        self.result = self.file.as_ref().map(|f| disasm::disassemble_all(&f.bytes[f.head..], &Self::options(f)));
    }

    /// The source of the disassembly: what assembles to the file again.
    fn source(&self) -> Option<String> {
        let f = self.file.as_ref()?;
        let r = self.result.as_ref()?;
        let name = f.path.rsplit('/').next().unwrap_or(&f.path);
        let ext = name.rsplit_once('.').map_or("bin", |(_, e)| e).to_lowercase();
        let prg = f.layout.as_ref().is_some_and(|l| l.prg);
        let head = (!prg).then(|| &f.bytes[..f.head]);
        let format = if prg { ("prg", "cbm") } else { (ext.as_str(), "plain") };
        Some(disasm::to_source(r, &Self::options(f), name, head, Some(format)))
    }

    fn change(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut FileConfig)) {
        let Some(file) = self.file.as_mut() else { return };
        self.undo.push(file.cfg.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        f(&mut file.cfg);
        cx.emit(DisasmEvent::ConfigChanged(file.path.clone(), file.cfg.clone()));
        self.analyze();
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.file.as_mut() else { return };
        if let Some(cfg) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut file.cfg, cfg));
            cx.emit(DisasmEvent::ConfigChanged(file.path.clone(), file.cfg.clone()));
            self.analyze();
            cx.notify();
        }
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.file.as_mut() else { return };
        if let Some(cfg) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut file.cfg, cfg));
            cx.emit(DisasmEvent::ConfigChanged(file.path.clone(), file.cfg.clone()));
            self.analyze();
            cx.notify();
        }
    }

    fn selection(&self) -> (usize, usize) {
        match self.anchor {
            Some(a) => (a.min(self.cursor), a.max(self.cursor)),
            None => (self.cursor, self.cursor),
        }
    }

    fn mark(&mut self, ty: Option<DataType>, cx: &mut Context<Self>) {
        let (a, b) = self.selection();
        let base = self.file.as_ref().map_or(0, |f| f.cfg.base);
        self.change(cx, |c| c.regions = disasm::mark_range(&c.regions, a, b, ty));
        let range = format!("${:04x}-${:04x}", (base + a as u32) & 0xffff, (base + b as u32) & 0xffff);
        self.message = Some(match ty {
            Some(t) => format!("{range} marked as {}", t.name().to_lowercase()),
            None => format!("{range} unmarked"),
        });
    }

    /// Selects a byte range: a click selects, Shift extends from the anchor.
    fn select(&mut self, start: usize, end: usize, extend: bool, reveal_dis: bool, reveal_hex: bool, cx: &mut Context<Self>) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
            self.cursor = if start < self.anchor.unwrap() { start } else { end };
        } else {
            self.anchor = Some(start);
            self.cursor = end;
        }
        if reveal_dis && let Some(ix) = self.result.as_ref().and_then(|r| r.line_at(self.cursor)) {
            self.dis_scroll.scroll_to_item(ix, ScrollStrategy::Center);
        }
        if reveal_hex {
            self.hex_scroll.scroll_to_item(self.cursor / hex::BYTES_PER_ROW, ScrollStrategy::Center);
        }
        cx.notify();
    }

    /// Enter: to where the operand (or the word, the SYS) of the line points.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let Some(r) = &self.result else { return };
        let Some(ix) = r.line_at(self.cursor) else { return };
        let Some(to) = r.lines[ix].reference else { return };
        let Some(target) = r.line_at(to) else { return };
        let (off, len) = (r.lines[target].off, r.lines[target].len);
        self.back.push(self.cursor);
        self.select(off, off + len - 1, false, true, true, cx);
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        if let Some(at) = self.back.pop()
            && let Some(ix) = self.result.as_ref().and_then(|r| r.line_at(at))
        {
            let (off, len) = { let l = &self.result.as_ref().unwrap().lines[ix]; (l.off, l.len) };
            self.select(off, off + len - 1, false, true, true, cx);
        }
    }

    fn type_color(ty: DataType, theme: &Theme) -> Hsla {
        match ty {
            DataType::Code => theme.on_surface.into(),
            DataType::Bytes => theme.on_surface_variant.into(),
            DataType::Words => theme.number.into(),
            DataType::Text | DataType::Petscii | DataType::Screen => theme.string.into(),
            DataType::Chars | DataType::Sprites | DataType::Bitmap | DataType::Tiles => theme.macro_.into(),
            DataType::Basic => theme.keyword.into(),
        }
    }
}

fn chip(id: impl Into<gpui::ElementId>, label: impl Into<SharedString>, active: bool, theme: Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px_2()
        .py_0p5()
        .rounded_md()
        .border_1()
        .border_color(theme.outline)
        .cursor_pointer()
        .text_xs()
        .when(active, |el| el.bg(theme.container_highest))
        .hover(|el| el.bg(theme.container))
        .child(label.into())
}

impl Render for DisasmView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let files = self.files.clone();
        let open_path = self.path().map(str::to_owned);
        let file_bar = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(div().text_xs().text_color(theme.on_surface_variant).pr_1().child("FILE"))
            .children(files.iter().cloned().map(|path| {
                let active = open_path.as_deref() == Some(path.as_str());
                chip(SharedString::from(format!("dis-file-{path}")), path.clone(), active, theme)
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(DisasmEvent::OpenFile(path.clone()))))
            }))
            .when(files.is_empty(), |el| {
                el.child(div().text_sm().text_color(theme.on_surface_variant).child("No binaries in the project: Build makes one in build/."))
            });

        let (Some(file), Some(result)) = (&self.file, &self.result) else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .gap_3()
                .p_4()
                .child(file_bar)
                .child(div().text_color(theme.on_surface_variant).child("Choose a binary: a .prg, a cartridge (.nes, .sfc, .a26) or any file of bytes."))
                .into_any_element();
        };

        let (sel_a, sel_b) = self.selection();
        let n_bytes = file.bytes.len() - file.head;
        let base = file.cfg.base;
        let machine = Self::machine(file);
        let cpu = Self::cpu(file);
        let note = file.layout.as_ref().and_then(|l| l.note.clone());
        let info = format!(
            "{} bytes · ${:04x}-${:04x} · {machine} · {cpu}{}",
            n_bytes,
            base,
            (base + n_bytes.saturating_sub(1) as u32) & 0xffff,
            note.map(|n| format!(" · {n}")).unwrap_or_default()
        );
        let auto = file.cfg.auto;
        let has_ref = result.line_at(self.cursor).and_then(|ix| result.lines[ix].reference).is_some();

        let tools = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(chip("dis-auto", "Auto", auto, theme).on_click(cx.listener(move |this, _, _, cx| this.change(cx, |c| c.auto = !auto))))
            .child(div().w(px(10.)))
            .children(DataType::MARKS.into_iter().map(|ty| {
                chip(SharedString::from(format!("dis-mark-{}", ty.id())), ty.name(), false, theme)
                    .text_color(Self::type_color(ty, &theme))
                    .on_click(cx.listener(move |this, _, _, cx| this.mark(Some(ty), cx)))
            }))
            .child(chip("dis-unmark", "Unmark", false, theme).on_click(cx.listener(|this, _, _, cx| this.mark(None, cx))))
            .child(div().w(px(10.)))
            .child(chip("dis-follow", "Follow ↵", false, theme).when(!has_ref, |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.follow(cx))))
            .child(chip("dis-back", "Back", false, theme).when(self.back.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.go_back(cx))))
            .child(chip("dis-undo", "Undo", false, theme).when(self.undo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.undo(cx))))
            .child(chip("dis-redo", "Redo", false, theme).when(self.redo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.redo(cx))))
            .child(div().w(px(10.)))
            .child(chip("dis-copy", "Copy source", false, theme).on_click(cx.listener(|this, _, _, cx| {
                if let Some(src) = this.source() {
                    cx.write_to_clipboard(ClipboardItem::new_string(src));
                    this.message = Some("Copied as ACME source".into());
                    cx.notify();
                }
            })))
            .child(chip("dis-save", "Save source", false, theme).on_click(cx.listener(|this, _, _, cx| {
                let Some(text) = this.source() else { return };
                let Some(file) = &this.file else { return };
                let stem: String = file
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .rsplit_once('.')
                    .map_or(file.path.as_str(), |(s, _)| s)
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                    .collect();
                let path = file.cfg.source.clone().unwrap_or(format!("src/{stem}_dis.asm"));
                this.change(cx, |c| c.source = Some(path.clone()));
                this.message = Some(format!("Saved as {path}"));
                cx.emit(DisasmEvent::SaveSource { path, text });
            })));

        // the disassembly: a line a row
        let lines_count = result.lines.len();
        let dis = uniform_list(
            "dis-lines",
            lines_count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                let theme = *crate::theme::theme(cx);
                let Some(r) = &this.result else { return vec![] };
                let (a, b) = this.selection();
                range
                    .map(|ix| {
                        let l = &r.lines[ix];
                        let selected = l.off <= b && l.off + l.len > a;
                        let bytes = this.file.as_ref().map(|f| {
                            let d = &f.bytes[f.head + l.off..f.head + l.off + l.len.min(4)];
                            let mut s = d.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
                            if l.len > 4 {
                                s.push('…');
                            }
                            s
                        });
                        let (off, len) = (l.off, l.len);
                        div()
                            .id(("dis-line", ix))
                            .w_full()
                            .flex()
                            .gap_3()
                            .px_2()
                            .h(px(20.))
                            .cursor_pointer()
                            .when(selected, |el| el.bg(theme.container_highest))
                            .hover(|el| el.bg(theme.container))
                            .child(div().w(px(40.)).flex_none().text_color(theme.on_surface_variant).child(format!("{:04x}", l.addr)))
                            .child(div().w(px(100.)).flex_none().text_color(theme.gutter).child(bytes.unwrap_or_default()))
                            .child(div().w(px(90.)).flex_none().overflow_hidden().text_color(theme.label).child(l.label.clone().unwrap_or_default()))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(DisasmView::type_color(l.ty, &theme))
                                    .child(l.view.clone().unwrap_or_else(|| l.text.clone())),
                            )
                            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_color(theme.comment).child(l.comment.clone().map(|c| format!("; {c}")).unwrap_or_default()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                                    if e.click_count >= 2 {
                                        this.follow(cx);
                                    } else {
                                        this.select(off, off + len - 1, e.modifiers.shift, false, true, cx);
                                    }
                                }),
                            )
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.dis_scroll)
        .flex_1()
        .font_family(mono_font())
        .text_size(px(13.))
        .line_height(px(20.));

        // the bytes: 16 a row, coloured by their type
        let rows = hex::row_count(n_bytes);
        let charset = self.charset;
        let hex_list = uniform_list(
            "dis-hex",
            rows,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                let theme = *crate::theme::theme(cx);
                let (Some(f), Some(r)) = (&this.file, &this.result) else { return vec![] };
                let data = &f.bytes[f.head..];
                let (a, b) = this.selection();
                let base = f.cfg.base;
                range
                    .map(|row| {
                        let start = row * hex::BYTES_PER_ROW;
                        let end = (start + hex::BYTES_PER_ROW).min(data.len());
                        let cells = (start..end).map(|i| {
                            let selected = i >= a && i <= b;
                            div()
                                .id(("dis-byte", i))
                                .w(px(22.))
                                .flex_none()
                                .cursor_pointer()
                                .text_color(DisasmView::type_color(r.types()[i], &theme))
                                .when(selected, |el| el.bg(theme.container_highest))
                                .child(format!("{:02x}", data[i]))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| this.select(i, i, e.modifiers.shift, true, false, cx)),
                                )
                        });
                        div()
                            .flex()
                            .gap_2()
                            .px_2()
                            .h(px(20.))
                            .child(div().w(px(40.)).flex_none().text_color(theme.on_surface_variant).child(format!("{:04x}", (base + start as u32) & 0xffff)))
                            .child(div().flex().flex_none().children(cells))
                            .child(div().text_color(theme.on_primary_container).child(hex::char_cells(&data[start..end], charset)))
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.hex_scroll)
        .flex_1()
        .font_family(mono_font())
        .text_size(px(13.))
        .line_height(px(20.));

        let selection = format!(
            "${:04x}-${:04x} ({} bytes){}",
            (base + sel_a as u32) & 0xffff,
            (base + sel_b as u32) & 0xffff,
            sel_b - sel_a + 1,
            self.message.as_ref().map(|m| format!(" · {m}")).unwrap_or_default()
        );

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(file_bar)
            .child(div().text_xs().text_color(theme.on_surface_variant).child(info))
            .child(tools)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .rounded_lg()
                            .border_1()
                            .border_color(theme.outline_variant)
                            .child(dis),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(620.))
                            .flex_none()
                            .rounded_lg()
                            .border_1()
                            .border_color(theme.outline_variant)
                            .child(
                                div().flex().gap_1().p_1().children(CharSet::ALL.into_iter().map(|c| {
                                    chip(SharedString::from(format!("dis-cs-{}", c.label())), c.label(), c == charset, theme)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.charset = c;
                                            cx.notify();
                                        }))
                                })),
                            )
                            .child(hex_list),
                    ),
            )
            .child(div().text_xs().font_weight(FontWeight::MEDIUM).text_color(theme.on_surface_variant).child(selection))
            .into_any_element()
    }
}
