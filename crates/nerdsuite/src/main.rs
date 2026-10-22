//! NerdSuite: an IDE for programs of 8 bit machines, the Rust and gpui version of nerdsuite2.
//!
//! ```sh
//! nerdsuite                 # the welcome page: open a folder, or the demo
//! nerdsuite path/to/project # a project folder (with its nerdsuite.json)
//! ```

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

/// A key with the modifier of the system's shortcuts: Cmd on macOS, Ctrl elsewhere
/// (gpui's `secondary-` needs the system, which `KeyBinding::new` does not get).
macro_rules! secondary {
    ($keys:literal) => {
        if cfg!(target_os = "macos") {
            concat!("cmd-", $keys)
        } else {
            concat!("ctrl-", $keys)
        }
    };
}

mod code_view;
mod debug_panel;
mod disasm_view;
mod editor;
mod hex_view;
mod text;
mod theme;
mod workspace;

use std::path::PathBuf;

use gpui::{App, Bounds, Focusable, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};
use gpui_platform::application;

use crate::{
    theme::Theme,
    workspace::{Quit, Workspace},
};

/// The example project of a checkout (`examples/demo`), when there is one.
pub fn demo_project() -> Option<PathBuf> {
    let demo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo");
    demo.join("nerdsuite.json")
        .exists()
        .then(|| demo.canonicalize().unwrap_or(demo))
}

fn main() {
    let project = std::env::args_os().nth(1).map(PathBuf::from);

    application().run(move |cx: &mut App| {
        cx.set_global(Theme::light());
        editor::bind_keys(cx);
        workspace::bind_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        // the last window closed ends the program, on every system
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("NerdSuite".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    cx.set_global(Theme::for_appearance(window.appearance()));
                    cx.new(|cx| Workspace::new(project, window, cx))
                },
            )
            .expect("opening the window");
        window
            .update(cx, |workspace, window, cx| {
                window.focus(&workspace.focus_handle(cx), cx);
                // a change of the system's light or dark repaints in the other colours
                cx.observe_window_appearance(window, |_, _, cx| cx.notify()).detach();
            })
            .ok();
        cx.activate(true);
    });
}
