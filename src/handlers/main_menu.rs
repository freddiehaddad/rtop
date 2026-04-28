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
            ctx.overlay.menu_state = MenuState::None;
            ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        "up" | "k" | "shift_tab" => {
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
        "down" | "j" | "tab" => {
            ctx.overlay.main_menu_selected = (ctx.overlay.main_menu_selected + 1) % 3;
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                ctx.overlay.main_menu_selected,
                ctx.theme,
            );
            return HandleResult::synced(menu_out);
        }
        "enter" | "space" => {
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
                    ctx.overlay.menu_state = MenuState::Options;
                    return HandleResult::raw(menu_out);
                }
                1 => {
                    // Help
                    let menu_out =
                        menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, ctx.runtime.rounded);
                    ctx.overlay.menu_return_to = MenuState::Main;
                    ctx.overlay.menu_state = MenuState::Help;
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
            ctx.overlay.menu_state = MenuState::Options;
            return HandleResult::raw(menu_out);
        }
        "h" | "?" | "f1" => {
            let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, ctx.runtime.rounded);
            ctx.overlay.menu_return_to = MenuState::Main;
            ctx.overlay.menu_state = MenuState::Help;
            return HandleResult::raw(menu_out);
        }
        _ => {}
    }
    HandleResult::none()
}
