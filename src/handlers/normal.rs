//! Per-action handlers for "normal" (no-overlay) mode and the
//! Win32 helpers shared by them.
//!
//! Every public function with the suffix `_action` is referenced
//! by [`crate::handlers::keybinds::BINDINGS`]; signatures match
//! [`crate::handlers::keybinds::ActionFn`].

use crate::{
    collect::process_display::ProcSort,
    dirty::Dirty,
    domain::widget_kind::WidgetKind,
    event::SubsystemKind,
    handlers::{HandleResult, InputContext, MenuState},
    input::Key,
    menu, theme,
};

// ---------------------------------------------------------------------------
// Quit / menu transitions
// ---------------------------------------------------------------------------

pub(super) fn quit_action(_ctx: &mut InputContext, _key: &Key) -> HandleResult {
    HandleResult::quit()
}

pub(super) fn open_main_menu_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.overlay.main_menu_selected = 0;
    let menu_out = menu::main_menu::draw_with_selection(
        ctx.size.width,
        ctx.size.height,
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
    HandleResult::raw(menu_out)
}

pub(super) fn open_help_menu_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let menu_out = menu::help_menu::draw(
        ctx.size.width,
        ctx.size.height,
        ctx.theme,
        ctx.config.ui.rounded_corners,
    );
    ctx.overlay.menu_return_to = MenuState::None;
    ctx.overlay.set_menu_state(MenuState::Help);
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "help",
        opened = true,
        "menu transition",
    );
    HandleResult::raw(menu_out)
}

pub(super) fn open_options_menu_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.overlay.options_cat = 0;
    ctx.overlay.options_selected = 0;
    ctx.overlay.options_page = 0;
    // Sync RuntimeView -> config.view so the menu shows current
    // values for runtime-toggle keys (proc_tree, io_mode,
    // net_iface, etc.).
    ctx.view.sync_to_config(&mut ctx.config.view);
    let menu_out = menu::options_menu::draw(&menu::options_menu::DrawParams {
        term_width: ctx.size.width,
        term_height: ctx.size.height,
        cat: ctx.overlay.options_cat,
        selected: ctx.overlay.options_selected,
        page: ctx.overlay.options_page,
        config: ctx.config,
        theme: ctx.theme,
        option_edit: None,
    });
    ctx.overlay.menu_return_to = MenuState::None;
    ctx.overlay.set_menu_state(MenuState::Options);
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "options",
        opened = true,
        "menu transition",
    );
    HandleResult::raw(menu_out)
}

// ---------------------------------------------------------------------------
// Presets, config reload, update rate
// ---------------------------------------------------------------------------

pub(super) fn preset_forward_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    cycle_preset(ctx, true)
}

pub(super) fn preset_back_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    cycle_preset(ctx, false)
}

fn cycle_preset(ctx: &mut InputContext, forward: bool) -> HandleResult {
    ctx.config.cycle_preset(forward);
    sync_update_ms(ctx);
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "preset_cycle",
        preset = ctx.config.preset.active().name(),
        "preset action",
    );
    ctx.render.dirty |= Dirty::FULL;
    HandleResult::none()
}

pub(super) fn config_reload_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let warnings = ctx.config.reload();
    for w in &warnings {
        tracing::warn!(
            subsystem = %crate::log::Subsystem::Config,
            warning = %w,
            "config reload warning",
        );
    }
    let theme_name = ctx.config.ui.color_theme.clone();
    *ctx.theme = theme::Theme::from_name(&theme_name);
    let base = ctx.theme.base_style(ctx.config.ui.theme_background);
    sync_update_ms(ctx);
    crate::log::set_level(ctx.config.log.log_level).expect("log level change must succeed");
    // Re-initialise RuntimeView from the freshly loaded config so
    // runtime-toggle state reflects the on-disk values (otherwise
    // we'd carry the previous session's runtime values forward
    // and the user's edits to `rtop.toml` would be lost).
    ctx.view.sync_from_config(&ctx.config.view);
    // Reload may load a different active layout; the runtime view
    // filter no longer applies to widgets the user didn't choose
    // to hide. Treat reload as a fresh slate and clear the filter.
    ctx.filter.hidden.clear();
    tracing::info!(subsystem = %crate::log::Subsystem::Config, "config reloaded");
    ctx.render.dirty |= Dirty::FULL;
    HandleResult::raw(base)
}

