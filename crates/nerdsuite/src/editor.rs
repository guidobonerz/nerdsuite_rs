//! The editor surface of the Code view: a source with line numbers and colours, the caret and a
//! selection, the keys of nerdsuite2's `codeeditor.js` (Enter keeps the indent - BASIC: proposes
//! the next line number -, Tab and Shift+Tab indent, Ctrl+/ comments lines in or out), undo and
//! the input of the system (IME) through gpui's `EntityInputHandler`.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, ContentMask, CursorStyle, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollEvent, ShapedLine, SharedString, Style, TextRun, UTF16Selection, Window, actions,
    div, fill, point, prelude::*, px, relative, size,
};
use nerdsuite_core::syntax::{self, Language};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{text::Text, theme::theme};

actions!(
    editor,
    [
        Backspace,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        Home,
        End,
        SelectHome,
        SelectEnd,
        DocStart,
        DocEnd,
        PageUp,
        PageDown,
        SelectAll,
        Newline,
        Indent,
        Outdent,
        ToggleComment,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
    ]
);

const CONTEXT: &str = "CodeEditor";
const INDENT: &str = "        ";
const UNDO_LIMIT: usize = 500;

pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("shift-up", SelectUp, context),
        KeyBinding::new("shift-down", SelectDown, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("shift-home", SelectHome, context),
        KeyBinding::new("shift-end", SelectEnd, context),
        KeyBinding::new(secondary!("home"), DocStart, context),
        KeyBinding::new(secondary!("end"), DocEnd, context),
        KeyBinding::new("pageup", PageUp, context),
        KeyBinding::new("pagedown", PageDown, context),
        KeyBinding::new(secondary!("a"), SelectAll, context),
        KeyBinding::new("enter", Newline, context),
        KeyBinding::new("tab", Indent, context),
        KeyBinding::new("shift-tab", Outdent, context),
        KeyBinding::new(secondary!("/"), ToggleComment, context),
        KeyBinding::new(secondary!("c"), Copy, context),
        KeyBinding::new(secondary!("x"), Cut, context),
        KeyBinding::new(secondary!("v"), Paste, context),
        KeyBinding::new(secondary!("z"), Undo, context),
        KeyBinding::new(secondary!("shift-z"), Redo, context),
        KeyBinding::new(secondary!("y"), Redo, context),
    ]);
}

/// The editor has changed: its text (dirty), or the caret.
pub enum EditorEvent {
    Edited,
    CaretMoved,
    /// a click into the gutter: the line (from 1)
    BreakpointToggled(usize),
}

impl EventEmitter<EditorEvent> for CodeEditor {}

#[derive(Clone)]
struct Snapshot {
    text: String,
    selection: Range<usize>,
}

/// What the last paint laid out: for the mouse, the input of the system and scrolling.
struct Layout {
    bounds: Bounds<Pixels>,
    text_left: Pixels,
    line_height: Pixels,
    first_line: usize,
    lines: Vec<ShapedLine>,
}

pub struct CodeEditor {
    focus_handle: FocusHandle,
    pub path: String,
    language: Language,
    text: Text,
    saved_text: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    /// The column Up and Down keep to, in chars.
    goal_column: Option<usize>,
    scroll_top: Pixels,
    autoscroll: bool,
    is_selecting: bool,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    /// The last edit was typing: the next typed char joins its undo step.
    typing: bool,
    layout: Option<Layout>,
    /// the lines (from 0) of the problems of the last build: true for an error
    marks: std::collections::HashMap<usize, bool>,
    /// the lines (from 0) with a breakpoint
    breakpoints: std::collections::BTreeSet<usize>,
    /// the line (from 0) the CPU stands at in the debugger
    exec_line: Option<usize>,
}

impl CodeEditor {
    pub fn new(path: String, language: Language, content: String, cx: &mut Context<Self>) -> Self {
        let text = Text::new(content);
        CodeEditor {
            focus_handle: cx.focus_handle(),
            path,
            language,
            saved_text: text.as_str().to_owned(),
            text,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            goal_column: None,
            scroll_top: px(0.),
            autoscroll: false,
            is_selecting: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            typing: false,
            layout: None,
            marks: Default::default(),
            breakpoints: Default::default(),
            exec_line: None,
        }
    }

    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    pub fn is_dirty(&self) -> bool {
        self.text.as_str() != self.saved_text
    }

