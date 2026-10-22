//! The Hex view: a file of the project as bytes, the offsets (the load address of a .prg),
//! the hex and the chars of ASCII, PETSCII or the screen codes side by side. Read only for now.

use gpui::{
    Context, FontWeight, SharedString, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};
use nerdsuite_core::hex::{self, BYTES_PER_ROW, CharSet};

use crate::theme::{mono_font, theme};

#[derive(Default)]
pub struct HexView {
    file: Option<(String, Vec<u8>)>,
    charset: CharSet,
    scroll: UniformListScrollHandle,
}

impl HexView {
    pub fn open(&mut self, path: String, bytes: Vec<u8>, cx: &mut Context<Self>) {
        self.file = Some((path, bytes));
        self.scroll = UniformListScrollHandle::new();
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.file = None;
        cx.notify();
    }
}

impl Render for HexView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let Some((path, bytes)) = self.file.as_ref() else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.on_surface_variant)
                .child("Open a file of the project in the explorer to see its bytes.");
        };

        let load = hex::prg_load_address(path, bytes);
        let toolbar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(theme.outline_variant)
            .child(div().font_weight(FontWeight::MEDIUM).child(path.clone()))
            .child(
                div()
                    .text_color(theme.on_surface_variant)
                    .child(match load {
                        Some(load) => format!("{} bytes · loads at ${load:04x}", bytes.len()),
                        None => format!("{} bytes", bytes.len()),
                    }),
            )
            .child(div().flex_1())
            .children(CharSet::ALL.into_iter().map(|charset| {
                let active = charset == self.charset;
                div()
                    .id(charset.label())
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.outline)
                    .cursor_pointer()
                    .when(active, |el| el.bg(theme.container_highest))
                    .hover(|el| el.bg(theme.container))
                    .child(charset.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.charset = charset;
                        cx.notify();
                    }))
            }));

        // a .prg shows its bytes at the addresses they load to, the two of the address before
        let base = load.map_or(0, |load| (load as usize).wrapping_sub(2));
        let rows = hex::row_count(bytes.len());
        let list = uniform_list(
            "hex-rows",
            rows,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                let theme = *crate::theme::theme(cx);
                let Some((_, bytes)) = this.file.as_ref() else {
                    return Vec::new();
                };
                range
                    .map(|row| {
                        let start = row * BYTES_PER_ROW;
                        let chunk = &bytes[start.min(bytes.len())..(start + BYTES_PER_ROW).min(bytes.len())];
                        let offset = base.wrapping_add(start) & 0xff_ffff;
                        div()
                            .flex()
                            .gap_4()
                            .px_4()
                            .child(
                                div()
                                    .text_color(theme.on_surface_variant)
                                    .child(SharedString::from(format!("{offset:06x}"))),
                            )
                            .child(div().w(px(420.)).child(SharedString::from(hex::hex_cells(chunk))))
                            .child(
                                div()
                                    .text_color(theme.on_primary_container)
                                    .child(SharedString::from(hex::char_cells(chunk, this.charset))),
                            )
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .py_2()
        .font_family(mono_font())
        .text_size(px(14.))
        .line_height(px(20.));

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(list)
    }
}
