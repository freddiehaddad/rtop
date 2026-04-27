use crate::{
    config,
    config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk},
    dirty::Dirty,
    draw, input, menu, runner, term, theme, tools, ui,
};

#[derive(Clone, Copy, PartialEq)]
enum MenuState {
    None,
    Main,
    Help,
    Options,
    Filter,
}

/// Run the main event loop: collect data, render UI, and handle input.
pub fn run(
    config: &mut config::Config,
    terminal: &mut term::Terminal,
    theme: &mut theme::Theme,
    runner: &mut runner::Runner,
) {
    let mut rounded = config.get_bool(bk::ROUNDED_CORNERS);
    let mut update_ms = config.get_int(ik::UPDATE_MS) as u64;

    let mut menu_state = MenuState::None;

    let mut options_cat: usize = 0;
    let mut options_selected: usize = 0;
    let mut options_page: usize = 0;
    let mut main_menu_selected: usize = 0;
    let mut proc_start: usize = 0;
    let mut proc_selected: usize = 0;
    let mut filter_text = String::new();
    let mut menu_return_to = MenuState::None;

    // Main event loop — timer-based collection with per-box dirty tracking.
    let mut dirty = Dirty::FULL;
    let mut cached_layout: Option<draw::layout::Layout> = None;
    let mut next_update = std::time::Instant::now();

    loop {
        // ── Phase 1: Detect what's dirty ──────────────────────────────────

        // Terminal resize
        if terminal.refresh() {
            dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        let (tw, th) = terminal.size();
        let tw = tw as usize;
        let th = th as usize;

        // Wall-clock collection deadline
        let now = std::time::Instant::now();
        if now >= next_update {
            dirty |= Dirty::COLLECT | Dirty::ALL_BOXES | Dirty::PROC_LIST;
            next_update = now + std::time::Duration::from_millis(update_ms);
        }

        // ── Phase 2: Execute dirty work (skip if menu overlay is active) ──

        let render_ui = menu_state == MenuState::None || menu_state == MenuState::Filter;

        if render_ui && !dirty.is_empty() {
            // Collect data from OS
            if dirty.contains(Dirty::COLLECT) {
                runner.collect_all();
            }

            // Rebuild derived process display list
            if dirty.contains(Dirty::PROC_LIST) {
                let sort_by = config.get_string(sk::PROC_SORTING);
                let reversed = config.get_bool(bk::PROC_REVERSED);
                let filter = config.get_string(sk::PROC_FILTER);
                let tree_mode = config.get_bool(bk::PROC_TREE);
                runner
                    .proc_collector
                    .rebuild_display(sort_by, reversed, filter, tree_mode);
            }

            // Calculate layout (or reuse cached)
            if dirty.contains(Dirty::LAYOUT) || cached_layout.is_none() {
                let shown: Vec<String> = config
                    .get_string(sk::SHOWN_BOXES)
                    .split_whitespace()
                    .map(String::from)
                    .collect();
                cached_layout = Some(draw::layout::calc_sizes(&draw::layout::LayoutConfig {
                    term_width: tw,
                    term_height: th,
                    shown_boxes: &shown,
                    cpu_bottom: config.get_bool(bk::CPU_BOTTOM),
                    mem_below_net: config.get_bool(bk::MEM_BELOW_NET),
                    proc_left: config.get_bool(bk::PROC_LEFT),
                    core_count: runner.cpu.info.core_count,
                    gpu_count: runner.gpu.gpu_count(),
                }));
            }
            let layout = cached_layout
                .as_ref()
                .expect("layout must be initialized before rendering");

            // ── Phase 3: Render dirty boxes ───────────────────────────────

            let mut output = String::new();

            // Full screen clear only when layout changed
            if dirty.contains(Dirty::LAYOUT) {
                output.push_str("\x1b[2J");
            }

            let is_filtering = menu_state == MenuState::Filter;
            let params = RenderParams {
                dirty,
                layout,
                runner,
                config,
                theme,
                rounded,
                update_ms,
                is_filtering,
            };
            output.push_str(&render_all(&params, &mut proc_selected, &mut proc_start));

            if let Err(e) = terminal.write_synced(&output) {
                tracing::debug!("terminal write failed: {e}");
            }

            dirty = Dirty::empty();
        }

        // Poll for input — wait at most until the next update deadline
        let remaining = next_update
            .saturating_duration_since(std::time::Instant::now())
            .as_millis() as u64;
        let poll_ms = remaining.clamp(10, 1000); // At least 10ms, at most 1s

        if input::poll(poll_ms) {
            if let Some(key) = input::get() {
                if key.is_empty() || key.starts_with("mouse_") || key == "resize" {
                    if key == "resize" {
                        dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
                    }
                    continue;
                }
                let mut ctx = InputContext {
                    config: &mut *config,
                    terminal: &mut *terminal,
                    theme: &mut *theme,
                    runner: &mut *runner,
                    menu_state: &mut menu_state,
                    dirty: &mut dirty,
                    rounded: &mut rounded,
                    update_ms: &mut update_ms,
                    main_menu_selected: &mut main_menu_selected,
                    options_cat: &mut options_cat,
                    options_selected: &mut options_selected,
                    options_page: &mut options_page,
                    proc_selected: &mut proc_selected,
                    proc_start: &mut proc_start,
                    filter_text: &mut filter_text,
                    cached_layout: &cached_layout,
                    menu_return_to: &mut menu_return_to,
                    tw,
                    th,
                };
                let quit = match *ctx.menu_state {
                    MenuState::Main => handle_main_menu_input(&key, &mut ctx),
                    MenuState::Help => handle_help_input(&key, &mut ctx),
                    MenuState::Options => handle_options_input(&key, &mut ctx),
                    MenuState::Filter => handle_filter_input(&key, &mut ctx),
                    MenuState::None => handle_normal_input(&key, &mut ctx),
                };
                if quit {
                    break;
                }
            }
        }
        // No else branch needed — the wall-clock check at the top of the loop
        // handles periodic updates regardless of input activity.
    }

    // Save config on exit
    if config.get_bool(bk::SAVE_CONFIG_ON_EXIT) {
        let conf_path = tools::config_dir().join("rtop.conf");
        let _ = config.write(&conf_path);
    }
}

/// Shared mutable state passed to each per-MenuState input handler.
struct InputContext<'a> {
    config: &'a mut config::Config,
    terminal: &'a mut term::Terminal,
    theme: &'a mut theme::Theme,
    runner: &'a mut runner::Runner,
    menu_state: &'a mut MenuState,
    dirty: &'a mut Dirty,
    rounded: &'a mut bool,
    update_ms: &'a mut u64,
    main_menu_selected: &'a mut usize,
    options_cat: &'a mut usize,
    options_selected: &'a mut usize,
    options_page: &'a mut usize,
    proc_selected: &'a mut usize,
    proc_start: &'a mut usize,
    filter_text: &'a mut String,
    cached_layout: &'a Option<draw::layout::Layout>,
    /// Where Options/Help was opened from — return here on escape.
    menu_return_to: &'a mut MenuState,
    tw: usize,
    th: usize,
}

