//! Per-action handlers for "normal" (no-overlay) mode and the
//! Win32 helpers shared by them.
//!
//! Every public function with the suffix `_action` is referenced
//! by [`crate::handlers::keybinds::BINDINGS`]; signatures match
//! [`crate::handlers::keybinds::ActionFn`].
//!
//! Actions mutate state directly and signal application exit by
//! setting `*ctx.quit = true`. They do not produce terminal
//! output — the central render path repaints from state.

use crate::{
    collect::process_display::ProcSort, dirty::RenderDirty, domain::widget_kind::WidgetKind,
    event::SubsystemKind, handlers::InputContext, input::Key, overlay::ReturnTarget, theme,
};

// ---------------------------------------------------------------------------
// Quit / menu transitions
// ---------------------------------------------------------------------------

pub(super) fn quit_action(ctx: &mut InputContext, _: &Key) {
    *ctx.quit = true;
}

pub(super) fn open_main_menu_action(ctx: &mut InputContext, _: &Key) {
    ctx.open_main_menu();
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "main",
        opened = true,
        "menu transition",
    );
}

pub(super) fn open_help_menu_action(ctx: &mut InputContext, _: &Key) {
    ctx.open_help_menu(ReturnTarget::Normal);
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "help",
        opened = true,
        "menu transition",
    );
}

pub(super) fn open_options_menu_action(ctx: &mut InputContext, _: &Key) {
    ctx.open_options_menu(ReturnTarget::Normal);
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "options",
        opened = true,
        "menu transition",
    );
}

// ---------------------------------------------------------------------------
// Presets, config reload, update rate
// ---------------------------------------------------------------------------

pub(super) fn preset_forward_action(ctx: &mut InputContext, _: &Key) {
    cycle_preset(ctx, true);
}

pub(super) fn preset_back_action(ctx: &mut InputContext, _: &Key) {
    cycle_preset(ctx, false);
}

fn cycle_preset(ctx: &mut InputContext, forward: bool) {
    ctx.config.cycle_preset(forward);
    sync_update_ms(ctx);
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "preset_cycle",
        preset = ctx.config.active_preset().name(),
        "preset action",
    );
    ctx.render.dirty = RenderDirty::full();
}

pub(super) fn config_reload_action(ctx: &mut InputContext, _: &Key) {
    let warnings = ctx.config.reload();
    for w in &warnings {
        tracing::warn!(
            subsystem = %crate::log::Subsystem::Config,
            warning = %w,
            "config reload warning",
        );
    }
    *ctx.theme = theme::Theme::from_name(&ctx.config.ui.color_theme);
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
    // Full redraw: the new theme's base style takes effect on the
    // next frame because `style_terminal_output` always prefixes
    // the buffer with the current base style, and `mark_layout`
    // forces a `CLEAR_SCREEN` so cells outside widgets pick up
    // the new background.
    ctx.render.dirty = RenderDirty::full();
}

pub(super) fn update_rate_up_action(ctx: &mut InputContext, _: &Key) {
    step_update_rate(ctx, 1);
}

pub(super) fn update_rate_down_action(ctx: &mut InputContext, _: &Key) {
    step_update_rate(ctx, -1);
}

fn step_update_rate(ctx: &mut InputContext, delta: i64) {
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
    // The rate label (`- Nms +`) lives on the statusbar widget;
    // changing it also resizes the statusbar's `min_width`, so
    // mark layout dirty (which marks every widget — including the
    // statusbar — and recomputes `min_terminal_size`).
    ctx.render.dirty.mark_layout();
}

// ---------------------------------------------------------------------------
// Process navigation
// ---------------------------------------------------------------------------

pub(super) fn nav_up_action(ctx: &mut InputContext, _: &Key) {
    if ctx.process.selected > 0 {
        ctx.process.selected -= 1;
        ctx.render.dirty.mark_proc_widget();
    }
}

pub(super) fn nav_down_action(ctx: &mut InputContext, _: &Key) {
    let count = ctx.process.entries.len();
    if ctx.process.selected + 1 < count {
        ctx.process.selected += 1;
        ctx.render.dirty.mark_proc_widget();
    }
}

pub(super) fn nav_page_up_action(ctx: &mut InputContext, _: &Key) {
    let page = ctx.size.height.saturating_sub(10);
    ctx.process.selected = ctx.process.selected.saturating_sub(page);
    ctx.render.dirty.mark_proc_widget();
}

pub(super) fn nav_page_down_action(ctx: &mut InputContext, _: &Key) {
    let page = ctx.size.height.saturating_sub(10);
    let count = ctx.process.entries.len();
    ctx.process.selected = (ctx.process.selected + page).min(count.saturating_sub(1));
    ctx.render.dirty.mark_proc_widget();
}

