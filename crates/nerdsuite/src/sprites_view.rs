//! The Sprites view, after nerdsuite2's `sprites.js`: a costume of the project (a `.spr`: frames of
//! w x h sprites of 24 x 21 pixels), the frame in work large - the left button draws, the right one
//! erases - and all frames small to pick from. Hires, or multicolor (a bit pair a pixel: clear,
//! $d025, the sprite color, $d026); new, duplicate and delete frames; clear, flip, shift; undo. The
//! layers of a costume are kept as they are. Save (Ctrl+S) writes the file.

use gpui::{Context, EventEmitter, FontWeight, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, SharedString, Window, div, prelude::*, px, rgb};
use nerdsuite_core::gfx::{COLOR_NAMES, PALETTE, SPRITE_BLOCK, SpriteFrame, Sprites};

use crate::{
    pixels::PixelCanvas,
    theme::{Theme, theme},
};

pub enum SpritesEvent {
    OpenFile(String),
    Changed,
}

impl EventEmitter<SpritesEvent> for SpritesView {}

/// The colors the pens are shown in: the background of the editor, $d025, the sprite color, $d026.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Background,
    Multi1,
    Sprite,
    Multi2,
}

pub struct SpritesView {
    files: Vec<String>,
    path: Option<String>,
    sprites: Option<Sprites>,
    saved: Option<Sprites>,
    frame: usize,
    /// the pen of the left button (multicolor: 1-3)
    pen: u8,
    /// the background, $d025, the sprite color, $d026
    colors: [u8; 4],
    slot: Slot,
    undo: Vec<Sprites>,
    redo: Vec<Sprites>,
    /// the Sprites before the stroke in work: an undo step once the stroke changes something
    stroke_undo: Option<Sprites>,
    stroke: Option<u8>,
    editor_bounds: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    error: Option<String>,
}

fn color(index: u8) -> Hsla {
    rgb(PALETTE[index as usize & 15]).into()
}

