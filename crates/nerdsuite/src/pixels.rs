//! A picture of the machine as quads: the editors of characters, sprites, screens and bitmaps draw
//! with it. A pixel may be wider than high (multicolor); runs of a color on a row are one quad. The
//! bounds of the last paint map the mouse to pixels.

use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, ElementId, GlobalElementId, Hsla, IntoElement, LayoutId, Pixels, Point, Style, Window, fill, point, px,
    size,
};

#[derive(Clone)]
pub struct PixelCanvas {
    pub width: usize,
    pub height: usize,
    pub pixel_w: Pixels,
    pub pixel_h: Pixels,
    /// a color a pixel, row by row; `None`: the background
    pub colors: Vec<Option<Hsla>>,
    pub background: Hsla,
    /// lines between the pixels, and stronger ones every so many pixels (x, y)
    pub grid: Option<(Hsla, Hsla, usize, usize)>,
    /// rectangles framed on top, in pixels: (x, y, w, h)
    pub frames: Vec<(usize, usize, usize, usize, Hsla)>,
    pub bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl PixelCanvas {
    pub fn new(width: usize, height: usize, pixel_w: f32, pixel_h: f32, background: Hsla) -> PixelCanvas {
        PixelCanvas {
            width,
            height,
            pixel_w: px(pixel_w),
            pixel_h: px(pixel_h),
            colors: vec![None; width * height],
            background,
            grid: None,
            frames: Vec::new(),
            bounds: Rc::new(Cell::new(None)),
        }
    }

    /// The pixel under a point of the window, if it is on the picture.
    pub fn pixel_at(bounds: &Rc<Cell<Option<Bounds<Pixels>>>>, width: usize, height: usize, position: Point<Pixels>) -> Option<(usize, usize)> {
        let b = bounds.get()?;
        if !b.contains(&position) {
            return None;
        }
        let x = ((position.x - b.left()) / b.size.width * width as f32) as usize;
        let y = ((position.y - b.top()) / b.size.height * height as f32) as usize;
        Some((x.min(width - 1), y.min(height - 1)))
    }
}

impl IntoElement for PixelCanvas {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for PixelCanvas {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = (self.pixel_w * self.width as f32).into();
        style.size.height = (self.pixel_h * self.height as f32).into();
        style.flex_shrink = 0.;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        self.bounds.set(Some(bounds));
        let (pw, ph) = (self.pixel_w, self.pixel_h);
        let origin = bounds.origin;
        window.paint_quad(fill(bounds, self.background));
        for y in 0..self.height {
            let row = &self.colors[y * self.width..(y + 1) * self.width];
            let mut x = 0;
            while x < self.width {
                let Some(color) = row[x] else {
                    x += 1;
                    continue;
                };
                let start = x;
                while x < self.width && row[x] == Some(color) {
                    x += 1;
                }
                window.paint_quad(fill(
                    Bounds::new(point(origin.x + pw * start as f32, origin.y + ph * y as f32), size(pw * (x - start) as f32, ph)),
                    color,
                ));
            }
        }
        if let Some((thin, strong, every_x, every_y)) = self.grid {
            for x in 1..self.width {
                let color = if every_x > 0 && x % every_x == 0 { strong } else { thin };
                window.paint_quad(fill(Bounds::new(point(origin.x + pw * x as f32, origin.y), size(px(1.), bounds.size.height)), color));
            }
            for y in 1..self.height {
                let color = if every_y > 0 && y % every_y == 0 { strong } else { thin };
                window.paint_quad(fill(Bounds::new(point(origin.x, origin.y + ph * y as f32), size(bounds.size.width, px(1.))), color));
            }
        }
        for &(x, y, w, h, color) in &self.frames {
            let b = Bounds::new(point(origin.x + pw * x as f32, origin.y + ph * y as f32), size(pw * w as f32, ph * h as f32));
            window.paint_quad(gpui::outline(b, color, gpui::BorderStyle::Solid));
        }
    }
}
