//! Inline editor for typed options in the options menu.
//!
//! Active while [`MenuState::OptionsEdit`] is the current menu state.
//! Captures every keystroke (so command keys like `q`, `j`, `k` become
//! buffer characters rather than menu commands), edits a UTF-8-safe
//! [`OptionEditState`] buffer, and either commits the new value via
//! [`ConfigKey::set_string`] / [`ConfigKey::set_int`] (Enter) or
//! discards the buffer (Esc).

use crate::{
    config::ConfigKey,
    handlers::{HandleResult, InputContext, TerminalOp, options::apply_post_change_effects},
    input::Key,
    menu,
};

/// Whether the buffer represents free-form text or an integer value.
///
/// Drives the [`Key::Char`] filter (`Integer` rejects everything that
/// is not an ASCII digit) and the commit-time validation
/// (`Integer` parses as `i64`; `Text` validates via
/// [`ConfigKey::validate_string`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EditKind {
    Text,
    Integer,
}

/// Mutable state for an in-progress inline edit.
///
/// Owned by [`crate::app::OverlayState`]; lifetime is bound to
/// [`MenuState::OptionsEdit`] via the invariant helpers
/// [`crate::app::OverlayState::enter_option_edit`] and
/// [`crate::app::OverlayState::exit_option_edit`].
///
/// `cursor` is a **char index** (not a byte offset) into `buffer`.
/// All mutating helpers (`insert_char`, `backspace`, `delete`,
/// `move_*`) maintain UTF-8 boundary correctness via
/// `char_indices` lookups, so a non-ASCII character in the buffer
/// (e.g. in `custom_cpu_name`) cannot land the cursor on a partial
/// char or panic on slicing.
#[derive(Debug, Clone)]
pub(crate) struct OptionEditState {
    pub(crate) key: ConfigKey,
    pub(crate) kind: EditKind,
    pub(crate) buffer: String,
    pub(crate) cursor: usize,
    pub(crate) error: Option<&'static str>,
}

impl OptionEditState {
    /// Create a new edit state for `key` with `buffer` as the
    /// initial value. Cursor lands at end-of-buffer so the user can
    /// keep typing or backspace to clear without an extra keystroke.
    pub(crate) fn new(key: ConfigKey, kind: EditKind, buffer: String) -> Self {
        let cursor = buffer.chars().count();
        Self {
            key,
            kind,
            buffer,
            cursor,
            error: None,
        }
    }

    fn char_count(&self) -> usize {
        self.buffer.chars().count()
    }

    /// Convert a char index into the corresponding byte offset.
    /// Returns `buffer.len()` when `char_idx == char_count()` so
    /// inserts at end work without special-casing.
    fn byte_index_at(&self, char_idx: usize) -> usize {
        self.buffer
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.buffer.len())
    }

    /// Insert `c` at the cursor and advance by one char.
    /// Clears `error` because the buffer just changed.
    pub(crate) fn insert_char(&mut self, c: char) {
        let byte_idx = self.byte_index_at(self.cursor);
        self.buffer.insert(byte_idx, c);
        self.cursor += 1;
        self.error = None;
    }

    /// Delete the char before the cursor. No-op when the cursor is
    /// at the start of the buffer.
    pub(crate) fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev_byte = self.byte_index_at(self.cursor - 1);
        let curr_byte = self.byte_index_at(self.cursor);
        self.buffer.replace_range(prev_byte..curr_byte, "");
        self.cursor -= 1;
        self.error = None;
    }

    /// Delete the char at the cursor. No-op when the cursor is
    /// past the last char.
    pub(crate) fn delete(&mut self) {
        let cc = self.char_count();
        if self.cursor >= cc {
            return;
        }
        let curr_byte = self.byte_index_at(self.cursor);
        let next_byte = self.byte_index_at(self.cursor + 1);
        self.buffer.replace_range(curr_byte..next_byte, "");
        self.error = None;
    }

    pub(crate) fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub(crate) fn move_right(&mut self) {
        let cc = self.char_count();
        if self.cursor < cc {
            self.cursor += 1;
        }
    }

    pub(crate) fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn move_end(&mut self) {
        self.cursor = self.char_count();
    }

    /// Whether `c` should be accepted as a buffer character given
    /// `self.kind`. Used by the input handler before [`insert_char`].
    pub(crate) fn accepts_char(&self, c: char) -> bool {
        match self.kind {
            EditKind::Text => true,
            // Integer keys: only ASCII digits. The rubber-duck
            // critique chose to reject the leading `-` entirely
            // because every editable int key is non-negative.
            EditKind::Integer => c.is_ascii_digit(),
        }
    }
}