pub(super) fn nav_half_page_down_action(ctx: &mut InputContext, _: &Key) {
    let page = ctx.size.height.saturating_sub(10);
    let half = page / 2;
    let count = ctx.process.entries.len();
    ctx.process.selected = (ctx.process.selected + half).min(count.saturating_sub(1));
    ctx.render.dirty.mark_proc_widget();
}

pub(super) fn nav_half_page_up_action(ctx: &mut InputContext, _: &Key) {
    let page = ctx.size.height.saturating_sub(10);
    let half = page / 2;
    ctx.process.selected = ctx.process.selected.saturating_sub(half);
    ctx.render.dirty.mark_proc_widget();
}

pub(super) fn nav_home_action(ctx: &mut InputContext, _: &Key) {
    ctx.process.selected = 0;
    ctx.process.start = 0;
    ctx.render.dirty.mark_proc_widget();
}

pub(super) fn nav_end_action(ctx: &mut InputContext, _: &Key) {
    let count = ctx.process.entries.len();
    ctx.process.selected = count.saturating_sub(1);
    ctx.render.dirty.mark_proc_widget();
}

// ---------------------------------------------------------------------------
// Process modes, sorting, and actions
// ---------------------------------------------------------------------------

pub(super) fn open_filter_action(ctx: &mut InputContext, _: &Key) {
    ctx.process.filter_text = ctx.view.proc_filter.clone();
    ctx.open_filter();
}

pub(super) fn toggle_tree_action(ctx: &mut InputContext, _: &Key) {
    ctx.view.proc_tree = !ctx.view.proc_tree;
    ctx.render.dirty.mark_proc_data_changed();
}

pub(super) fn toggle_reverse_action(ctx: &mut InputContext, _: &Key) {
    ctx.view.proc_reversed = !ctx.view.proc_reversed;
    ctx.render.dirty.mark_proc_data_changed();
}

pub(super) fn toggle_per_core_action(ctx: &mut InputContext, _: &Key) {
    ctx.view.proc_per_core = !ctx.view.proc_per_core;
    ctx.render.dirty.mark_proc_data_changed();
}

pub(super) fn toggle_io_action(ctx: &mut InputContext, _: &Key) {
    ctx.view.io_mode = !ctx.view.io_mode;
    ctx.render.dirty.mark_widget(WidgetKind::Disk);
}

pub(super) fn sort_back_action(ctx: &mut InputContext, _: &Key) {
    cycle_sort(ctx, -1);
}

pub(super) fn sort_forward_action(ctx: &mut InputContext, _: &Key) {
    cycle_sort(ctx, 1);
}

fn cycle_sort(ctx: &mut InputContext, dir: isize) {
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
    ctx.render.dirty.mark_proc_data_changed();
}

pub(super) fn terminate_action(ctx: &mut InputContext, _: &Key) {
    if let Some((armed_pid, _, false)) = ctx.process.armed_terminate {
        // Defensive: the armed PID may have died during the arm
        // window (live update arrived between the two `t` presses).
        // Reject the second press cleanly instead of attempting a
        // syscall the OS will reject. The dimmed bottom-border
        // chip is the user-visible signal; the debug log carries
        // the trace for anyone running with log_level=debug.
        if ctx.process.is_dead(armed_pid) {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::Input,
                action = "terminate",
                pid = armed_pid,
                "refused: process exited",
            );
            ctx.process.armed_terminate = None;
            ctx.render.dirty.mark_proc_widget();
            return;
        }
        tracing::info!(
            subsystem = %crate::log::Subsystem::Input,
            action = "process_terminate",
            pid = armed_pid,
            "graceful terminate requested",
        );
        graceful_terminate(armed_pid);
        ctx.process.armed_terminate = None;
    } else if let Some((pid, name)) = ctx.selected_proc_info() {
        // Pre-emptive rejection: don't arm on a dead row. The
        // dimmed `terminate` chip on the bottom border is the
        // affordance hint that this action is unavailable.
        if ctx.process.is_dead(pid) {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::Input,
                action = "terminate",
                pid,
                "refused: process exited",
            );
            return;
        }
        ctx.process.armed_terminate = Some((pid, name.to_string(), false));
    }
    ctx.render.dirty.mark_proc_widget();
}

