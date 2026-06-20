#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CursorPosition {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerBuffer {
    lines: Vec<String>,
    row: usize,
    col: usize,
    selection_anchor: Option<CursorPosition>,
}

impl Default for ComposerBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl ComposerBuffer {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            selection_anchor: None,
        }
    }

    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.row = 0;
        self.col = 0;
        self.selection_anchor = None;
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|line| line.trim().is_empty())
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn row(&self) -> usize {
        self.row
    }

    pub fn col(&self) -> usize {
        self.col
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn selection(&self) -> Option<(CursorPosition, CursorPosition)> {
        let anchor = self.selection_anchor?;
        let cursor = self.cursor();
        if anchor == cursor {
            None
        } else if anchor <= cursor {
            Some((anchor, cursor))
        } else {
            Some((cursor, anchor))
        }
    }

    pub fn to_text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn insert_char(&mut self, ch: char) {
        self.delete_selection();
        let byte_idx = char_to_byte_idx(&self.lines[self.row], self.col);
        self.lines[self.row].insert(byte_idx, ch);
        self.col += 1;
    }

    pub fn insert_newline(&mut self) {
        self.delete_selection();
        let byte_idx = char_to_byte_idx(&self.lines[self.row], self.col);
        let tail = self.lines[self.row].split_off(byte_idx);
        self.row += 1;
        self.col = 0;
        self.lines.insert(self.row, tail);
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }

        if self.col > 0 {
            let start = char_to_byte_idx(&self.lines[self.row], self.col - 1);
            let end = char_to_byte_idx(&self.lines[self.row], self.col);
            self.lines[self.row].replace_range(start..end, "");
            self.col -= 1;
            return;
        }

        if self.row == 0 {
            return;
        }

        let current = self.lines.remove(self.row);
        self.row -= 1;
        self.col = char_count(&self.lines[self.row]);
        self.lines[self.row].push_str(&current);
    }

    pub fn move_left(&mut self) {
        if let Some((start, _)) = self.selection() {
            self.selection_anchor = None;
            self.set_cursor(start);
            return;
        }
        self.step_left();
        self.selection_anchor = None;
    }

    pub fn move_right(&mut self) {
        if let Some((_, end)) = self.selection() {
            self.selection_anchor = None;
            self.set_cursor(end);
            return;
        }
        self.step_right();
        self.selection_anchor = None;
    }

    pub fn move_up(&mut self) {
        self.step_up();
        self.selection_anchor = None;
    }

    pub fn move_down(&mut self) {
        self.step_down();
        self.selection_anchor = None;
    }

    pub fn move_home(&mut self) {
        self.col = 0;
        self.selection_anchor = None;
    }

    pub fn move_end(&mut self) {
        self.col = char_count(&self.lines[self.row]);
        self.selection_anchor = None;
    }

    pub fn select_left(&mut self) {
        self.extend_selection(Self::step_left);
    }

    pub fn select_right(&mut self) {
        self.extend_selection(Self::step_right);
    }

    pub fn select_up(&mut self) {
        self.extend_selection(Self::step_up);
    }

    pub fn select_down(&mut self) {
        self.extend_selection(Self::step_down);
    }

    pub fn select_home(&mut self) {
        self.selection_anchor.get_or_insert(self.cursor());
        self.col = 0;
        self.clear_selection_if_collapsed();
    }

    pub fn select_end(&mut self) {
        self.selection_anchor.get_or_insert(self.cursor());
        self.col = char_count(&self.lines[self.row]);
        self.clear_selection_if_collapsed();
    }

    fn cursor(&self) -> CursorPosition {
        CursorPosition {
            row: self.row,
            col: self.col,
        }
    }

    fn set_cursor(&mut self, cursor: CursorPosition) {
        self.row = cursor.row;
        self.col = cursor.col;
    }

    fn extend_selection(&mut self, step: fn(&mut Self)) {
        self.selection_anchor.get_or_insert(self.cursor());
        step(self);
        self.clear_selection_if_collapsed();
    }

    fn clear_selection_if_collapsed(&mut self) {
        if self.selection_anchor == Some(self.cursor()) {
            self.selection_anchor = None;
        }
    }

    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };

        if start.row == end.row {
            let start_byte = char_to_byte_idx(&self.lines[start.row], start.col);
            let end_byte = char_to_byte_idx(&self.lines[end.row], end.col);
            self.lines[start.row].replace_range(start_byte..end_byte, "");
        } else {
            let start_prefix = self.lines[start.row]
                .chars()
                .take(start.col)
                .collect::<String>();
            let end_suffix = self.lines[end.row]
                .chars()
                .skip(end.col)
                .collect::<String>();
            self.lines[start.row] = format!("{start_prefix}{end_suffix}");
            self.lines.drain(start.row + 1..=end.row);
        }

        self.set_cursor(start);
        self.selection_anchor = None;
        true
    }

    fn step_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = char_count(&self.lines[self.row]);
        }
    }

    fn step_right(&mut self) {
        let len = char_count(&self.lines[self.row]);
        if self.col < len {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn step_up(&mut self) {
        if self.row == 0 {
            return;
        }
        self.row -= 1;
        self.col = self.col.min(char_count(&self.lines[self.row]));
    }

    fn step_down(&mut self) {
        if self.row + 1 >= self.lines.len() {
            return;
        }
        self.row += 1;
        self.col = self.col.min(char_count(&self.lines[self.row]));
    }
}

fn char_count(value: &str) -> usize {
    value.chars().count()
}

fn char_to_byte_idx(value: &str, char_idx: usize) -> usize {
    value
        .char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| value.len())
}