    pub fn mark_saved(&mut self, cx: &mut Context<Self>) {
        self.saved_text = self.text.as_str().to_owned();
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    /// The line and column of the caret, from 1.
    /// The lines (from 1) of the problems of a build: errors and warnings.
    /// The lines (from 1) with a breakpoint.
    pub fn set_breakpoints(&mut self, lines: impl IntoIterator<Item = usize>, cx: &mut Context<Self>) {
        self.breakpoints = lines.into_iter().map(|l| l.saturating_sub(1)).collect();
        cx.notify();
    }

    /// The line (from 1) where the debugger stopped, in sight; `None` while it runs.
    pub fn set_exec_line(&mut self, line: Option<usize>, cx: &mut Context<Self>) {
        let line = line.map(|l| l.saturating_sub(1));
        if self.exec_line != line {
            self.exec_line = line;
            if let (Some(line), Some(layout)) = (line, self.layout.as_ref()) {
                let top = layout.line_height * line as f32;
                let height = layout.bounds.size.height;
                if top < self.scroll_top || top + layout.line_height > self.scroll_top + height {
                    self.scroll_top = (top - height / 3.).max(px(0.));
                }
            }
            cx.notify();
        }
    }

    pub fn set_marks(&mut self, marks: impl IntoIterator<Item = (usize, bool)>, cx: &mut Context<Self>) {
        self.marks.clear();
        for (line, error) in marks {
            let error = error || self.marks.get(&line.saturating_sub(1)).copied().unwrap_or(false);
            self.marks.insert(line.saturating_sub(1), error);
        }
        cx.notify();
    }

    /// Goes to a line and column (from 1), as the problems do.
    pub fn go_to(&mut self, line: usize, column: usize, cx: &mut Context<Self>) {
        let offset = self.text.offset(line.saturating_sub(1), column.saturating_sub(1));
        self.goal_column = None;
        self.move_to(offset, cx);
    }

    pub fn caret_position(&self) -> (usize, usize) {
        let (line, column) = self.text.position(self.cursor_offset());
        (line + 1, column + 1)
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    // --- moving -------------------------------------------------------------------------------

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.after_move(cx);
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.after_move(cx);
    }

    fn after_move(&mut self, cx: &mut Context<Self>) {
        self.typing = false;
        self.autoscroll = true;
        cx.emit(EditorEvent::CaretMoved);
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.text
            .as_str()
            .grapheme_indices(true)
            .rev()
            .find_map(|(ix, _)| (ix < offset).then_some(ix))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.text
            .as_str()
            .grapheme_indices(true)
            .find_map(|(ix, _)| (ix > offset).then_some(ix))
            .unwrap_or(self.text.len())
    }

    /// The offset `delta` lines above or below the caret, in the column it keeps to.
    fn vertical(&mut self, delta: isize) -> usize {
        let (line, column) = self.text.position(self.cursor_offset());
        let column = *self.goal_column.get_or_insert(column);
        let target = line as isize + delta;
        if target < 0 {
            0
        } else if target as usize >= self.text.line_count() {
            self.text.len()
        } else {
            self.text.offset(target as usize, column)
        }
    }

    /// Home goes to the first char of the line that is not a space, then to its start.
    fn home_offset(&self) -> usize {
        let cursor = self.cursor_offset();
        let range = self.text.line_range(self.text.line_of(cursor));
        let line = self.text.slice(range.clone());
        let first = range.start + (line.len() - line.trim_start().len());
        if cursor == first { range.start } else { first }
    }

    fn end_offset(&self) -> usize {
        self.text.line_range(self.text.line_of(self.cursor_offset())).end
    }

    fn page_lines(&self) -> isize {
        self.layout
            .as_ref()
            .map_or(20, |layout| (layout.bounds.size.height / layout.line_height) as isize - 1)
            .max(1)
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_column = None;
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_column = None;
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.vertical(-1);
        self.move_to(offset, cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.vertical(1);
        self.move_to(offset, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_column = None;
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_column = None;
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.vertical(-1);
        self.select_to(offset, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.vertical(1);
        self.select_to(offset, cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_column = None;
        self.move_to(self.home_offset(), cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_column = None;
        self.move_to(self.end_offset(), cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_column = None;
        self.select_to(self.home_offset(), cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_column = None;
        self.select_to(self.end_offset(), cx);
    }

    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_column = None;
        self.move_to(0, cx);
    }

    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.goal_column = None;
        self.move_to(self.text.len(), cx);
    }

    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.vertical(-self.page_lines());
        self.move_to(offset, cx);
    }

    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        let offset = self.vertical(self.page_lines());
        self.move_to(offset, cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.selected_range = 0..self.text.len();
        self.selection_reversed = false;
        cx.notify();
    }

    // --- editing ------------------------------------------------------------------------------

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.as_str().to_owned(),
            selection: self.selected_range.clone(),
        }
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.snapshot());
        if self.undo_stack.len() > UNDO_LIMIT {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// Replaces a range and puts the caret after the new text.
    fn edit(&mut self, range: Range<usize>, new_text: &str, typing: bool, cx: &mut Context<Self>) {
        // a typed word is one step of undo; a space, a new line or anything else ends it
        let joins = typing && self.typing && !new_text.contains(char::is_whitespace);
        if !joins {
            self.push_undo();
        }
        self.text.replace(range.clone(), new_text);
        let caret = range.start + new_text.len();
        self.selected_range = caret..caret;
        self.selection_reversed = false;
        self.marked_range = None;
        self.goal_column = None;
        self.typing = typing;
        self.autoscroll = true;
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    fn restore(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        self.text = Text::new(snapshot.text);
        self.selected_range = snapshot.selection;
        self.selection_reversed = false;
        self.marked_range = None;
        self.typing = false;
        self.autoscroll = true;
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack.push(self.snapshot());
            self.restore(snapshot, cx);
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push(self.snapshot());
            self.restore(snapshot, cx);
        }
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selected_range.is_empty() {
            let cursor = self.cursor_offset();
            if cursor == 0 {
                window.play_system_bell();
                return;
            }
            self.previous_boundary(cursor)..cursor
        } else {
            self.selected_range.clone()
        };
        self.edit(range, "", false, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        let range = if self.selected_range.is_empty() {
            let cursor = self.cursor_offset();
            if cursor == self.text.len() {
                window.play_system_bell();
                return;
            }
            cursor..self.next_boundary(cursor)
        } else {
            self.selected_range.clone()
        };
        self.edit(range, "", false, cx);
    }

    /// Enter keeps the indent; in BASIC it proposes the next line number (the line's + 10).
    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        let line = self.text.line(self.text.line_of(self.selected_range.start));
        let mut insert = String::from("\n");
        match self.language {
            Language::Basic => {
                let digits = line.chars().take_while(char::is_ascii_digit).collect::<String>();
                if let Ok(number) = digits.parse::<u32>() {
                    insert.push_str(&format!("{} ", number + 10));
                }
            }
            _ => insert.extend(line.chars().take_while(|c| *c == ' ' || *c == '\t')),
        }
        self.edit(self.selected_range.clone(), &insert, false, cx);
    }

    /// The lines of the selection (or of the caret) as a range of line indices.
    fn selected_lines(&self) -> Range<usize> {
        let first = self.text.line_of(self.selected_range.start);
        let mut last = self.text.line_of(self.selected_range.end);
        // a selection that ends at the start of a line does not take that line
        if last > first && self.text.line_range(last).start == self.selected_range.end {
            last -= 1;
        }
        first..last + 1
    }

    /// Rewrites whole lines and selects them, as one step of undo.
    fn rewrite_lines(&mut self, f: impl Fn(&str) -> String, cx: &mut Context<Self>) {
        let lines = self.selected_lines();
        let start = self.text.line_range(lines.start).start;
        let end = self.text.line_range(lines.end - 1).end;
        let new_text = (lines.start..lines.end)
            .map(|line| f(self.text.line(line)))
            .collect::<Vec<_>>()
            .join("\n");
        self.edit(start..end, &new_text, false, cx);
        self.selected_range = start..start + new_text.len();
    }

    fn indent(&mut self, _: &Indent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.edit(self.selected_range.clone(), INDENT, false, cx);
        } else {
            self.rewrite_lines(|line| format!("{INDENT}{line}"), cx);
        }
    }

    fn outdent(&mut self, _: &Outdent, _: &mut Window, cx: &mut Context<Self>) {
        self.rewrite_lines(
            |line| {
                let spaces = line.len() - line.trim_start_matches(' ').len();
                line[spaces.min(INDENT.len())..].to_owned()
            },
            cx,
        );
    }

    /// Ctrl+/: `;` before the lines of an assembler source, or takes it away when all have one.
    fn toggle_comment(&mut self, _: &ToggleComment, _: &mut Window, cx: &mut Context<Self>) {
        let prefix = match self.language {
            Language::Acme => ";",
            Language::Basic => return toggle_rem(self, cx),
            Language::Plain => return,
        };
        let lines = self.selected_lines();
        let all_commented = lines
            .clone()
            .all(|line| self.text.line(line).trim_start().starts_with(prefix));
        self.rewrite_lines(
            |line| {
                if all_commented {
                    line.replacen(prefix, "", 1)
                } else {
                    format!("{prefix}{line}")
                }
            },
            cx,
        );

        /// BASIC: `REM` after the line number.
        fn toggle_rem(editor: &mut CodeEditor, cx: &mut Context<CodeEditor>) {
            fn split(line: &str) -> (&str, &str) {
                let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
                let (number, rest) = line.split_at(digits);
                (number, rest.trim_start())
            }
            let lines = editor.selected_lines();
            let all_rem = lines.clone().all(|line| {
                split(editor.text.line(line)).1.to_ascii_lowercase().starts_with("rem ")
            });
            editor.rewrite_lines(
                |line| {
                    let (number, rest) = split(line);
                    if all_rem {
                        format!("{number} {}", &rest[4..])
                    } else {
                        format!("{number} rem {rest}")
                    }
                },
                cx,
            );
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            let text = self.text.slice(self.selected_range.clone()).to_owned();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            let text = self.text.slice(self.selected_range.clone()).to_owned();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.edit(self.selected_range.clone(), "", false, cx);
        }
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = text.replace("\r\n", "\n");
            self.edit(self.selected_range.clone(), &text, false, cx);
        }
    }

    // --- the mouse and scrolling ----------------------------------------------------------------

    fn index_for_position(&self, position: Point<Pixels>) -> usize {
        let Some(layout) = self.layout.as_ref() else {
            return 0;
        };
        let y = position.y - layout.bounds.top() + self.scroll_top;
        if y < px(0.) {
            return 0;
        }
        let line = (y / layout.line_height) as usize;
        if line >= self.text.line_count() {
            return self.text.len();
        }
        let start = self.text.line_range(line).start;
        match layout.lines.get(line.wrapping_sub(layout.first_line)) {
            Some(shaped) => start + shaped.closest_index_for_x(position.x - layout.text_left),
            None => start,
        }
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle, cx);
        // a click into the gutter: a breakpoint on or off
        if let Some(layout) = self.layout.as_ref()
            && event.position.x < layout.text_left - px(4.)
        {
            let line = self.text.line_of(self.index_for_position(event.position));
            cx.emit(EditorEvent::BreakpointToggled(line + 1));
            return;
        }
        self.goal_column = None;
        let offset = self.index_for_position(event.position);
        if event.click_count >= 2 {
            self.select_word(offset, cx);
            return;
        }
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
    }

    fn select_word(&mut self, offset: usize, cx: &mut Context<Self>) {
        let range = self.text.line_range(self.text.line_of(offset));
        let line = self.text.slice(range.clone());
        let is_word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '@' | '!');
        let at = offset - range.start;
        let start = line[..at]
            .char_indices()
            .rev()
            .find(|&(_, c)| !is_word(c))
            .map_or(0, |(ix, c)| ix + c.len_utf8());
        let end = line[at..].find(|c| !is_word(c)).map_or(line.len(), |ix| at + ix);
        self.selected_range = range.start + start..range.start + end;
        self.selection_reversed = false;
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_position(event.position), cx);
        }
    }

    fn on_scroll(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(window.line_height());
        self.scroll_top = self.clamp_scroll(self.scroll_top - delta.y, window.line_height());
        cx.notify();
    }

    fn clamp_scroll(&self, scroll_top: Pixels, line_height: Pixels) -> Pixels {
        let viewport = self
            .layout
            .as_ref()
            .map_or(px(0.), |layout| layout.bounds.size.height);
        let max = (line_height * self.text.line_count() as f32 - viewport + line_height * 3.)
            .max(px(0.));
        scroll_top.clamp(px(0.), max)
    }
}

