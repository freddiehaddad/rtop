use crate::{
    config_keys::str_keys as sk,
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
};

/// Handle input while the process-filter text field is active.
pub(crate) fn handle(key: &str, ctx: &mut InputContext) -> HandleResult {
    match key {
        "escape" => {
            *ctx.menu_state = MenuState::None;
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "enter" => {
            ctx.config.set_string(sk::PROC_FILTER, ctx.filter_text);
            *ctx.menu_state = MenuState::None;
            *ctx.proc_selected = 0;
            *ctx.proc_start = 0;
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "backspace" => {
            ctx.filter_text.pop();
            ctx.config.set_string(sk::PROC_FILTER, ctx.filter_text);
            *ctx.proc_selected = 0;
            *ctx.proc_start = 0;
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "delete" => {
            ctx.filter_text.clear();
            ctx.config.set_string(sk::PROC_FILTER, "");
            *ctx.proc_selected = 0;
            *ctx.proc_start = 0;
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        s if s.len() == 1 && !s.starts_with('\x1b') => {
            ctx.filter_text.push_str(s);
            ctx.config.set_string(sk::PROC_FILTER, ctx.filter_text);
            *ctx.proc_selected = 0;
            *ctx.proc_start = 0;
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        _ => {}
    }
    HandleResult::none()
}
