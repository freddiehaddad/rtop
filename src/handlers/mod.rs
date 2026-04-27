pub(crate) mod filter;
pub(crate) mod help;
pub(crate) mod main_menu;
pub(crate) mod normal;
pub(crate) mod options;

use crate::{config, dirty::Dirty, draw, runner, term, theme};

/// The current menu overlay state.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum MenuState {
    None,
    Main,
    Help,
    Options,
    Filter,
}

/// Shared mutable state passed to each per-MenuState input handler.
pub(crate) struct InputContext<'a> {
    pub(crate) config: &'a mut config::Config,
    pub(crate) terminal: &'a mut term::Terminal,
    pub(crate) theme: &'a mut theme::Theme,
    pub(crate) runner: &'a mut runner::Runner,
    pub(crate) menu_state: &'a mut MenuState,
    pub(crate) dirty: &'a mut Dirty,
    pub(crate) rounded: &'a mut bool,
    pub(crate) update_ms: &'a mut u64,
    pub(crate) main_menu_selected: &'a mut usize,
    pub(crate) options_cat: &'a mut usize,
    pub(crate) options_selected: &'a mut usize,
    pub(crate) options_page: &'a mut usize,
    pub(crate) proc_selected: &'a mut usize,
    pub(crate) proc_start: &'a mut usize,
    pub(crate) filter_text: &'a mut String,
    pub(crate) cached_layout: &'a Option<draw::layout::Layout>,
    /// Where Options/Help was opened from — return here on escape.
    pub(crate) menu_return_to: &'a mut MenuState,
    pub(crate) tw: usize,
    pub(crate) th: usize,
}

/// Redraw the underlying UI after closing a menu overlay.
///
/// Clears the screen, renders all boxes, and optionally re-draws the
/// main menu if we're returning to it. Returns the output string.
pub(crate) fn redraw_after_overlay(ctx: &mut InputContext) -> String {
    use crate::app::{RenderParams, render_all};

    let mut out = String::new();
    if let Some(layout) = ctx.cached_layout.as_ref() {
        let params = RenderParams {
            dirty: Dirty::ALL_BOXES,
            layout,
            runner: ctx.runner,
            config: ctx.config,
            theme: ctx.theme,
            rounded: *ctx.rounded,
            update_ms: *ctx.update_ms,
            is_filtering: false,
        };
        out.push_str("\x1b[2J");
        out.push_str(&render_all(&params, ctx.proc_selected, ctx.proc_start));
        if *ctx.menu_return_to == MenuState::Main {
            out.push_str(&crate::menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                *ctx.main_menu_selected,
                ctx.theme,
            ));
        }
    }
    out
}
