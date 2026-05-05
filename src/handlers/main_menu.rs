use crate::{
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
    input::Key,
    menu,
};

/// Handle input while the main menu overlay is visible.
pub(crate) fn handle(key: &Key, ctx: &mut InputContext) -> HandleResult {
    match *key {
        Key::Char('q') => return HandleResult::quit(),
        Key::Escape | Key::Char('m') => {
            ctx.overlay.set_menu_state(MenuState::None);
            tracing::debug!(
                subsystem = %crate::log::Subsystem::Ui,
                menu = "main",
                opened = false,
                "menu transition",
            );
            ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
        }
        Key::Up | Key::ShiftTab => {
            if ctx.config.vim_keys && matches!(key, Key::Up) {
                // Up always works
            }
            ctx.overlay.main_menu_selected = if ctx.overlay.main_menu_selected == 0 {
                2
            } else {
                ctx.overlay.main_menu_selected - 1
            };
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                ctx.overlay.main_menu_selected,
                ctx.theme,
            );
            return HandleResult::synced(menu_out);
        }
        Key::Char('k') if ctx.config.vim_keys => {
            ctx.overlay.main_menu_selected = if ctx.overlay.main_menu_selected == 0 {
                2
            } else {
                ctx.overlay.main_menu_selected - 1
            };
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                ctx.overlay.main_menu_selected,
                ctx.theme,
            );
            return HandleResult::synced(menu_out);
        }
        Key::Down | Key::Tab => {
            ctx.overlay.main_menu_selected = (ctx.overlay.main_menu_selected + 1) % 3;
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                ctx.overlay.main_menu_selected,
                ctx.theme,
            );
            return HandleResult::synced(menu_out);
        }
        Key::Char('j') if ctx.config.vim_keys => {
            ctx.overlay.main_menu_selected = (ctx.overlay.main_menu_selected + 1) % 3;
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                ctx.overlay.main_menu_selected,
                ctx.theme,
            );
            return HandleResult::synced(menu_out);
        }
        Key::Enter | Key::Space => {
            match ctx.overlay.main_menu_selected {
                0 => {
                    // Options
                    ctx.overlay.options_cat = 0;
                    ctx.overlay.options_selected = 0;
                    ctx.overlay.options_page = 0;
                    let menu_out = menu::options_menu::draw(
                        ctx.tw,
                        ctx.th,
                        ctx.overlay.options_cat,
                        ctx.overlay.options_selected,
                        ctx.overlay.options_page,
                        ctx.config,
                        ctx.theme,
                    );
                    ctx.overlay.menu_return_to = MenuState::Main;
                    ctx.overlay.set_menu_state(MenuState::Options);
                    tracing::debug!(
                        subsystem = %crate::log::Subsystem::Ui,
                        menu = "options",
                        opened = true,
                        "menu transition",
                    );
                    return HandleResult::raw(menu_out);
                }
                1 => {
                    // Help
                    let menu_out =
                        menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, ctx.runtime.rounded);
                    ctx.overlay.menu_return_to = MenuState::Main;
                    ctx.overlay.set_menu_state(MenuState::Help);
                    tracing::debug!(
                        subsystem = %crate::log::Subsystem::Ui,
                        menu = "help",
                        opened = true,
                        "menu transition",
                    );
                    return HandleResult::raw(menu_out);
                }
                2 => {
                    // Quit
                    return HandleResult::quit();
                }
                _ => {}
            }
        }
        Key::Char('o') | Key::F(2) => {
            ctx.overlay.options_cat = 0;
            ctx.overlay.options_selected = 0;
            ctx.overlay.options_page = 0;
            let menu_out = menu::options_menu::draw(
                ctx.tw,
                ctx.th,
                ctx.overlay.options_cat,
                ctx.overlay.options_selected,
                ctx.overlay.options_page,
                ctx.config,
                ctx.theme,
            );
            ctx.overlay.menu_return_to = MenuState::Main;
            ctx.overlay.set_menu_state(MenuState::Options);
            tracing::debug!(
                subsystem = %crate::log::Subsystem::Ui,
                menu = "options",
                opened = true,
                "menu transition",
            );
            return HandleResult::raw(menu_out);
        }
        Key::Char('?') | Key::F(1) => {
            let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, ctx.runtime.rounded);
            ctx.overlay.menu_return_to = MenuState::Main;
            ctx.overlay.set_menu_state(MenuState::Help);
            tracing::debug!(
                subsystem = %crate::log::Subsystem::Ui,
                menu = "help",
                opened = true,
                "menu transition",
            );
            return HandleResult::raw(menu_out);
        }
        _ => {}
    }
    HandleResult::none()
}
