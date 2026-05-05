use crate::{
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
    input::Key,
};

/// Handle input while the process-filter text field is active.
pub(crate) fn handle(key: &Key, ctx: &mut InputContext) -> HandleResult {
    match *key {
        Key::Escape => {
            ctx.overlay.set_menu_state(MenuState::None);
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
        }
        Key::Enter => {
            ctx.config.proc_filter = ctx.process.filter_text.clone();
            ctx.overlay.set_menu_state(MenuState::None);
            ctx.process.selected = 0;
            ctx.process.start = 0;
            tracing::info!(
                subsystem = %crate::log::Subsystem::Input,
                action = "filter_commit",
                filter = %ctx.config.proc_filter,
                "filter applied",
            );
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
        }
        Key::Backspace => {
            ctx.process.filter_text.pop();
            ctx.config.proc_filter = ctx.process.filter_text.clone();
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
        }
        Key::Delete => {
            ctx.process.filter_text.clear();
            ctx.config.proc_filter.clear();
            ctx.process.selected = 0;
            ctx.process.start = 0;
            tracing::info!(
                subsystem = %crate::log::Subsystem::Input,
                action = "filter_clear",
                "filter cleared",
            );
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
        }
        // Any key that maps to a typed character (regular Char or
        // the standalone Space variant) appends to the buffer; any
        // other key is ignored. See `Key::typed_char` for the
        // bridge that prevents Space from silently being dropped.
        other => {
            if let Some(c) = other.typed_char() {
                ctx.process.filter_text.push(c);
                ctx.config.proc_filter = ctx.process.filter_text.clone();
                ctx.process.selected = 0;
                ctx.process.start = 0;
                ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
            }
        }
    }
    HandleResult::none()
}