/// Handle input while the main menu overlay is visible.
fn handle_main_menu_input(key: &str, ctx: &mut InputContext) -> bool {
    match key {
        "q" => return true,
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
            let _ = ctx.terminal.write_synced(&menu_out);
        }
        "down" | "j" | "tab" => {
            *ctx.main_menu_selected = (*ctx.main_menu_selected + 1) % 3;
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                *ctx.main_menu_selected,
                ctx.theme,
            );
            let _ = ctx.terminal.write_synced(&menu_out);
        }
        "enter" | "space" => {
            match *ctx.main_menu_selected {
                0 => {
                    // Options
                    *ctx.options_cat = 0;
                    *ctx.options_selected = 0;
                    *ctx.options_page = 0;
                    let menu_out = draw_options_menu(
                        ctx.tw,
                        ctx.th,
                        ctx.config,
                        ctx.theme,
                        *ctx.options_cat,
                        *ctx.options_selected,
                        *ctx.options_page,
                    );
                    let _ = ctx.terminal.write_raw(&menu_out);
                    *ctx.menu_return_to = MenuState::Main;
                    *ctx.menu_state = MenuState::Options;
                }
                1 => {
                    // Help
                    let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, *ctx.rounded);
                    let _ = ctx.terminal.write_raw(&menu_out);
                    *ctx.menu_return_to = MenuState::Main;
                    *ctx.menu_state = MenuState::Help;
                }
                2 => {
                    // Quit
                    return true;
                }
                _ => {}
            }
        }
        "o" | "f2" => {
            *ctx.options_cat = 0;
            *ctx.options_selected = 0;
            *ctx.options_page = 0;
            let menu_out = draw_options_menu(
                ctx.tw,
                ctx.th,
                ctx.config,
                ctx.theme,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
            );
            let _ = ctx.terminal.write_raw(&menu_out);
            *ctx.menu_return_to = MenuState::Main;
            *ctx.menu_state = MenuState::Options;
        }
        "h" | "?" | "f1" => {
            let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, *ctx.rounded);
            let _ = ctx.terminal.write_raw(&menu_out);
            *ctx.menu_return_to = MenuState::Main;
            *ctx.menu_state = MenuState::Help;
        }
        _ => {}
    }
    false
}