pub(super) fn kill_action(ctx: &mut InputContext, _: &Key) {
    if let Some((armed_pid, _, true)) = ctx.process.armed_terminate {
        if ctx.process.is_dead(armed_pid) {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::Input,
                action = "kill",
                pid = armed_pid,
                "refused: process exited",
            );
            ctx.process.armed_terminate = None;
            ctx.render.dirty.mark_proc_widget();
            return;
        }
        tracing::info!(
            subsystem = %crate::log::Subsystem::Input,
            action = "process_kill",
            pid = armed_pid,
            "kill requested",
        );
        terminate_process(armed_pid);
        ctx.process.armed_terminate = None;
    } else if let Some((pid, name)) = ctx.selected_proc_info() {
        if ctx.process.is_dead(pid) {
            tracing::debug!(
                subsystem = %crate::log::Subsystem::Input,
                action = "kill",
                pid,
                "refused: process exited",
            );
            return;
        }
        ctx.process.armed_terminate = Some((pid, name.to_string(), true));
    }
    ctx.render.dirty.mark_proc_widget();
}

/// Toggle the proc-list pause state. See `ProcessViewState::pause`
/// for the snapshot-freeze invariant.
pub(super) fn pause_action(ctx: &mut InputContext, _: &Key) {
    let now_paused = ctx.process.toggle_pause(ctx.live);
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "pause_toggle",
        paused = now_paused,
        "process list pause toggled",
    );
    // Toggling pause changes which procs slice the display rebuilds
    // from (live ↔ paused snapshot), so we need a full proc-list
    // rebuild. The proc widget redraw is implied by
    // `mark_proc_data_changed`.
    ctx.render.dirty.mark_proc_data_changed();
}

pub(super) fn follow_action(ctx: &mut InputContext, _: &Key) {
    if ctx.process.selected < ctx.process.entries.len()
        && let Some(pid) = ctx.selected_proc_pid()
    {
        if ctx.process.followed_pid == pid {
            ctx.process.followed_pid = 0;
        } else {
            ctx.process.followed_pid = pid;
        }
        ctx.render.dirty.mark_proc_widget();
    }
}

pub(super) fn detail_action(ctx: &mut InputContext, _: &Key) {
    if ctx.process.selected >= ctx.process.entries.len() {
        return;
    }
    let Some(info) = resolve_selected_proc(ctx) else {
        return;
    };
    ctx.process.toggle_detail(info);
    ctx.render.dirty.mark_proc_widget();
}

/// Clone the `ProcInfo` for the row currently under the cursor.
///
/// Returns `None` when no procs source is available (first-frame
/// race) or when `selected` does not resolve to a valid entry. The
/// returned value is owned so the caller can release the immutable
/// borrow on `ctx.process` before performing mutations.
fn resolve_selected_proc(ctx: &InputContext<'_>) -> Option<crate::domain::process::ProcInfo> {
    let procs = ctx.process.procs_source(ctx.live)?;
    ctx.process
        .entries
        .get(ctx.process.selected)
        .and_then(|entry| procs.get(entry.proc_index))
        .cloned()
}

/// Close the process detail panel from anywhere in NORMAL mode.
///
/// Bound to `Esc` to give the user a one-press dismissal even when
/// the panel's PID is no longer in the live list (e.g. after the
/// watched process has exited and the row has dropped out of the
/// process list). No-op when the panel is already closed; the
/// keystroke is then consumed without marking anything dirty.
pub(super) fn close_detail_action(ctx: &mut InputContext, _: &Key) {
    if ctx.process.close_detail_and_unfollow() {
        ctx.render.dirty.mark_proc_widget();
    }
}

// ---------------------------------------------------------------------------
// Widget visibility toggles
// ---------------------------------------------------------------------------

pub(super) fn toggle_widget_main_action(ctx: &mut InputContext, key: &Key) {
    let Key::Char(c) = key else {
        return;
    };
    let kind = match (*c as u8) - b'0' {
        crate::ui::CPU_KEY => WidgetKind::Cpu,
        crate::ui::MEM_KEY => WidgetKind::Mem,
        crate::ui::NET_KEY => WidgetKind::Net,
        crate::ui::PROC_KEY => WidgetKind::Proc,
        crate::ui::DISK_KEY => WidgetKind::Disk,
        crate::ui::GPU_KEY => WidgetKind::Gpu,
        _ => return,
    };
    toggle_widget(ctx, kind);
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
    ctx.render.dirty.mark_layout();
}

pub(super) fn restore_widgets_action(ctx: &mut InputContext, _: &Key) {
    if ctx.filter.hidden.is_empty() {
        // Idempotent: if nothing is hidden, do nothing visible —
        // and don't mark dirty. Avoids a redundant repaint on
        // accidental presses.
        return;
    }
    ctx.filter.hidden.clear();
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "widget_filter_reset",
        "all hidden widgets restored",
    );
    ctx.render.dirty.mark_layout();
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

