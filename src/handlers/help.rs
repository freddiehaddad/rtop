use crate::{
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
};

/// Handle input while the help overlay is visible.
pub(crate) fn handle(key: &str, ctx: &mut InputContext) -> HandleResult {
    match key {
        "q" => return HandleResult::quit(),
        "escape" | "h" | "?" | "f1" => {
            let return_to = ctx.overlay.menu_return_to;
            ctx.overlay.menu_state = return_to;
            if return_to == MenuState::None {
                ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
            }
            return HandleResult::redraw();
        }
        _ => {}
    }
    HandleResult::none()
}