/// Handle input while the help overlay is visible.
fn handle_help_input(key: &str, ctx: &mut InputContext) -> bool {
    match key {
        "q" => return true,
        "escape" | "h" | "?" | "f1" => {
            let return_to = *ctx.menu_return_to;
            *ctx.menu_state = return_to;
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
                let mut out = String::new();
                out.push_str("\x1b[2J");
                out.push_str(&render_all(&params, ctx.proc_selected, ctx.proc_start));
                if return_to == MenuState::Main {
                    out.push_str(&menu::main_menu::draw_with_selection(
                        ctx.tw,
                        ctx.th,
                        *ctx.main_menu_selected,
                        ctx.theme,
                    ));
                }
                let _ = ctx.terminal.write_synced(&out);
            }
            if return_to == MenuState::None {
                *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
            }
        }
        _ => {}
    }
    false
}

/// Handle input while the options overlay is visible.
fn handle_options_input(key: &str, ctx: &mut InputContext) -> bool {
    match key {
        "q" => return true,
        "escape" | "backspace" => {
            let return_to = *ctx.menu_return_to;
            *ctx.menu_state = return_to;
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
                let mut out = String::new();
                out.push_str("\x1b[2J");
                out.push_str(&render_all(&params, ctx.proc_selected, ctx.proc_start));
                if return_to == MenuState::Main {
                    out.push_str(&menu::main_menu::draw_with_selection(
                        ctx.tw,
                        ctx.th,
                        *ctx.main_menu_selected,
                        ctx.theme,
                    ));
                }
                let _ = ctx.terminal.write_synced(&out);
            }
            if return_to == MenuState::None {
                *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
            }
        }
        "tab" => {
            *ctx.options_cat = (*ctx.options_cat + 1) % 7;
            *ctx.options_page = 0;
            *ctx.options_selected = 0;
            let menu_out = draw_options_menu(
                ctx.tw,
                ctx.th,
                ctx.config,
                ctx.theme,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
            );
            let _ = ctx.terminal.write_synced(&menu_out);
        }
        "shift_tab" => {
            *ctx.options_cat = if *ctx.options_cat == 0 {
                6
            } else {
                *ctx.options_cat - 1
            };
            *ctx.options_page = 0;
            *ctx.options_selected = 0;
            let menu_out = draw_options_menu(
                ctx.tw,
                ctx.th,
                ctx.config,
                ctx.theme,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
            );
            let _ = ctx.terminal.write_synced(&menu_out);
        }
        "0" | "1" | "2" | "3" | "4" | "5" | "6" => {
            let new_cat = key.parse::<usize>().unwrap_or(0);
            if new_cat != *ctx.options_cat {
                *ctx.options_cat = new_cat;
                *ctx.options_page = 0;
                *ctx.options_selected = 0;
            }
            let menu_out = draw_options_menu(
                ctx.tw,
                ctx.th,
                ctx.config,
                ctx.theme,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
            );
            let _ = ctx.terminal.write_synced(&menu_out);
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
            let menu_out = draw_options_menu(
                ctx.tw,
                ctx.th,
                ctx.config,
                ctx.theme,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
            );
            let _ = ctx.terminal.write_synced(&menu_out);
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
            let menu_out = draw_options_menu(
                ctx.tw,
                ctx.th,
                ctx.config,
                ctx.theme,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
            );
            let _ = ctx.terminal.write_synced(&menu_out);
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
            let menu_out = draw_options_menu(
                ctx.tw,
                ctx.th,
                ctx.config,
                ctx.theme,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
            );
            let _ = ctx.terminal.write_synced(&menu_out);
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
            let menu_out = draw_options_menu(
                ctx.tw,
                ctx.th,
                ctx.config,
                ctx.theme,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
            );
            let _ = ctx.terminal.write_synced(&menu_out);
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
                                ctx.theme.c("main_fg"),
                                ctx.theme.c("main_bg").replace("38;2", "48;2"),
                            );
                            let _ = ctx.terminal.write_raw(&base);
                        }
                    }
                    menu::options_menu::OptKind::StringVal => {
                        // No inline editing yet — strings shown read-only
                    }
                }
            }
            let menu_out = draw_options_menu(
                ctx.tw,
                ctx.th,
                ctx.config,
                ctx.theme,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
            );
            let _ = ctx.terminal.write_synced(&menu_out);
        }
        _ => {}
    }
    false
}