/// Handle a single keystroke while [`MenuState::OptionsEdit`] is
/// active.
pub(crate) fn handle(key: &Key, ctx: &mut InputContext) -> HandleResult {
    match *key {
        Key::Escape => cancel(ctx),
        Key::Enter => commit(ctx),
        Key::Backspace => mutate(ctx, OptionEditState::backspace),
        Key::Delete => mutate(ctx, OptionEditState::delete),
        Key::Left => mutate(ctx, OptionEditState::move_left),
        Key::Right => mutate(ctx, OptionEditState::move_right),
        Key::Home => mutate(ctx, OptionEditState::move_home),
        Key::End => mutate(ctx, OptionEditState::move_end),
        // Space is its own enum variant (not Key::Char(' ')); it
        // must be routed through the same insert path so widget
        // lists and disk filters can be typed.
        Key::Space => insert(ctx, ' '),
        Key::Char(c) => insert(ctx, c),
        // Tab, PageUp, PageDown, function keys etc. are intentionally
        // ignored: silently switching categories on Tab could surprise
        // a user who has an invalid buffer but has not yet noticed.
        _ => HandleResult::none(),
    }
}

fn cancel(ctx: &mut InputContext) -> HandleResult {
    ctx.overlay.exit_option_edit();
    redraw_options(ctx)
}

fn commit(ctx: &mut InputContext) -> HandleResult {
    let Some(edit) = ctx.overlay.option_edit() else {
        return HandleResult::none();
    };
    let key = edit.key;
    let kind = edit.kind;
    let buffer = edit.buffer.clone();

    match kind {
        EditKind::Integer => {
            let n = match key.parse_int(&buffer) {
                Ok(n) => n,
                Err(msg) => {
                    if let Some(state) = ctx.overlay.option_edit_mut() {
                        state.error = Some(msg);
                    }
                    return redraw_options(ctx);
                }
            };
            key.set_int(ctx.config, n);
            ctx.config.validate();
        }
        EditKind::Text => {
            if let Err(msg) = key.validate_string(&buffer) {
                if let Some(state) = ctx.overlay.option_edit_mut() {
                    state.error = Some(msg);
                }
                return redraw_options(ctx);
            }
            if let Err(err) = key.set_string(ctx.config, &buffer) {
                // set_string only fails on a contract violation
                // (validate_string returned Ok but the parser
                // disagreed). Surface a generic message rather
                // than panicking so the user can recover.
                if let Some(state) = ctx.overlay.option_edit_mut() {
                    state.error = Some("could not save value");
                }
                tracing::warn!(
                    subsystem = %crate::log::Subsystem::Input,
                    option = %err.key,
                    value = %err.value,
                    "set_string failed after validate_string passed",
                );
                return redraw_options(ctx);
            }
        }
    }

    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "option_edit_commit",
        option = key.name(),
        value = %buffer,
        "option committed",
    );

    let mut ops: Vec<TerminalOp> = Vec::new();
    apply_post_change_effects(key, ctx, &mut ops);

    ctx.overlay.exit_option_edit();

    let menu_out = render_options_menu(ctx);
    ops.push(TerminalOp::Synced(menu_out));
    HandleResult {
        quit: false,
        ops,
        redraw_overlay: false,
    }
}

fn insert(ctx: &mut InputContext, c: char) -> HandleResult {
    let accepted = ctx
        .overlay
        .option_edit()
        .map(|s| s.accepts_char(c))
        .unwrap_or(false);
    if !accepted {
        return HandleResult::none();
    }
    if let Some(state) = ctx.overlay.option_edit_mut() {
        state.insert_char(c);
    }
    redraw_options(ctx)
}

fn mutate<F: Fn(&mut OptionEditState)>(ctx: &mut InputContext, f: F) -> HandleResult {
    if let Some(state) = ctx.overlay.option_edit_mut() {
        f(state);
    }
    redraw_options(ctx)
}

fn redraw_options(ctx: &mut InputContext) -> HandleResult {
    let menu_out = render_options_menu(ctx);
    HandleResult::synced(menu_out)
}

fn render_options_menu(ctx: &InputContext) -> String {
    menu::options_menu::draw(&menu::options_menu::DrawParams {
        term_width: ctx.tw,
        term_height: ctx.th,
        cat: ctx.overlay.options_cat,
        selected: ctx.overlay.options_selected,
        page: ctx.overlay.options_page,
        config: ctx.config,
        theme: ctx.theme,
        option_edit: ctx.overlay.option_edit(),
    })
}

// `MenuState` transitions for entering/leaving OptionsEdit are
// enforced by `OverlayState::enter_option_edit` and
// `OverlayState::exit_option_edit`, which is why this handler
// never references `MenuState` directly.

#[cfg(test)]
mod tests {
    use super::*;

