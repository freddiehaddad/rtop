//! Per-action handlers for the main-menu overlay.

use crate::{
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
    input::Key,
    menu,
};

const MAIN_MENU_ITEM_COUNT: usize = 3;

pub(super) fn quit_action(_ctx: &mut InputContext, _key: &Key) -> HandleResult {
    HandleResult::quit()
}

pub(super) fn close_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.overlay.set_menu_state(MenuState::None);
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "main",
        opened = false,
        "menu transition",
    );
    ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
    HandleResult::none()
}

pub(super) fn select_prev_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.overlay.main_menu_selected = if ctx.overlay.main_menu_selected == 0 {
        MAIN_MENU_ITEM_COUNT - 1
    } else {
        ctx.overlay.main_menu_selected - 1
    };
    redraw(ctx)
}

pub(super) fn select_next_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.overlay.main_menu_selected = (ctx.overlay.main_menu_selected + 1) % MAIN_MENU_ITEM_COUNT;
    redraw(ctx)
}

pub(super) fn activate_selected_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    match ctx.overlay.main_menu_selected {
        0 => open_options_from_main(ctx),
        1 => open_help_from_main(ctx),
        2 => HandleResult::quit(),
        _ => HandleResult::none(),
    }
}

pub(super) fn open_options_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    open_options_from_main(ctx)
}

pub(super) fn open_help_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    open_help_from_main(ctx)
}

fn redraw(ctx: &mut InputContext) -> HandleResult {
    let menu_out = menu::main_menu::draw_with_selection(
        ctx.tw,
        ctx.th,
        ctx.overlay.main_menu_selected,
        ctx.theme,
    );
    HandleResult::synced(menu_out)
}

fn open_options_from_main(ctx: &mut InputContext) -> HandleResult {
    ctx.overlay.options_cat = 0;
    ctx.overlay.options_selected = 0;
    ctx.overlay.options_page = 0;
    // Sync RuntimeView -> config.view so the menu shows current
    // values for runtime-toggle keys.
    ctx.view.sync_to_config(&mut ctx.config.view);
    let menu_out = menu::options_menu::draw(&menu::options_menu::DrawParams {
        term_width: ctx.tw,
        term_height: ctx.th,
        cat: ctx.overlay.options_cat,
        selected: ctx.overlay.options_selected,
        page: ctx.overlay.options_page,
        config: ctx.config,
        theme: ctx.theme,
        option_edit: None,
    });
    ctx.overlay.menu_return_to = MenuState::Main;
    ctx.overlay.set_menu_state(MenuState::Options);
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "options",
        opened = true,
        "menu transition",
    );
    HandleResult::raw(menu_out)
}

fn open_help_from_main(ctx: &mut InputContext) -> HandleResult {
    let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, ctx.runtime.rounded);
    ctx.overlay.menu_return_to = MenuState::Main;
    ctx.overlay.set_menu_state(MenuState::Help);
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "help",
        opened = true,
        "menu transition",
    );
    HandleResult::raw(menu_out)
}