pub(super) fn net_back_action(ctx: &mut InputContext, _: &Key) {
    cycle_net(ctx, -1);
}

pub(super) fn net_forward_action(ctx: &mut InputContext, _: &Key) {
    cycle_net(ctx, 1);
}

fn cycle_net(ctx: &mut InputContext, direction: isize) {
    if ctx.live.net.as_ref().is_none_or(|n| n.nets.is_empty()) {
        return;
    }
    cycle_net_iface(ctx, direction);
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "net_iface_cycle",
        iface = %ctx.network.selected_iface,
        "network interface switched",
    );
    ctx.render.dirty.mark_widget(WidgetKind::Net);
}

pub(super) fn net_auto_action(ctx: &mut InputContext, _: &Key) {
    ctx.view.net_auto = !ctx.view.net_auto;
    ctx.render.dirty.mark_widget(WidgetKind::Net);
}

pub(super) fn net_sync_action(ctx: &mut InputContext, _: &Key) {
    ctx.view.net_sync = !ctx.view.net_sync;
    ctx.render.dirty.mark_widget(WidgetKind::Net);
}

pub(super) fn net_zero_action(ctx: &mut InputContext, _: &Key) {
    if ctx.network.selected_iface.is_empty() {
        return;
    }
    ctx.manager
        .reset_net_totals(ctx.network.selected_iface.clone());
    ctx.render.dirty.mark_widget(WidgetKind::Net);
}

// ---------------------------------------------------------------------------
// GPU device cycle
// ---------------------------------------------------------------------------

pub(super) fn gpu_back_action(ctx: &mut InputContext, _: &Key) {
    cycle_gpu(ctx, -1);
}

pub(super) fn gpu_forward_action(ctx: &mut InputContext, _: &Key) {
    cycle_gpu(ctx, 1);
}

fn cycle_gpu(ctx: &mut InputContext, direction: isize) {
    if ctx.live.gpu.iter().all(|s| s.is_none()) {
        return;
    }
    cycle_gpu_iface(ctx, direction);
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "gpu_iface_cycle",
        iface = %ctx.gpu.selected_iface,
        "GPU device switched",
    );
    ctx.render.dirty.mark_widget(WidgetKind::Gpu);
}

// ---------------------------------------------------------------------------
// Helpers (shared with options actions)
// ---------------------------------------------------------------------------

/// Sync all collector intervals to their effective values.
///
/// Called when global `update_ms` changes — collectors using the default
/// (per-widget interval == 0) get the new global value, while collectors
/// with a custom per-widget interval keep their own. The GPU subsystem
/// shares a single `gpu_update_ms` across every detected device, so the
/// resolved value broadcasts to every GPU thread via a `0..gpu_count`
/// loop.
pub(crate) fn sync_all_intervals(ctx: &mut InputContext) {
    let base_intervals = [
        (SubsystemKind::Cpu, ctx.config.refresh.cpu_update_ms),
        (SubsystemKind::Mem, ctx.config.refresh.mem_update_ms),
        (SubsystemKind::Disk, ctx.config.refresh.disk_update_ms),
        (SubsystemKind::Net, ctx.config.refresh.net_update_ms),
        (SubsystemKind::Proc, ctx.config.refresh.proc_update_ms),
    ];
    for (kind, widget_ms) in base_intervals {
        ctx.manager
            .set_interval(kind, ctx.config.effective_interval(widget_ms));
    }
    let gpu_ms = ctx
        .config
        .effective_interval(ctx.config.refresh.gpu_update_ms);
    for n in 0..ctx.manager.gpu_count() {
        ctx.manager.set_interval(SubsystemKind::Gpu(n), gpu_ms);
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

fn cycle_gpu_iface(ctx: &mut InputContext, direction: isize) {
    let present: Vec<&str> = ctx
        .live
        .gpu
        .iter()
        .filter_map(|s| s.as_deref().map(|s| s.info.stable_id.as_str()))
        .collect();
    if present.is_empty() {
        return;
    }

    let current = present
        .iter()
        .position(|id| *id == ctx.gpu.selected_iface)
        .unwrap_or(0);
    let new_idx = if direction < 0 {
        current.checked_sub(1).unwrap_or(present.len() - 1)
    } else {
        (current + 1) % present.len()
    };
    ctx.gpu.selected_iface = present[new_idx].to_string();
    ctx.view.gpu_iface = ctx.gpu.selected_iface.clone();
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
