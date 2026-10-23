//! The Screen view, after nerdsuite2's `screen.js`: a screen of the project (a `.scr`: w x h
//! characters with their colors, its patches and its look) as the VIC-II shows it, in its charset
//! (a .chr of the project, else the C64's ROM font read from VICE or mimic). The left button puts the
//! character and color chosen, the right one picks them up; a patch (a door open, closed …) is
//! edited in its place over the screen. Fill, clear, the background and border colors; undo. Save
//! (Ctrl+S) writes the file.

use gpui::{Context, EventEmitter, FontWeight, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, SharedString, Window, div, prelude::*, px, rgb};
use nerdsuite_core::gfx::{COLOR_NAMES, CharMap, Look, PALETTE, Screen};

use crate::{
    pixels::PixelCanvas,
    theme::{Theme, theme},
};

pub enum ScreenEvent {
    OpenFile(String),
    Changed,
}

impl EventEmitter<ScreenEvent> for ScreenView {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Char,
    Background,
    Border,
}

pub struct ScreenView {
    files: Vec<String>,
    path: Option<String>,
    screen: Option<Screen>,
    saved: Option<Screen>,
    /// the charset the screen is shown in (2048 bytes), and where it came from
    charset: Vec<u8>,
    charset_name: String,
    /// the map in work: the screen, or a patch
    layer: Option<usize>,
    code: u8,
    color: u8,
    slot: Slot,
    grid: bool,
    undo: Vec<Screen>,
    redo: Vec<Screen>,
    /// the Screen before the stroke in work: an undo step once the stroke changes something
    stroke_undo: Option<Screen>,
    /// a stroke of the left button
    drawing: bool,
    canvas_bounds: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    picker_bounds: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    error: Option<String>,
}

fn color(index: u8) -> Hsla {
    rgb(PALETTE[index as usize & 15]).into()
}

