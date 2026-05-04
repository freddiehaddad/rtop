use crate::{
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
    input::Key,
};

/// Handle input while the help overlay is visible.
pub(crate) fn handle(key: &Key, ctx: &mut InputContext) -> HandleResult {
    match *key {
        Key::Char('q') => return HandleResult::quit(),
        Key::Escape | Key::Char('?') | Key::F(1) => {
            let return_to = ctx.overlay.menu_return_to;
            ctx.overlay.set_menu_state(return_to);
            tracing::debug!(
                subsystem = %crate::log::Subsystem::Ui,
                menu = "help",
                opened = false,
                "menu transition",
            );
            if return_to == MenuState::None {
                ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
            }
            return HandleResult::redraw();
        }
        _ => {}
    }
    HandleResult::none()
}