impl EntityInputHandler for CodeEditor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.text.offset_from_utf16(range_utf16.start)
            ..self.text.offset_from_utf16(range_utf16.end);
        actual_range.replace(self.text.offset_to_utf16(range.start)..self.text.offset_to_utf16(range.end));
        Some(self.text.slice(range).to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.text.offset_to_utf16(self.selected_range.start)
                ..self.text.offset_to_utf16(self.selected_range.end),
            reversed: self.selection_reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_range = self.text.offset_from_utf16(range_utf16.start)
            ..self.text.offset_from_utf16(range_utf16.end);
        self.selection_reversed = false;
        cx.notify();
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.text.offset_to_utf16(self.text.len()))
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range.as_ref().map(|range| {
            self.text.offset_to_utf16(range.start)..self.text.offset_to_utf16(range.end)
        })
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.text.offset_from_utf16(range.start)..self.text.offset_from_utf16(range.end))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        // Enter and Tab come as actions; what the system sends here is typed text
        self.edit(range, new_text, true, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|range| self.text.offset_from_utf16(range.start)..self.text.offset_from_utf16(range.end))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.edit(range.clone(), new_text, true, cx);
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        }
        if let Some(selected) = new_selected_range_utf16 {
            let start_utf16 = self.text.offset_to_utf16(range.start);
            let inserted_utf16 = new_text.encode_utf16().count();
            self.selected_range = self
                .text
                .offset_from_utf16(start_utf16 + selected.start.min(inserted_utf16))
                ..self
                    .text
                    .offset_from_utf16(start_utf16 + selected.end.min(inserted_utf16));
        }
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let start = self.text.offset_from_utf16(range_utf16.start);
        let line = self.text.line_of(start);
        let shaped = layout.lines.get(line.checked_sub(layout.first_line)?)?;
        let x = layout.text_left + shaped.x_for_index(start - self.text.line_range(line).start);
        let y = layout.bounds.top() + layout.line_height * line as f32 - self.scroll_top;
        Some(Bounds::new(point(x, y), size(px(2.), layout.line_height)))
    }

    fn selection_bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.bounds_for_range(range_utf16, element_bounds, window, cx)
            .into_iter()
            .collect()
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.text.offset_to_utf16(self.index_for_position(point)))
    }
}

