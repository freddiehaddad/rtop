use crossterm::{
    cursor, execute,
    terminal::{self, ClearType},
};
use std::io::{self, Write};

/// ANSI escape code constants matching btop's Fx namespace.
pub mod fx {
    pub const BOLD: &str = "\x1b[1m";
    pub const UNBOLD: &str = "\x1b[22m";
    pub const DIM: &str = "\x1b[2m";
    pub const UNDIM: &str = "\x1b[22m";
    pub const ITALIC: &str = "\x1b[3m";
    pub const UNITALIC: &str = "\x1b[23m";
    pub const UNDERLINE: &str = "\x1b[4m";
    pub const UNUNDERLINE: &str = "\x1b[24m";
    pub const BLINK: &str = "\x1b[5m";
    pub const UNBLINK: &str = "\x1b[25m";
    pub const STRIKETHROUGH: &str = "\x1b[9m";
    pub const UNSTRIKETHROUGH: &str = "\x1b[29m";
    pub const RESET: &str = "\x1b[0m";
}

/// Cursor movement helpers matching btop's Mv namespace.
pub mod mv {
    pub fn to(line: u16, col: u16) -> String {
        format!("\x1b[{};{}H", line, col)
    }
    pub fn right(n: u16) -> String {
        format!("\x1b[{}C", n)
    }
    pub fn left(n: u16) -> String {
        format!("\x1b[{}D", n)
    }
    pub fn up(n: u16) -> String {
        format!("\x1b[{}A", n)
    }
    pub fn down(n: u16) -> String {
        format!("\x1b[{}B", n)
    }
    pub const SAVE: &str = "\x1b[s";
    pub const RESTORE: &str = "\x1b[u";
}

/// Terminal sequences for screen management.
pub const HIDE_CURSOR: &str = "\x1b[?25l";
pub const SHOW_CURSOR: &str = "\x1b[?25h";
pub const ALT_SCREEN: &str = "\x1b[?1049h";
pub const NORMAL_SCREEN: &str = "\x1b[?1049l";
pub const MOUSE_ON: &str = "\x1b[?1002h\x1b[?1015h\x1b[?1006h";
pub const MOUSE_OFF: &str = "\x1b[?1002l\x1b[?1015l\x1b[?1006l";
pub const SYNC_START: &str = "\x1b[?2026h";
pub const SYNC_END: &str = "\x1b[?2026l";

/// Terminal state wrapper.
pub struct Terminal {
    pub width: u16,
    pub height: u16,
}

impl Terminal {
    /// Initialize the terminal: raw mode, alternate screen, hide cursor, enable mouse.
    pub fn init() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            crossterm::event::EnableMouseCapture
        )?;
        let (w, h) = terminal::size()?;
        Ok(Self {
            width: w,
            height: h,
        })
    }

    /// Restore terminal to normal state.
    pub fn restore(&self) {
        let mut stdout = io::stdout();
        let _ = execute!(
            stdout,
            crossterm::event::DisableMouseCapture,
            cursor::Show,
            terminal::LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }

    /// Check if the terminal size has changed. Returns true if resized.
    pub fn refresh(&mut self) -> bool {
        if let Ok((w, h)) = terminal::size() {
            if w != self.width || h != self.height {
                self.width = w;
                self.height = h;
                return true;
            }
        }
        false
    }

    /// Get current terminal size.
    pub fn size(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// Write raw string to stdout and flush.
    pub fn write_raw(&self, s: &str) -> io::Result<()> {
        let mut stdout = io::stdout();
        stdout.write_all(s.as_bytes())?;
        stdout.flush()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mv_to_produces_correct_sequence() {
        assert_eq!(mv::to(1, 1), "\x1b[1;1H");
        assert_eq!(mv::to(10, 20), "\x1b[10;20H");
    }

    #[test]
    fn mv_right_left_up_down() {
        assert_eq!(mv::right(5), "\x1b[5C");
        assert_eq!(mv::left(3), "\x1b[3D");
        assert_eq!(mv::up(2), "\x1b[2A");
        assert_eq!(mv::down(1), "\x1b[1B");
    }

    #[test]
    fn escape_code_constants_valid() {
        assert!(fx::BOLD.starts_with("\x1b["));
        assert!(fx::RESET.starts_with("\x1b["));
        assert!(HIDE_CURSOR.contains("25l"));
        assert!(SHOW_CURSOR.contains("25h"));
    }
}
