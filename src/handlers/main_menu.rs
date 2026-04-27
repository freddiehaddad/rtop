use crate::{
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
    menu,
};

/// Handle input while the main menu overlay is visible.
pub(crate) fn handle(key: &str, ctx: &mut InputContext) -> HandleResult {
    match key {
        "q" => return HandleResult::quit(),
        "escape" | "m" => {
            *ctx.menu_state = MenuState::None;
            *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        "up" | "k" | "shift_tab" => {
            *ctx.main_menu_selected = if *ctx.main_menu_selected == 0 {
                2
            } else {
                *ctx.main_menu_selected - 1
            };
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                *ctx.main_menu_selected,
                ctx.theme,
            );
            return HandleResult::synced(menu_out);
        }
        "down" | "j" | "tab" => {
            *ctx.main_menu_selected = (*ctx.main_menu_selected + 1) % 3;
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                *ctx.main_menu_selected,
                ctx.theme,
            );
            return HandleResult::synced(menu_out);
        }
        "enter" | "space" => {
            match *ctx.main_menu_selected {
                0 => {
                    // Options
                    *ctx.options_cat = 0;
                    *ctx.options_selected = 0;
                    *ctx.options_page = 0;
                    let menu_out = menu::options_menu::draw(
                        ctx.tw,
                        ctx.th,
                        *ctx.options_cat,
                        *ctx.options_selected,
                        *ctx.options_page,
                        ctx.config,
                        ctx.theme,
                    );
                    *ctx.menu_return_to = MenuState::Main;
                    *ctx.menu_state = MenuState::Options;
                    return HandleResult::raw(menu_out);
                }
                1 => {
                    // Help
                    let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, *ctx.rounded);
                    *ctx.menu_return_to = MenuState::Main;
                    *ctx.menu_state = MenuState::Help;
                    return HandleResult::raw(menu_out);
                }
                2 => {
                    // Quit
                    return HandleResult::quit();
                }
                _ => {}
            }
        }
        "o" | "f2" => {
            *ctx.options_cat = 0;
            *ctx.options_selected = 0;
            *ctx.options_page = 0;
            let menu_out = menu::options_menu::draw(
                ctx.tw,
                ctx.th,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
                ctx.config,
                ctx.theme,
            );
            *ctx.menu_return_to = MenuState::Main;
            *ctx.menu_state = MenuState::Options;
            return HandleResult::raw(menu_out);
        }
        "h" | "?" | "f1" => {
            let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, *ctx.rounded);
            *ctx.menu_return_to = MenuState::Main;
            *ctx.menu_state = MenuState::Help;
            return HandleResult::raw(menu_out);
        }
        _ => {}
    }
    HandleResult::none()
}
