use crate::{
    collect::process_display::SORT_OPTIONS,
    config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk},
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
    input::Key,
    menu, theme,
};

/// Handle input in normal (no-menu) mode.
pub(crate) fn handle(key: &Key, ctx: &mut InputContext) -> HandleResult {
    if let Some(result) = handle_quit_and_menus(key, ctx) {
        return result;
    }
    if let Some(result) = handle_presets(key, ctx) {
        return result;
    }
    if let Some(result) = handle_config_reload(key, ctx) {
        return result;
    }
    handle_process_nav(key, ctx);
    handle_process_keys(key, ctx);
    handle_box_toggles(key, ctx);
    handle_network(key, ctx);
    handle_update_rate(key, ctx);
    HandleResult::none()
}

// --- Quit / menu transitions ---

fn handle_quit_and_menus(key: &Key, ctx: &mut InputContext) -> Option<HandleResult> {
    match *key {
        Key::Char('q') => Some(HandleResult::quit()),
        Key::Escape | Key::Char('m') => {
            ctx.overlay.main_menu_selected = 0;
            let menu_out = menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                ctx.overlay.main_menu_selected,
                ctx.theme,
            );
            ctx.overlay.set_menu_state(MenuState::Main);
            Some(HandleResult::raw(menu_out))
        }
        Key::Char('h') | Key::Char('?') | Key::F(1) => {
            let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, ctx.runtime.rounded);
            ctx.overlay.menu_return_to = MenuState::None;
            ctx.overlay.set_menu_state(MenuState::Help);
            Some(HandleResult::raw(menu_out))
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
            ctx.overlay.set_menu_state(MenuState::Options);
            Some(HandleResult::raw(menu_out))
        }
        _ => None,
    }
}

// --- Presets ---

fn handle_presets(key: &Key, ctx: &mut InputContext) -> Option<HandleResult> {
    match *key {
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
            Some(HandleResult::none())
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
            Some(HandleResult::none())
        }
        Key::CtrlS => {
            ctx.config.save_preset();
            ctx.render.dirty |= Dirty::CPU_BOX;
            Some(HandleResult::none())
        }
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
            Some(HandleResult::none())
        }
        _ => None,
    }
}

// --- Config reload ---

fn handle_config_reload(key: &Key, ctx: &mut InputContext) -> Option<HandleResult> {
    if *key != Key::CtrlR {
        return None;
    }
    let warnings = ctx.config.reload();
    for w in &warnings {
        tracing::warn!("{}", w);
    }
    let theme_name = ctx.config.get_string(sk::COLOR_THEME).to_string();
    *ctx.theme = theme::Theme::from_name(&theme_name);
    let base = ctx
        .theme
        .base_style(ctx.config.get_bool(bk::THEME_BACKGROUND));
    ctx.runtime.rounded = ctx.config.get_bool(bk::ROUNDED_CORNERS);
    sync_update_ms(ctx);
    ctx.render.dirty |= Dirty::FULL;
    Some(HandleResult::raw(base))
}

// --- Process navigation ---

fn handle_process_nav(key: &Key, ctx: &mut InputContext) {
    match *key {
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
        _ => {}
    }
}

// --- Process modes, sorting, and actions ---

fn handle_process_keys(key: &Key, ctx: &mut InputContext) {
    match *key {
        Key::Char('f') | Key::Char('/') => {
            ctx.overlay.set_menu_state(MenuState::Filter);
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
            if let Some(pid) = ctx.selected_proc_pid() {
                terminate_process(pid);
            }
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Enter if ctx.process.selected < ctx.process.entries.len() => {
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
        _ => {}
    }
}

// --- Box visibility toggles ---

fn handle_box_toggles(key: &Key, ctx: &mut InputContext) {
    match *key {
        Key::Char('1') => ctx.config.toggle_box("cpu"),
        Key::Char('2') => ctx.config.toggle_box("mem"),
        Key::Char('3') => ctx.config.toggle_box("net"),
        Key::Char('4') => ctx.config.toggle_box("proc"),
        Key::Char('5') => {
            let gpu_count = ctx.snapshot.map_or(0, |s| s.gpu.gpus.len());
            for i in 0..gpu_count {
                ctx.config.toggle_box(&format!("gpu{i}"));
            }
            ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
            return;
        }
        Key::Char('6') => ctx.config.toggle_box("disk"),
        _ => return,
    };
    ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
}

// --- Network ---

fn handle_network(key: &Key, ctx: &mut InputContext) {
    match *key {
        Key::Char('b') if ctx.snapshot.is_some_and(|s| !s.net.nets.is_empty()) => {
            cycle_net_iface(ctx, -1);
            ctx.render.dirty |= Dirty::NET_BOX;
        }
        Key::Char('n') if ctx.snapshot.is_some_and(|s| !s.net.nets.is_empty()) => {
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
        Key::Char('z') if !ctx.network.selected_iface.is_empty() => {
            ctx.worker
                .reset_net_totals(ctx.network.selected_iface.clone());
            ctx.render.dirty |= Dirty::NET_BOX;
        }
        _ => {}
    }
}

// --- Update rate ---

fn handle_update_rate(key: &Key, ctx: &mut InputContext) {
    let delta: i64 = match *key {
        Key::Char('+') => 1,
        Key::Char('-') => -1,
        _ => return,
    };
    let step = if ctx.runtime.update_ms > 2000 {
        1000
    } else {
        100
    };
    let new_ms = (ctx.runtime.update_ms as i64 + delta * step).clamp(100, 86_400_000);
    ctx.config.set_int(ik::UPDATE_MS, new_ms);
    ctx.runtime.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
    ctx.worker.set_update_ms(ctx.runtime.update_ms);
    ctx.render.dirty |= Dirty::CPU_BOX;
}

// --- Helpers ---

fn sync_update_ms(ctx: &mut InputContext) {
    ctx.runtime.update_ms = ctx.config.get_int(ik::UPDATE_MS) as u64;
    ctx.worker.set_update_ms(ctx.runtime.update_ms);
}

fn cycle_net_iface(ctx: &mut InputContext, direction: isize) {
    let Some(snapshot) = ctx.snapshot else {
        return;
    };
    let nets = &snapshot.net.nets;
    if nets.is_empty() {
        return;
    }

    let current = nets
        .iter()
        .position(|n| n.name == ctx.network.selected_iface)
        .unwrap_or(0);
    let new_idx = if direction < 0 {
        current.checked_sub(1).unwrap_or(nets.len() - 1)
    } else {
        (current + 1) % nets.len()
    };
    ctx.network.selected_iface = nets[new_idx].name.clone();
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
