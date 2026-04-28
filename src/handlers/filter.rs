use crate::{
    config_keys::str_keys as sk,
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
    input::Key,
};

/// Handle input while the process-filter text field is active.
pub(crate) fn handle(key: &Key, ctx: &mut InputContext) -> HandleResult {
    match *key {
        Key::Escape => {
            ctx.overlay.menu_state = MenuState::None;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Enter => {
            ctx.config
                .set_string(sk::PROC_FILTER, &ctx.process.filter_text);
            ctx.overlay.menu_state = MenuState::None;
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Backspace => {
            ctx.process.filter_text.pop();
            ctx.config
                .set_string(sk::PROC_FILTER, &ctx.process.filter_text);
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Delete => {
            ctx.process.filter_text.clear();
            ctx.config.set_string(sk::PROC_FILTER, "");
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Char(c) => {
            ctx.process.filter_text.push(c);
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
