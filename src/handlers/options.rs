use crate::{
    config_keys::{bool_keys as bk, str_keys as sk},
    dirty::Dirty,
    handlers::{InputContext, MenuState, redraw_after_overlay},
    menu, theme, theme_keys as tc,
};

/// Handle input while the options overlay is visible.
pub(crate) fn handle(key: &str, ctx: &mut InputContext) -> bool {
    match key {
        "q" => return true,
        "escape" | "backspace" => {
            let return_to = *ctx.menu_return_to;
            *ctx.menu_state = return_to;
            let out = redraw_after_overlay(ctx);
            let _ = ctx.terminal.write_synced(&out);
            if return_to == MenuState::None {
                *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
            }
        }
        "tab" => {
            *ctx.options_cat = (*ctx.options_cat + 1) % 7;
            *ctx.options_page = 0;
            *ctx.options_selected = 0;
            redraw_options(ctx);
        }
        "shift_tab" => {
            *ctx.options_cat = if *ctx.options_cat == 0 {
                6
            } else {
                *ctx.options_cat - 1
            };
            *ctx.options_page = 0;
            *ctx.options_selected = 0;
            redraw_options(ctx);
        }
        "0" | "1" | "2" | "3" | "4" | "5" | "6" => {
            let new_cat = key.parse::<usize>().unwrap_or(0);
            if new_cat != *ctx.options_cat {
                *ctx.options_cat = new_cat;
                *ctx.options_page = 0;
                *ctx.options_selected = 0;
            }
            redraw_options(ctx);
        }
        "up" | "k" => {
            if *ctx.options_selected > 0 {
                *ctx.options_selected -= 1;
            } else {
                // wrap to previous page or last page
                let pages = menu::options_menu::page_count(*ctx.options_cat, ctx.th);
                if *ctx.options_page > 0 {
                    *ctx.options_page -= 1;
                } else if pages > 1 {
                    *ctx.options_page = pages - 1;
                }
                *ctx.options_selected =
                    menu::options_menu::select_max(*ctx.options_cat, *ctx.options_page, ctx.th);
            }
            redraw_options(ctx);
        }
        "down" | "j" => {
            let sm = menu::options_menu::select_max(*ctx.options_cat, *ctx.options_page, ctx.th);
            if *ctx.options_selected < sm {
                *ctx.options_selected += 1;
            } else {
                // wrap to next page or first page
                let pages = menu::options_menu::page_count(*ctx.options_cat, ctx.th);
                if *ctx.options_page < pages - 1 {
                    *ctx.options_page += 1;
                } else if pages > 1 {
                    *ctx.options_page = 0;
                }
                *ctx.options_selected = 0;
            }
            redraw_options(ctx);
        }
        "page_up" => {
            let pages = menu::options_menu::page_count(*ctx.options_cat, ctx.th);
            if pages > 1 {
                *ctx.options_page = if *ctx.options_page > 0 {
                    *ctx.options_page - 1
                } else {
                    pages - 1
                };
                *ctx.options_selected = 0;
            }
            redraw_options(ctx);
        }
        "page_down" => {
            let pages = menu::options_menu::page_count(*ctx.options_cat, ctx.th);
            if pages > 1 {
                *ctx.options_page = if *ctx.options_page < pages - 1 {
                    *ctx.options_page + 1
                } else {
                    0
                };
                *ctx.options_selected = 0;
            }
            redraw_options(ctx);
        }
        "left" | "right" | "h" | "l" | "enter" | "space" => {
            if let Some(opt_key) = menu::options_menu::opt_key(
                *ctx.options_cat,
                *ctx.options_page,
                *ctx.options_selected,
                ctx.th,
            ) {
                let kind = if ctx.config.bools.contains_key(opt_key) {
                    menu::options_menu::OptKind::Bool
                } else if ctx.config.ints.contains_key(opt_key) {
                    menu::options_menu::OptKind::Int
                } else if !menu::options_menu::browsable_values(opt_key).is_empty() {
                    menu::options_menu::OptKind::Browsable
                } else {
                    menu::options_menu::OptKind::StringVal
                };

                let dir: i64 = if key == "left" || key == "h" { -1 } else { 1 };

                match kind {
                    menu::options_menu::OptKind::Bool => {
                        ctx.config.flip(opt_key);
                        *ctx.rounded = ctx.config.get_bool(bk::ROUNDED_CORNERS);
                    }
                    menu::options_menu::OptKind::Int => {
                        menu::options_menu::step_int(opt_key, ctx.config, dir);
                    }
                    menu::options_menu::OptKind::Browsable => {
                        menu::options_menu::cycle_browsable(opt_key, ctx.config, dir as i32);
                        if opt_key == sk::COLOR_THEME {
                            let name = ctx.config.get_string(sk::COLOR_THEME).to_string();
                            *ctx.theme = theme::Theme::from_name(&name);
                            let base = format!(
                                "{}{}",
                                ctx.theme.c(tc::MAIN_FG),
                                ctx.theme.bg(tc::MAIN_BG),
                            );
                            let _ = ctx.terminal.write_raw(&base);
                        }
                    }
                    menu::options_menu::OptKind::StringVal => {
                        // No inline editing yet — strings shown read-only
                    }
                }
            }
            redraw_options(ctx);
        }
        _ => {}
    }
    false
}

/// Redraw the options menu overlay.
fn redraw_options(ctx: &mut InputContext) {
    let menu_out = menu::options_menu::draw(
        ctx.tw,
        ctx.th,
        *ctx.options_cat,
        *ctx.options_selected,
        *ctx.options_page,
        ctx.config,
        ctx.theme,
    );
    let _ = ctx.terminal.write_synced(&menu_out);
}