impl SpritesView {
    pub fn new() -> Self {
        SpritesView {
            files: vec![],
            path: None,
            sprites: None,
            saved: None,
            frame: 0,
            pen: 2,
            colors: [6, 0, 1, 1],
            slot: Slot::Sprite,
            undo: vec![],
            redo: vec![],
            stroke_undo: None,
            stroke: None,
            editor_bounds: Default::default(),
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
        match Sprites::decode(bytes) {
            Ok(s) => {
                // the sprite color: the color of the first part, as the file has it
                if let Some(&c) = s.colors.first() {
                    self.colors[2] = c & 15;
                }
                self.saved = Some(s.clone());
                self.sprites = Some(s);
                self.error = None;
            }
            Err(e) => {
                self.sprites = None;
                self.saved = None;
                self.error = Some(format!("{path}: {e}"));
            }
        }
        self.path = Some(path);
        self.frame = 0;
        self.undo.clear();
        self.redo.clear();
        cx.emit(SpritesEvent::Changed);
        cx.notify();
    }

    pub fn is_dirty(&self) -> bool {
        self.sprites.is_some() && self.sprites != self.saved
    }

    pub fn unsaved(&self) -> Option<(String, Vec<u8>)> {
        self.is_dirty().then(|| (self.path.clone().unwrap(), self.sprites.as_ref().unwrap().encode()))
    }

    pub fn mark_saved(&mut self, cx: &mut Context<Self>) {
        self.saved = self.sprites.clone();
        cx.emit(SpritesEvent::Changed);
        cx.notify();
    }

    fn change(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Sprites, usize)) {
        let Some(s) = self.sprites.as_mut() else { return };
        self.undo.push(s.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        f(s, self.frame);
        self.frame = self.frame.min(s.frames.len().saturating_sub(1));
        cx.emit(SpritesEvent::Changed);
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        if let (Some(prev), Some(s)) = (self.undo.pop(), self.sprites.as_mut()) {
            self.redo.push(std::mem::replace(s, prev));
            self.frame = self.frame.min(s.frames.len().saturating_sub(1));
            cx.emit(SpritesEvent::Changed);
            cx.notify();
        }
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        if let (Some(next), Some(s)) = (self.redo.pop(), self.sprites.as_mut()) {
            self.undo.push(std::mem::replace(s, next));
            self.frame = self.frame.min(s.frames.len().saturating_sub(1));
            cx.emit(SpritesEvent::Changed);
            cx.notify();
        }
    }

    /// The color of a pen: hires 1 the sprite color; multicolor 1 $d025, 2 the sprite color, 3 $d026.
    fn pen_color(&self, multicolor: bool, pen: u8) -> Hsla {
        match (multicolor, pen) {
            (false, _) | (true, 2) => color(self.colors[2]),
            (true, 1) => color(self.colors[1]),
            _ => color(self.colors[3]),
        }
    }

    /// A frame as a picture: a pixel `scale` high (a multicolor pixel twice as wide).
    fn canvas(&self, s: &Sprites, frame: usize, scale: f32) -> PixelCanvas {
        let (w, h) = s.size();
        let pw = if s.multicolor { scale * 2. } else { scale };
        let mut canvas = PixelCanvas::new(w, h, pw, scale, color(self.colors[0]));
        for y in 0..h {
            for x in 0..w {
                let pen = s.pixel(frame, x, y);
                if pen != 0 {
                    canvas.colors[y * w + x] = Some(self.pen_color(s.multicolor, pen));
                }
            }
        }
        canvas
    }

    fn paint_at(&mut self, position: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let (Some(pen), Some(s)) = (self.stroke, self.sprites.as_mut()) else { return };
        let (w, h) = s.size();
        if let Some((x, y)) = PixelCanvas::pixel_at(&self.editor_bounds, w, h, position)
            && s.pixel(self.frame, x, y) != pen
        {
            if let Some(before) = self.stroke_undo.take() {
                self.undo.push(before);
                self.redo.clear();
            }
            s.set_pixel(self.frame, x, y, pen);
            cx.emit(SpritesEvent::Changed);
            cx.notify();
        }
    }

    fn on_editor_down(&mut self, e: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = self.sprites.as_ref() else { return };
        let pen = if e.button == MouseButton::Right { 0 } else if s.multicolor { self.pen.max(1) } else { 1 };
        self.stroke_undo = Some(s.clone());
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

impl Render for SpritesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let dirty = self.is_dirty();
        let file_bar = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(div().text_xs().text_color(theme.on_surface_variant).pr_1().child("SPRITES"))
            .children(self.files.iter().cloned().map(|path| {
                let active = self.path.as_deref() == Some(path.as_str());
                let label = if active && dirty { format!("{path} ●") } else { path.clone() };
                chip(SharedString::from(format!("spr-{path}")), label, active, theme)
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(SpritesEvent::OpenFile(path.clone()))))
            }))
            .when(self.files.is_empty(), |el| el.child(div().text_sm().text_color(theme.on_surface_variant).child("No sprites (.spr) in the project.")));
        let Some(s) = self.sprites.as_ref() else {
            return div().size_full().p_4().flex().flex_col().gap_3().child(file_bar).child(
                div().text_color(if self.error.is_some() { theme.error } else { theme.on_surface_variant }).child(
                    self.error.clone().unwrap_or_else(|| "Choose a costume.".into()),
                ),
            )
            .into_any_element();
        };

