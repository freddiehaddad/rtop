use crossterm::{cursor, execute, terminal};
use std::io::{self, Write};

pub const SYNC_START: &str = "\x1b[?2026h";
pub const SYNC_END: &str = "\x1b[?2026l";
pub const CLEAR_SCREEN: &str = "\x1b[2J";
pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const BOLD_OFF: &str = "\x1b[22m";
pub const UNDERLINE: &str = "\x1b[4m";
pub const UNDERLINE_OFF: &str = "\x1b[24m";

/// Return an ANSI escape sequence that moves the cursor to column `x`, row `y`.
///
/// Both `x` and `y` are 1-based (matching ANSI CUP convention).
#[inline]
pub fn mv(x: usize, y: usize) -> String {
    format!("\x1b[{y};{x}H")
}

pub struct Terminal {
    pub width: u16,
    pub height: u16,
    pub sync_enabled: bool,
}

impl Terminal {
    /// Initialize the terminal: raw mode, alternate screen, hide cursor.
    pub fn init(sync_enabled: bool) -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
        let (w, h) = terminal::size()?;
        tracing::info!(
            subsystem = %crate::log::Subsystem::Terminal,
            width = w,
            height = h,
            sync_enabled,
            "terminal initialized",
        );
        Ok(Self {
            width: w,
            height: h,
            sync_enabled,
        })
    }

    pub fn set_sync(&mut self, enabled: bool) {
        self.sync_enabled = enabled;
    }

    /// Restore terminal to normal state.
    pub fn restore(&self) {
        let mut stdout = io::stdout();
        if let Err(e) = execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen) {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Terminal,
                error = %e,
                "terminal restore failed",
            );
        }
        if let Err(e) = terminal::disable_raw_mode() {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Terminal,
                error = %e,
                "raw mode restore failed",
            );
        }
        tracing::info!(
            subsystem = %crate::log::Subsystem::Terminal,
            "terminal restored",
        );
    }

    /// Check if the terminal size has changed. Returns true if resized.
    pub fn refresh(&mut self) -> bool {
        if let Ok((w, h)) = terminal::size()
            && (w != self.width || h != self.height)
        {
            self.width = w;
            self.height = h;
            return true;
        }
        false
    }

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
        if self.sync_enabled {
            self.write_raw(&format!("{}{}{}", SYNC_START, content, SYNC_END))
        } else {
            self.write_raw(content)
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.restore();
    }
}
