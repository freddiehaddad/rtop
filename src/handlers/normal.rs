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
        ctx.render.dirty |= Dirty::PROC_WIDGET;
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
    handle_widget_toggles(key, ctx);
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
            tracing::debug!(
                subsystem = %crate::log::Subsystem::Ui,
                menu = "main",
                opened = true,
                "menu transition",
            );
            Some(HandleResult::raw(menu_out))
        }
        Key::Char('?') | Key::F(1) => {
            let menu_out = menu::help_menu::draw(ctx.tw, ctx.th, ctx.theme, ctx.runtime.rounded);
            ctx.overlay.menu_return_to = MenuState::None;
            ctx.overlay.set_menu_state(MenuState::Help);
            tracing::debug!(
                subsystem = %crate::log::Subsystem::Ui,
                menu = "help",
                opened = true,
                "menu transition",
            );
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
            tracing::debug!(
                subsystem = %crate::log::Subsystem::Ui,
                menu = "options",
                opened = true,
                "menu transition",
            );
            Some(HandleResult::raw(menu_out))
        }
        _ => None,
    }
}

// --- Presets ---

fn handle_presets(key: &Key, ctx: &mut InputContext) -> Option<HandleResult> {
    let delta = match *key {
        Key::Char('p') => 1,
        Key::Char('P') => -1,
        _ => return None,
    };
    ctx.config.cycle_preset(delta);
    sync_update_ms(ctx);
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "preset_cycle",
        preset = ctx.config.current_preset,
        "preset action",
    );
    ctx.render.dirty |= Dirty::FULL;
    Some(HandleResult::none())
}

// --- Config reload ---