    fn state(buffer: &str) -> OptionEditState {
        OptionEditState::new(ConfigKey::ClockFormat, EditKind::Text, buffer.to_string())
    }

    fn int_state(buffer: &str) -> OptionEditState {
        OptionEditState::new(ConfigKey::UpdateMs, EditKind::Integer, buffer.to_string())
    }

    #[test]
    fn new_places_cursor_at_end() {
        let s = state("hello");
        assert_eq!(s.cursor, 5);
    }

    #[test]
    fn insert_char_at_end_appends() {
        let mut s = state("ab");
        s.insert_char('c');
        assert_eq!(s.buffer, "abc");
        assert_eq!(s.cursor, 3);
    }

    #[test]
    fn insert_char_at_middle_keeps_cursor_after_insertion() {
        let mut s = state("ac");
        s.cursor = 1;
        s.insert_char('b');
        assert_eq!(s.buffer, "abc");
        assert_eq!(s.cursor, 2);
    }

    #[test]
    fn insert_char_supports_multibyte_utf8() {
        // Cursor is a char index; a 4-byte char inserted at char 1
        // must land at byte 1 (after 'a') without splitting bytes.
        let mut s = state("ac");
        s.cursor = 1;
        s.insert_char('🦀');
        assert_eq!(s.buffer, "a🦀c");
        assert_eq!(s.cursor, 2);
    }

    #[test]
    fn backspace_removes_char_before_cursor() {
        let mut s = state("abc");
        s.backspace();
        assert_eq!(s.buffer, "ab");
        assert_eq!(s.cursor, 2);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut s = state("abc");
        s.cursor = 0;
        s.backspace();
        assert_eq!(s.buffer, "abc");
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn backspace_after_multibyte_char_removes_full_codepoint() {
        let mut s = state("a🦀c");
        s.cursor = 2; // after the crab
        s.backspace();
        assert_eq!(s.buffer, "ac");
        assert_eq!(s.cursor, 1);
    }

    #[test]
    fn delete_removes_char_at_cursor() {
        let mut s = state("abc");
        s.cursor = 1;
        s.delete();
        assert_eq!(s.buffer, "ac");
        assert_eq!(s.cursor, 1);
    }

    #[test]
    fn delete_at_end_is_noop() {
        let mut s = state("abc");
        s.delete();
        assert_eq!(s.buffer, "abc");
        assert_eq!(s.cursor, 3);
    }

    #[test]
    fn delete_removes_full_multibyte_char() {
        let mut s = state("a🦀c");
        s.cursor = 1; // on the crab
        s.delete();
        assert_eq!(s.buffer, "ac");
        assert_eq!(s.cursor, 1);
    }

    #[test]
    fn move_left_clamps_at_zero() {
        let mut s = state("ab");
        s.cursor = 0;
        s.move_left();
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn move_right_clamps_at_end() {
        let mut s = state("ab");
        s.move_right();
        assert_eq!(s.cursor, 2);
    }

    #[test]
    fn move_home_and_end() {
        let mut s = state("hello");
        s.move_home();
        assert_eq!(s.cursor, 0);
        s.move_end();
        assert_eq!(s.cursor, 5);
    }

    #[test]
    fn editing_clears_error() {
        let mut s = state("a");
        s.error = Some("bad");
        s.insert_char('b');
        assert!(s.error.is_none());

        let mut s = state("ab");
        s.error = Some("bad");
        s.backspace();
        assert!(s.error.is_none());

        let mut s = state("ab");
        s.cursor = 0;
        s.error = Some("bad");
        s.delete();
        assert!(s.error.is_none());
    }

    #[test]
    fn integer_kind_rejects_non_digits_and_minus() {
        let s = int_state("");
        assert!(s.accepts_char('0'));
        assert!(s.accepts_char('9'));
        assert!(!s.accepts_char('-'));
        assert!(!s.accepts_char('a'));
        assert!(!s.accepts_char(' '));
    }

    #[test]
    fn text_kind_accepts_anything() {
        let s = state("");
        assert!(s.accepts_char('a'));
        assert!(s.accepts_char(' '));
        assert!(s.accepts_char('!'));
        assert!(s.accepts_char('🦀'));
    }

    #[test]
    fn buffer_preserves_spaces_for_widget_lists() {
        // Regression: Key::Space is its own enum variant (not
        // Key::Char(' ')); the handler dispatch must route both
        // through insert_char so widget lists ("cpu mem") and
        // disk filters ("C: !D:") can be typed.
        let mut s = state("");
        s.insert_char('c');
        s.insert_char('p');
        s.insert_char('u');
        s.insert_char(' ');
        s.insert_char('m');
        s.insert_char('e');
        s.insert_char('m');
        assert_eq!(s.buffer, "cpu mem");
        assert_eq!(s.cursor, 7);
    }
}
