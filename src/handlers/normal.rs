use crate::{
    collect::process::SORT_OPTIONS,
    config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk},
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
    menu, theme, theme_keys as tc,
};

/// Handle input in normal (no-menu) mode.
pub(crate) fn handle(key: &str, ctx: &mut InputContext) -> HandleResult {
    match key {
        "q" => return HandleResult::quit(),
        "escape" | "m" => {
            *ctx.main_menu_selected = 0;
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                *ctx.main_menu_selected,
                ctx.theme,
            );
            *ctx.menu_state = MenuState::Main;
            return HandleResult::raw(menu_out);
        }
        "h" | "?" | "f1" => {
            let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, *ctx.rounded);
            *ctx.menu_return_to = MenuState::None;
            *ctx.menu_state = MenuState::Help;
            return HandleResult::raw(menu_out);
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
            *ctx.menu_return_to = MenuState::None;
            *ctx.menu_state = MenuState::Options;
            return HandleResult::raw(menu_out);
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
                sync_update_ms(ctx);
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
                sync_update_ms(ctx);
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
                    sync_update_ms(ctx);
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
            let base = format!("{}{}", ctx.theme.c(tc::MAIN_FG), ctx.theme.bg(tc::MAIN_BG),);
            *ctx.rounded = ctx.config.get_bool(bk::ROUNDED_CORNERS);
            sync_update_ms(ctx);
            *ctx.dirty |= Dirty::FULL;
            return HandleResult::raw(base);
        }
        "up" | "k" if *ctx.proc_selected > 0 => {
            *ctx.proc_selected -= 1;
            *ctx.dirty |= Dirty::PROC_BOX;
        }
        "down" | "j" => {
            let count = ctx.proc_entries.len();
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
            let count = ctx.proc_entries.len();
            *ctx.proc_selected = (*ctx.proc_selected + page).min(count.saturating_sub(1));
            *ctx.dirty |= Dirty::PROC_BOX;
        }
        "home" | "g" => {
            *ctx.proc_selected = 0;
            *ctx.proc_start = 0;
            *ctx.dirty |= Dirty::PROC_BOX;
        }
        "end" | "G" => {
            let count = ctx.proc_entries.len();
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
            let gpu_count = ctx.snapshot.map_or(0, |snapshot| snapshot.gpu.gpus.len());
            for i in 0..gpu_count {
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
            let current = ctx.config.get_string(sk::PROC_SORTING).to_string();
            let idx = SORT_OPTIONS.iter().position(|&s| s == current).unwrap_or(0);
            let new_idx = if idx == 0 {
                SORT_OPTIONS.len() - 1
            } else {
                idx - 1
            };
            ctx.config
                .set_string(sk::PROC_SORTING, SORT_OPTIONS[new_idx]);
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "right" => {
            let current = ctx.config.get_string(sk::PROC_SORTING).to_string();
            let idx = SORT_OPTIONS.iter().position(|&s| s == current).unwrap_or(0);
            let new_idx = (idx + 1) % SORT_OPTIONS.len();
            ctx.config
                .set_string(sk::PROC_SORTING, SORT_OPTIONS[new_idx]);
            *ctx.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        "t" if *ctx.proc_selected < ctx.proc_entries.len() => {
            // Terminate selected process
            if let Some(pid) = ctx.selected_proc_pid() {
                terminate_process(pid);
            }
            *ctx.dirty |= Dirty::PROC_BOX;
        }
        "enter" if *ctx.proc_selected < ctx.proc_entries.len() => {
            // Toggle process detailed view
            if let Some(pid) = ctx.selected_proc_pid() {
                let current_detailed = ctx.config.get_int(ik::DETAILED_PID);
                if current_detailed == pid as i64 {
                    ctx.config.set_int(ik::DETAILED_PID, 0);
                } else {
                    ctx.config.set_int(ik::DETAILED_PID, pid as i64);
                }
            }
            *ctx.dirty |= Dirty::PROC_BOX;
        }
        // Network keybinds
        "b" if ctx
            .snapshot
            .is_some_and(|snapshot| !snapshot.net.interfaces.is_empty()) =>
        {
            cycle_net_iface(ctx, -1);
            *ctx.dirty |= Dirty::NET_BOX;
        }
        "n" if ctx
            .snapshot
            .is_some_and(|snapshot| !snapshot.net.interfaces.is_empty()) =>
        {
            cycle_net_iface(ctx, 1);
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
        "z" if !ctx.selected_iface.is_empty() => {
            ctx.worker.reset_net_totals(ctx.selected_iface.clone());
            *ctx.dirty |= Dirty::NET_BOX;
        }
        // Update rate keybinds
        "+" => {
            let step = if *ctx.update_ms > 2000 { 1000 } else { 100 };
            let new_ms = (*ctx.update_ms as i64 + step).min(86_400_000);
            ctx.config.set_int(ik::UPDATE_MS, new_ms);
            *ctx.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
            ctx.worker.set_update_ms(*ctx.update_ms);
            *ctx.dirty |= Dirty::CPU_BOX;
        }
        "-" => {
            let step = if *ctx.update_ms > 2000 { 1000 } else { 100 };
            let new_ms = (*ctx.update_ms as i64 - step).max(100);
            ctx.config.set_int(ik::UPDATE_MS, new_ms);
            *ctx.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
            ctx.worker.set_update_ms(*ctx.update_ms);
            *ctx.dirty |= Dirty::CPU_BOX;
        }
        _ => {}
    }
    HandleResult::none()
}

fn sync_update_ms(ctx: &mut InputContext) {
    *ctx.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
    ctx.worker.set_update_ms(*ctx.update_ms);
}

fn cycle_net_iface(ctx: &mut InputContext, direction: isize) {
    let Some(snapshot) = ctx.snapshot else {
        return;
    };
    let interfaces = &snapshot.net.interfaces;
    if interfaces.is_empty() {
        return;
    }

    let current = interfaces
        .iter()
        .position(|iface| iface == ctx.selected_iface.as_str())
        .unwrap_or(0);
    let new_idx = if direction < 0 {
        current.checked_sub(1).unwrap_or(interfaces.len() - 1)
    } else {
        (current + 1) % interfaces.len()
    };
    *ctx.selected_iface = interfaces[new_idx].clone();
}

fn terminate_process(pid: u32) {
    use crate::collect::win::OwnedHandle;
    use windows::Win32::System::Threading::*;

    // SAFETY: OpenProcess returns a valid handle on success (checked by `Ok`).
    // TerminateProcess is safe with a valid process handle, and OwnedHandle
    // closes the handle on all paths.
    unsafe {
        if let Some(handle) = OpenProcess(PROCESS_TERMINATE, false, pid)
            .ok()
            .and_then(OwnedHandle::new)
        {
            let _ = TerminateProcess(handle.get(), 1);
        }
    }
}
