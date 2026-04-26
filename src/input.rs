use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use std::collections::HashMap;
use std::time::Duration;

/// A clickable region on screen.
#[derive(Debug, Clone)]
#[allow(dead_code)] // used in tests and as mouse support grows
pub struct MouseLoc {
    pub line: u16,
    pub col: u16,
    pub height: u16,
    pub width: u16,
}

#[allow(dead_code)]
impl MouseLoc {
    /// Check if a point (x, y) is within this region.
    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.col
            && x < self.col + self.width
            && y >= self.line
            && y < self.line + self.height
    }
}

/// Map of clickable region name → location.
#[allow(dead_code)]
pub type MouseMappings = HashMap<String, MouseLoc>;

/// Poll for input with a timeout in milliseconds. Returns true if input is available.
pub fn poll(timeout_ms: u64) -> bool {
    event::poll(Duration::from_millis(timeout_ms)).unwrap_or(false)
}

/// Read and translate one input event to a btop key name.
pub fn get() -> Option<String> {
    match event::read() {
        Ok(Event::Key(key)) => Some(translate_key(key)),
        Ok(Event::Mouse(mouse)) => Some(translate_mouse(mouse)),
        Ok(Event::Resize(_, _)) => Some("resize".to_string()),
        _ => None,
    }
}

/// Translate a crossterm KeyEvent to a btop key name.
fn translate_key(key: KeyEvent) -> String {
    // Only process key press events, ignore release and repeat
    if key.kind != KeyEventKind::Press {
        return String::new();
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('r') => return "ctrl_r".to_string(),
            KeyCode::Char('c') => return "q".to_string(),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc => "escape".to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "page_up".to_string(),
        KeyCode::PageDown => "page_down".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => "shift_tab".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Char(c) => c.to_string(),
        _ => String::new(),
    }
}

/// Translate a crossterm MouseEvent to a btop mouse event name.
fn translate_mouse(mouse: MouseEvent) -> String {
    match mouse.kind {
        MouseEventKind::Down(_) => "mouse_click".to_string(),
        MouseEventKind::Up(_) => "mouse_release".to_string(),
        MouseEventKind::Drag(_) => "mouse_drag".to_string(),
        MouseEventKind::ScrollUp => "mouse_scroll_up".to_string(),
        MouseEventKind::ScrollDown => "mouse_scroll_down".to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

    fn make_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    #[test]
    fn translate_escape_key() {
        assert_eq!(translate_key(make_key(KeyCode::Esc, KeyModifiers::NONE)), "escape");
    }

    #[test]
    fn translate_arrow_keys() {
        assert_eq!(translate_key(make_key(KeyCode::Up, KeyModifiers::NONE)), "up");
        assert_eq!(translate_key(make_key(KeyCode::Down, KeyModifiers::NONE)), "down");
        assert_eq!(translate_key(make_key(KeyCode::Left, KeyModifiers::NONE)), "left");
        assert_eq!(translate_key(make_key(KeyCode::Right, KeyModifiers::NONE)), "right");
    }

    #[test]
    fn translate_function_keys() {
        assert_eq!(translate_key(make_key(KeyCode::F(1), KeyModifiers::NONE)), "f1");
        assert_eq!(translate_key(make_key(KeyCode::F(12), KeyModifiers::NONE)), "f12");
    }

    #[test]
    fn translate_ctrl_r() {
        assert_eq!(
            translate_key(make_key(KeyCode::Char('r'), KeyModifiers::CONTROL)),
            "ctrl_r"
        );
    }

    #[test]
    fn translate_regular_char() {
        assert_eq!(translate_key(make_key(KeyCode::Char('q'), KeyModifiers::NONE)), "q");
        assert_eq!(translate_key(make_key(KeyCode::Char('1'), KeyModifiers::NONE)), "1");
    }

    #[test]
    fn translate_backspace() {
        assert_eq!(
            translate_key(make_key(KeyCode::Backspace, KeyModifiers::NONE)),
            "backspace"
        );
    }

    #[test]
    fn mouse_loc_contains_point() {
        let loc = MouseLoc {
            line: 5,
            col: 10,
            height: 3,
            width: 5,
        };
        assert!(loc.contains(10, 5));
        assert!(loc.contains(14, 7));
        assert!(!loc.contains(9, 5));
        assert!(!loc.contains(15, 5));
        assert!(!loc.contains(10, 4));
        assert!(!loc.contains(10, 8));
    }
}