fn handle_config_reload(key: &Key, ctx: &mut InputContext) -> Option<HandleResult> {
    if *key != Key::CtrlR {
        return None;
    }
    let warnings = ctx.config.reload();
    for w in &warnings {
        tracing::warn!(
            subsystem = %crate::log::Subsystem::Config,
            warning = %w,
            "config reload warning",
        );
    }
    let theme_name = ctx.config.color_theme.clone();
    *ctx.theme = theme::Theme::from_name(&theme_name);
    let base = ctx.theme.base_style(ctx.config.theme_background);
    ctx.runtime.rounded = ctx.config.rounded_corners;
    sync_update_ms(ctx);
    crate::log::set_level(ctx.config.log_level).expect("log level change must succeed");
    tracing::info!(subsystem = %crate::log::Subsystem::Config, "config reloaded");
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
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::Char('k') if vim && ctx.process.selected > 0 => {
            ctx.process.selected -= 1;
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::Down => {
            let count = ctx.process.entries.len();
            if ctx.process.selected + 1 < count {
                ctx.process.selected += 1;
                ctx.render.dirty |= Dirty::PROC_WIDGET;
            }
        }
        Key::Char('j') if vim => {
            let count = ctx.process.entries.len();
            if ctx.process.selected + 1 < count {
                ctx.process.selected += 1;
                ctx.render.dirty |= Dirty::PROC_WIDGET;
            }
        }
        Key::PageUp | Key::CtrlB if !matches!(key, Key::CtrlB) || vim => {
            let page = ctx.th.saturating_sub(10);
            ctx.process.selected = ctx.process.selected.saturating_sub(page);
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::PageDown | Key::CtrlF if !matches!(key, Key::CtrlF) || vim => {
            let page = ctx.th.saturating_sub(10);
            let count = ctx.process.entries.len();
            ctx.process.selected = (ctx.process.selected + page).min(count.saturating_sub(1));
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::CtrlD if vim => {
            let page = ctx.th.saturating_sub(10);
            let half = page / 2;
            let count = ctx.process.entries.len();
            ctx.process.selected = (ctx.process.selected + half).min(count.saturating_sub(1));
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::CtrlU if vim => {
            let page = ctx.th.saturating_sub(10);
            let half = page / 2;
            ctx.process.selected = ctx.process.selected.saturating_sub(half);
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::Home => {
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::Char('g') if vim => {
            ctx.process.selected = 0;
            ctx.process.start = 0;
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::End => {
            let count = ctx.process.entries.len();
            ctx.process.selected = count.saturating_sub(1);
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::Char('G') if vim => {
            let count = ctx.process.entries.len();
            ctx.process.selected = count.saturating_sub(1);
            ctx.render.dirty |= Dirty::PROC_WIDGET;
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
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::Char('e') => {
            ctx.config.proc_tree = !ctx.config.proc_tree;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
        }
        Key::Char('r') => {
            ctx.config.proc_reversed = !ctx.config.proc_reversed;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
        }
        Key::Char('c') => {
            ctx.config.proc_per_core = !ctx.config.proc_per_core;
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
        }
        Key::Char('i') => {
            ctx.config.io_mode = !ctx.config.io_mode;
            ctx.render.dirty |= Dirty::DISK_WIDGET;
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
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
        }
        Key::Right => {
            let current = ctx.config.proc_sorting;
            let idx = ProcSort::ALL
                .iter()
                .position(|&s| s == current)
                .expect("config.proc_sorting must always be a known ProcSort variant");
            let new_idx = (idx + 1) % ProcSort::ALL.len();
            ctx.config.proc_sorting = ProcSort::ALL[new_idx];
            ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
        }
        Key::Char('t') => {
            if let Some((armed_pid, _, false)) = ctx.process.armed_terminate {
                tracing::info!(
                    subsystem = %crate::log::Subsystem::Input,
                    action = "process_terminate",
                    pid = armed_pid,
                    "graceful terminate requested",
                );
                graceful_terminate(armed_pid);
                ctx.process.armed_terminate = None;
            } else if let Some((pid, name)) = ctx.selected_proc_info() {
                ctx.process.armed_terminate = Some((pid, name.to_string(), false));
            }
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::Char('T') => {
            if let Some((armed_pid, _, true)) = ctx.process.armed_terminate {
                tracing::info!(
                    subsystem = %crate::log::Subsystem::Input,
                    action = "process_kill",
                    pid = armed_pid,
                    "kill requested",
                );
                terminate_process(armed_pid);
                ctx.process.armed_terminate = None;
            } else if let Some((pid, name)) = ctx.selected_proc_info() {
                ctx.process.armed_terminate = Some((pid, name.to_string(), true));
            }
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        Key::Char('F') if ctx.process.selected < ctx.process.entries.len() => {
            if let Some(pid) = ctx.selected_proc_pid() {
                if ctx.process.followed_pid == pid {
                    ctx.process.followed_pid = 0;
                } else {
                    ctx.process.followed_pid = pid;
                }
            }
            ctx.render.dirty |= Dirty::PROC_WIDGET;
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
            ctx.render.dirty |= Dirty::PROC_WIDGET;
        }
        _ => {}
    }
}

// --- Widget visibility toggles ---

fn handle_widget_toggles(key: &Key, ctx: &mut InputContext) {
    use crate::domain::widget_kind::WidgetKind;

    let kind: WidgetKind = match *key {
        Key::Char(c @ '1'..='9') => {
            let digit = (c as u8) - b'0';
            match digit {
                crate::ui::CPU_KEY => WidgetKind::Cpu,
                crate::ui::MEM_KEY => WidgetKind::Mem,
                crate::ui::NET_KEY => WidgetKind::Net,
                crate::ui::PROC_KEY => WidgetKind::Proc,
                crate::ui::DISK_KEY => WidgetKind::Disk,
                d if d >= crate::ui::GPU_KEY_BASE => {
                    let gpu_idx = (d - crate::ui::GPU_KEY_BASE) as usize;
                    let Some(gpu_kind) = WidgetKind::gpu(gpu_idx) else {
                        return;
                    };
                    gpu_kind
                }
                _ => return,
            }
        }
        Key::Char('0') => {
            // The `0` key toggles the second half of GPU slots
            // (indices 4..MAX_GPUS) as a single batch action so users
            // with many GPUs can collapse them quickly.
            for i in 4..crate::config::MAX_GPUS {
                if let Some(gpu_kind) = WidgetKind::gpu(i) {
                    ctx.config.toggle_widget(gpu_kind);
                }
            }
            tracing::info!(
                subsystem = %crate::log::Subsystem::Input,
                action = "widget_toggle",
                r#widget = "gpu_extras",
                "widget visibility toggled",
            );
            ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
            return;
        }
        _ => return,
    };
    ctx.config.toggle_widget(kind);
    let shown = ctx.config.widgets().contains(&kind);
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "widget_toggle",
        r#widget = %kind,
        shown,
        "widget visibility toggled",
    );
    ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
}

// --- Network ---

fn handle_network(key: &Key, ctx: &mut InputContext) {
    match *key {
        Key::Char('b') if ctx.live.net.as_ref().is_some_and(|n| !n.nets.is_empty()) => {
            cycle_net_iface(ctx, -1);
            tracing::info!(
                subsystem = %crate::log::Subsystem::Input,
                action = "net_iface_cycle",
                iface = %ctx.network.selected_iface,
                "network interface switched",
            );
            ctx.render.dirty |= Dirty::NET_WIDGET;
        }
        Key::Char('n') if ctx.live.net.as_ref().is_some_and(|n| !n.nets.is_empty()) => {
            cycle_net_iface(ctx, 1);
            tracing::info!(
                subsystem = %crate::log::Subsystem::Input,
                action = "net_iface_cycle",
                iface = %ctx.network.selected_iface,
                "network interface switched",
            );
            ctx.render.dirty |= Dirty::NET_WIDGET;
        }
        Key::Char('a') => {
            ctx.config.net_auto = !ctx.config.net_auto;
            ctx.render.dirty |= Dirty::NET_WIDGET;
        }
        Key::Char('y') => {
            ctx.config.net_sync = !ctx.config.net_sync;
            ctx.render.dirty |= Dirty::NET_WIDGET;
        }
        Key::Char('z') if !ctx.network.selected_iface.is_empty() => {
            ctx.manager
                .reset_net_totals(ctx.network.selected_iface.clone());
            ctx.render.dirty |= Dirty::NET_WIDGET;
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
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "update_rate",
        update_ms = new_ms,
        "update interval changed",
    );
    ctx.render.dirty |= Dirty::CPU_WIDGET;
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
            if let Err(e) = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
                tracing::warn!(
                    subsystem = %crate::log::Subsystem::Process,
                    pid = data.target_pid,
                    error = %e,
                    "PostMessageW(WM_CLOSE) failed",
                );
            }
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
        if let Err(e) = EnumWindows(Some(enum_callback), LPARAM(&mut data as *mut _ as isize)) {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Process,
                pid,
                error = %e,
                "EnumWindows failed during graceful terminate",
            );
        }
    }

    if !data.found {
        tracing::debug!(
            subsystem = %crate::log::Subsystem::Process,
            pid,
            "graceful terminate skipped: no visible window",
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
        {
            if TerminateProcess(handle.get(), 1).is_err() {
                tracing::warn!(
                    subsystem = %crate::log::Subsystem::Process,
                    pid,
                    "TerminateProcess failed",
                );
            } else {
                tracing::info!(
                    subsystem = %crate::log::Subsystem::Process,
                    pid,
                    "process terminated",
                );
            }
        } else {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Process,
                pid,
                "OpenProcess failed",
            );
        }
    }
}
