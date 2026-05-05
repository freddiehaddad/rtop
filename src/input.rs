use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Typed input key, replacing stringly-typed dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Escape,
    Enter,
    Space,
    Backspace,
    Tab,
    ShiftTab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
    CtrlR,
    CtrlD,
    CtrlF,
    CtrlB,
    CtrlU,
}

/// Translate a crossterm KeyEvent to a typed Key.
pub(crate) fn translate_key(key: KeyEvent) -> Option<Key> {
    if key.kind != KeyEventKind::Press {
        return None;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('r') => return Some(Key::CtrlR),
            KeyCode::Char('d') => return Some(Key::CtrlD),
            KeyCode::Char('f') => return Some(Key::CtrlF),
            KeyCode::Char('b') => return Some(Key::CtrlB),
            KeyCode::Char('u') => return Some(Key::CtrlU),
            KeyCode::Char('c') => return Some(Key::Char('q')),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc => Some(Key::Escape),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Char(' ') => Some(Key::Space),
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::Up => Some(Key::Up),
        KeyCode::Down => Some(Key::Down),
        KeyCode::Left => Some(Key::Left),
        KeyCode::Right => Some(Key::Right),
        KeyCode::Insert => Some(Key::Insert),
        KeyCode::Delete => Some(Key::Delete),
        KeyCode::Home => Some(Key::Home),
        KeyCode::End => Some(Key::End),
        KeyCode::PageUp => Some(Key::PageUp),
        KeyCode::PageDown => Some(Key::PageDown),
        KeyCode::Tab => Some(Key::Tab),
        KeyCode::BackTab => Some(Key::ShiftTab),
        KeyCode::F(n) => Some(Key::F(n)),
        KeyCode::Char(c) => Some(Key::Char(c)),
        _ => None,
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
            Some(Key::Escape)
        );
    }

    #[test]
    fn translate_arrow_keys() {
        assert_eq!(
            translate_key(make_key(KeyCode::Up, KeyModifiers::NONE)),
            Some(Key::Up)
        );
        assert_eq!(
            translate_key(make_key(KeyCode::Down, KeyModifiers::NONE)),
            Some(Key::Down)
        );
        assert_eq!(
            translate_key(make_key(KeyCode::Left, KeyModifiers::NONE)),
            Some(Key::Left)
        );
        assert_eq!(
            translate_key(make_key(KeyCode::Right, KeyModifiers::NONE)),
            Some(Key::Right)
        );
    }

    #[test]
    fn translate_function_keys() {
        assert_eq!(
            translate_key(make_key(KeyCode::F(1), KeyModifiers::NONE)),
            Some(Key::F(1))
        );
        assert_eq!(
            translate_key(make_key(KeyCode::F(12), KeyModifiers::NONE)),
            Some(Key::F(12))
        );
    }

    #[test]
    fn translate_ctrl_r() {
        assert_eq!(
            translate_key(make_key(KeyCode::Char('r'), KeyModifiers::CONTROL)),
            Some(Key::CtrlR)
        );
    }

    #[test]
    fn translate_ctrl_c_maps_to_quit() {
        assert_eq!(
            translate_key(make_key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Key::Char('q'))
        );
    }

    #[test]
    fn translate_regular_char() {
        assert_eq!(
            translate_key(make_key(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Key::Char('q'))
        );
        assert_eq!(
            translate_key(make_key(KeyCode::Char('1'), KeyModifiers::NONE)),
            Some(Key::Char('1'))
        );
    }

    #[test]
    fn translate_backspace() {
        assert_eq!(
            translate_key(make_key(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(Key::Backspace)
        );
    }
}
