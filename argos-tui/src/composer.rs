#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerBuffer {
    lines: Vec<String>,
    row: usize,
    col: usize,
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
        }
    }

    pub fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.row = 0;
        self.col = 0;
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

    pub fn to_text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn insert_char(&mut self, ch: char) {
        let byte_idx = char_to_byte_idx(&self.lines[self.row], self.col);
        self.lines[self.row].insert(byte_idx, ch);
        self.col += 1;
    }

    pub fn insert_newline(&mut self) {
        let byte_idx = char_to_byte_idx(&self.lines[self.row], self.col);
        let tail = self.lines[self.row].split_off(byte_idx);
        self.row += 1;
        self.col = 0;
        self.lines.insert(self.row, tail);
    }

    pub fn backspace(&mut self) {
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
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = char_count(&self.lines[self.row]);
        }
    }

    pub fn move_right(&mut self) {
        let len = char_count(&self.lines[self.row]);
        if self.col < len {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.row == 0 {
            return;
        }
        self.row -= 1;
        self.col = self.col.min(char_count(&self.lines[self.row]));
    }

    pub fn move_down(&mut self) {
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
    use super::ComposerBuffer;

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
}