pub(super) fn update_rate_up_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    step_update_rate(ctx, 1)
}

pub(super) fn update_rate_down_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    step_update_rate(ctx, -1)
}

fn step_update_rate(ctx: &mut InputContext, delta: i64) -> HandleResult {
    let step = if ctx.config.refresh.update_ms > 2000 {
        1000
    } else {
        100
    };
    let new_ms = (ctx.config.refresh.update_ms + delta * step).clamp(100, 86_400_000);
    ctx.config.refresh.update_ms = new_ms;
    sync_all_intervals(ctx);
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "update_rate",
        update_ms = new_ms,
        "update interval changed",
    );
    ctx.render.dirty |= Dirty::CPU_WIDGET;
    HandleResult::none()
}

// ---------------------------------------------------------------------------
// Process navigation
// ---------------------------------------------------------------------------

pub(super) fn nav_up_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    if ctx.process.selected > 0 {
        ctx.process.selected -= 1;
        ctx.render.dirty |= Dirty::PROC_WIDGET;
    }
    HandleResult::none()
}

pub(super) fn nav_down_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let count = ctx.process.entries.len();
    if ctx.process.selected + 1 < count {
        ctx.process.selected += 1;
        ctx.render.dirty |= Dirty::PROC_WIDGET;
    }
    HandleResult::none()
}

pub(super) fn nav_page_up_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let page = ctx.size.height.saturating_sub(10);
    ctx.process.selected = ctx.process.selected.saturating_sub(page);
    ctx.render.dirty |= Dirty::PROC_WIDGET;
    HandleResult::none()
}

pub(super) fn nav_page_down_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let page = ctx.size.height.saturating_sub(10);
    let count = ctx.process.entries.len();
    ctx.process.selected = (ctx.process.selected + page).min(count.saturating_sub(1));
    ctx.render.dirty |= Dirty::PROC_WIDGET;
    HandleResult::none()
}

pub(super) fn nav_half_page_down_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let page = ctx.size.height.saturating_sub(10);
    let half = page / 2;
    let count = ctx.process.entries.len();
    ctx.process.selected = (ctx.process.selected + half).min(count.saturating_sub(1));
    ctx.render.dirty |= Dirty::PROC_WIDGET;
    HandleResult::none()
}

pub(super) fn nav_half_page_up_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let page = ctx.size.height.saturating_sub(10);
    let half = page / 2;
    ctx.process.selected = ctx.process.selected.saturating_sub(half);
    ctx.render.dirty |= Dirty::PROC_WIDGET;
    HandleResult::none()
}

pub(super) fn nav_home_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.process.selected = 0;
    ctx.process.start = 0;
    ctx.render.dirty |= Dirty::PROC_WIDGET;
    HandleResult::none()
}

pub(super) fn nav_end_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let count = ctx.process.entries.len();
    ctx.process.selected = count.saturating_sub(1);
    ctx.render.dirty |= Dirty::PROC_WIDGET;
    HandleResult::none()
}

// ---------------------------------------------------------------------------
// Process modes, sorting, and actions
// ---------------------------------------------------------------------------

pub(super) fn open_filter_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.overlay.set_menu_state(MenuState::Filter);
    ctx.process.filter_text = ctx.view.proc_filter.clone();
    ctx.render.dirty |= Dirty::PROC_WIDGET;
    HandleResult::none()
}

pub(super) fn toggle_tree_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.view.proc_tree = !ctx.view.proc_tree;
    ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
    HandleResult::none()
}