        let multicolor = s.multicolor;
        let frame = self.frame;
        let (w, h) = s.size();
        let tools = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(chip("spr-new", "New frame", false, theme).on_click(cx.listener(|this, _, _, cx| {
                this.change(cx, |s, at| {
                    let name = s.free_name("frame");
                    s.frames.insert(at + 1, SpriteFrame { name, default: false, data: vec![0; s.w * s.h * SPRITE_BLOCK] });
                });
                this.frame += 1;
            })))
            .child(chip("spr-dup", "Duplicate", false, theme).on_click(cx.listener(|this, _, _, cx| {
                this.change(cx, |s, at| {
                    let mut copy = s.frames[at].clone();
                    copy.name = s.free_name(&format!("{}_", copy.name));
                    copy.default = false;
                    s.frames.insert(at + 1, copy);
                });
                this.frame += 1;
            })))
            .child(chip("spr-del", "Delete", false, theme).when(s.frames.len() < 2, |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| {
                if this.sprites.as_ref().is_some_and(|s| s.frames.len() > 1) {
                    this.change(cx, |s, at| {
                        s.frames.remove(at);
                    });
                }
            })))
            .child(div().w(px(10.)))
            .child(chip("spr-clear", "Clear", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |s, f| s.clear(f)))))
            .child(chip("spr-flipx", "Flip ↔", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |s, f| s.flip_x(f)))))
            .child(chip("spr-flipy", "Flip ↕", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |s, f| s.flip_y(f)))))
            .child(chip("spr-left", "←", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |s, f| s.shift(f, -1, 0)))))
            .child(chip("spr-right", "→", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |s, f| s.shift(f, 1, 0)))))
            .child(chip("spr-up", "↑", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |s, f| s.shift(f, 0, -1)))))
            .child(chip("spr-down", "↓", false, theme).on_click(cx.listener(|this, _, _, cx| this.change(cx, |s, f| s.shift(f, 0, 1)))))
            .child(div().w(px(10.)))
            .child(chip("spr-undo", "Undo", false, theme).when(self.undo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.undo(cx))))
            .child(chip("spr-redo", "Redo", false, theme).when(self.redo.is_empty(), |el| el.opacity(0.5)).on_click(cx.listener(|this, _, _, cx| this.redo(cx))));

        // the editor: as large as fits
        let cell = (560. / h as f32).min(560. / (w as f32 * if multicolor { 2. } else { 1. })).min(20.).floor();
        let mut editor = self.canvas(s, frame, cell);
        let mut thin = theme.outline;
        thin.a = 0.4;
        editor.grid = Some((thin.into(), theme.outline.into(), w / s.w, 21));
        editor.bounds = self.editor_bounds.clone();

        let slots: Vec<(Slot, &str, usize, u8)> = if multicolor {
            vec![
                (Slot::Background, "background", 0, 0),
                (Slot::Multi1, "01 $d025", 1, 1),
                (Slot::Sprite, "10 sprite color", 2, 2),
                (Slot::Multi2, "11 $d026", 3, 3),
            ]
        } else {
            vec![(Slot::Background, "background", 0, 0), (Slot::Sprite, "sprite color", 2, 1)]
        };
        let pens = div().flex().flex_col().gap_1().children(slots.into_iter().map(|(slot, label, ix, pen)| {
            let drawing = multicolor && pen != 0 && self.pen == pen;
            div()
                .id(SharedString::from(format!("spr-slot-{ix}")))
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
                    if pen != 0 {
                        this.pen = pen;
                    }
                    cx.notify();
                }))
        }));
        let palette = div().flex().flex_wrap().w(px(8. * 26.)).gap_0p5().children((0..16u8).map(|i| {
            div()
                .id(("spr-color", i as usize))
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
                        Slot::Sprite => 2,
                        Slot::Multi2 => 3,
                    };
                    this.colors[ix] = i;
                    cx.notify();
                }))
        }));

        // all frames, small
        let frames = div().flex().flex_wrap().gap_2().children(s.frames.iter().enumerate().map(|(ix, f)| {
            let active = ix == frame;
            div()
                .id(("spr-frame", ix))
                .flex()
                .flex_col()
                .items_center()
                .gap_0p5()
                .p_1()
                .rounded_md()
                .cursor_pointer()
                .border_1()
                .border_color(if active { theme.caret } else { theme.outline_variant })
                .child(self.canvas(s, ix, 2.))
                .child(div().text_xs().when(f.default, |el| el.font_weight(FontWeight::SEMIBOLD)).child(f.name.clone()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.frame = ix;
                    cx.notify();
                }))
        }));

        let info = format!(
            "{} · {} x {} sprites · {} frames{}",
            if multicolor { "multicolor" } else { "hires" },
            s.w,
            s.h,
            s.frames.len(),
            if s.layers.is_empty() { String::new() } else { format!(" · {} layers (kept, not shown)", s.layers.len()) }
        );
        div()
            .id("sprites")
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .overflow_y_scroll()
            .child(file_bar)
            .child(div().text_xs().text_color(theme.on_surface_variant).child(info))
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
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(s.frames[frame].name.clone()))
                            .child(
                                div()
                                    .cursor_crosshair()
                                    .child(editor)
                                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_editor_down))
                                    .on_mouse_down(MouseButton::Right, cx.listener(Self::on_editor_down))
                                    .on_mouse_move(cx.listener(Self::on_editor_move))
                                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.stroke = None))
                                    .on_mouse_up(MouseButton::Right, cx.listener(|this, _, _, _| this.stroke = None)),
                            )
                            .child(div().text_xs().text_color(theme.on_surface_variant).child("Left button: draw · right button: erase")),
                    )
                    .child(div().flex().flex_col().gap_2().w(px(230.)).child(pens).child(palette)),
            )
            .child(div().text_xs().text_color(theme.on_surface_variant).child("FRAMES"))
            .child(frames)
            .into_any_element()
    }
}