impl ScreenView {
    pub fn new() -> Self {
        ScreenView {
            files: vec![],
            path: None,
            screen: None,
            saved: None,
            charset: vec![0; 2048],
            charset_name: String::new(),
            layer: None,
            code: 0xa0,
            color: 1,
            slot: Slot::Char,
            grid: false,
            undo: vec![],
            redo: vec![],
            stroke_undo: None,
            drawing: false,
            canvas_bounds: Default::default(),
            picker_bounds: Default::default(),
            error: None,
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

    /// The charset file the screen names (empty: the ROM font).
    pub fn charset_path(&self) -> Option<String> {
        self.screen.as_ref().map(|s| s.look.as_ref().map(|l| l.charset.clone()).unwrap_or_default())
    }

    /// The charset to show the screen in; the same again changes nothing.
    pub fn set_charset(&mut self, bytes: &[u8], name: &str, cx: &mut Context<Self>) {
        if self.charset != bytes || self.charset_name != name {
            self.charset = bytes.to_vec();
            self.charset.resize(2048, 0);
            self.charset_name = name.to_owned();
            cx.notify();
        }
    }

    pub fn open(&mut self, path: String, bytes: &[u8], cx: &mut Context<Self>) {
        match Screen::decode(bytes) {
            Ok(s) => {
                self.saved = Some(s.clone());
                self.screen = Some(s);
                self.error = None;
            }
            Err(e) => {
                self.screen = None;
                self.saved = None;
                self.error = Some(format!("{path}: {e}"));
            }
        }
        self.path = Some(path);
        self.layer = None;
        self.undo.clear();
        self.redo.clear();
        cx.emit(ScreenEvent::Changed);
        cx.notify();
    }

    pub fn is_dirty(&self) -> bool {
        self.screen.is_some() && self.screen != self.saved
    }

    pub fn unsaved(&self) -> Option<(String, Vec<u8>)> {
        self.is_dirty().then(|| (self.path.clone().unwrap(), self.screen.as_ref().unwrap().encode()))
    }

    pub fn mark_saved(&mut self, cx: &mut Context<Self>) {
        self.saved = self.screen.clone();
        cx.emit(ScreenEvent::Changed);
        cx.notify();
    }

    fn change(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Screen, Option<usize>)) {
        let Some(s) = self.screen.as_mut() else { return };
        self.undo.push(s.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        f(s, self.layer);
        cx.emit(ScreenEvent::Changed);
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        if let (Some(prev), Some(s)) = (self.undo.pop(), self.screen.as_mut()) {
            self.redo.push(std::mem::replace(s, prev));
            cx.emit(ScreenEvent::Changed);
            cx.notify();
        }
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        if let (Some(next), Some(s)) = (self.redo.pop(), self.screen.as_mut()) {
            self.undo.push(std::mem::replace(s, next));
            cx.emit(ScreenEvent::Changed);
            cx.notify();
        }
    }

    /// The map in work and where it lies on the screen.
    fn map_mut(s: &mut Screen, layer: Option<usize>) -> (&mut CharMap, usize, usize) {
        match layer {
            Some(p) => {
                let name = s.patches[p].0.clone();
                let (col, row) = s.patch_at(&name);
                (&mut s.patches[p].1, col, row)
            }
            None => (&mut s.map, 0, 0),
        }
    }

    /// The cell of the map in work under the mouse.
    fn cell_at(&self, position: gpui::Point<gpui::Pixels>) -> Option<(usize, usize)> {
        let s = self.screen.as_ref()?;
        let (px, py) = PixelCanvas::pixel_at(&self.canvas_bounds, s.map.w * 8, s.map.h * 8, position)?;
        Some((px / 8, py / 8))
    }

    fn put_at(&mut self, position: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let Some((cx0, cy0)) = self.cell_at(position) else { return };
        let (code, col, layer) = (self.code, self.color, self.layer);
        let Some(s) = self.screen.as_mut() else { return };
        let (map, ox, oy) = Self::map_mut(s, layer);
        let (Some(x), Some(y)) = (cx0.checked_sub(ox), cy0.checked_sub(oy)) else { return };
        if x >= map.w || y >= map.h {
            return;
        }
        let ix = y * map.w + x;
        let same = map.chars[ix] == code && map.colors.as_ref().is_none_or(|c| c[ix] == col);
        if !same {
            if let Some(before) = self.stroke_undo.take() {
                self.undo.push(before);
                self.redo.clear();
            }
            map.chars[ix] = code;
            if let Some(c) = map.colors.as_mut() {
                c[ix] = col;
            }
            cx.emit(ScreenEvent::Changed);
            cx.notify();
        }
    }

    fn on_canvas_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if e.button == MouseButton::Right {
            // pick up the character and the color of a cell
            let layer = self.layer;
            if let (Some((x, y)), Some(s)) = (self.cell_at(e.position), self.screen.as_mut()) {
                let text = s.look.as_ref().map_or(1, |l| l.text);
                let (map, ox, oy) = Self::map_mut(s, layer);
                if let (Some(x), Some(y)) = (x.checked_sub(ox), y.checked_sub(oy))
                    && x < map.w
                    && y < map.h
                {
                    self.code = map.chars[y * map.w + x];
                    self.color = map.colors.as_ref().map_or(text, |c| c[y * map.w + x]);
                    cx.notify();
                }
            }
            return;
        }
        let Some(s) = self.screen.as_ref() else { return };
        self.stroke_undo = Some(s.clone());
        self.drawing = true;
        self.put_at(e.position, cx);
    }

    fn on_canvas_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.drawing || e.pressed_button != Some(MouseButton::Left) {
            self.drawing = false;
            return;
        }
        self.put_at(e.position, cx);
    }

