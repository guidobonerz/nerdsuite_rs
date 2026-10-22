//! The colours of nerdsuite2 (`m3.css`, `nerdsuite.css`): zinc surfaces, the orange of the
//! selected code tab, light and dark after the system.

use gpui::{App, Global, Hsla, Rgba, WindowAppearance, rgb};
use nerdsuite_core::syntax::Token;

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub surface: Rgba,
    pub on_surface: Rgba,
    pub on_surface_variant: Rgba,
    pub container_low: Rgba,
    pub container: Rgba,
    pub container_highest: Rgba,
    pub outline: Rgba,
    pub outline_variant: Rgba,
    pub on_primary_container: Rgba,
    pub error: Rgba,
    pub warning: Rgba,
    pub tab_active: Rgba,
    pub tab_active_text: Rgba,
    pub caret: Rgba,
    pub caret_line: Rgba,
    pub selection: Rgba,
    pub gutter: Rgba,
    pub comment: Rgba,
    pub string: Rgba,
    pub number: Rgba,
    pub mnemonic: Rgba,
    pub keyword: Rgba,
    pub label: Rgba,
    pub macro_: Rgba,
}

impl Global for Theme {}

impl Theme {
    pub fn light() -> Theme {
        Theme {
            dark: false,
            surface: rgb(0xffffff),
            on_surface: rgb(0x09090b),
            on_surface_variant: rgb(0x71717a),
            container_low: rgb(0xfafafa),
            container: rgb(0xf4f4f5),
            container_highest: rgb(0xe4e4e7),
            outline: rgb(0xd4d4d8),
            outline_variant: rgb(0xe4e4e7),
            on_primary_container: rgb(0x1e3a8a),
            error: rgb(0xdc2626),
            warning: rgb(0xd97706),
            tab_active: rgb(0xb3641f),
            tab_active_text: rgb(0xfff7ef),
            caret: rgb(0xea7a00),
            caret_line: gpui::rgba(0x5fcf6f22),
            selection: gpui::rgba(0x3b82f640),
            gutter: rgb(0xa1a1aa),
            comment: rgb(0x71717a),
            string: rgb(0x16a34a),
            number: rgb(0xea580c),
            mnemonic: rgb(0x2563eb),
            keyword: rgb(0x9333ea),
            label: rgb(0xb45309),
            macro_: rgb(0x0891b2),
        }
    }

    pub fn dark() -> Theme {
        Theme {
            dark: true,
            surface: rgb(0x09090b),
            on_surface: rgb(0xfafafa),
            on_surface_variant: rgb(0xa1a1aa),
            container_low: rgb(0x111113),
            container: rgb(0x18181b),
            container_highest: rgb(0x27272a),
            outline: rgb(0x3f3f46),
            outline_variant: rgb(0x27272a),
            on_primary_container: rgb(0xbfdbfe),
            error: rgb(0xf87171),
            warning: rgb(0xfbbf24),
            tab_active: rgb(0x9a5418),
            tab_active_text: rgb(0xfff1e2),
            caret: rgb(0xff9a1f),
            caret_line: gpui::rgba(0x8ee59a18),
            selection: gpui::rgba(0x60a5fa40),
            gutter: rgb(0x52525b),
            comment: rgb(0x71717a),
            string: rgb(0x4ade80),
            number: rgb(0xfb923c),
            mnemonic: rgb(0x60a5fa),
            keyword: rgb(0xc084fc),
            label: rgb(0xfbbf24),
            macro_: rgb(0x22d3ee),
        }
    }

    pub fn for_appearance(appearance: WindowAppearance) -> Theme {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Theme::light(),
        }
    }

    pub fn token(&self, token: Token) -> Hsla {
        match token {
            Token::Comment => self.comment,
            Token::String => self.string,
            Token::Number => self.number,
            Token::Mnemonic => self.mnemonic,
            Token::Keyword => self.keyword,
            Token::Label => self.label,
            Token::Macro => self.macro_,
            Token::LineNumber => self.gutter,
        }
        .into()
    }
}

pub fn theme(cx: &App) -> &Theme {
    cx.global::<Theme>()
}

/// The monospaced font of the system: the code, the hex view.
pub fn mono_font() -> &'static str {
    if cfg!(target_os = "windows") {
        "Consolas"
    } else if cfg!(target_os = "macos") {
        "Menlo"
    } else {
        "DejaVu Sans Mono"
    }
}
