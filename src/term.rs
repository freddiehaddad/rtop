use crossterm::{cursor, execute, terminal};
use std::io::{self, Write};

/// Terminal sequences for screen management.
pub const SYNC_START: &str = "\x1b[?2026h";
pub const SYNC_END: &str = "\x1b[?2026l";

/// Return an ANSI escape sequence that moves the cursor to column `x`, row `y`.
///
/// Both `x` and `y` are 1-based (matching ANSI CUP convention).
#[inline]
pub fn mv(x: usize, y: usize) -> String {
    format!("\x1b[{y};{x}H")
}

/// Terminal state wrapper.
pub struct Terminal {
    pub width: u16,
    pub height: u16,
}

impl Terminal {
    /// Initialize the terminal: raw mode, alternate screen, hide cursor.
    pub fn init() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
        let (w, h) = terminal::size()?;
        Ok(Self {
            width: w,
            height: h,
        })
    }

    /// Restore terminal to normal state.
    pub fn restore(&self) {
        let mut stdout = io::stdout();
        let _ = execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen);
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

    /// Write content wrapped in terminal sync sequences and flush.
    pub fn write_synced(&self, content: &str) -> io::Result<()> {
        self.write_raw(&format!("{}{}{}", SYNC_START, content, SYNC_END))
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.restore();
    }
}