    fn on_picker_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some((x, y)) = PixelCanvas::pixel_at(&self.picker_bounds, 128, 128, e.position) {
            self.code = ((y / 8) * 16 + x / 8) as u8;
            cx.notify();
        }
    }

    /// The 256 characters of the charset, 16 a row, in the color chosen.
    fn picker(&self, theme: &Theme, bg: u8) -> PixelCanvas {
        let mut canvas = PixelCanvas::new(128, 128, 2., 2., color(bg));
        for code in 0..256usize {
            let (x0, y0) = ((code % 16) * 8, (code / 16) * 8);
            for y in 0..8 {
                let row = self.charset[code * 8 + y];
                for x in 0..8 {
                    if row & (0x80 >> x) != 0 {
                        canvas.colors[(y0 + y) * 128 + x0 + x] = Some(color(self.color));
                    }
                }
            }
        }
        let mut line = theme.outline_variant;
        line.a = 0.5;
        canvas.grid = Some((gpui::transparent_black(), line.into(), 8, 8));
        canvas.frames.push(((self.code as usize % 16) * 8, (self.code as usize / 16) * 8, 8, 8, theme.caret.into()));
        canvas.bounds = self.picker_bounds.clone();
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

impl Render for ScreenView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let dirty = self.is_dirty();
        let file_bar = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(div().text_xs().text_color(theme.on_surface_variant).pr_1().child("SCREEN"))
            .children(self.files.iter().cloned().map(|path| {
                let active = self.path.as_deref() == Some(path.as_str());
                let label = if active && dirty { format!("{path} ●") } else { path.clone() };
                chip(SharedString::from(format!("scr-{path}")), label, active, theme)
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(ScreenEvent::OpenFile(path.clone()))))
            }))
            .when(self.files.is_empty(), |el| el.child(div().text_sm().text_color(theme.on_surface_variant).child("No screens (.scr) in the project.")));
        let Some(s) = self.screen.as_ref() else {
            return div()
                .size_full()
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(file_bar)
                .child(
                    div()
                        .text_color(if self.error.is_some() { theme.error } else { theme.on_surface_variant })
                        .child(self.error.clone().unwrap_or_else(|| "Choose a screen.".into())),
                )
                .into_any_element();
        };
        let look = s.look.clone().unwrap_or(Look { text: 1, ..Default::default() });

        // the screen as the machine shows it, in a border
        let (w, h) = (s.map.w, s.map.h);
        let scale = (900. / (w * 8) as f32).min(3.).floor().max(1.);
        let pixels = s.render(&self.charset, self.layer);
        let mut canvas = PixelCanvas::new(w * 8, h * 8, scale, scale, color(look.bg));
        for (ix, &c) in pixels.iter().enumerate() {
            if c != look.bg & 15 {
                canvas.colors[ix] = Some(color(c));
            }
        }
        if self.grid {
            let mut line = theme.outline;
            line.a = 0.35;
            canvas.grid = Some((gpui::transparent_black(), line.into(), 8, 8));
        }
        if let Some(p) = self.layer {
            let (name, m) = &s.patches[p];
            let (col, row) = s.patch_at(name);
            canvas.frames.push((col * 8, row * 8, m.w * 8, m.h * 8, theme.caret.into()));
        }
        canvas.bounds = self.canvas_bounds.clone();

        let layers = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(div().text_xs().text_color(theme.on_surface_variant).pr_1().child("EDIT"))
            .child(chip("scr-layer-main", "Screen", self.layer.is_none(), theme).on_click(cx.listener(|this, _, _, cx| {
                this.layer = None;
                cx.notify();
            })))
            .children(s.patches.iter().enumerate().map(|(ix, (name, _))| {
                chip(("scr-layer", ix), name.clone(), self.layer == Some(ix), theme).on_click(cx.listener(move |this, _, _, cx| {
                    this.layer = Some(ix);
                    cx.notify();
                }))
            }));
        let tools = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(chip("scr-fill", "Fill", false, theme).on_click(cx.listener(|this, _, _, cx| {
                let (code, col) = (this.code, this.color);
                this.change(cx, |s, layer| {
                    let (m, _, _) = ScreenView::map_mut(s, layer);
                    m.chars.fill(code);
                    if let Some(c) = m.colors.as_mut() {
                        c.fill(col);
                    }
                });
            })))
            .child(chip("scr-clear", "Clear", false, theme).on_click(cx.listener(|this, _, _, cx| {
                let col = this.color;
                this.change(cx, |s, layer| {
                    let (m, _, _) = ScreenView::map_mut(s, layer);
                    m.chars.fill(0x20);
                    if let Some(c) = m.colors.as_mut() {
                        c.fill(col);
                    }
                });
            })))
            .child(chip("scr-grid", "Grid", self.grid, theme).on_click(cx.listener(|this, _, _, cx| {
                this.grid = !this.grid;
                cx.notify();
            })))
            .child(div().w(px(10.)))
            .child(chip("scr-undo", "Undo", false, theme).when(self.undo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.undo(cx))))
            .child(chip("scr-redo", "Redo", false, theme).when(self.redo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.redo(cx))));

        let slots = [(Slot::Char, "char color", self.color), (Slot::Background, "background", look.bg), (Slot::Border, "border", look.border)];
        let pens = div().flex().flex_col().gap_1().children(slots.into_iter().map(|(slot, label, value)| {
            div()
                .id(SharedString::from(format!("scr-slot-{label}")))
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .border_1()
                .border_color(if self.slot == slot { theme.caret } else { theme.outline_variant })
                .child(div().w(px(18.)).h(px(18.)).rounded_sm().border_1().border_color(theme.outline).bg(color(value)))
                .child(div().text_xs().child(format!("{label}: {}{}", COLOR_NAMES[value as usize & 15], if slot == Slot::Char && value & 8 != 0 && look.multicolor.is_some() { " (multicolor)" } else { "" })))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.slot = slot;
                    cx.notify();
                }))
        }));
        let palette = div().flex().flex_wrap().w(px(8. * 26.)).gap_0p5().children((0..16u8).map(|i| {
            div()
                .id(("scr-color", i as usize))
                .w(px(24.))
                .h(px(24.))
                .rounded_sm()
                .border_1()
                .border_color(theme.outline)
                .cursor_pointer()
                .bg(color(i))
                .on_click(cx.listener(move |this, _, _, cx| match this.slot {
                    Slot::Char => {
                        this.color = i;
                        cx.notify();
                    }
                    Slot::Background => this.change(cx, |s, _| s.look.get_or_insert_with(|| Look { text: 1, ..Default::default() }).bg = i),
                    Slot::Border => this.change(cx, |s, _| s.look.get_or_insert_with(|| Look { text: 1, ..Default::default() }).border = i),
                }))
        }));

        let mode = match (&look.multicolor, &look.ecm) {
            (Some(_), _) => "multicolor",
            (_, Some(_)) => "extended color mode",
            _ => "hires",
        };
        let info = format!(
            "{w} x {h} characters · {mode} · charset: {} · {} patches",
            if self.charset_name.is_empty() { "the ROM font" } else { &self.charset_name },
            s.patches.len()
        );
        let code = self.code;

        div()
            .id("screen")
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .overflow_y_scroll()
            .child(file_bar)
            .child(div().text_xs().text_color(theme.on_surface_variant).child(info))
            .child(layers)
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
                            .child(
                                div()
                                    .p(px(16.))
                                    .bg(color(look.border))
                                    .cursor_crosshair()
                                    .child(canvas)
                                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_canvas_down))
                                    .on_mouse_down(MouseButton::Right, cx.listener(Self::on_canvas_down))
                                    .on_mouse_move(cx.listener(Self::on_canvas_move))
                                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.drawing = false)),
                            )
                            .child(div().text_xs().text_color(theme.on_surface_variant).child("Left button: put the character · right button: pick it up")),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("Char {code} (${code:02x})")))
                            .child(div().cursor_pointer().child(self.picker(&theme, look.bg)).on_mouse_down(MouseButton::Left, cx.listener(Self::on_picker_down)))
                            .child(pens)
                            .child(palette),
                    ),
            )
            .into_any_element()
    }
}