/// Handle input while the process-filter text field is active.
fn handle_filter_input(key: &str, ctx: &mut InputContext) -> bool {
    match key {
        "escape" => {
            *ctx.menu_state = MenuState::None;
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "enter" => {
            ctx.config.set_string(sk::PROC_FILTER, ctx.filter_text);
            *ctx.menu_state = MenuState::None;
            *ctx.proc_selected = 0;
            *ctx.proc_start = 0;
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "backspace" => {
            ctx.filter_text.pop();
            ctx.config.set_string(sk::PROC_FILTER, ctx.filter_text);
            *ctx.proc_selected = 0;
            *ctx.proc_start = 0;
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "delete" => {
            ctx.filter_text.clear();
            ctx.config.set_string(sk::PROC_FILTER, "");
            *ctx.proc_selected = 0;
            *ctx.proc_start = 0;
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        s if s.len() == 1 && !s.starts_with('\x1b') => {
            ctx.filter_text.push_str(s);
            ctx.config.set_string(sk::PROC_FILTER, ctx.filter_text);
            *ctx.proc_selected = 0;
            *ctx.proc_start = 0;
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        _ => {}
    }
    false
}

/// Handle input in normal (no-menu) mode.
fn handle_normal_input(key: &str, ctx: &mut InputContext) -> bool {
    match key {
        "q" => return true,
        "escape" | "m" => {
            *ctx.main_menu_selected = 0;
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                *ctx.main_menu_selected,
                ctx.theme,
            );
            let _ = ctx.terminal.write_raw(&menu_out);
            *ctx.menu_state = MenuState::Main;
        }
        "h" | "?" | "f1" => {
            let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, *ctx.rounded);
            let _ = ctx.terminal.write_raw(&menu_out);
            *ctx.menu_return_to = MenuState::None;
            *ctx.menu_state = MenuState::Help;
        }
        "o" | "f2" => {
            *ctx.options_cat = 0;
            *ctx.options_selected = 0;
            *ctx.options_page = 0;
            let menu_out = draw_options_menu(
                ctx.tw,
                ctx.th,
                ctx.config,
                ctx.theme,
                *ctx.options_cat,
                *ctx.options_selected,
                *ctx.options_page,
            );
            let _ = ctx.terminal.write_raw(&menu_out);
            *ctx.menu_return_to = MenuState::None;
            *ctx.menu_state = MenuState::Options;
        }
        // Preset cycling
        "p" => {
            let presets = ctx.config.preset_list();
            if !presets.is_empty() {
                let cur = ctx.config.get_int(ik::CURRENT_PRESET);
                let next = if (cur + 1) >= presets.len() as i64 {
                    0i64
                } else {
                    cur + 1
                };
                ctx.config.set_int(ik::CURRENT_PRESET, next);
                ctx.config.apply_preset(&presets[next as usize]);
                *ctx.dirty |= Dirty::FULL;
            }
        }
        "P" => {
            let presets = ctx.config.preset_list();
            if !presets.is_empty() {
                let cur = ctx.config.get_int(ik::CURRENT_PRESET);
                let next = if cur <= 0 {
                    presets.len() as i64 - 1
                } else {
                    cur - 1
                };
                ctx.config.set_int(ik::CURRENT_PRESET, next);
                ctx.config.apply_preset(&presets[next as usize]);
                *ctx.dirty |= Dirty::FULL;
            }
        }
        // Save current layout as preset
        "ctrl_s" => {
            ctx.config.save_preset();
            *ctx.dirty |= Dirty::CPU_BOX;
        }
        // Delete current preset
        "ctrl_d" => {
            let cur = ctx.config.get_int(ik::CURRENT_PRESET);
            if cur > 0 {
                ctx.config.delete_preset(cur as usize);
                let presets = ctx.config.preset_list();
                let new_cur = ctx.config.get_int(ik::CURRENT_PRESET);
                if !presets.is_empty() && (new_cur as usize) < presets.len() {
                    ctx.config.apply_preset(&presets[new_cur as usize]);
                }
                *ctx.dirty |= Dirty::FULL;
            }
        }
        // Config reload
        "ctrl_r" => {
            let warnings = ctx.config.reload();
            for w in &warnings {
                tracing::warn!("{}", w);
            }
            // Reapply theme
            let theme_name = ctx.config.get_string(sk::COLOR_THEME).to_string();
            *ctx.theme = theme::Theme::from_name(&theme_name);
            let base = format!(
                "{}{}",
                ctx.theme.c("main_fg"),
                ctx.theme.c("main_bg").replace("38;2", "48;2"),
            );
            let _ = ctx.terminal.write_raw(&base);
            *ctx.rounded = ctx.config.get_bool(bk::ROUNDED_CORNERS);
            *ctx.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
            *ctx.dirty |= Dirty::FULL;
        }
        "up" | "k"
            if *ctx.proc_selected > 0 => {
                *ctx.proc_selected -= 1;
                *ctx.dirty |= Dirty::PROC_BOX;
            }
        "down" | "j" => {
            let count = ctx.runner.proc_collector.display_procs.len();
            if *ctx.proc_selected + 1 < count {
                *ctx.proc_selected += 1;
                *ctx.dirty |= Dirty::PROC_BOX;
            }
        }
        "page_up" => {
            let page = ctx.th.saturating_sub(10);
            *ctx.proc_selected = ctx.proc_selected.saturating_sub(page);
            *ctx.dirty |= Dirty::PROC_BOX;
        }
        "page_down" => {
            let page = ctx.th.saturating_sub(10);
            let count = ctx.runner.proc_collector.display_procs.len();
            *ctx.proc_selected = (*ctx.proc_selected + page).min(count.saturating_sub(1));
            *ctx.dirty |= Dirty::PROC_BOX;
        }
        "home" | "g" => {
            *ctx.proc_selected = 0;
            *ctx.proc_start = 0;
            *ctx.dirty |= Dirty::PROC_BOX;
        }
        "end" | "G" => {
            let count = ctx.runner.proc_collector.display_procs.len();
            *ctx.proc_selected = count.saturating_sub(1);
            *ctx.dirty |= Dirty::PROC_BOX;
        }
        "1" => {
            ctx.config.toggle_box("cpu");
            *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        "2" => {
            ctx.config.toggle_box("mem");
            *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        "3" => {
            ctx.config.toggle_box("net");
            *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        "4" => {
            ctx.config.toggle_box("proc");
            *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        "5" => {
            // Toggle all detected GPU boxes
            for i in 0..ctx.runner.gpu.gpu_count() {
                ctx.config.toggle_box(&format!("gpu{i}"));
            }
            *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        "6" => {
            ctx.config.toggle_box("disk");
            *ctx.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        // Process keybinds
        "f" | "/" => {
            *ctx.menu_state = MenuState::Filter;
            *ctx.filter_text = ctx.config.get_string(sk::PROC_FILTER).to_string();
            *ctx.dirty |= Dirty::PROC_BOX;
        }
        "e" => {
            ctx.config.flip(bk::PROC_TREE);
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "r" => {
            ctx.config.flip(bk::PROC_REVERSED);
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "c" => {
            ctx.config.flip(bk::PROC_PER_CORE);
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "i" => {
            ctx.config.flip(bk::IO_MODE);
            *ctx.dirty |= Dirty::MEM_BOX | Dirty::PROC_BOX;
        }
        "left" => {
            use crate::collect::process::SORT_OPTIONS;
            let current = ctx.config.get_string(sk::PROC_SORTING).to_string();
            let idx = SORT_OPTIONS.iter().position(|&s| s == current).unwrap_or(0);
            let new_idx = if idx == 0 { SORT_OPTIONS.len() - 1 } else { idx - 1 };
            ctx.config.set_string(sk::PROC_SORTING, SORT_OPTIONS[new_idx]);
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "right" => {
            use crate::collect::process::SORT_OPTIONS;
            let current = ctx.config.get_string(sk::PROC_SORTING).to_string();
            let idx = SORT_OPTIONS.iter().position(|&s| s == current).unwrap_or(0);
            let new_idx = (idx + 1) % SORT_OPTIONS.len();
            ctx.config.set_string(sk::PROC_SORTING, SORT_OPTIONS[new_idx]);
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "t"
            // Terminate selected process
            if *ctx.proc_selected < ctx.runner.proc_collector.display_procs.len() => {
                let pid = ctx.runner.proc_collector.display_procs[*ctx.proc_selected].pid;
                terminate_process(pid);
                *ctx.dirty |= Dirty::PROC_BOX;
            }
        "enter"
            // Toggle process detailed view
            if *ctx.proc_selected < ctx.runner.proc_collector.display_procs.len() => {
                let pid = ctx.runner.proc_collector.display_procs[*ctx.proc_selected].pid;
                let current_detailed = ctx.config.get_int(ik::DETAILED_PID);
                if current_detailed == pid as i64 {
                    ctx.config.set_int(ik::DETAILED_PID, 0);
                } else {
                    ctx.config.set_int(ik::DETAILED_PID, pid as i64);
                }
                *ctx.dirty |= Dirty::PROC_BOX;
            }
        // Network keybinds
        "b"
            if !ctx.runner.net.interfaces.is_empty() => {
                let idx = ctx.runner.net.interfaces.iter()
                    .position(|s| s == &ctx.runner.net.selected_iface)
                    .unwrap_or(0);
                let new_idx = if idx == 0 { ctx.runner.net.interfaces.len() - 1 } else { idx - 1 };
                ctx.runner.net.selected_iface = ctx.runner.net.interfaces[new_idx].clone();
                *ctx.dirty |= Dirty::NET_BOX;
            }
        "n"
            if !ctx.runner.net.interfaces.is_empty() => {
                let idx = ctx.runner.net.interfaces.iter()
                    .position(|s| s == &ctx.runner.net.selected_iface)
                    .unwrap_or(0);
                let new_idx = (idx + 1) % ctx.runner.net.interfaces.len();
                ctx.runner.net.selected_iface = ctx.runner.net.interfaces[new_idx].clone();
                *ctx.dirty |= Dirty::NET_BOX;
            }
        "a" => {
            ctx.config.flip(bk::NET_AUTO);
            *ctx.dirty |= Dirty::NET_BOX;
        }
        "y" => {
            ctx.config.flip(bk::NET_SYNC);
            *ctx.dirty |= Dirty::NET_BOX;
        }
        // Network zero reset
        "z" => {
            let iface = ctx.runner.net.selected_iface.clone();
            if let Some(net_info) = ctx.runner.net.current_net.get_mut(&iface) {
                let dl = net_info.stat.download.clone();
                let ul = net_info.stat.upload.clone();
                if dl.offset + ul.offset > 0 {
                    net_info.stat.download.offset = 0;
                    net_info.stat.upload.offset = 0;
                } else {
                    net_info.stat.download.offset = dl.last + dl.rollover;
                    net_info.stat.upload.offset = ul.last + ul.rollover;
                }
                *ctx.dirty |= Dirty::NET_BOX;
            }
        }
        // Update rate keybinds
        "+" => {
            let step = if *ctx.update_ms > 2000 { 1000 } else { 100 };
            let new_ms = (*ctx.update_ms as i64 + step).min(86_400_000);
            ctx.config.set_int(ik::UPDATE_MS, new_ms);
            *ctx.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
            *ctx.dirty |= Dirty::CPU_BOX;
        }
        "-" => {
            let step = if *ctx.update_ms > 2000 { 1000 } else { 100 };
            let new_ms = (*ctx.update_ms as i64 - step).max(100);
            ctx.config.set_int(ik::UPDATE_MS, new_ms);
            *ctx.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
            *ctx.dirty |= Dirty::CPU_BOX;
        }
        _ => {}
    }
    false
}

fn clamp_proc_selection(
    procs: &[crate::domain::process::ProcInfo],
    box_height: usize,
    selected: &mut usize,
    start: &mut usize,
) {
    let count = procs.len();
    let max_visible = box_height.saturating_sub(5);
    if count == 0 {
        *selected = 0;
        *start = 0;
        return;
    }
    if *selected >= count {
        *selected = count - 1;
    }
    if max_visible == 0 {
        *start = *selected;
        return;
    }
    if *selected >= *start + max_visible {
        *start = *selected - max_visible + 1;
    }
    if *selected < *start {
        *start = *selected;
    }
}

fn draw_options_menu(
    tw: usize,
    th: usize,
    config: &config::Config,
    theme: &theme::Theme,
    cat: usize,
    selected: usize,
    page: usize,
) -> String {
    menu::options_menu::draw(tw, th, cat, selected, page, config, theme)
}

fn terminate_process(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::*;

    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let _ = TerminateProcess(handle, 1);
            let _ = CloseHandle(handle);
        }
    }
}

/// Parameters for rendering the UI boxes.
struct RenderParams<'a> {
    dirty: Dirty,
    layout: &'a draw::layout::Layout,
    runner: &'a runner::Runner,
    config: &'a config::Config,
    theme: &'a theme::Theme,
    rounded: bool,
    update_ms: u64,
    is_filtering: bool,
}

/// Render UI boxes into an ANSI output string.
///
/// Only renders boxes whose corresponding dirty flag is set.
/// Pass `Dirty::ALL_BOXES` to render everything.
fn render_all(params: &RenderParams, proc_selected: &mut usize, proc_start: &mut usize) -> String {
    let dirty = params.dirty;
    let layout = params.layout;
    let runner = params.runner;
    let config = params.config;
    let theme = params.theme;
    let rounded = params.rounded;
    let update_ms = params.update_ms;
    let is_filtering = params.is_filtering;
    let mut output = String::new();

    if dirty.intersects(Dirty::CPU_BOX) {
        if let Some(ref cpu_dim) = layout.cpu {
            let area = ui::BoxArea::from_dim(cpu_dim, rounded);
            let cpu_settings = ui::cpu_box::CpuBoxSettings {
                graph_symbol: crate::draw::graph::GraphSymbol::from_config(
                    config.get_string(sk::GRAPH_SYMBOL_CPU),
                    config.get_string(sk::GRAPH_SYMBOL),
                ),
                upper_source: config.get_string(sk::CPU_GRAPH_UPPER),
                lower_source: config.get_string(sk::CPU_GRAPH_LOWER),
                check_temp: config.get_bool(bk::CHECK_TEMP),
                show_coretemp: config.get_bool(bk::SHOW_CORETEMP),
                temp_scale: config.get_string(sk::TEMP_SCALE),
                update_ms,
                current_preset: config.get_int(ik::CURRENT_PRESET),
            };
            output.push_str(&ui::cpu_box::draw(
                &runner.cpu.info,
                &area,
                theme,
                &cpu_settings,
            ));
        }
    }

    if dirty.intersects(Dirty::GPU_BOX) {
        let gpu_settings = ui::gpu_box::GpuBoxSettings {
            temp_scale: config.get_string(sk::TEMP_SCALE),
        };
        for (gi, gpu_dim) in layout.gpu.iter().enumerate() {
            if gi < runner.gpu.gpus.len() {
                let area = ui::BoxArea::from_dim(gpu_dim, rounded);
                output.push_str(&ui::gpu_box::draw(
                    &runner.gpu.gpus[gi],
                    gi,
                    &area,
                    theme,
                    &gpu_settings,
                ));
            }
        }
    }

    if dirty.intersects(Dirty::MEM_BOX) {
        if let Some(ref mem_dim) = layout.mem {
            let area = ui::BoxArea::from_dim(mem_dim, rounded);
            output.push_str(&ui::mem_box::draw(
                &runner.mem.info,
                &area,
                theme,
                config.get_bool(bk::SHOW_SWAP),
            ));
        }
    }

    if dirty.intersects(Dirty::DISK_BOX) {
        if let Some(ref disk_dim) = layout.disk {
            let area = ui::BoxArea::from_dim(disk_dim, rounded);
            output.push_str(&ui::disk_box::draw(&runner.disk.data, &area, theme));
        }
    }

    if dirty.intersects(Dirty::NET_BOX) {
        if let Some(ref net_dim) = layout.net {
            let iface = &runner.net.selected_iface;
            let net_info = runner
                .net
                .current_net
                .get(iface)
                .cloned()
                .unwrap_or_default();
            let area = ui::BoxArea::from_dim(net_dim, rounded);
            let net_settings = ui::net_box::NetBoxSettings {
                auto_scale: config.get_bool(bk::NET_AUTO),
                sync_scale: config.get_bool(bk::NET_SYNC),
                max_download: config.get_int(ik::NET_DOWNLOAD),
                max_upload: config.get_int(ik::NET_UPLOAD),
                graph_symbol: crate::draw::graph::GraphSymbol::from_config(
                    config.get_string(sk::GRAPH_SYMBOL_NET),
                    config.get_string(sk::GRAPH_SYMBOL),
                ),
            };
            output.push_str(&ui::net_box::draw(
                &net_info,
                iface,
                &area,
                theme,
                &net_settings,
            ));
        }
    }

    if dirty.intersects(Dirty::PROC_BOX) {
        if let Some(ref proc_dim) = layout.proc_box {
            let procs = &runner.proc_collector.display_procs;
            clamp_proc_selection(procs, proc_dim.height, proc_selected, proc_start);
            let sort_by = config.get_string(sk::PROC_SORTING);
            let reversed = config.get_bool(bk::PROC_REVERSED);
            let tree_mode = config.get_bool(bk::PROC_TREE);
            let detailed_pid = config.get_int(ik::DETAILED_PID) as u32;
            let pf = config.get_string(sk::PROC_FILTER);
            let area = ui::BoxArea::from_dim(proc_dim, rounded);
            let view = ui::ProcView {
                start: *proc_start,
                selected: *proc_selected,
                sort_by,
                sort_reversed: reversed,
                tree_mode,
                detailed_pid,
                filter: pf,
                filtering: is_filtering,
            };
            output.push_str(&ui::proc_box::draw_with_sort(procs, &area, &view, theme));
        }
    }

    output
}
