use crate::{
    collect::process_display::ProcSort,
    dirty::Dirty,
    handlers::{HandleResult, InputContext, MenuState},
    input::Key,
    menu, theme,
};

/// Handle input in normal (no-menu) mode.
pub(crate) fn handle(key: &Key, ctx: &mut InputContext) -> HandleResult {
    // When a terminate confirmation is pending, only t/T can confirm.
    // Any other key disarms and is consumed (no further action).
    if ctx.process.armed_terminate.is_some() && !matches!(key, Key::Char('t') | Key::Char('T')) {
        ctx.process.armed_terminate = None;
        ctx.render.dirty |= Dirty::PROC_BOX;
        return HandleResult::none();
    }

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
        Key::Char('?') | Key::F(1) => {
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
                let cur = ctx.config.current_preset;
                let next = if (cur + 1) >= presets.len() as i64 {
                    0i64
                } else {
                    cur + 1
                };
                ctx.config.current_preset = next;
                ctx.config.apply_preset(&presets[next as usize]);
                sync_update_ms(ctx);
                ctx.render.dirty |= Dirty::FULL;
            }
            Some(HandleResult::none())
        }
        Key::Char('P') => {
            let presets = ctx.config.preset_list();
            if !presets.is_empty() {
                let cur = ctx.config.current_preset;
                let next = if cur <= 0 {
                    presets.len() as i64 - 1
                } else {
                    cur - 1
                };
                ctx.config.current_preset = next;
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
        Key::CtrlX => {
            let cur = ctx.config.current_preset;
            if cur > 0 {
                ctx.config.delete_preset(cur as usize);
                let presets = ctx.config.preset_list();
                let new_cur = ctx.config.current_preset;
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
    let theme_name = ctx.config.color_theme.clone();
    *ctx.theme = theme::Theme::from_name(&theme_name);
    let base = ctx.theme.base_style(ctx.config.theme_background);
    ctx.runtime.rounded = ctx.config.rounded_corners;
    sync_update_ms(ctx);
    crate::log::set_level(ctx.config.log_level).expect("log level change must succeed");
    ctx.render.dirty |= Dirty::FULL;
    Some(HandleResult::raw(base))
}

// --- Process navigation ---

fn handle_process_nav(key: &Key, ctx: &mut InputContext) {
    let vim = ctx.config.vim_keys;

    // Any navigation key cancels Follow mode (btop behavior).
    let is_nav = matches!(
        key,
        Key::Up
            | Key::Down
            | Key::PageUp
            | Key::PageDown
            | Key::Home
            | Key::End
            | Key::Char('j' | 'k')
            | Key::CtrlB
            | Key::CtrlF
            | Key::CtrlD
            | Key::CtrlU
    );
    if is_nav && ctx.process.followed_pid > 0 {
        ctx.process.followed_pid = 0;
    }

    match *key {
        Key::Up if ctx.process.selected > 0 => {
            ctx.process.selected -= 1;
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Char('k') if vim && ctx.process.selected > 0 => {
            ctx.process.selected -= 1;
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Down => {
            let count = ctx.process.entries.len();
            if ctx.process.selected + 1 < count {
                ctx.process.selected += 1;
                ctx.render.dirty |= Dirty::PROC_BOX;
            }
        }
        Key::Char('j') if vim => {
            let count = ctx.process.entries.len();
            if ctx.process.selected + 1 < count {
                ctx.process.selected += 1;
                ctx.render.dirty |= Dirty::PROC_BOX;
            }
        }
        Key::PageUp | Key::CtrlB if !matches!(key, Key::CtrlB) || vim => {
            let page = ctx.th.saturating_sub(10);
            ctx.process.selected = ctx.process.selected.saturating_sub(page);
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::PageDown | Key::CtrlF if !matches!(key, Key::CtrlF) || vim => {
            let page = ctx.th.saturating_sub(10);
            let count = ctx.process.entries.len();
            ctx.process.selected = (ctx.process.selected + page).min(count.saturating_sub(1));
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::CtrlD if vim => {
            let page = ctx.th.saturating_sub(10);
            let half = page / 2;
            let count = ctx.process.entries.len();
            ctx.process.selected = (ctx.process.selected + half).min(count.saturating_sub(1));
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::CtrlU if vim => {
            let page = ctx.th.saturating_sub(10);
            let half = page / 2;
            ctx.process.selected = ctx.process.selected.saturating_sub(half);
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Home => {
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Char('g') if vim => {
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::End => {
            let count = ctx.process.entries.len();
            ctx.process.selected = count.saturating_sub(1);
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Char('G') if vim => {
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
            ctx.process.filter_text = ctx.config.proc_filter.clone();
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Char('e') => {
            ctx.config.proc_tree = !ctx.config.proc_tree;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Char('r') => {
            ctx.config.proc_reversed = !ctx.config.proc_reversed;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Char('c') => {
            ctx.config.proc_per_core = !ctx.config.proc_per_core;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Char('i') => {
            ctx.config.io_mode = !ctx.config.io_mode;
            ctx.render.dirty |= Dirty::DISK_BOX;
        }
        Key::Left => {
            let current = ctx.config.proc_sorting;
            let idx = ProcSort::ALL
                .iter()
                .position(|&s| s == current)
                .expect("config.proc_sorting must always be a known ProcSort variant");
            let new_idx = if idx == 0 {
                ProcSort::ALL.len() - 1
            } else {
                idx - 1
            };
            ctx.config.proc_sorting = ProcSort::ALL[new_idx];
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Right => {
            let current = ctx.config.proc_sorting;
            let idx = ProcSort::ALL
                .iter()
                .position(|&s| s == current)
                .expect("config.proc_sorting must always be a known ProcSort variant");
            let new_idx = (idx + 1) % ProcSort::ALL.len();
            ctx.config.proc_sorting = ProcSort::ALL[new_idx];
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_BOX;
        }
        Key::Char('t') => {
            if let Some((armed_pid, _, false)) = ctx.process.armed_terminate {
                graceful_terminate(armed_pid);
                ctx.process.armed_terminate = None;
            } else if let Some((pid, name)) = ctx.selected_proc_info() {
                ctx.process.armed_terminate = Some((pid, name.to_string(), false));
            }
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Char('T') => {
            if let Some((armed_pid, _, true)) = ctx.process.armed_terminate {
                terminate_process(armed_pid);
                ctx.process.armed_terminate = None;
            } else if let Some((pid, name)) = ctx.selected_proc_info() {
                ctx.process.armed_terminate = Some((pid, name.to_string(), true));
            }
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Char('F') if ctx.process.selected < ctx.process.entries.len() => {
            if let Some(pid) = ctx.selected_proc_pid() {
                if ctx.process.followed_pid == pid {
                    ctx.process.followed_pid = 0;
                } else {
                    ctx.process.followed_pid = pid;
                }
            }
            ctx.render.dirty |= Dirty::PROC_BOX;
        }
        Key::Enter if ctx.process.selected < ctx.process.entries.len() => {
            if let Some(pid) = ctx.selected_proc_pid() {
                let current_detailed = ctx.process.detailed_pid;
                if current_detailed == pid {
                    ctx.process.detailed_pid = 0;
                    ctx.process.followed_pid = 0;
                } else {
                    ctx.process.detailed_pid = pid;
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
        Key::Char(c @ '1'..='9') => {
            let digit = (c as u8) - b'0';
            match digit {
                crate::ui::CPU_KEY => ctx.config.toggle_box("cpu"),
                crate::ui::MEM_KEY => ctx.config.toggle_box("mem"),
                crate::ui::NET_KEY => ctx.config.toggle_box("net"),
                crate::ui::PROC_KEY => ctx.config.toggle_box("proc"),
                crate::ui::DISK_KEY => ctx.config.toggle_box("disk"),
                d if d >= crate::ui::GPU_KEY_BASE => {
                    let gpu_idx = (d - crate::ui::GPU_KEY_BASE) as usize;
                    ctx.config.toggle_box(&format!("gpu{gpu_idx}"))
                }
                _ => return,
            };
        }
        Key::Char('0') => {
            for i in 4..crate::config::MAX_GPUS {
                ctx.config.toggle_box(&format!("gpu{i}"));
            }
        }
        _ => return,
    };
    ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_BOXES;
}

// --- Network ---

fn handle_network(key: &Key, ctx: &mut InputContext) {
    match *key {
        Key::Char('b') if ctx.live.net.as_ref().is_some_and(|n| !n.nets.is_empty()) => {
            cycle_net_iface(ctx, -1);
            ctx.render.dirty |= Dirty::NET_BOX;
        }
        Key::Char('n') if ctx.live.net.as_ref().is_some_and(|n| !n.nets.is_empty()) => {
            cycle_net_iface(ctx, 1);
            ctx.render.dirty |= Dirty::NET_BOX;
        }
        Key::Char('a') => {
            ctx.config.net_auto = !ctx.config.net_auto;
            ctx.render.dirty |= Dirty::NET_BOX;
        }
        Key::Char('y') => {
            ctx.config.net_sync = !ctx.config.net_sync;
            ctx.render.dirty |= Dirty::NET_BOX;
        }
        Key::Char('z') if !ctx.network.selected_iface.is_empty() => {
            ctx.manager
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
    ctx.config.update_ms = new_ms;
    ctx.runtime.update_ms = ctx.config.update_ms as u64;
    sync_all_intervals(ctx);
    ctx.render.dirty |= Dirty::CPU_BOX;
}

// --- Helpers ---

/// Sync all collector intervals to their effective values.
///
/// Called when global `update_ms` changes — collectors using the default
/// (per-widget interval == 0) get the new global value, while collectors
/// with a custom per-widget interval keep their own.
pub(super) fn sync_all_intervals(ctx: &mut InputContext) {
    ctx.manager
        .set_cpu_interval(ctx.config.effective_interval(ctx.config.cpu_update_ms));
    ctx.manager
        .set_mem_interval(ctx.config.effective_interval(ctx.config.mem_update_ms));
    ctx.manager
        .set_disk_interval(ctx.config.effective_interval(ctx.config.disk_update_ms));
    ctx.manager
        .set_net_interval(ctx.config.effective_interval(ctx.config.net_update_ms));
    ctx.manager
        .set_gpu_interval(ctx.config.effective_interval(ctx.config.gpu_update_ms));
    ctx.manager
        .set_proc_interval(ctx.config.effective_interval(ctx.config.proc_update_ms));
}

fn sync_update_ms(ctx: &mut InputContext) {
    ctx.runtime.update_ms = ctx.config.update_ms as u64;
    sync_all_intervals(ctx);
}

fn cycle_net_iface(ctx: &mut InputContext, direction: isize) {
    let Some(net_snap) = ctx.live.net.as_ref() else {
        return;
    };
    let nets = &net_snap.nets;
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
    ctx.config.net_iface = ctx.network.selected_iface.clone();
}

/// Attempt graceful termination by sending WM_CLOSE to the process's
/// visible windows. If the process has no windows, does nothing — the
/// user can escalate to force kill with `T`.
fn graceful_terminate(pid: u32) {
    use windows::Win32::Foundation::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    struct CallbackData {
        target_pid: u32,
        found: bool,
    }

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let data = unsafe { &mut *(lparam.0 as *mut CallbackData) };
        let mut window_pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut window_pid)) };
        if window_pid == data.target_pid && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            data.found = true;
        }
        TRUE
    }

    let mut data = CallbackData {
        target_pid: pid,
        found: false,
    };

    // SAFETY: enum_callback receives a valid pointer to stack-allocated data.
    // EnumWindows iterates all top-level windows; we filter by PID.
    unsafe {
        let _ = EnumWindows(Some(enum_callback), LPARAM(&mut data as *mut _ as isize));
    }

    if !data.found {
        tracing::debug!(
            "graceful terminate skipped for pid {pid}: no visible window to send WM_CLOSE (use T to force kill)"
        );
    }
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
            && TerminateProcess(handle.get(), 1).is_err()
        {
            tracing::warn!("Process: TerminateProcess failed for pid {pid}");
        }
    }
}
