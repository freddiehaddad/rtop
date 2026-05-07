//! Per-action handlers for the help-menu overlay.

use crate::{
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
    input::Key,
};

pub(super) fn close_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let return_to = ctx.overlay.menu_return_to;
    ctx.overlay.set_menu_state(return_to);
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "help",
        opened = false,
        "menu transition",
    );
    if return_to == MenuState::None {
        ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
    }
    HandleResult::redraw()
}
