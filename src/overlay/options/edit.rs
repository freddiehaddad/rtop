//! Inline editor state for the options overlay.
//!
//! When the user activates an editable option (e.g. `update_ms`,
//! `clock_format`, `custom_cpu_name`), an [`OptionEditState`] is
//! constructed and attached to the parent [`super::OptionsState`].
//! While `edit.is_some()`, the overlay is in *edit mode* and key
//! dispatch routes to the edit handlers; on commit/cancel the
//! buffer is taken back out and (if committed) applied to config.
//!
//! `cursor` is a **char index** (not a byte offset) into `buffer`.
//! All mutating helpers maintain UTF-8 boundary correctness via
//! `char_indices` lookups, so a non-ASCII character in the buffer
//! cannot land the cursor on a partial char or panic on slicing.
//!
//! Per-key actions and the renderer are added in later stages of
//! the refactor.

use crate::config::ConfigKey;

/// Whether the buffer represents free-form text or an integer value.
///
/// Drives the typed-character filter (`Integer` rejects everything
/// that is not an ASCII digit) and the commit-time validation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditKind {
    Text,
    Integer,
}

/// Mutable state for an in-progress inline edit.
#[derive(Debug, Clone)]
pub struct OptionEditState {
    key: ConfigKey,
    kind: EditKind,
    buffer: String,
    cursor: usize,
    error: Option<&'static str>,
}

impl OptionEditState {
    /// Create a new edit state for `key` with `buffer` as the
    /// initial value. Cursor lands at end-of-buffer so the user can
    /// keep typing or backspace to clear without an extra keystroke.
    pub fn new(key: ConfigKey, kind: EditKind, buffer: String) -> Self {
        let cursor = buffer.chars().count();
        Self {
            key,
            kind,
            buffer,
            cursor,
            error: None,
        }
    }

    /// The config key being edited.
    pub fn key(&self) -> ConfigKey {
        self.key
    }

    /// Whether the buffer is text or integer.
    pub fn kind(&self) -> EditKind {
        self.kind
    }