#[cfg(test)]
mod tests {
    use super::{ComposerBuffer, CursorPosition};

    #[test]
    fn composer_builds_multiline_text() {
        let mut composer = ComposerBuffer::new();
        composer.insert_char('H');
        composer.insert_char('i');
        composer.insert_newline();
        composer.insert_char('!');

        assert_eq!(composer.to_text(), "Hi\n!");
        assert_eq!(composer.line_count(), 2);
        assert_eq!(composer.row(), 1);
        assert_eq!(composer.col(), 1);
    }

    #[test]
    fn composer_backspace_merges_lines() {
        let mut composer = ComposerBuffer::new();
        composer.insert_char('A');
        composer.insert_newline();
        composer.insert_char('B');
        composer.move_left();
        composer.backspace();

        assert_eq!(composer.to_text(), "AB");
        assert_eq!(composer.line_count(), 1);
        assert_eq!(composer.row(), 0);
        assert_eq!(composer.col(), 1);
    }

    #[test]
    fn composer_keeps_cursor_in_bounds_when_moving_vertically() {
        let mut composer = ComposerBuffer::new();
        composer.insert_char('a');
        composer.insert_char('b');
        composer.insert_char('c');
        composer.insert_newline();
        composer.insert_char('x');
        composer.move_up();
        composer.move_right();
        composer.move_right();
        composer.move_down();

        assert_eq!(composer.row(), 1);
        assert_eq!(composer.col(), 1);
    }

    #[test]
    fn backspace_deletes_active_selection() {
        let mut composer = ComposerBuffer::new();
        composer.insert_char('a');
        composer.insert_char('b');
        composer.insert_char('c');
        composer.select_left();
        composer.select_left();
        composer.backspace();

        assert_eq!(composer.to_text(), "a");
        assert_eq!(composer.row(), 0);
        assert_eq!(composer.col(), 1);
        assert_eq!(composer.selection(), None);
    }

    #[test]
    fn insert_replaces_selection_across_lines() {
        let mut composer = ComposerBuffer::new();
        composer.insert_char('a');
        composer.insert_char('b');
        composer.insert_newline();
        composer.insert_char('c');
        composer.select_up();
        composer.select_home();
        composer.insert_char('z');

        assert_eq!(composer.to_text(), "z");
        assert_eq!(composer.row(), 0);
        assert_eq!(composer.col(), 1);
    }

    #[test]
    fn selection_reports_ordered_bounds() {
        let mut composer = ComposerBuffer::new();
        composer.insert_char('a');
        composer.insert_char('b');
        composer.insert_char('c');
        composer.select_left();
        composer.select_left();

        assert_eq!(
            composer.selection(),
            Some((
                CursorPosition { row: 0, col: 1 },
                CursorPosition { row: 0, col: 3 }
            ))
        );
    }
}