pub(super) fn toggle_reverse_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.view.proc_reversed = !ctx.view.proc_reversed;
    ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
    HandleResult::none()
}

pub(super) fn toggle_per_core_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.view.proc_per_core = !ctx.view.proc_per_core;
    ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
    HandleResult::none()
}

pub(super) fn toggle_io_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.view.io_mode = !ctx.view.io_mode;
    ctx.render.dirty |= Dirty::DISK_WIDGET;
    HandleResult::none()
}

pub(super) fn sort_back_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    cycle_sort(ctx, -1)
}

pub(super) fn sort_forward_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    cycle_sort(ctx, 1)
}

fn cycle_sort(ctx: &mut InputContext, dir: isize) -> HandleResult {
    let current = ctx.view.proc_sorting;
    let idx = ProcSort::ALL
        .iter()
        .position(|&s| s == current)
        .expect("config.view.proc_sorting must always be a known ProcSort variant");
    let new_idx = if dir < 0 {
        if idx == 0 {
            ProcSort::ALL.len() - 1
        } else {
            idx - 1
        }
    } else {
        (idx + 1) % ProcSort::ALL.len()
    };
    ctx.view.proc_sorting = ProcSort::ALL[new_idx];
    ctx.render.dirty |= Dirty::PROC_LIST | Dirty::PROC_WIDGET;
    HandleResult::none()
}

pub(super) fn terminate_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
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
    HandleResult::none()
}

pub(super) fn kill_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
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
    HandleResult::none()
}

pub(super) fn follow_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    if ctx.process.selected < ctx.process.entries.len()
        && let Some(pid) = ctx.selected_proc_pid()
    {
        if ctx.process.followed_pid == pid {
            ctx.process.followed_pid = 0;
        } else {
            ctx.process.followed_pid = pid;
        }
        ctx.render.dirty |= Dirty::PROC_WIDGET;
    }
    HandleResult::none()
}

pub(super) fn detail_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    if ctx.process.selected < ctx.process.entries.len()
        && let Some(pid) = ctx.selected_proc_pid()
    {
        let current_detailed = ctx.process.detailed_pid;
        if current_detailed == pid {
            ctx.process.detailed_pid = 0;
            ctx.process.followed_pid = 0;
        } else {
            ctx.process.detailed_pid = pid;
        }
        ctx.render.dirty |= Dirty::PROC_WIDGET;
    }
    HandleResult::none()
}

// ---------------------------------------------------------------------------
// Widget visibility toggles
// ---------------------------------------------------------------------------

pub(super) fn toggle_widget_main_action(ctx: &mut InputContext, key: &Key) -> HandleResult {
    let Key::Char(c) = key else {
        return HandleResult::none();
    };
    let kind = match (*c as u8) - b'0' {
        crate::ui::CPU_KEY => WidgetKind::Cpu,
        crate::ui::MEM_KEY => WidgetKind::Mem,
        crate::ui::NET_KEY => WidgetKind::Net,
        crate::ui::PROC_KEY => WidgetKind::Proc,
        crate::ui::DISK_KEY => WidgetKind::Disk,
        _ => return HandleResult::none(),
    };
    toggle_widget(ctx, kind);
    HandleResult::none()
}

pub(super) fn toggle_widget_gpu_low_action(ctx: &mut InputContext, key: &Key) -> HandleResult {
    let Key::Char(c) = key else {
        return HandleResult::none();
    };
    let digit = (*c as u8) - b'0';
    if digit < crate::ui::GPU_KEY_BASE {
        return HandleResult::none();
    }
    let gpu_idx = (digit - crate::ui::GPU_KEY_BASE) as usize;
    let Some(gpu_kind) = WidgetKind::gpu(gpu_idx) else {
        return HandleResult::none();
    };
    toggle_widget(ctx, gpu_kind);
    HandleResult::none()
}

