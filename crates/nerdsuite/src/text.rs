//! The text of an editor: a string and where its lines start. Offsets are bytes of UTF-8.

use std::ops::Range;

#[derive(Debug, Clone, Default)]
pub struct Text {
    string: String,
    line_starts: Vec<usize>,
}

impl Text {
    pub fn new(string: impl Into<String>) -> Text {
        // the editor knows only `\n`; a file with `\r\n` is saved with `\n`
        let string = string.into().replace("\r\n", "\n");
        let mut text = Text {
            string,
            line_starts: Vec::new(),
        };
        text.index();
        text
    }

    fn index(&mut self) {
        self.line_starts.clear();
        self.line_starts.push(0);
        self.line_starts
            .extend(self.string.match_indices('\n').map(|(ix, _)| ix + 1));
    }

    pub fn as_str(&self) -> &str {
        &self.string
    }

    pub fn len(&self) -> usize {
        self.string.len()
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// The bytes of a line, without its `\n`.
    pub fn line_range(&self, line: usize) -> Range<usize> {
        let start = self.line_starts[line];
        let end = self
            .line_starts
            .get(line + 1)
            .map_or(self.string.len(), |next| next - 1);
        start..end
    }

    pub fn line(&self, line: usize) -> &str {
        &self.string[self.line_range(line)]
    }

    pub fn line_of(&self, offset: usize) -> usize {
        self.line_starts.partition_point(|&start| start <= offset) - 1
    }

    /// The line and the column (in chars) of an offset.
    pub fn position(&self, offset: usize) -> (usize, usize) {
        let line = self.line_of(offset);
        let column = self.string[self.line_starts[line]..offset].chars().count();
        (line, column)
    }

    /// The offset of a column (in chars) of a line, the end of the line when it is shorter.
    pub fn offset(&self, line: usize, column: usize) -> usize {
        let range = self.line_range(line.min(self.line_count() - 1));
        self.string[range.clone()]
            .char_indices()
            .nth(column)
            .map_or(range.end, |(ix, _)| range.start + ix)
    }

    pub fn replace(&mut self, range: Range<usize>, new_text: &str) {
        self.string.replace_range(range, new_text);
        self.index();
    }

    pub fn slice(&self, range: Range<usize>) -> &str {
        &self.string[range]
    }

    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        self.string[..offset].encode_utf16().count()
    }

    pub fn offset_from_utf16(&self, offset_utf16: usize) -> usize {
        let mut utf16 = 0;
        for (ix, ch) in self.string.char_indices() {
            if utf16 >= offset_utf16 {
                return ix;
            }
            utf16 += ch.len_utf16();
        }
        self.string.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines() {
        let text = Text::new("lda #0\r\nsta $d020\n\nrts");
        assert_eq!(text.as_str(), "lda #0\nsta $d020\n\nrts");
        assert_eq!(text.line_count(), 4);
        assert_eq!(text.line(1), "sta $d020");
        assert_eq!(text.line(2), "");
        assert_eq!(text.line(3), "rts");
        assert_eq!(text.line_of(0), 0);
        assert_eq!(text.line_of(6), 0);
        assert_eq!(text.line_of(7), 1);
        assert_eq!(text.position(11), (1, 4));
        assert_eq!(text.offset(1, 4), 11);
        assert_eq!(text.offset(1, 99), 16);
        assert_eq!(text.offset(99, 0), 18);
    }

    #[test]
    fn empty_and_trailing_newline() {
        let text = Text::new("");
        assert_eq!(text.line_count(), 1);
        assert_eq!(text.line(0), "");
        let text = Text::new("a\n");
        assert_eq!(text.line_count(), 2);
        assert_eq!(text.line(1), "");
        assert_eq!(text.line_of(2), 1);
    }

    #[test]
    fn edits_and_utf16() {
        let mut text = Text::new("£ab\ncd");
        text.replace(2..3, "x\ny");
        assert_eq!(text.as_str(), "£x\nyb\ncd");
        assert_eq!(text.line_count(), 3);
        assert_eq!(text.offset_to_utf16(2), 1);
        assert_eq!(text.offset_from_utf16(1), 2);
        assert_eq!(text.offset_from_utf16(99), text.len());
    }
}
