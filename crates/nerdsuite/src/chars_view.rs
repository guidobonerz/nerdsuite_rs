//! The Chars view, after nerdsuite2's `chars.js`: a charset of the project (a .chr of 2048 bytes),
//! the character in work large - the left button draws, the right one erases - and all 256 of them
//! in a grid to pick from. Hires or multicolor (a bit pair a pixel: the background, $d022, $d023,
//! the cell color); clear, invert, flip, shift, rotate, copy and paste; undo. Save (Ctrl+S) writes
//! the file.

use gpui::{
    Context, EventEmitter, FontWeight, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, SharedString, Window, div, prelude::*, px,
    rgb,
};
use nerdsuite_core::gfx::{COLOR_NAMES, Charset, Mode, PALETTE};

use crate::{
    pixels::PixelCanvas,
    theme::{Theme, mono_font, theme},
};

/// The colors a character is shown in: the background ($d021), $d022, $d023 and the cell color.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    Background,
    Multi1,
    Multi2,
    Cell,
}

pub enum CharsEvent {
    /// a charset of the project chosen in the bar
    OpenFile(String),
    /// the dirty mark changed
    Changed,
}

impl EventEmitter<CharsEvent> for CharsView {}

pub struct CharsView {
    files: Vec<String>,
    path: Option<String>,
    charset: Charset,
    saved: Charset,
    code: u8,
    mode: Mode,
    /// the pen the left button draws with: 1 in hires, 0-3 in multicolor
    pen: u8,
    /// the colors of the slots: background, $d022, $d023, the cell color
    colors: [u8; 4],
    slot: Slot,
    clip: Option<[u8; 8]>,
    undo: Vec<Charset>,
    redo: Vec<Charset>,
    /// the Charset before the stroke in work: an undo step once the stroke changes something
    stroke_undo: Option<Charset>,
    /// the pen of the stroke in work (the right button: 0)
    stroke: Option<u8>,
    editor_bounds: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    grid_bounds: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
}

fn color(index: u8) -> Hsla {
    rgb(PALETTE[index as usize & 15]).into()
}

impl CharsView {
    pub fn new() -> Self {
        CharsView {
            files: vec![],
            path: None,
            charset: Charset::default(),
            saved: Charset::default(),
            code: 1,
            mode: Mode::Hires,
            pen: 1,
            colors: [6, 11, 12, 14],
            slot: Slot::Cell,
            clip: None,
            undo: vec![],
            redo: vec![],
            stroke_undo: None,
            stroke: None,
            editor_bounds: Default::default(),
            grid_bounds: Default::default(),
        }
    }

