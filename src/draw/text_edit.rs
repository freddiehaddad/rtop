use unicode_segmentation::UnicodeSegmentation;

/// A single-line text input field with cursor support.
#[derive(Debug, Clone)]
pub struct TextEdit {
    /// The editable text.
    pub text: String,
    /// Byte position of cursor.
    pos: usize,
    /// Grapheme cluster (character) position of cursor.
    upos: usize,
    /// If true, only accept digits.
    numeric: bool,
}

impl TextEdit {
    pub fn new(text: String, numeric: bool) -> Self {
        let upos = text.graphemes(true).count();
        let pos = text.len();
        Self {
            text,
            pos,
            upos,
            numeric,
        }
    }

    /// Process a key command. Returns true if the text changed.
    pub fn command(&mut self, key: &str) -> bool {
        match key {
            "left" => {
                if self.upos > 0 {
                    self.upos -= 1;
                    self.pos = self.grapheme_byte_pos(self.upos);
                }
                false
            }
            "right" => {
                let total = self.text.graphemes(true).count();
                if self.upos < total {
                    self.upos += 1;
                    self.pos = self.grapheme_byte_pos(self.upos);
                }
                false
            }
            "home" => {
                self.upos = 0;
                self.pos = 0;
                false
            }
            "end" => {
                self.upos = self.text.graphemes(true).count();
                self.pos = self.text.len();
                false
            }
            "backspace" => {
                if self.upos > 0 {
                    self.upos -= 1;
                    let start = self.grapheme_byte_pos(self.upos);
                    let end = self.pos;
                    self.text.drain(start..end);
                    self.pos = start;
                    true
                } else {
                    false
                }
            }
            "delete" => {
                let total = self.text.graphemes(true).count();
                if self.upos < total {
                    let start = self.pos;
                    let end = self.grapheme_byte_pos(self.upos + 1);
                    self.text.drain(start..end);
                    true
                } else {
                    false
                }
            }
            s if s.len() == 1 => {
                let ch = s.chars().next().unwrap();
                if self.numeric && !ch.is_ascii_digit() {
                    return false;
                }
                self.text.insert(self.pos, ch);
                self.pos += ch.len_utf8();
                self.upos += 1;
                true
            }
            _ => false,
        }
    }

    /// Render the text with a cursor indicator (underline at cursor position).
    /// If `limit` > 0, truncate display to that many characters.
    pub fn render(&self, limit: usize) -> String {
        let graphemes: Vec<&str> = self.text.graphemes(true).collect();
        let total = graphemes.len();

        let (start, end) = if limit > 0 && total > limit {
            let half = limit / 2;
            if self.upos < half {
                (0, limit)
            } else if self.upos > total - half {
                (total - limit, total)
            } else {
                (self.upos - half, self.upos - half + limit)
            }
        } else {
            (0, total)
        };

        let mut result = String::new();
        for (i, g) in graphemes[start..end].iter().enumerate() {
            let abs_i = start + i;
            if abs_i == self.upos {
                result.push_str("\x1b[4m"); // underline on
                result.push_str(g);
                result.push_str("\x1b[24m"); // underline off
            } else {
                result.push_str(g);
            }
        }

        // If cursor is at end, show underline space
        if self.upos >= end && self.upos == total {
            result.push_str("\x1b[4m \x1b[24m");
        }

        result
    }

    /// Clear the text and reset cursor.
    pub fn clear(&mut self) {
        self.text.clear();
        self.pos = 0;
        self.upos = 0;
    }

    fn grapheme_byte_pos(&self, grapheme_idx: usize) -> usize {
        self.text
            .grapheme_indices(true)
            .nth(grapheme_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_left_moves_cursor() {
        let mut te = TextEdit::new("hello".into(), false);
        te.command("left");
        assert_eq!(te.upos, 4);
    }

    #[test]
    fn command_right_at_end_stays() {
        let mut te = TextEdit::new("hi".into(), false);
        te.command("right");
        assert_eq!(te.upos, 2); // Already at end
    }

    #[test]
    fn command_home_goes_to_start() {
        let mut te = TextEdit::new("hello".into(), false);
        te.command("home");
        assert_eq!(te.upos, 0);
    }

    #[test]
    fn command_end_goes_to_end() {
        let mut te = TextEdit::new("hello".into(), false);
        te.command("home");
        te.command("end");
        assert_eq!(te.upos, 5);
    }

    #[test]
    fn command_backspace_deletes() {
        let mut te = TextEdit::new("hello".into(), false);
        assert!(te.command("backspace"));
        assert_eq!(te.text, "hell");
    }

    #[test]
    fn command_delete_removes_at_cursor() {
        let mut te = TextEdit::new("hello".into(), false);
        te.command("home");
        assert!(te.command("delete"));
        assert_eq!(te.text, "ello");
    }

    #[test]
    fn command_char_inserts() {
        let mut te = TextEdit::new("hllo".into(), false);
        te.command("home");
        te.command("right");
        assert!(te.command("e"));
        assert_eq!(te.text, "hello");
    }

    #[test]
    fn command_numeric_rejects_non_digits() {
        let mut te = TextEdit::new("123".into(), true);
        assert!(!te.command("a"));
        assert_eq!(te.text, "123");
        assert!(te.command("4"));
        assert_eq!(te.text, "1234");
    }

    #[test]
    fn render_shows_cursor_underline() {
        let te = TextEdit::new("hi".into(), false);
        let result = te.render(0);
        assert!(result.contains("\x1b[4m"));
    }

    #[test]
    fn render_truncates_to_limit() {
        let te = TextEdit::new("hello world this is long".into(), false);
        let result = te.render(5);
        // Should contain at most 5 visible chars
        let visible: String = result
            .replace("\x1b[4m", "")
            .replace("\x1b[24m", "");
        assert!(visible.chars().count() <= 6); // 5 + maybe cursor space
    }

    #[test]
    fn clear_resets() {
        let mut te = TextEdit::new("hello".into(), false);
        te.clear();
        assert_eq!(te.text, "");
        assert_eq!(te.upos, 0);
    }
}
