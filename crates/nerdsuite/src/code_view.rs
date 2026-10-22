//! The Code view: the sources in tabs and the editor of the one in front.

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, MouseButton, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use nerdsuite_core::{build::Problem, syntax::Language};

use crate::{
    editor::{CodeEditor, EditorEvent},
    theme::theme,
};

pub struct CodeView {
    tabs: Vec<Tab>,
    active: Option<usize>,
    focus_handle: FocusHandle,
}

struct Tab {
    editor: Entity<CodeEditor>,
    _subscription: Subscription,
}

pub enum CodeViewEvent {
    /// Something the shell shows has changed: the dirty marks, the caret in the status bar.
    Changed,
    /// A click into the gutter of a file: its line (from 1).
    BreakpointToggled(String, usize),
}

impl EventEmitter<CodeViewEvent> for CodeView {}

impl CodeView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        CodeView {
            tabs: Vec::new(),
            active: None,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Opens a source in a tab, or brings its tab to the front.
    pub fn open(
        &mut self,
        path: &str,
        language: Language,
        content: impl FnOnce() -> String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = match self.tabs.iter().position(|tab| tab.editor.read(cx).path == path) {
            Some(ix) => ix,
            None => {
                let editor = cx.new(|cx| CodeEditor::new(path.to_owned(), language, content(), cx));
                let subscription = cx.subscribe(&editor, |_, editor, event: &EditorEvent, cx| {
                    if let EditorEvent::BreakpointToggled(line) = event {
                        let path = editor.read(cx).path.clone();
                        cx.emit(CodeViewEvent::BreakpointToggled(path, *line));
                    }
                    cx.emit(CodeViewEvent::Changed);
                    cx.notify();
                });
                self.tabs.push(Tab {
                    editor,
                    _subscription: subscription,
                });
                self.tabs.len() - 1
            }
        };
        self.activate(ix, window, cx);
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.active = Some(ix);
        window.focus(&self.tabs[ix].editor.focus_handle(cx), cx);
        cx.emit(CodeViewEvent::Changed);
        cx.notify();
    }

    fn close(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        // TODO: ask before a tab with changes is closed; for now its changes stay unsaved in memory
        // only as long as the tab is open
        self.tabs.remove(ix);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(active) if active > ix => Some(active - 1),
            Some(active) => Some(active.min(self.tabs.len() - 1)),
            None => None,
        };
        if let Some(active) = self.active {
            self.activate(active, window, cx);
        } else {
            cx.emit(CodeViewEvent::Changed);
            cx.notify();
        }
    }

    pub fn close_all(&mut self, cx: &mut Context<Self>) {
        self.tabs.clear();
        self.active = None;
        cx.emit(CodeViewEvent::Changed);
        cx.notify();
    }

    /// The texts of the open sources, for a build of what is not saved yet.
    pub fn texts(&self, cx: &App) -> std::collections::HashMap<String, String> {
        self.editors()
            .map(|editor| {
                let editor = editor.read(cx);
                (editor.path.clone(), editor.text().to_owned())
            })
            .collect()
    }

    /// Marks the lines of the problems of a build in the editors of their files.
    pub fn set_problems(&self, problems: &[Problem], cx: &mut App) {
        for editor in self.editors() {
            let path = editor.read(cx).path.clone();
            let marks = problems
                .iter()
                .filter(|p| p.file.as_deref() == Some(path.as_str()))
                .filter_map(|p| Some((p.line?, !p.warning)))
                .collect::<Vec<_>>();
            editor.update(cx, |editor, cx| editor.set_marks(marks, cx));
        }
    }

    pub fn editor_for(&self, path: &str, cx: &App) -> Option<Entity<CodeEditor>> {
        self.editors().find(|e| e.read(cx).path == path).cloned()
    }

    /// The breakpoints of a file (lines from 1) into its editor.
    pub fn set_breakpoints(&self, path: &str, lines: Vec<usize>, cx: &mut App) {
        if let Some(editor) = self.editor_for(path, cx) {
            editor.update(cx, |editor, cx| editor.set_breakpoints(lines, cx));
        }
    }

    /// Where the debugger stopped (file, line from 1), or nothing while it runs.
    pub fn set_exec_line(&self, at: Option<(&str, usize)>, cx: &mut App) {
        for editor in self.editors().cloned().collect::<Vec<_>>() {
            let path = editor.read(cx).path.clone();
            let line = at.filter(|(file, _)| *file == path).map(|(_, line)| line);
            editor.update(cx, |editor, cx| editor.set_exec_line(line, cx));
        }
    }

    pub fn active_editor(&self) -> Option<&Entity<CodeEditor>> {
        self.active.map(|ix| &self.tabs[ix].editor)
    }

    pub fn editors(&self) -> impl Iterator<Item = &Entity<CodeEditor>> {
        self.tabs.iter().map(|tab| &tab.editor)
    }

    pub fn has_unsaved(&self, cx: &App) -> bool {
        self.editors().any(|editor| editor.read(cx).is_dirty())
    }
}

impl Focusable for CodeView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.active_editor() {
            Some(editor) => editor.focus_handle(cx),
            None => self.focus_handle.clone(),
        }
    }
}

impl Render for CodeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *theme(cx);
        let tabs = self.tabs.iter().enumerate().map(|(ix, tab)| {
            let editor = tab.editor.read(cx);
            let name = editor.path.rsplit('/').next().unwrap_or(&editor.path).to_owned();
            let active = Some(ix) == self.active;
            div()
                .id(SharedString::from(format!("tab-{}", editor.path)))
                .flex()
                .items_center()
                .gap_1()
                .h(px(32.))
                .pl_3()
                .pr_1()
                .rounded_t_md()
                .cursor_pointer()
                .text_sm()
                .when(active, |el| el.bg(theme.tab_active).text_color(theme.tab_active_text))
                .when(!active, |el| {
                    el.text_color(theme.on_surface_variant)
                        .hover(|el| el.bg(theme.container))
                })
                .child(name)
                .child(
                    div()
                        .id(SharedString::from(format!("close-{}", editor.path)))
                        .w(px(18.))
                        .flex()
                        .justify_center()
                        .rounded_sm()
                        .hover(|el| el.bg(theme.container_highest).text_color(theme.on_surface))
                        .child(if editor.is_dirty() { "●" } else { "×" })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close(ix, window, cx);
                        })),
                )
                .on_click(cx.listener(move |this, _, window, cx| this.activate(ix, window, cx)))
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _, window, cx| this.close(ix, window, cx)),
                )
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus_handle)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_end()
                    .gap_0p5()
                    .px_4()
                    .pt_2()
                    .border_b_1()
                    .border_color(theme.outline_variant)
                    .children(tabs),
            )
            .child(match self.active_editor() {
                Some(editor) => div().flex_1().min_h_0().child(editor.clone()),
                None => div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.on_surface_variant)
                    .child("Open a source of the project in the explorer."),
            })
    }
}