    pub fn set_files(&mut self, files: Vec<String>, cx: &mut Context<Self>) {
        if self.files != files {
            self.files = files;
            cx.notify();
        }
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub fn open(&mut self, path: String, bytes: &[u8], cx: &mut Context<Self>) {
        self.charset = Charset::from_file(bytes);
        self.saved = self.charset.clone();
        self.path = Some(path);
        self.undo.clear();
        self.redo.clear();
        cx.emit(CharsEvent::Changed);
        cx.notify();
    }

    pub fn is_dirty(&self) -> bool {
        self.path.is_some() && self.charset != self.saved
    }

    /// The file to write when it has changes: its path and bytes.
    /// The 2048 bytes of the charset as they are now.
    pub fn bytes(&self) -> &[u8] {
        &self.charset.bytes
    }

    pub fn unsaved(&self) -> Option<(String, Vec<u8>)> {
        self.is_dirty().then(|| (self.path.clone().unwrap(), self.charset.to_file()))
    }

    pub fn mark_saved(&mut self, cx: &mut Context<Self>) {
        self.saved = self.charset.clone();
        cx.emit(CharsEvent::Changed);
        cx.notify();
    }

    /// A change of the charset, undone with Undo.
    fn change(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Charset, u8, Mode)) {
        self.undo.push(self.charset.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        f(&mut self.charset, self.code, self.mode);
        cx.emit(CharsEvent::Changed);
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.charset, c));
            cx.emit(CharsEvent::Changed);
            cx.notify();
        }
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.charset, c));
            cx.emit(CharsEvent::Changed);
            cx.notify();
        }
    }

    /// The color of a pen of a character.
    fn pen_color(&self, pen: u8) -> Hsla {
        match (self.mode, pen) {
            (Mode::Hires, 0) | (Mode::Multicolor, 0) => color(self.colors[0]),
            (Mode::Multicolor, 1) => color(self.colors[1]),
            (Mode::Multicolor, 2) => color(self.colors[2]),
            _ => color(self.colors[3]),
        }
    }

    fn paint_at(&mut self, position: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let Some(pen) = self.stroke else { return };
        let w = self.mode.width();
        if let Some((x, y)) = PixelCanvas::pixel_at(&self.editor_bounds, w, 8, position)
            && self.charset.pixel(self.code, self.mode, x, y) != pen
        {
            if let Some(before) = self.stroke_undo.take() {
                self.undo.push(before);
                self.redo.clear();
            }
            let code = self.code;
            let mode = self.mode;
            self.charset.set_pixel(code, mode, x, y, pen);
            cx.emit(CharsEvent::Changed);
            cx.notify();
        }
    }

    fn on_editor_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let pen = if e.button == MouseButton::Right { 0 } else if self.mode == Mode::Hires { 1 } else { self.pen };
        // one step of undo a stroke
        self.stroke_undo = Some(self.charset.clone());
        self.stroke = Some(pen);
        self.paint_at(e.position, cx);
    }

    fn on_editor_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if e.pressed_button.is_none() {
            self.stroke = None;
            return;
        }
        self.paint_at(e.position, cx);
    }

    fn on_grid_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some((x, y)) = PixelCanvas::pixel_at(&self.grid_bounds, 128, 128, e.position) {
            self.code = ((y / 8) * 16 + x / 8) as u8;
            cx.notify();
        }
    }

    /// The character in work, a cell of `scale` pixels.
    fn editor_canvas(&self, theme: &Theme) -> PixelCanvas {
        let w = self.mode.width();
        let cell = 40.;
        let mut canvas = PixelCanvas::new(w, 8, cell * (8 / w) as f32, cell, color(self.colors[0]));
        for y in 0..8 {
            for x in 0..w {
                let pen = self.charset.pixel(self.code, self.mode, x, y);
                if pen != 0 {
                    canvas.colors[y * w + x] = Some(self.pen_color(pen));
                }
            }
        }
        let mut thin = theme.outline;
        thin.a = 0.5;
        canvas.grid = Some((thin.into(), thin.into(), 0, 0));
        canvas.bounds = self.editor_bounds.clone();
        canvas
    }

    /// All 256 characters, 16 a row.
    fn grid_canvas(&self, theme: &Theme) -> PixelCanvas {
        let w = self.mode.width();
        let scale = 3.;
        let mut canvas = PixelCanvas::new(128, 128, scale, scale, color(self.colors[0]));
        for code in 0..=255u8 {
            let (cx0, cy0) = ((code as usize % 16) * 8, (code as usize / 16) * 8);
            for y in 0..8 {
                for x in 0..w {
                    let pen = self.charset.pixel(code, self.mode, x, y);
                    if pen != 0 {
                        let c = Some(self.pen_color(pen));
                        for k in 0..8 / w {
                            canvas.colors[(cy0 + y) * 128 + cx0 + x * (8 / w) + k] = c;
                        }
                    }
                }
            }
        }
        let mut line = theme.outline_variant;
        line.a = 0.6;
        canvas.grid = Some((gpui::transparent_black(), line.into(), 8, 8));
        canvas.frames.push(((self.code as usize % 16) * 8, (self.code as usize / 16) * 8, 8, 8, theme.caret.into()));
        canvas.bounds = self.grid_bounds.clone();
        canvas
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

impl Render for CharsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let file_bar = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(div().text_xs().text_color(theme.on_surface_variant).pr_1().child("CHARSET"))
            .children(self.files.iter().cloned().map(|path| {
                let active = self.path.as_deref() == Some(path.as_str());
                let label = if active && self.is_dirty() { format!("{path} ●") } else { path.clone() };
                chip(SharedString::from(format!("chr-{path}")), label, active, theme)
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(CharsEvent::OpenFile(path.clone()))))
            }))
            .when(self.files.is_empty(), |el| {
                el.child(div().text_sm().text_color(theme.on_surface_variant).child("No charsets (.chr) in the project."))
            });
        if self.path.is_none() {
            return div().size_full().p_4().flex().flex_col().gap_3().child(file_bar).child(
                div().text_color(theme.on_surface_variant).child("Choose a charset."),
            );
        }

        let mode = self.mode;
        let tools = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(chip("chr-hires", "Hires", mode == Mode::Hires, theme).on_click(cx.listener(|this, _, _, cx| {
                this.mode = Mode::Hires;
                cx.notify();
            })))
            .child(chip("chr-mc", "Multicolor", mode == Mode::Multicolor, theme).on_click(cx.listener(|this, _, _, cx| {
                this.mode = Mode::Multicolor;
                if this.pen == 0 {
                    this.pen = 3;
                }
                cx.notify();
            })))
            .child(div().w(px(10.)))
            .child(chip("chr-clear", "Clear", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |c, code, _| c.clear(code)))))
            .child(chip("chr-invert", "Invert", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |c, code, _| c.invert(code)))))
            .child(chip("chr-flipx", "Flip ↔", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |c, code, m| c.flip_x(code, m)))))
            .child(chip("chr-flipy", "Flip ↕", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |c, code, _| c.flip_y(code)))))
            .when(mode == Mode::Hires, |el| {
                el.child(chip("chr-rotate", "Rotate ↻", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |c, code, _| c.rotate(code)))))
            })
            .child(chip("chr-left", "←", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |c, code, m| c.shift(code, m, -1, 0)))))
            .child(chip("chr-right", "→", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |c, code, m| c.shift(code, m, 1, 0)))))
            .child(chip("chr-up", "↑", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |c, code, m| c.shift(code, m, 0, -1)))))
            .child(chip("chr-down", "↓", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |c, code, m| c.shift(code, m, 0, 1)))))
            .child(div().w(px(10.)))
            .child(chip("chr-copy", "Copy", false, theme).on_click(cx.listener(|this, _, _, cx| {
                this.clip = Some(this.charset.glyph(this.code).try_into().unwrap());
                cx.notify();
            })))
            .child(chip("chr-paste", "Paste", false, theme).when(self.clip.is_none(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| {
                if let Some(clip) = this.clip {
                    this.change(cx, |c, code, _| c.glyph_mut(code).copy_from_slice(&clip));
                }
            })))
            .child(div().w(px(10.)))
            .child(chip("chr-undo", "Undo", false, theme).when(self.undo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.undo(cx))))
            .child(chip("chr-redo", "Redo", false, theme).when(self.redo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.redo(cx))));

        // the pens and the colors of the slots
        let slots: Vec<(Slot, &str, usize)> = match mode {
            Mode::Hires => vec![(Slot::Background, "Background", 0), (Slot::Cell, "Color", 3)],
            Mode::Multicolor => vec![
                (Slot::Background, "00 background", 0),
                (Slot::Multi1, "01 $d022", 1),
                (Slot::Multi2, "10 $d023", 2),
                (Slot::Cell, "11 cell color", 3),
            ],
        };
        let pens = div().flex().flex_col().gap_1().children(slots.into_iter().map(|(slot, label, ix)| {
            let pen = ix as u8;
            let drawing = mode == Mode::Multicolor && self.pen == pen;
            div()
                .id(SharedString::from(format!("chr-slot-{ix}")))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .border_1()
                .border_color(if self.slot == slot { theme.caret } else { theme.outline_variant })
                .when(drawing, |el| el.bg(theme.container_highest))
                .child(div().w(px(18.)).h(px(18.)).rounded_sm().border_1().border_color(theme.outline).bg(color(self.colors[ix])))
                .child(div().text_xs().child(format!("{label}: {}", COLOR_NAMES[self.colors[ix] as usize])))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.slot = slot;
                    if this.mode == Mode::Multicolor {
                        this.pen = pen;
                    }
                    cx.notify();
                }))
        }));
        let palette = div().flex().flex_wrap().w(px(8. * 26.)).gap_0p5().children((0..16u8).map(|i| {
            div()
                .id(("chr-color", i as usize))
                .w(px(24.))
                .h(px(24.))
                .rounded_sm()
                .border_1()
                .border_color(theme.outline)
                .cursor_pointer()
                .bg(color(i))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let ix = match this.slot {
                        Slot::Background => 0,
                        Slot::Multi1 => 1,
                        Slot::Multi2 => 2,
                        Slot::Cell => 3,
                    };
                    this.colors[ix] = i;
                    cx.notify();
                }))
        }));

        let code = self.code;
        let rows = self.charset.glyph(code).iter().map(|b| format!("{b:08b}  ${b:02x}")).collect::<Vec<_>>();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(file_bar)
            .child(tools)
            .child(
                div()
                    .flex()
                    .gap_6()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("Char {code} (${code:02x})")))
                            .child(
                                div()
                                    .cursor_crosshair()
                                    .child(self.editor_canvas(&theme))
                                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_editor_down))
                                    .on_mouse_down(MouseButton::Right, cx.listener(Self::on_editor_down))
                                    .on_mouse_move(cx.listener(Self::on_editor_move))
                                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.stroke = None))
                                    .on_mouse_up(MouseButton::Right, cx.listener(|this, _, _, _| this.stroke = None)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.on_surface_variant)
                                    .child("Left button: draw · right button: erase"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(pens)
                            .child(palette)
                            .child(
                                div()
                                    .pt_2()
                                    .font_family(mono_font())
                                    .text_size(px(13.))
                                    .text_color(theme.on_surface_variant)
                                    .children(rows),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_xs().text_color(theme.on_surface_variant).child("THE CHARSET"))
                            .child(
                                div()
                                    .cursor_pointer()
                                    .child(self.grid_canvas(&theme))
                                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_grid_down)),
                            ),
                    ),
            )
    }
}