impl Focusable for CodeEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CodeEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = theme(cx);
        div()
            .size_full()
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .bg(theme.surface)
            .text_color(theme.on_surface)
            .font_family(crate::theme::mono_font())
            .text_size(px(14.))
            .line_height(px(20.))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::indent))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::toggle_comment))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll(cx.listener(Self::on_scroll))
            .child(CodeElement {
                editor: cx.entity(),
            })
    }
}

/// Paints the lines in sight: the gutter with the line numbers, the line of the caret, the
/// selection, the coloured text and the caret.
struct CodeElement {
    editor: Entity<CodeEditor>,
}

struct Prepaint {
    gutter: Vec<(Point<Pixels>, ShapedLine)>,
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    quads: Vec<gpui::PaintQuad>,
    caret: Option<gpui::PaintQuad>,
    gutter_bounds: Bounds<Pixels>,
}

impl IntoElement for CodeElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for CodeElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

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
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        let theme = *theme(cx);
        let style = window.text_style();
        let font = style.font();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();

        let run = |len: usize, color: gpui::Hsla| TextRun {
            len,
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        self.editor.update(cx, |editor, _| {
            // keep the caret in sight after a move or an edit
            if editor.autoscroll {
                editor.autoscroll = false;
                let caret_line = editor.text.line_of(editor.cursor_offset()) as f32;
                let caret_top = line_height * caret_line;
                if caret_top < editor.scroll_top {
                    editor.scroll_top = caret_top;
                } else if caret_top + line_height > editor.scroll_top + bounds.size.height {
                    editor.scroll_top = caret_top + line_height - bounds.size.height;
                }
            }
            editor.scroll_top = editor.clamp_scroll(editor.scroll_top, line_height);
        });
        let editor = self.editor.read(cx);
        let text = &editor.text;

        let digits = text.line_count().to_string().len().max(3);
        let char_width = window
            .text_system()
            .shape_line("0".into(), font_size, &[run(1, theme.gutter.into())], None)
            .width;
        let gutter_width = char_width * (digits as f32 + 3.);
        let text_left = bounds.left() + gutter_width + px(8.);

        let first_line = (editor.scroll_top / line_height) as usize;
        let visible = (bounds.size.height / line_height) as usize + 2;
        let last_line = (first_line + visible).min(text.line_count());
        let caret = editor.cursor_offset();
        let caret_line = text.line_of(caret);
        let selection = editor.selected_range.clone();
        let focused = editor.focus_handle.is_focused(window);

        let mut gutter = Vec::new();
        let mut lines = Vec::new();
        let mut shaped_lines = Vec::new();
        let mut quads = Vec::new();
        let mut caret_quad = None;

        for line in first_line..last_line {
            let top = bounds.top() + line_height * line as f32 - editor.scroll_top;
            let range = text.line_range(line);
            let content = text.line(line).replace('\t', " ");

            if line == caret_line && selection.is_empty() {
                quads.push(fill(
                    Bounds::new(point(bounds.left(), top), size(bounds.size.width, line_height)),
                    theme.caret_line,
                ));
            }

            if editor.exec_line == Some(line) {
                let mut tint = theme.warning;
                tint.a = 0.28;
                quads.push(fill(
                    Bounds::new(point(bounds.left(), top), size(bounds.size.width, line_height)),
                    tint,
                ));
            }
            if editor.breakpoints.contains(&line) {
                let d = line_height * 0.55;
                quads.push(
                    fill(
                        Bounds::new(point(bounds.left() + char_width * 0.5, top + (line_height - d) / 2.), size(d, d)),
                        theme.error,
                    )
                    .corner_radii(d / 2.),
                );
            }

            let number = format!("{:>digits$}", line + 1);
            let mark = editor.marks.get(&line).copied();
            if let Some(error) = mark {
                let mut tint = if error { theme.error } else { theme.warning };
                tint.a = 0.12;
                quads.push(fill(
                    Bounds::new(point(bounds.left(), top), size(bounds.size.width, line_height)),
                    tint,
                ));
            }
            let number_color = match mark {
                Some(true) => theme.error,
                Some(false) => theme.warning,
                None if line == caret_line => theme.on_surface_variant,
                None => theme.gutter,
            };
            let number_len = number.len();
            gutter.push((
                point(bounds.left() + char_width * 2., top),
                window
                    .text_system()
                    .shape_line(number.into(), font_size, &[run(number_len, number_color.into())], None),
            ));

            // the coloured runs of the line, the gaps between the tokens in the plain colour
            let mut runs = Vec::new();
            let mut at = 0;
            for (span, token) in syntax::highlight(editor.language, &content) {
                if span.start > at {
                    runs.push(run(span.start - at, theme.on_surface.into()));
                }
                runs.push(run(span.len(), theme.token(token)));
                at = span.end;
            }
            if at < content.len() || runs.is_empty() {
                runs.push(run(content.len() - at, theme.on_surface.into()));
            }
            let shaped = window
                .text_system()
                .shape_line(SharedString::from(content), font_size, &runs, None);

            // the selection on this line, to its end (and a little more for the line break)
            let sel_start = selection.start.max(range.start);
            let sel_end = selection.end.min(range.end);
            if !selection.is_empty() && selection.start <= range.end && selection.end >= range.start {
                let x0 = shaped.x_for_index(sel_start.saturating_sub(range.start));
                let mut x1 = shaped.x_for_index(sel_end.saturating_sub(range.start));
                if selection.end > range.end {
                    x1 += char_width;
                }
                quads.push(fill(
                    Bounds::from_corners(point(text_left + x0, top), point(text_left + x1, top + line_height)),
                    theme.selection,
                ));
            }

            if focused && line == caret_line {
                let x = shaped.x_for_index(caret - range.start);
                caret_quad = Some(fill(
                    Bounds::new(point(text_left + x, top), size(px(2.), line_height)),
                    theme.caret,
                ));
            }

            lines.push((point(text_left, top), shaped.clone()));
            shaped_lines.push(shaped);
        }

        self.editor.update(cx, |editor, _| {
            editor.layout = Some(Layout {
                bounds,
                text_left,
                line_height,
                first_line,
                lines: shaped_lines,
            });
        });

        Prepaint {
            gutter,
            lines,
            quads,
            caret: caret_quad,
            gutter_bounds: Bounds::new(bounds.origin, size(gutter_width, bounds.size.height)),
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let theme = *theme(cx);
        let focus_handle = self.editor.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );
        let line_height = window.line_height();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(prepaint.gutter_bounds, theme.container_low));
            for quad in prepaint.quads.drain(..) {
                window.paint_quad(quad);
            }
            for (origin, line) in prepaint.gutter.drain(..).chain(prepaint.lines.drain(..)) {
                line.paint(origin, line_height, gpui::TextAlign::Left, None, window, cx)
                    .ok();
            }
            if let Some(caret) = prepaint.caret.take() {
                window.paint_quad(caret);
            }
        });
    }
}
