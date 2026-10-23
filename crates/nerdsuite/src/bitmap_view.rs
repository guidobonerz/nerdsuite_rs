//! The Bitmap view, after nerdsuite2's `bitmap.js`: a picture of the project - Koala (.kla,
//! multicolor) or Art Studio (.art, hires) - with the color rules of the VIC-II's bitmap modes. The
//! left button paints with the color, the right one with the background; in multicolor a color takes
//! the slot of its cell that holds it, a free one or the one it paints over, and a fourth color over
//! the background is a clash, counted. Undo. Save (Ctrl+S) writes the file.

use gpui::{Context, EventEmitter, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, SharedString, Window, div, prelude::*, px, rgb};
use nerdsuite_core::gfx::{Bitmap, COLOR_NAMES, PALETTE};

use crate::{
    pixels::PixelCanvas,
    theme::{Theme, theme},
};

pub enum BitmapEvent {
    OpenFile(String),
    Changed,
}

impl EventEmitter<BitmapEvent> for BitmapView {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Color,
    Background,
}

pub struct BitmapView {
    files: Vec<String>,
    path: Option<String>,
    picture: Option<Bitmap>,
    saved: Option<Bitmap>,
    /// the color of the left button, and of the right one in hires
    pen: u8,
    background: u8,
    slot: Slot,
    undo: Vec<Bitmap>,
    redo: Vec<Bitmap>,
    stroke: Option<bool>,
    stroke_undo: Option<Bitmap>,
    /// the pixels a stroke could not paint (a color clash)
    clashes: usize,
    zoom: f32,
    bounds: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    error: Option<String>,
}

fn color(index: u8) -> Hsla {
    rgb(PALETTE[index as usize & 15]).into()
}

