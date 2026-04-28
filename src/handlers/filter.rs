use crate::{
    config_keys::str_keys as sk,
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
};

/// Handle input while the process-filter text field is active.
pub(crate) fn handle(key: &str, ctx: &mut InputContext) -> HandleResult {
    match key {
        "escape" => {
            ctx.overlay.menu_state = MenuState::None;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "enter" => {
            ctx.config
                .set_string(sk::PROC_FILTER, &ctx.process.filter_text);
            ctx.overlay.menu_state = MenuState::None;
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "backspace" => {
            ctx.process.filter_text.pop();
            ctx.config
                .set_string(sk::PROC_FILTER, &ctx.process.filter_text);
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "delete" => {
            ctx.process.filter_text.clear();
            ctx.config.set_string(sk::PROC_FILTER, "");
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        s if s.len() == 1 && !s.starts_with('\x1b') => {
            ctx.process.filter_text.push_str(s);
            ctx.config
                .set_string(sk::PROC_FILTER, &ctx.process.filter_text);
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        _ => {}
    }
    HandleResult::none()
}
