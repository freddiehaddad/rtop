use crossterm::{
    cursor, execute,
    terminal,
};
use std::io::{self, Write};

/// Terminal sequences for screen management.
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