pub(super) fn toggle_widget_gpu_high_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    // The `0` key toggles the second half of GPU slots
    // (indices 4..MAX_GPUS) as a single batch action so users
    // with many GPUs can collapse them quickly. A single
    // batch flips every Gpu(4..MAX_GPUS) — if any is hidden
    // they all become visible; otherwise they all become
    // hidden. (Pre-existing semantics.)
    for i in 4..crate::config::MAX_GPUS {
        if let Some(gpu_kind) = WidgetKind::gpu(i) {
            ctx.filter.hidden.toggle(gpu_kind);
        }
    }
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "widget_toggle",
        r#widget = "gpu_extras",
        "widget visibility toggled",
    );
    ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
    HandleResult::none()
}

fn toggle_widget(ctx: &mut InputContext, kind: WidgetKind) {
    let now_hidden = ctx.filter.hidden.toggle(kind);
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "widget_toggle",
        r#widget = %kind,
        shown = !now_hidden,
        "widget visibility toggled",
    );
    ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
}

pub(super) fn restore_widgets_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    if ctx.filter.hidden.is_empty() {
        // Idempotent: if nothing is hidden, do nothing visible —
        // and don't mark dirty. Avoids a redundant repaint on
        // accidental presses.
        return HandleResult::none();
    }
    ctx.filter.hidden.clear();
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "widget_filter_reset",
        "all hidden widgets restored",
    );
    ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
    HandleResult::none()
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

pub(super) fn iface_back_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    cycle_iface(ctx, -1)
}

pub(super) fn iface_forward_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    cycle_iface(ctx, 1)
}

fn cycle_iface(ctx: &mut InputContext, direction: isize) -> HandleResult {
    if ctx.live.net.as_ref().is_none_or(|n| n.nets.is_empty()) {
        return HandleResult::none();
    }
    cycle_net_iface(ctx, direction);
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "net_iface_cycle",
        iface = %ctx.network.selected_iface,
        "network interface switched",
    );
    ctx.render.dirty |= Dirty::NET_WIDGET;
    HandleResult::none()
}

pub(super) fn net_auto_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.view.net_auto = !ctx.view.net_auto;
    ctx.render.dirty |= Dirty::NET_WIDGET;
    HandleResult::none()
}

pub(super) fn net_sync_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.view.net_sync = !ctx.view.net_sync;
    ctx.render.dirty |= Dirty::NET_WIDGET;
    HandleResult::none()
}

pub(super) fn net_zero_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    if ctx.network.selected_iface.is_empty() {
        return HandleResult::none();
    }
    ctx.manager
        .reset_net_totals(ctx.network.selected_iface.clone());
    ctx.render.dirty |= Dirty::NET_WIDGET;
    HandleResult::none()
}

// ---------------------------------------------------------------------------
// Helpers (shared with options actions)
// ---------------------------------------------------------------------------

/// Sync all collector intervals to their effective values.
///
/// Called when global `update_ms` changes — collectors using the default
/// (per-widget interval == 0) get the new global value, while collectors
/// with a custom per-widget interval keep their own.
pub(crate) fn sync_all_intervals(ctx: &mut InputContext) {
    let intervals = [
        (SubsystemKind::Cpu, ctx.config.refresh.cpu_update_ms),
        (SubsystemKind::Mem, ctx.config.refresh.mem_update_ms),
        (SubsystemKind::Disk, ctx.config.refresh.disk_update_ms),
        (SubsystemKind::Net, ctx.config.refresh.net_update_ms),
        (SubsystemKind::Gpu, ctx.config.refresh.gpu_update_ms),
        (SubsystemKind::Proc, ctx.config.refresh.proc_update_ms),
    ];
    for (kind, widget_ms) in intervals {
        ctx.manager
            .set_interval(kind, ctx.config.effective_interval(widget_ms));
    }
}

fn sync_update_ms(ctx: &mut InputContext) {
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
    ctx.view.net_iface = ctx.network.selected_iface.clone();
}

// ---------------------------------------------------------------------------
// Process termination (Win32)
// ---------------------------------------------------------------------------

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
