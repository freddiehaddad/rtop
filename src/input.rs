use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use std::borrow::Cow;
use std::time::Duration;

/// Poll for input with a timeout in milliseconds. Returns true if input is available.
pub fn poll(timeout_ms: u64) -> bool {
    event::poll(Duration::from_millis(timeout_ms)).unwrap_or(false)
}

/// Read and translate one input event to a key name.
pub fn get() -> Option<Cow<'static, str>> {
    match event::read() {
        Ok(Event::Key(key)) => {
            let k = translate_key(key);
            if k.is_empty() { None } else { Some(k) }
        }
        Ok(Event::Mouse(mouse)) => Some(translate_mouse(mouse)),
        Ok(Event::Resize(_, _)) => Some(Cow::Borrowed("resize")),
        _ => None,
    }
}

/// Translate a crossterm KeyEvent to a key name.
fn translate_key(key: KeyEvent) -> Cow<'static, str> {
    if key.kind != KeyEventKind::Press {
        return Cow::Borrowed("");
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('r') => return Cow::Borrowed("ctrl_r"),
            KeyCode::Char('s') => return Cow::Borrowed("ctrl_s"),
            KeyCode::Char('d') => return Cow::Borrowed("ctrl_d"),
            KeyCode::Char('c') => return Cow::Borrowed("q"),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc => Cow::Borrowed("escape"),
        KeyCode::Enter => Cow::Borrowed("enter"),
        KeyCode::Char(' ') => Cow::Borrowed("space"),
        KeyCode::Backspace => Cow::Borrowed("backspace"),
        KeyCode::Up => Cow::Borrowed("up"),
        KeyCode::Down => Cow::Borrowed("down"),
        KeyCode::Left => Cow::Borrowed("left"),
        KeyCode::Right => Cow::Borrowed("right"),
        KeyCode::Insert => Cow::Borrowed("insert"),
        KeyCode::Delete => Cow::Borrowed("delete"),
        KeyCode::Home => Cow::Borrowed("home"),
        KeyCode::End => Cow::Borrowed("end"),
        KeyCode::PageUp => Cow::Borrowed("page_up"),
        KeyCode::PageDown => Cow::Borrowed("page_down"),
        KeyCode::Tab => Cow::Borrowed("tab"),
        KeyCode::BackTab => Cow::Borrowed("shift_tab"),
        KeyCode::F(n) => Cow::Owned(format!("f{n}")),
        KeyCode::Char(c) => Cow::Owned(c.to_string()),
        _ => Cow::Borrowed(""),
    }
}

/// Translate a crossterm MouseEvent to a mouse event name.
fn translate_mouse(mouse: MouseEvent) -> Cow<'static, str> {
    match mouse.kind {
        MouseEventKind::Down(_) => Cow::Borrowed("mouse_click"),
        MouseEventKind::Up(_) => Cow::Borrowed("mouse_release"),
        MouseEventKind::Drag(_) => Cow::Borrowed("mouse_drag"),
        MouseEventKind::ScrollUp => Cow::Borrowed("mouse_scroll_up"),
        MouseEventKind::ScrollDown => Cow::Borrowed("mouse_scroll_down"),
        _ => Cow::Borrowed(""),
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
        assert_eq!(
            translate_key(make_key(KeyCode::Esc, KeyModifiers::NONE)),
            "escape"
        );
    }

    #[test]
    fn translate_arrow_keys() {
        assert_eq!(
            translate_key(make_key(KeyCode::Up, KeyModifiers::NONE)),
            "up"
        );
        assert_eq!(
            translate_key(make_key(KeyCode::Down, KeyModifiers::NONE)),
            "down"
        );
        assert_eq!(
            translate_key(make_key(KeyCode::Left, KeyModifiers::NONE)),
            "left"
        );
        assert_eq!(
            translate_key(make_key(KeyCode::Right, KeyModifiers::NONE)),
            "right"
        );
    }

    #[test]
    fn translate_function_keys() {
        assert_eq!(
            translate_key(make_key(KeyCode::F(1), KeyModifiers::NONE)),
            "f1"
        );
        assert_eq!(
            translate_key(make_key(KeyCode::F(12), KeyModifiers::NONE)),
            "f12"
        );
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
        assert_eq!(
            translate_key(make_key(KeyCode::Char('q'), KeyModifiers::NONE)),
            "q"
        );
        assert_eq!(
            translate_key(make_key(KeyCode::Char('1'), KeyModifiers::NONE)),
            "1"
        );
    }

    #[test]
    fn translate_backspace() {
        assert_eq!(
            translate_key(make_key(KeyCode::Backspace, KeyModifiers::NONE)),
            "backspace"
        );
    }
}
