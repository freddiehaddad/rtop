//! Process-filter overlay subsystem: state and per-key actions.
//!
//! The filter overlay is an inline, single-line text input drawn at
//! the bottom of the proc widget — *not* a centered modal. It does
//! not dim the underlay. The in-progress text buffer lives in
//! `ProcessViewState::filter_text` (the proc widget reads it
//! directly from there); this struct is the typed variant marker
//! so [`super::ActiveModal`] can carry the same shape as the other
//! modal states.

use crate::handlers::InputContext;
use crate::input::Key;

/// Marker state for an active filter overlay.
#[derive(Debug, Clone, Default)]
pub struct FilterState;

// ---------------------------------------------------------------------------
// Per-action handlers (referenced by handlers/keybinds/table.rs)
// ---------------------------------------------------------------------------

pub(crate) fn cancel_action(ctx: &mut InputContext, _key: &Key) {
    ctx.close_overlay();
    ctx.render.dirty.mark_proc_data_changed();
}

pub(crate) fn commit_action(ctx: &mut InputContext, _key: &Key) {
    ctx.view.proc_filter = ctx.process.filter_text.clone();
    ctx.close_overlay();
    ctx.process.selected = 0;
    ctx.process.start = 0;
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "filter_commit",
        filter = %ctx.view.proc_filter,
        "filter applied",
    );
    ctx.render.dirty.mark_proc_data_changed();
}

pub(crate) fn backspace_action(ctx: &mut InputContext, _key: &Key) {
    ctx.process.filter_text.pop();
    ctx.view.proc_filter = ctx.process.filter_text.clone();
    ctx.process.selected = 0;
    ctx.process.start = 0;
    ctx.render.dirty.mark_proc_data_changed();
}

pub(crate) fn delete_clear_action(ctx: &mut InputContext, _key: &Key) {
    ctx.process.filter_text.clear();
    ctx.view.proc_filter.clear();
    ctx.process.selected = 0;
    ctx.process.start = 0;
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "filter_clear",
        "filter cleared",
    );
    ctx.render.dirty.mark_proc_data_changed();
}

/// Dispatcher fallback for the filter overlay. Any key whose
/// [`Key::typed_char`] is `Some(c)` is appended to the filter
/// buffer; any other key is consumed as a no-op.
pub(crate) fn fallback_typed_char(key: &Key, ctx: &mut InputContext) {
    if let Some(c) = key.typed_char() {
        ctx.process.filter_text.push(c);
        ctx.view.proc_filter = ctx.process.filter_text.clone();
        ctx.process.selected = 0;
        ctx.process.start = 0;
        ctx.render.dirty.mark_proc_data_changed();
    }
}
