use crate::{
    dirty::Dirty,
    handlers::{InputContext, MenuState, redraw_after_overlay},
};

/// Handle input while the help overlay is visible.
pub(crate) fn handle(key: &str, ctx: &mut InputContext) -> bool {
    match key {
        "q" => return true,
        "escape" | "h" | "?" | "f1" => {
            let return_to = *ctx.menu_return_to;
            *ctx.menu_state = return_to;
            let out = redraw_after_overlay(ctx);
            let _ = ctx.terminal.write_synced(&out);
            if return_to == MenuState::None {
                *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
            }
        }
        _ => {}
    }
    false
}
