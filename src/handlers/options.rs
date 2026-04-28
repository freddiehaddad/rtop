use crate::{
    config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk},
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState, TerminalOp},
    menu, theme,
};

/// Handle input while the options overlay is visible.
pub(crate) fn handle(key: &str, ctx: &mut InputContext) -> HandleResult {
    match key {
        "q" => return HandleResult::quit(),
        "escape" | "backspace" => {
            let return_to = *ctx.menu_return_to;
            *ctx.menu_state = return_to;
            if return_to == MenuState::None {
                *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
            }
            return HandleResult::redraw();
        }
        "tab" => {
            *ctx.options_cat = (*ctx.options_cat + 1) % 7;
            *ctx.options_page = 0;
            *ctx.options_selected = 0;
            return options_menu_output(ctx);
        }
        "shift_tab" => {
            *ctx.options_cat = if *ctx.options_cat == 0 {
                6
            } else {
                *ctx.options_cat - 1
            };
            *ctx.options_page = 0;
            *ctx.options_selected = 0;
            return options_menu_output(ctx);
        }
        "0" | "1" | "2" | "3" | "4" | "5" | "6" => {
            let new_cat = key.parse::<usize>().unwrap_or(0);
            if new_cat != *ctx.options_cat {
                *ctx.options_cat = new_cat;
                *ctx.options_page = 0;
                *ctx.options_selected = 0;
            }
            return options_menu_output(ctx);
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
            return options_menu_output(ctx);
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
            return options_menu_output(ctx);
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
            return options_menu_output(ctx);
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
            return options_menu_output(ctx);
        }
        "left" | "right" | "h" | "l" | "enter" | "space" => {
            let mut extra_ops: Vec<TerminalOp> = Vec::new();

            if let Some(opt_key) = menu::options_menu::opt_key(
                *ctx.options_cat,
                *ctx.options_page,
                *ctx.options_selected,
                ctx.th,
            ) {
                let kind = match crate::config::Config::key_kind(opt_key) {
                    Some(crate::config::KeyKind::Bool) => menu::options_menu::OptKind::Bool,
                    Some(crate::config::KeyKind::Int) => menu::options_menu::OptKind::Int,
                    _ if !menu::options_menu::browsable_values(opt_key).is_empty() => {
                        menu::options_menu::OptKind::Browsable
                    }
                    _ => menu::options_menu::OptKind::StringVal,
                };

                let dir: i64 = if key == "left" || key == "h" { -1 } else { 1 };

                match kind {
                    menu::options_menu::OptKind::Bool => {
                        ctx.config.flip(opt_key);
                        *ctx.rounded = ctx.config.get_bool(bk::ROUNDED_CORNERS);
                        if opt_key == bk::THEME_BACKGROUND {
                            extra_ops.push(TerminalOp::Raw(
                                ctx.theme
                                    .base_style(ctx.config.get_bool(bk::THEME_BACKGROUND)),
                            ));
                        }
                    }
                    menu::options_menu::OptKind::Int => {
                        menu::options_menu::step_int(opt_key, ctx.config, dir);
                        if opt_key == ik::UPDATE_MS {
                            *ctx.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
                            ctx.worker.set_update_ms(*ctx.update_ms);
                        }
                    }
                    menu::options_menu::OptKind::Browsable => {
                        menu::options_menu::cycle_browsable(opt_key, ctx.config, dir as i32);
                        if opt_key == sk::COLOR_THEME {
                            let name = ctx.config.get_string(sk::COLOR_THEME).to_string();
                            *ctx.theme = theme::Theme::from_name(&name);
                            let base = ctx
                                .theme
                                .base_style(ctx.config.get_bool(bk::THEME_BACKGROUND));
                            extra_ops.push(TerminalOp::Raw(base));
                        }
                    }
                    menu::options_menu::OptKind::StringVal => {
                        // No inline editing yet — strings shown read-only
                    }
                }
            }

            let menu_out = menu::options_menu::draw(
                ctx.tw,
                ctx.th,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
                ctx.config,
                ctx.theme,
            );
            extra_ops.push(TerminalOp::Synced(menu_out));
            return HandleResult {
                quit: false,
                ops: extra_ops,
                redraw_overlay: false,
            };
        }
        _ => {}
    }
    HandleResult::none()
}

/// Build a HandleResult that redraws the options menu overlay.
fn options_menu_output(ctx: &mut InputContext) -> HandleResult {
    let menu_out = menu::options_menu::draw(
        ctx.tw,
        ctx.th,
        *ctx.options_cat,
        *ctx.options_selected,
        *ctx.options_page,
        ctx.config,
        ctx.theme,
    );
    HandleResult::synced(menu_out)
}
