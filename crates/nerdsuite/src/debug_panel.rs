//! The panels of the Debugger view, as nerdsuite2's `debugview.js`: the registers and the flags, the
//! code at the PC with its labels and source lines, the stack, the memory and the log.

use gpui::{Div, FontWeight, SharedString, div, prelude::*, px};
use nerdsuite_core::{
    acme::{self, Flags},
    build::DebugInfo,
    hex::CharSet,
    run::debug::Snapshot,
};

use crate::theme::{Theme, mono_font};

/// The flags of a CPU's status register: the 6502's P (fl), the Z80's F.
fn flags_of(snapshot: &Snapshot) -> Option<(u32, &'static [(&'static str, u32)])> {
    const P6502: &[(&str, u32)] =
        &[("N", 0x80), ("V", 0x40), ("-", 0x20), ("B", 0x10), ("D", 0x08), ("I", 0x04), ("Z", 0x02), ("C", 0x01)];
    const FZ80: &[(&str, u32)] =
        &[("S", 0x80), ("Z", 0x40), ("5", 0x20), ("H", 0x10), ("3", 0x08), ("P", 0x04), ("N", 0x02), ("C", 0x01)];
    snapshot
        .register("fl")
        .map(|v| (v, P6502))
        .or_else(|| snapshot.register("f").map(|v| (v, FZ80)))
}

pub fn panel(title: &str, theme: Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(theme.outline_variant)
        .bg(theme.container_low)
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.on_surface_variant)
                .child(title.to_uppercase()),
        )
}

fn mono(theme: Theme) -> Div {
    div().font_family(mono_font()).text_size(px(13.)).line_height(px(19.)).text_color(theme.on_surface)
}

pub fn registers(snapshot: &Snapshot, theme: Theme) -> Div {
    // 16 bits: the PC, the Z80's index registers, its shadow pairs and its SP (the 6502's SP is 8 bits)
    let wide = |name: &str| {
        matches!(name, "pc" | "ix" | "iy" | "wz") || name.ends_with('\'') || (name == "sp" && snapshot.register("fl").is_none())
    };
    let regs = snapshot.registers.iter().filter(|(n, _)| n != "fl" && n != "f").map(|(name, v)| {
        let value = if wide(name) || *v > 0xff { format!("${v:04x}") } else { format!("${v:02x}") };
        div()
            .flex()
            .gap_2()
            .child(div().w(px(40.)).text_color(theme.on_surface_variant).child(name.to_uppercase()))
            .child(value)
    });
    let flags = flags_of(snapshot).map(|(value, flags)| {
        div().flex().gap_1().pt_1().children(flags.iter().map(move |(name, bit)| {
            let on = value & bit != 0;
            div()
                .w(px(18.))
                .flex()
                .justify_center()
                .rounded_sm()
                .when(on, |el| el.bg(theme.tab_active).text_color(theme.tab_active_text))
                .when(!on, |el| el.text_color(theme.gutter))
                .child(*name)
        }))
    });
    panel("Registers", theme).child(mono(theme).flex().flex_col().children(regs).children(flags))
}

/// The instructions from the PC on, with the labels of the build and the lines of the sources.
pub fn disassembly(snapshot: &Snapshot, info: Option<&DebugInfo>, cpu: &str, theme: Theme) -> Div {
    let Some(pc) = snapshot.pc else {
        return panel("Code", theme).child(div().text_color(theme.on_surface_variant).child("—"));
    };
    let lines = acme::disassemble(&snapshot.code, pc, cpu, Flags::default());
    let rows = lines.into_iter().take(24).map(|line| {
        let at_pc = line.addr == pc;
        let label = info.and_then(|i| i.label(line.addr)).map(|l| format!("{l}:"));
        let source = info
            .and_then(|i| i.line_of(line.addr))
            .map(|(file, l)| format!("{}:{l}", file.rsplit('/').next().unwrap_or(file)));
        let bytes = line.bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
        div()
            .flex()
            .gap_3()
            .px_1()
            .rounded_sm()
            .when(at_pc, |el| el.bg(theme.tab_active).text_color(theme.tab_active_text))
            .child(div().w(px(44.)).child(format!("{:04x}", line.addr)))
            .child(div().w(px(84.)).text_color(if at_pc { theme.tab_active_text } else { theme.gutter }).child(bytes))
            .child(div().w(px(110.)).text_color(if at_pc { theme.tab_active_text } else { theme.label }).child(label.unwrap_or_default()))
            .child(div().flex_1().child(line.text))
            .child(div().text_color(if at_pc { theme.tab_active_text } else { theme.on_surface_variant }).child(source.unwrap_or_default()))
    });
    panel("Code", theme).child(mono(theme).flex().flex_col().children(rows))
}

pub fn stack(snapshot: &Snapshot, theme: Theme) -> Div {
    let text = if snapshot.stack.is_empty() {
        "—".to_owned()
    } else {
        snapshot.stack.iter().take(32).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
    };
    panel("Stack", theme).child(mono(theme).child(text))
}

pub fn memory(snapshot: &Snapshot, charset: CharSet, theme: Theme) -> Div {
    let rows = snapshot.memory.chunks(16).enumerate().map(|(i, row)| {
        let addr = snapshot.memory_addr as usize + i * 16;
        div()
            .flex()
            .gap_3()
            .child(div().text_color(theme.on_surface_variant).child(SharedString::from(format!("{addr:04x}"))))
            .child(SharedString::from(nerdsuite_core::hex::hex_cells(row)))
            .child(div().text_color(theme.on_primary_container).child(SharedString::from(nerdsuite_core::hex::char_cells(row, charset))))
    });
    mono(theme).flex().flex_col().children(rows)
}

pub fn log(snapshot: &Snapshot, theme: Theme) -> Div {
    let lines = snapshot.log.iter().rev().take(6).rev().cloned();
    panel("Log", theme).child(mono(theme).text_color(theme.on_surface_variant).flex().flex_col().children(lines))
}