    /// Borrow the buffer (for rendering or commit-time validation).
    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    /// Cursor position as a char index into `buffer`.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Last commit-validation error, if any.
    pub fn error(&self) -> Option<&'static str> {
        self.error
    }

    /// Set the validation error message (called on a failed commit).
    pub fn set_error(&mut self, error: Option<&'static str>) {
        self.error = error;
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

    /// Insert `c` at the cursor and advance by one char. Clears
    /// `error` because the buffer just changed.
    pub fn insert_char(&mut self, c: char) {
        let byte_idx = self.byte_index_at(self.cursor);
        self.buffer.insert(byte_idx, c);
        self.cursor += 1;
        self.error = None;
    }

    /// Delete the char before the cursor. No-op when the cursor is
    /// at the start of the buffer.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev_byte = self.byte_index_at(self.cursor - 1);
        let curr_byte = self.byte_index_at(self.cursor);
        self.buffer.replace_range(prev_byte..curr_byte, "");
        self.cursor -= 1;
        self.error = None;
    }

    /// Delete the char at the cursor. No-op when the cursor is past
    /// the last char.
    pub fn delete(&mut self) {
        let cc = self.char_count();
        if self.cursor >= cc {
            return;
        }
        let curr_byte = self.byte_index_at(self.cursor);
        let next_byte = self.byte_index_at(self.cursor + 1);
        self.buffer.replace_range(curr_byte..next_byte, "");
        self.error = None;
    }

    /// Move cursor one char to the left, clamped at start.
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor one char to the right, clamped at end-of-buffer.
    pub fn move_right(&mut self) {
        let cc = self.char_count();
        if self.cursor < cc {
            self.cursor += 1;
        }
    }

    /// Move cursor to the start of the buffer.
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor past the last char of the buffer.
    pub fn move_end(&mut self) {
        self.cursor = self.char_count();
    }

    /// Whether `c` should be accepted as a buffer character given
    /// `self.kind`. Used by the input handler before [`insert_char`].
    pub fn accepts_char(&self, c: char) -> bool {
        match self.kind {
            EditKind::Text => true,
            // Integer keys: only ASCII digits. Every editable int
            // key in the options menu is non-negative, so the
            // leading `-` is rejected entirely.
            EditKind::Integer => c.is_ascii_digit(),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-action handlers (referenced by handlers/keybinds/table.rs)
// ---------------------------------------------------------------------------

use crate::handlers::InputContext;
use crate::input::Key;
use crate::overlay::ActiveModal;

/// Handler for the `Esc` keybinding while editing: discards the
/// in-progress edit and returns to the options menu without
/// committing any change.
pub(crate) fn cancel_action(ctx: &mut InputContext, _key: &Key) {
    if let ActiveModal::Options(s) = &mut ctx.overlay.active {
        let _ = s.exit_edit();
    }
    ctx.render.dirty.mark_overlay();
}

/// Handler for the `Enter` keybinding: validate the buffer, commit
/// the value via the appropriate typed sub-enum's `set` /
/// `set_canonical`, run [`super::apply_post_change_effects`], and
/// return to the options menu. On validation failure, attaches an
/// error to the edit state and stays in the editor.
pub(crate) fn commit_action(ctx: &mut InputContext, _key: &Key) {
    let (key, kind, buffer) = {
        let ActiveModal::Options(s) = &ctx.overlay.active else {
            return;
        };
        let Some(edit) = s.edit() else {
            return;
        };
        (edit.key(), edit.kind(), edit.buffer().to_string())
    };

    match kind {
        EditKind::Integer => {
            // Inline integer editor only opens for `IntKey` options;
            // the kind dispatch above guarantees this.
            let ConfigKey::Int(int_key) = key else {
                return;
            };
            let n = match int_key.parse(&buffer) {
                Ok(n) => n,
                Err(msg) => {
                    set_edit_error(ctx, Some(msg));
                    ctx.render.dirty.mark_overlay();
                    return;
                }
            };
            int_key.set(ctx.config, n);
            ctx.config.validate();
        }
        EditKind::Text => {
            // Inline text editor opens for `StringKey` and `EnumKey`;
            // dispatch by the wrapper variant.
            match key {
                ConfigKey::String(string_key) => {
                    if let Err(msg) = string_key.validate(&buffer) {
                        set_edit_error(ctx, Some(msg));
                        ctx.render.dirty.mark_overlay();
                        return;
                    }
                    if let Err(err) = string_key.set(ctx.config, &buffer) {
                        // set() only fails on a contract violation
                        // (validate returned Ok but the parser
                        // disagreed). Surface a generic message
                        // rather than panicking so the user can
                        // recover.
                        set_edit_error(ctx, Some("could not save value"));
                        tracing::warn!(
                            subsystem = %crate::log::Subsystem::Input,
                            option = %err.key,
                            value = %err.value,
                            "StringKey::set failed after validate passed",
                        );
                        ctx.render.dirty.mark_overlay();
                        return;
                    }
                }
                ConfigKey::Enum(enum_key) => {
                    if let Err(err) = enum_key.set_canonical(ctx.config, &buffer) {
                        set_edit_error(ctx, Some("invalid value"));
                        tracing::warn!(
                            subsystem = %crate::log::Subsystem::Input,
                            option = %err.key,
                            value = %err.value,
                            "EnumKey::set_canonical failed",
                        );
                        ctx.render.dirty.mark_overlay();
                        return;
                    }
                }
                ConfigKey::Bool(_) | ConfigKey::Int(_) => {
                    // Inline text editor never opens for Bool / Int —
                    // those go through arrow-step or the integer
                    // editor. Reaching here is a logic bug.
                    set_edit_error(ctx, Some("not a string-typed option"));
                    ctx.render.dirty.mark_overlay();
                    return;
                }
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

    super::apply_post_change_effects(key, ctx);
    if let ActiveModal::Options(s) = &mut ctx.overlay.active {
        let _ = s.exit_edit();
    }
    ctx.render.dirty.mark_overlay();
}

pub(crate) fn backspace_action(ctx: &mut InputContext, _key: &Key) {
    mutate(ctx, OptionEditState::backspace);
}

pub(crate) fn delete_action(ctx: &mut InputContext, _key: &Key) {
    mutate(ctx, OptionEditState::delete);
}

pub(crate) fn move_left_action(ctx: &mut InputContext, _key: &Key) {
    mutate(ctx, OptionEditState::move_left);
}

pub(crate) fn move_right_action(ctx: &mut InputContext, _key: &Key) {
    mutate(ctx, OptionEditState::move_right);
}

pub(crate) fn move_home_action(ctx: &mut InputContext, _key: &Key) {
    mutate(ctx, OptionEditState::move_home);
}

pub(crate) fn move_end_action(ctx: &mut InputContext, _key: &Key) {
    mutate(ctx, OptionEditState::move_end);
}

/// Dispatcher fallback for the inline-editor overlay. Any key
/// whose [`Key::typed_char`] is `Some(c)` is offered to the active
/// edit state's `accepts_char` filter; if accepted, the char is
/// inserted at the cursor. Tab, PageUp, PageDown, function keys,
/// and other non-printable keys fall through as no-ops.
///
/// Tab is intentionally ignored: silently switching options
/// categories on Tab could surprise a user who has an invalid
/// buffer but has not yet noticed.
pub(crate) fn fallback_typed_char(key: &Key, ctx: &mut InputContext) {
    if let Some(c) = key.typed_char() {
        insert(ctx, c);
    }
}

fn insert(ctx: &mut InputContext, c: char) {
    let accepted = match &ctx.overlay.active {
        ActiveModal::Options(s) => s.edit().is_some_and(|e| e.accepts_char(c)),
        _ => false,
    };
    if !accepted {
        return;
    }
    if let ActiveModal::Options(s) = &mut ctx.overlay.active
        && let Some(state) = s.edit_mut()
    {
        state.insert_char(c);
    }
    ctx.render.dirty.mark_overlay();
}

fn mutate<F: Fn(&mut OptionEditState)>(ctx: &mut InputContext, f: F) {
    if let ActiveModal::Options(s) = &mut ctx.overlay.active
        && let Some(state) = s.edit_mut()
    {
        f(state);
    }
    ctx.render.dirty.mark_overlay();
}

fn set_edit_error(ctx: &mut InputContext, error: Option<&'static str>) {
    if let ActiveModal::Options(s) = &mut ctx.overlay.active
        && let Some(state) = s.edit_mut()
    {
        state.set_error(error);
    }
}

#[cfg(test)]
impl OptionEditState {
    /// Construct a placeholder edit state for tests that exercise
    /// the enclosing `OptionsState`'s edit transitions but do not
    /// care which config key is being edited.
    pub fn placeholder() -> Self {
        Self::new(
            ConfigKey::String(crate::config::StringKey::ProcFilter),
            EditKind::Text,
            String::new(),
        )
    }

    /// Test-only setter for the cursor position. Lets test setup
    /// place the cursor at an arbitrary index without composing a
    /// long sequence of `move_left` / `move_right` calls.
    /// Production code positions the cursor through the typed
    /// movement methods.
    pub(crate) fn set_cursor_for_test(&mut self, cursor: usize) {
        self.cursor = cursor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{IntKey, StringKey};

    fn state(buffer: &str) -> OptionEditState {
        OptionEditState::new(
            ConfigKey::String(StringKey::ClockFormat),
            EditKind::Text,
            buffer.to_string(),
        )
    }

    fn int_state(buffer: &str) -> OptionEditState {
        OptionEditState::new(
            ConfigKey::Int(IntKey::UpdateMs),
            EditKind::Integer,
            buffer.to_string(),
        )
    }

    #[test]
    fn new_places_cursor_at_end() {
        let s = state("hello");
        assert_eq!(s.cursor(), 5);
    }

    #[test]
    fn insert_char_at_end_appends() {
        let mut s = state("ab");
        s.insert_char('c');
        assert_eq!(s.buffer(), "abc");
        assert_eq!(s.cursor(), 3);
    }

    #[test]
    fn insert_char_at_middle_keeps_cursor_after_insertion() {
        let mut s = state("ac");
        s.set_cursor_for_test(1);
        s.insert_char('b');
        assert_eq!(s.buffer(), "abc");
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn insert_char_supports_multibyte_utf8() {
        // Cursor is a char index; a 4-byte char inserted at char 1
        // must land at byte 1 (after 'a') without splitting bytes.
        let mut s = state("ac");
        s.set_cursor_for_test(1);
        s.insert_char('🦀');
        assert_eq!(s.buffer(), "a🦀c");
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn backspace_removes_char_before_cursor() {
        let mut s = state("abc");
        s.backspace();
        assert_eq!(s.buffer(), "ab");
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut s = state("abc");
        s.set_cursor_for_test(0);
        s.backspace();
        assert_eq!(s.buffer(), "abc");
        assert_eq!(s.cursor(), 0);
    }

    #[test]
    fn backspace_after_multibyte_char_removes_full_codepoint() {
        let mut s = state("a🦀c");
        s.set_cursor_for_test(2); // after the crab
        s.backspace();
        assert_eq!(s.buffer(), "ac");
        assert_eq!(s.cursor(), 1);
    }

    #[test]
    fn delete_removes_char_at_cursor() {
        let mut s = state("abc");
        s.set_cursor_for_test(1);
        s.delete();
        assert_eq!(s.buffer(), "ac");
        assert_eq!(s.cursor(), 1);
    }

    #[test]
    fn delete_at_end_is_noop() {
        let mut s = state("abc");
        s.delete();
        assert_eq!(s.buffer(), "abc");
        assert_eq!(s.cursor(), 3);
    }

    #[test]
    fn delete_removes_full_multibyte_char() {
        let mut s = state("a🦀c");
        s.set_cursor_for_test(1); // on the crab
        s.delete();
        assert_eq!(s.buffer(), "ac");
        assert_eq!(s.cursor(), 1);
    }

    #[test]
    fn move_left_clamps_at_zero() {
        let mut s = state("ab");
        s.set_cursor_for_test(0);
        s.move_left();
        assert_eq!(s.cursor(), 0);
    }

    #[test]
    fn move_right_clamps_at_end() {
        let mut s = state("ab");
        s.move_right();
        assert_eq!(s.cursor(), 2);
    }

    #[test]
    fn move_home_and_end() {
        let mut s = state("hello");
        s.move_home();
        assert_eq!(s.cursor(), 0);
        s.move_end();
        assert_eq!(s.cursor(), 5);
    }

    #[test]
    fn editing_clears_error() {
        let mut s = state("a");
        s.set_error(Some("bad"));
        s.insert_char('b');
        assert!(s.error().is_none());

        let mut s = state("ab");
        s.set_error(Some("bad"));
        s.backspace();
        assert!(s.error().is_none());

        let mut s = state("ab");
        s.set_cursor_for_test(0);
        s.set_error(Some("bad"));
        s.delete();
        assert!(s.error().is_none());
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
        assert_eq!(s.buffer(), "cpu mem");
        assert_eq!(s.cursor(), 7);
    }
}
