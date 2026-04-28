use crate::{
    collect::process::SORT_OPTIONS,
    config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk},
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
    input::Key,
    menu, theme,
};

/// Handle input in normal (no-menu) mode.
pub(crate) fn handle(key: &Key, ctx: &mut InputContext) -> HandleResult {
    match *key {
        Key::Char('q') => return HandleResult::quit(),
        Key::Escape | Key::Char('m') => {
            ctx.overlay.main_menu_selected = 0;
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                ctx.overlay.main_menu_selected,
                ctx.theme,
            );
            ctx.overlay.menu_state = MenuState::Main;
            return HandleResult::raw(menu_out);
        }
        Key::Char('h') | Key::Char('?') | Key::F(1) => {
            let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, ctx.runtime.rounded);
            ctx.overlay.menu_return_to = MenuState::None;
            ctx.overlay.menu_state = MenuState::Help;
            return HandleResult::raw(menu_out);
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
            ctx.overlay.menu_return_to = MenuState::None;
            ctx.overlay.menu_state = MenuState::Options;
            return HandleResult::raw(menu_out);
        }
        // Preset cycling
        Key::Char('p') => {
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
                ctx.render.dirty |= Dirty::FULL;
            }
        }
        Key::Char('P') => {
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
                ctx.render.dirty |= Dirty::FULL;
            }
        }
        // Save current layout as preset
        Key::CtrlS => {
            ctx.config.save_preset();
            ctx.render.dirty |= Dirty::CPU_BOX;
        }
        // Delete current preset
        Key::CtrlD => {
            let cur = ctx.config.get_int(ik::CURRENT_PRESET);
            if cur > 0 {
                ctx.config.delete_preset(cur as usize);
                let presets = ctx.config.preset_list();
                let new_cur = ctx.config.get_int(ik::CURRENT_PRESET);
                if !presets.is_empty() && (new_cur as usize) < presets.len() {
                    ctx.config.apply_preset(&presets[new_cur as usize]);
                    sync_update_ms(ctx);
                }
                ctx.render.dirty |= Dirty::FULL;
            }
        }
        // Config reload
        Key::CtrlR => {
            let warnings = ctx.config.reload();
            for w in &warnings {
                tracing::warn!("{}", w);
            }
            // Reapply theme
            let theme_name = ctx.config.get_string(sk::COLOR_THEME).to_string();
            *ctx.theme = theme::Theme::from_name(&theme_name);
            let base = ctx
                .theme
                .base_style(ctx.config.get_bool(bk::THEME_BACKGROUND));
            ctx.runtime.rounded = ctx.config.get_bool(bk::ROUNDED_CORNERS);
            sync_update_ms(ctx);
            ctx.render.dirty |= Dirty::FULL;
            return HandleResult::raw(base);
        }
        Key::Up | Key::Char('k') if ctx.process.selected > 0 => {
            ctx.process.selected -= 1;
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Down | Key::Char('j') => {
            let count = ctx.process.entries.len();
            if ctx.process.selected + 1 < count {
                ctx.process.selected += 1;
                ctx.render.dirty |= Dirty::PROC_BOX;
            }
        }
        Key::PageUp => {
            let page = ctx.th.saturating_sub(10);
            ctx.process.selected = ctx.process.selected.saturating_sub(page);
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::PageDown => {
            let page = ctx.th.saturating_sub(10);
            let count = ctx.process.entries.len();
            ctx.process.selected = (ctx.process.selected + page).min(count.saturating_sub(1));
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Home | Key::Char('g') => {
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::End | Key::Char('G') => {
            let count = ctx.process.entries.len();
            ctx.process.selected = count.saturating_sub(1);
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Char('1') => {
            ctx.config.toggle_box("cpu");
            ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        Key::Char('2') => {
            ctx.config.toggle_box("mem");
            ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        Key::Char('3') => {
            ctx.config.toggle_box("net");
            ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        Key::Char('4') => {
            ctx.config.toggle_box("proc");
            ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        Key::Char('5') => {
            // Toggle all detected GPU boxes
            let gpu_count = ctx.snapshot.map_or(0, |snapshot| snapshot.gpu.gpus.len());
            for i in 0..gpu_count {
                ctx.config.toggle_box(&format!("gpu{i}"));
            }
            ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        Key::Char('6') => {
            ctx.config.toggle_box("disk");
            ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
        }
        // Process keybinds
        Key::Char('f') | Key::Char('/') => {
            ctx.overlay.menu_state = MenuState::Filter;
            ctx.process.filter_text = ctx.config.get_string(sk::PROC_FILTER).to_string();
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Char('e') => {
            ctx.config.flip(bk::PROC_TREE);
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Char('r') => {
            ctx.config.flip(bk::PROC_REVERSED);
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Char('c') => {
            ctx.config.flip(bk::PROC_PER_CORE);
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Char('i') => {
            ctx.config.flip(bk::IO_MODE);
            ctx.render.dirty |= Dirty::MEM_BOX | Dirty::PROC_BOX;
        }
        Key::Left => {
            let current = ctx.config.get_string(sk::PROC_SORTING).to_string();
            let idx = SORT_OPTIONS.iter().position(|&s| s == current).unwrap_or(0);
            let new_idx = if idx == 0 {
                SORT_OPTIONS.len() - 1
            } else {
                idx - 1
            };
            ctx.config
                .set_string(sk::PROC_SORTING, SORT_OPTIONS[new_idx]);
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Right => {
            let current = ctx.config.get_string(sk::PROC_SORTING).to_string();
            let idx = SORT_OPTIONS.iter().position(|&s| s == current).unwrap_or(0);
            let new_idx = (idx + 1) % SORT_OPTIONS.len();
            ctx.config
                .set_string(sk::PROC_SORTING, SORT_OPTIONS[new_idx]);
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Char('t') if ctx.process.selected < ctx.process.entries.len() => {
            // Terminate selected process
            if let Some(pid) = ctx.selected_proc_pid() {
                terminate_process(pid);
            }
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Enter if ctx.process.selected < ctx.process.entries.len() => {
            // Toggle process detailed view
            if let Some(pid) = ctx.selected_proc_pid() {
                let current_detailed = ctx.config.get_int(ik::DETAILED_PID);
                if current_detailed == pid as i64 {
                    ctx.config.set_int(ik::DETAILED_PID, 0);
                } else {
                    ctx.config.set_int(ik::DETAILED_PID, pid as i64);
                }
            }
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        // Network keybinds
        Key::Char('b')
            if ctx
                .snapshot
                .is_some_and(|snapshot| !snapshot.net.interfaces.is_empty()) =>
        {
            cycle_net_iface(ctx, -1);
            ctx.render.dirty |= Dirty::NET_BOX;
        }
        Key::Char('n')
            if ctx
                .snapshot
                .is_some_and(|snapshot| !snapshot.net.interfaces.is_empty()) =>
        {
            cycle_net_iface(ctx, 1);
            ctx.render.dirty |= Dirty::NET_BOX;
        }
        Key::Char('a') => {
            ctx.config.flip(bk::NET_AUTO);
            ctx.render.dirty |= Dirty::NET_BOX;
        }
        Key::Char('y') => {
            ctx.config.flip(bk::NET_SYNC);
            ctx.render.dirty |= Dirty::NET_BOX;
        }
        // Network zero reset
        Key::Char('z') if !ctx.network.selected_iface.is_empty() => {
            ctx.worker
                .reset_net_totals(ctx.network.selected_iface.clone());
            ctx.render.dirty |= Dirty::NET_BOX;
        }
        // Update rate keybinds
        Key::Char('+') => {
            let step = if ctx.runtime.update_ms > 2000 {
                1000
            } else {
                100
            };
            let new_ms = (ctx.runtime.update_ms as i64 + step).min(86_400_000);
            ctx.config.set_int(ik::UPDATE_MS, new_ms);
            ctx.runtime.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
            ctx.worker.set_update_ms(ctx.runtime.update_ms);
            ctx.render.dirty |= Dirty::CPU_BOX;
        }
        Key::Char('-') => {
            let step = if ctx.runtime.update_ms > 2000 {
                1000
            } else {
                100
            };
            let new_ms = (ctx.runtime.update_ms as i64 - step).max(100);
            ctx.config.set_int(ik::UPDATE_MS, new_ms);
            ctx.runtime.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
            ctx.worker.set_update_ms(ctx.runtime.update_ms);
            ctx.render.dirty |= Dirty::CPU_BOX;
        }
        _ => {}
    }
    HandleResult::none()
}

fn sync_update_ms(ctx: &mut InputContext) {
    ctx.runtime.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
    ctx.worker.set_update_ms(ctx.runtime.update_ms);
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
        .position(|iface| iface == ctx.network.selected_iface.as_str())
        .unwrap_or(0);
    let new_idx = if direction < 0 {
        current.checked_sub(1).unwrap_or(interfaces.len() - 1)
    } else {
        (current + 1) % interfaces.len()
    };
    ctx.network.selected_iface = interfaces[new_idx].clone();
}

fn terminate_process(pid: u32) {
    use crate::collect::win::OwnedHandle;
    use windows::Win32::System::Threading::*;

    // SAFETY: OpenProcess returns a valid handle on success (checked by `Ok`).
    // TerminateProcess receives that valid process handle, its result is
    // checked, and OwnedHandle closes the handle on all paths.
    unsafe {
        if let Some(handle) = OpenProcess(PROCESS_TERMINATE, false, pid)
            .ok()
            .and_then(OwnedHandle::new)
        {
            if TerminateProcess(handle.get(), 1).is_err() {
                tracing::warn!("Process: TerminateProcess failed for pid {pid}");
            }
        }
    }
}
