//! Per-action handlers for the process-filter text-input state.
//!
//! [`MenuState::Filter`] consumes printable characters via the
//! dispatcher's text-input fallback ([`fallback_typed_char`]); the
//! command keys (Esc, Enter, Backspace, Delete) live as bindings in
//! [`crate::handlers::keybinds::BINDINGS`].

use crate::{
    handlers::{HandleResult, InputContext, MenuState},
    input::Key,
};

pub(super) fn cancel_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.overlay.set_menu_state(MenuState::None);
    ctx.render.dirty.mark_proc_data_changed();
    HandleResult::none()
}

pub(super) fn commit_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.view.proc_filter = ctx.process.filter_text.clone();
    ctx.overlay.set_menu_state(MenuState::None);
    ctx.process.selected = 0;
    ctx.process.start = 0;
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "filter_commit",
        filter = %ctx.view.proc_filter,
        "filter applied",
    );
    ctx.render.dirty.mark_proc_data_changed();
    HandleResult::none()
}

pub(super) fn backspace_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.process.filter_text.pop();
    ctx.view.proc_filter = ctx.process.filter_text.clone();
    ctx.process.selected = 0;
    ctx.process.start = 0;
    ctx.render.dirty.mark_proc_data_changed();
    HandleResult::none()
}

pub(super) fn delete_clear_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
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
    HandleResult::none()
}

/// Dispatcher fallback for [`MenuState::Filter`]. Any key whose
/// [`Key::typed_char`] is `Some(c)` is appended to the filter
/// buffer; any other key is consumed as a no-op.
pub(crate) fn fallback_typed_char(key: &Key, ctx: &mut InputContext) -> HandleResult {
    if let Some(c) = key.typed_char() {
        ctx.process.filter_text.push(c);
        ctx.view.proc_filter = ctx.process.filter_text.clone();
        ctx.process.selected = 0;
        ctx.process.start = 0;
        ctx.render.dirty.mark_proc_data_changed();
    }
    HandleResult::none()
}
