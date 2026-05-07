use crate::term;
use std::fmt::Write;

/// A buffer for building ANSI terminal output with fluent positioning and color API.
///
/// Encapsulates cursor positioning and color escape codes so callers
/// don't need to know about ANSI internals.
pub struct AnsiBuffer {
    buf: String,
}

impl AnsiBuffer {
    /// Create a new empty buffer.
    pub fn new() -> Self {
        Self {
            buf: String::with_capacity(4096),
        }
    }

    /// Move cursor to column x, row y (1-based).
    pub fn mv(&mut self, x: usize, y: usize) -> &mut Self {
        // Writing into a `String` is infallible by construction —
        // `String`'s `fmt::Write` impl never returns `Err`. We
        // assert this rather than `unwrap_or_default()` so a future
        // change that swaps the writer for a fallible one fails
        // loudly.
        write!(self.buf, "\x1b[{y};{x}H").expect("writing into a String is infallible");
        self
    }

    /// Set the foreground color from a theme escape string.
    pub fn color(&mut self, color: &str) -> &mut Self {
        self.buf.push_str(color);
        self
    }

    /// Write text at the current cursor position.
    pub fn text(&mut self, s: &str) -> &mut Self {
        self.buf.push_str(s);
        self
    }

    /// Reset all formatting.
    pub fn reset(&mut self) -> &mut Self {
        self.buf.push_str(term::RESET);
        self
    }

    /// Consume the buffer and return the built string.
    pub fn finish(mut self) -> String {
        self.buf.push_str(term::RESET);
        self.buf
    }

    /// Get the current buffer contents without consuming.
    #[cfg(test)]
    pub fn as_str(&self) -> &str {
        &self.buf
    }
}

impl Default for AnsiBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_builds_positioned_colored_text() {
        let mut buf = AnsiBuffer::new();
        buf.mv(5, 10).color("\x1b[32m").text("hello");
        let s = buf.as_str();
        assert!(s.contains("\x1b[10;5H"));
        assert!(s.contains("\x1b[32m"));
        assert!(s.contains("hello"));
    }

    #[test]
    fn finish_appends_reset() {
        let buf = AnsiBuffer::new();
        let s = buf.finish();
        assert!(s.ends_with("\x1b[0m"));
    }

    #[test]
    fn default_creates_empty() {
        let buf = AnsiBuffer::default();
        assert!(buf.as_str().is_empty());
    }
}