impl BitmapView {
    pub fn new() -> Self {
        BitmapView {
            files: vec![],
            path: None,
            picture: None,
            saved: None,
            pen: 1,
            background: 0,
            slot: Slot::Color,
            undo: vec![],
            redo: vec![],
            stroke: None,
            stroke_undo: None,
            clashes: 0,
            zoom: 3.,
            bounds: Default::default(),
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

    pub fn open(&mut self, path: String, bytes: &[u8], cx: &mut Context<Self>) {
        match Bitmap::from_file(bytes) {
            Ok(b) => {
                self.background = if b.multicolor { b.bg & 15 } else { 0 };
                self.saved = Some(b.clone());
                self.picture = Some(b);
                self.error = None;
            }
            Err(e) => {
                self.picture = None;
                self.saved = None;
                self.error = Some(format!("{path}: {e}"));
            }
        }
        self.path = Some(path);
        self.undo.clear();
        self.redo.clear();
        self.clashes = 0;
        cx.emit(BitmapEvent::Changed);
        cx.notify();
    }

    pub fn is_dirty(&self) -> bool {
        self.picture.is_some() && self.picture != self.saved
    }

    pub fn unsaved(&self) -> Option<(String, Vec<u8>)> {
        self.is_dirty().then(|| (self.path.clone().unwrap(), self.picture.as_ref().unwrap().to_file()))
    }

    pub fn mark_saved(&mut self, cx: &mut Context<Self>) {
        self.saved = self.picture.clone();
        cx.emit(BitmapEvent::Changed);
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        if let (Some(prev), Some(p)) = (self.undo.pop(), self.picture.as_mut()) {
            self.redo.push(std::mem::replace(p, prev));
            cx.emit(BitmapEvent::Changed);
            cx.notify();
        }
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        if let (Some(next), Some(p)) = (self.redo.pop(), self.picture.as_mut()) {
            self.undo.push(std::mem::replace(p, next));
            cx.emit(BitmapEvent::Changed);
            cx.notify();
        }
    }

    fn paint_at(&mut self, position: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let (Some(foreground), Some(p)) = (self.stroke, self.picture.as_mut()) else { return };
        let (w, h) = p.size();
        let Some((x, y)) = PixelCanvas::pixel_at(&self.bounds, w, h, position) else { return };
        let color = if foreground { self.pen } else { self.background };
        let before = (p.bitmap.clone(), p.screen.clone(), p.color.clone(), p.bg);
        let painted = p.paint(x, y, color, foreground);
        let changed = before != (p.bitmap.clone(), p.screen.clone(), p.color.clone(), p.bg);
        if !painted {
            self.clashes += 1;
        }
        if changed {
            if let Some(b) = self.stroke_undo.take() {
                self.undo.push(b);
                if self.undo.len() > 60 {
                    self.undo.remove(0);
                }
                self.redo.clear();
            }
            cx.emit(BitmapEvent::Changed);
        }
        cx.notify();
    }

    fn on_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.picture.as_ref() else { return };
        self.stroke_undo = Some(p.clone());
        self.stroke = Some(e.button == MouseButton::Left);
        self.clashes = 0;
        self.paint_at(e.position, cx);
    }

    fn on_move(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if e.pressed_button.is_none() {
            self.stroke = None;
            return;
        }
        self.paint_at(e.position, cx);
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

impl Render for BitmapView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let dirty = self.is_dirty();
        let file_bar = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(div().text_xs().text_color(theme.on_surface_variant).pr_1().child("PICTURE"))
            .children(self.files.iter().cloned().map(|path| {
                let active = self.path.as_deref() == Some(path.as_str());
                let label = if active && dirty { format!("{path} ●") } else { path.clone() };
                chip(SharedString::from(format!("bmp-{path}")), label, active, theme)
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(BitmapEvent::OpenFile(path.clone()))))
            }))
            .when(self.files.is_empty(), |el| el.child(div().text_sm().text_color(theme.on_surface_variant).child("No pictures (.kla, .art) in the project.")));
        let Some(p) = self.picture.as_ref() else {
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
                        .child(self.error.clone().unwrap_or_else(|| "Choose a picture.".into())),
                )
                .into_any_element();
        };

        let (w, h) = p.size();
        let zoom = self.zoom;
        let mut canvas = PixelCanvas::new(w, h, if p.multicolor { zoom * 2. } else { zoom }, zoom, gpui::transparent_black());
        for y in 0..h {
            for x in 0..w {
                canvas.colors[y * w + x] = Some(color(p.color_at(x, y)));
            }
        }
        canvas.bounds = self.bounds.clone();

        let tools = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .children([2., 3., 4.].map(|z| {
                chip(SharedString::from(format!("bmp-zoom-{z}")), format!("{z}x"), self.zoom == z, theme).on_click(cx.listener(move |this, _, _, cx| {
                    this.zoom = z;
                    cx.notify();
                }))
            }))
            .child(div().w(px(10.)))
            .child(chip("bmp-undo", "Undo", false, theme).when(self.undo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.undo(cx))))
            .child(chip("bmp-redo", "Redo", false, theme).when(self.redo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.redo(cx))));

        let bg_label = if p.multicolor { "background (the picture's)" } else { "background (right button)" };
        let slots = [(Slot::Color, "color (left button)", self.pen), (Slot::Background, bg_label, self.background)];
        let pens = div().flex().gap_1().children(slots.into_iter().map(|(slot, label, value)| {
            div()
                .id(SharedString::from(format!("bmp-slot-{label}")))
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
                .child(div().text_xs().child(format!("{label}: {}", COLOR_NAMES[value as usize & 15])))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.slot = slot;
                    cx.notify();
                }))
        }));
        let palette = div().flex().gap_0p5().children((0..16u8).map(|i| {
            div()
                .id(("bmp-color", i as usize))
                .w(px(24.))
                .h(px(24.))
                .rounded_sm()
                .border_1()
                .border_color(theme.outline)
                .cursor_pointer()
                .bg(color(i))
                .on_click(cx.listener(move |this, _, _, cx| {
                    match this.slot {
                        Slot::Color => this.pen = i,
                        Slot::Background => {
                            this.background = i;
                            // multicolor: the background is the register of the whole picture
                            if let Some(p) = this.picture.as_mut()
                                && p.multicolor
                                && p.bg != i
                            {
                                this.undo.push(p.clone());
                                this.redo.clear();
                                p.bg = i;
                                cx.emit(BitmapEvent::Changed);
                            }
                        }
                    }
                    cx.notify();
                }))
        }));

        let info = format!(
            "{} · {} x {} pixels · load address ${:02x}{:02x}{}",
            if p.multicolor { "Koala, multicolor" } else { "Art Studio, hires" },
            w,
            h,
            p.load_address[1],
            p.load_address[0],
            if self.clashes > 0 { format!(" · {} pixels not painted: a color clash (three other colors in the cell)", self.clashes) } else { String::new() }
        );

        div()
            .id("bitmap")
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .overflow_scroll()
            .child(file_bar)
            .child(div().text_xs().text_color(if self.clashes > 0 { theme.warning } else { theme.on_surface_variant }).child(info))
            .child(tools)
            .child(div().flex().flex_wrap().items_center().gap_4().child(pens).child(palette))
            .child(
                div()
                    .flex()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .cursor_crosshair()
                                    .child(canvas)
                                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
                                    .on_mouse_down(MouseButton::Right, cx.listener(Self::on_down))
                                    .on_mouse_move(cx.listener(Self::on_move))
                                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.stroke = None))
                                    .on_mouse_up(MouseButton::Right, cx.listener(|this, _, _, _| this.stroke = None)),
                            )
                            .child(div().text_xs().text_color(theme.on_surface_variant).child("Left button: the color · right button: the background")),
                    ),
            )
            .into_any_element()
    }
}
