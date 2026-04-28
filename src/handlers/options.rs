use crate::{
    config::ConfigKey,
    config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk},
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState, TerminalOp},
    input::Key,
    menu, theme,
};

/// Handle input while the options overlay is visible.
pub(crate) fn handle(key: &Key, ctx: &mut InputContext) -> HandleResult {
    match *key {
        Key::Char('q') => return HandleResult::quit(),
        Key::Escape | Key::Backspace => {
            let return_to = ctx.overlay.menu_return_to;
            ctx.overlay.menu_state = return_to;
            if return_to == MenuState::None {
                ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
            }
            return HandleResult::redraw();
        }
        Key::Tab => {
            ctx.overlay.options_cat = (ctx.overlay.options_cat + 1) % 7;
            ctx.overlay.options_page = 0;
            ctx.overlay.options_selected = 0;
            return options_menu_output(ctx);
        }
        Key::ShiftTab => {
            ctx.overlay.options_cat = if ctx.overlay.options_cat == 0 {
                6
            } else {
                ctx.overlay.options_cat - 1
            };
            ctx.overlay.options_page = 0;
            ctx.overlay.options_selected = 0;
            return options_menu_output(ctx);
        }
        Key::Char(c @ '0'..='6') => {
            let new_cat = (c as usize) - ('0' as usize);
            if new_cat != ctx.overlay.options_cat {
                ctx.overlay.options_cat = new_cat;
                ctx.overlay.options_page = 0;
                ctx.overlay.options_selected = 0;
            }
            return options_menu_output(ctx);
        }
        Key::Up | Key::Char('k') => {
            if ctx.overlay.options_selected > 0 {
                ctx.overlay.options_selected -= 1;
            } else {
                // wrap to previous page or last page
                let pages = menu::options_menu::page_count(ctx.overlay.options_cat, ctx.th);
                if ctx.overlay.options_page > 0 {
                    ctx.overlay.options_page -= 1;
                } else if pages > 1 {
                    ctx.overlay.options_page = pages - 1;
                }
                ctx.overlay.options_selected = menu::options_menu::select_max(
                    ctx.overlay.options_cat,
                    ctx.overlay.options_page,
                    ctx.th,
                );
            }
            return options_menu_output(ctx);
        }
        Key::Down | Key::Char('j') => {
            let sm = menu::options_menu::select_max(
                ctx.overlay.options_cat,
                ctx.overlay.options_page,
                ctx.th,
            );
            if ctx.overlay.options_selected < sm {
                ctx.overlay.options_selected += 1;
            } else {
                // wrap to next page or first page
                let pages = menu::options_menu::page_count(ctx.overlay.options_cat, ctx.th);
                if ctx.overlay.options_page < pages - 1 {
                    ctx.overlay.options_page += 1;
                } else if pages > 1 {
                    ctx.overlay.options_page = 0;
                }
                ctx.overlay.options_selected = 0;
            }
            return options_menu_output(ctx);
        }
        Key::PageUp => {
            let pages = menu::options_menu::page_count(ctx.overlay.options_cat, ctx.th);
            if pages > 1 {
                ctx.overlay.options_page = if ctx.overlay.options_page > 0 {
                    ctx.overlay.options_page - 1
                } else {
                    pages - 1
                };
                ctx.overlay.options_selected = 0;
            }
            return options_menu_output(ctx);
        }
        Key::PageDown => {
            let pages = menu::options_menu::page_count(ctx.overlay.options_cat, ctx.th);
            if pages > 1 {
                ctx.overlay.options_page = if ctx.overlay.options_page < pages - 1 {
                    ctx.overlay.options_page + 1
                } else {
                    0
                };
                ctx.overlay.options_selected = 0;
            }
            return options_menu_output(ctx);
        }
        Key::Left | Key::Right | Key::Char('h') | Key::Char('l') | Key::Enter | Key::Space => {
            let mut extra_ops: Vec<TerminalOp> = Vec::new();

            if let Some(opt_key) = menu::options_menu::opt_key(
                ctx.overlay.options_cat,
                ctx.overlay.options_page,
                ctx.overlay.options_selected,
                ctx.th,
            ) {
                let kind = menu::options_menu::opt_kind(opt_key, ctx.config);
                let dir: i64 = if matches!(key, Key::Left | Key::Char('h')) {
                    -1
                } else {
                    1
                };

                match kind {
                    menu::options_menu::OptKind::Bool => {
                        let ConfigKey::Bool(opt_key) = opt_key else {
                            unreachable!("bool option kind without bool key");
                        };
                        ctx.config.flip(opt_key);
                        ctx.runtime.rounded = ctx.config.get_bool(bk::ROUNDED_CORNERS);
                        if opt_key == bk::THEME_BACKGROUND {
                            extra_ops.push(TerminalOp::Raw(
                                ctx.theme
                                    .base_style(ctx.config.get_bool(bk::THEME_BACKGROUND)),
                            ));
                        }
                    }
                    menu::options_menu::OptKind::Int => {
                        let ConfigKey::Int(opt_key) = opt_key else {
                            unreachable!("int option kind without int key");
                        };
                        menu::options_menu::step_int(opt_key, ctx.config, dir);
                        if opt_key == ik::UPDATE_MS {
                            ctx.runtime.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
                            ctx.worker.set_update_ms(ctx.runtime.update_ms);
                        }
                    }
                    menu::options_menu::OptKind::Browsable => {
                        let ConfigKey::String(string_key) = opt_key else {
                            unreachable!("browsable option kind without string key");
                        };
                        menu::options_menu::cycle_browsable(opt_key, ctx.config, dir as i32);
                        if string_key == sk::COLOR_THEME {
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
                ctx.overlay.options_cat,
                ctx.overlay.options_selected,
                ctx.overlay.options_page,
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
        ctx.overlay.options_cat,
        ctx.overlay.options_selected,
        ctx.overlay.options_page,
        ctx.config,
        ctx.theme,
    );
    HandleResult::synced(menu_out)
}
