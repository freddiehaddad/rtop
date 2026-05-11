//! Drain ready collector slots into [`AppState::live`] and mark
//! the corresponding widgets dirty.
//!
//! This is the "policy" half of the per-frame data path: it owns
//! per-subsystem ingest (where each snapshot lands, and what side
//! effects its arrival triggers) and the layout-hint change check.
//! The "mechanism" half (the loop, the channel) lives in `app::run`.

use crate::app::state::AppState;
use crate::config;
use crate::domain::widget_kind::WidgetKind;
use crate::event::{PerSubsystem, SubsystemKind};
use crate::runner;

/// Drain ready subsystems into `state.live` and set the corresponding
/// dirty flags. Performs per-subsystem post-ingest side effects
/// (stale-process tracking, network-iface reconciliation, layout-hint
/// change detection) before returning.
pub(crate) fn pull_subsystem_data(
    state: &mut AppState,
    config: &mut config::Config,
    manager: &runner::CollectorManager,
    render_ui: bool,
    ready: &PerSubsystem<bool>,
) {
    // Each arm owns the per-subsystem fetch + side effects; the loop
    // owns the dispatch.
    for kind in SubsystemKind::all_for(manager.gpu_count()) {
        if !*ready.get(kind) {
            continue;
        }
        match kind {
            SubsystemKind::Cpu => {
                if let Some(snap) = manager.cpu_slot.latest() {
                    state.live.core_count = snap.info.core_count;
                    state.live.cpu = Some(snap);
                    if render_ui {
                        state.render.dirty.mark_widget(WidgetKind::Cpu);
                    }
                }
            }
            SubsystemKind::Mem => {
                if let Some(snap) = manager.mem_slot.latest() {
                    state.live.total_mem = snap
                        .info
                        .stats
                        .used
                        .saturating_add(snap.info.stats.available);
                    state.live.mem = Some(snap);
                    if render_ui {
                        state.render.dirty.mark_widget(WidgetKind::Mem);
                    }
                }
            }
            SubsystemKind::Disk => {
                if let Some(snap) = manager.disk_slot.latest() {
                    state.live.disk = Some(snap);
                    if render_ui {
                        state.render.dirty.mark_widget(WidgetKind::Disk);
                    }
                }
            }
            SubsystemKind::Net => {
                if let Some(snap) = manager.net_slot.latest() {
                    state.live.net = Some(snap);
                    if render_ui {
                        state.render.dirty.mark_widget(WidgetKind::Net);
                    }
                }
            }
            SubsystemKind::Gpu(n) => {
                let n = n as usize;
                if let Some(snap) = manager.gpu_slots[n].latest() {
                    if render_ui {
                        // Only the displayed device drives a redraw;
                        // background-device updates publish silently.
                        let is_displayed = !state.gpu.selected_iface.is_empty()
                            && state.gpu.selected_iface == snap.info.stable_id;
                        if is_displayed {
                            let prev = state.live.gpu[n].as_ref();
                            let fingerprint_changed = prev
                                .map(|p| {
                                    p.info.render_fingerprint() != snap.info.render_fingerprint()
                                })
                                .unwrap_or(true);
                            let status_changed = prev.is_none_or(|p| p.status != snap.status);
                            if fingerprint_changed || status_changed {
                                state.render.dirty.mark_widget(WidgetKind::Gpu);
                            }
                        }
                    }
                    state.live.gpu[n] = Some(snap);
                }
            }
            SubsystemKind::Proc => {
                if let Some(snap) = manager.proc_slot.latest() {
                    if state.process.pause.is_some() {
                        // Paused: refresh dead_pids from this live
                        // update so the dead-row styling stays
                        // current as snapshot processes exit. Mark
                        // the proc widget dirty only when the dead
                        // set actually changes — the snapshot data
                        // itself is frozen and does not need a
                        // full proc-list rebuild.
                        //
                        // The detail panel's `dead` flag is
                        // computed at render time from
                        // `!live.contains(open_pid)`, which for any
                        // PID present in the paused snapshot is
                        // equivalent to "PID in dead_pids". Any
                        // flip of the open PID's dead flag therefore
                        // implies a change in dead_pids — covered
                        // by `dead_changed` below — so no separate
                        // redraw trigger is needed for the panel.
                        let dead_changed = state.process.refresh_dead_pids(&snap);
                        // Refresh the detail-panel cache from the
                        // paused snapshot. The snapshot is frozen,
                        // so this is a no-op on every cycle after
                        // the first while pause is active. Kept
                        // for symmetry with the live branch and so
                        // a snapshot-edit (re-pause on a later
                        // snapshot) cannot leave the cache stale.
                        if let Some(p) = state.process.pause.as_ref() {
                            // Borrow-release before refresh_detail_cache
                            // takes &mut self.
                            let snap = std::sync::Arc::clone(&p.snapshot);
                            state.process.refresh_detail_cache(&snap.procs);
                        }
                        state.live.proc_data = Some(snap);
                        if render_ui && dead_changed {
                            state.render.dirty.mark_widget(WidgetKind::Proc);
                        }
                    } else {
                        // Live: refresh the detail-panel cache from
                        // the new snapshot so `last_seen` mirrors
                        // the most recent observation. When the open
                        // PID is no longer present the cache is
                        // preserved as-is — the panel keeps showing
                        // the values from the moment the process
                        // exited and `resolve_detail_view` computes
                        // `dead = true` for it.
                        //
                        // `mark_proc_data_changed` below already
                        // forces a proc-widget repaint every cycle
                        // a live snapshot arrives, so the panel
                        // automatically re-renders with the freshly
                        // computed `dead` flag — no separate
                        // panel-specific dirty trigger is needed.
                        state.process.refresh_detail_cache(&snap.procs);
                        state.live.proc_data = Some(snap);
                        if render_ui {
                            state.render.dirty.mark_proc_data_changed();
                        }
                    }
                }
            }
            SubsystemKind::Statusbar => {
                if let Some(snap) = manager.statusbar_slot.latest() {
                    state.live.statusbar = Some(snap);
                    if render_ui {
                        state.render.dirty.mark_widget(WidgetKind::Statusbar);
                    }
                }
            }
        }
    }

    // Gate the layout-hints change check on `render_ui` so the
    // pull path stays write-free while a dimming overlay is open
    // (matching the per-subsystem `if render_ui` gates above).
    // See `maybe_mark_layout_dirty_from_hints_change` for the
    // full rationale and post-overlay correctness argument.
    maybe_mark_layout_dirty_from_hints_change(state, config, render_ui);

    reconcile_selected_net_iface(state);
    reconcile_selected_gpu_iface(state);
}

/// Compare the current `LayoutHints` against the cached
/// `last_layout_hints` and mark layout dirty if they differ.
///
/// **Gated on `render_ui`.** While a dimming overlay
/// (Main / Help / Options) is active, widget snapshots ingest
/// into [`crate::app::LiveData`] but no terminal writes happen
/// — every per-subsystem `mark_widget(...)` call in
/// [`pull_subsystem_data`] is gated on `render_ui`, and this
/// function follows the same regime. Without the gate, a change
/// in `LayoutHints` (PawnIO temp/watts flicker, GPU add/remove,
/// removable disk add/remove, statusbar uptime crossing a
/// digit-count boundary) would call `mark_layout()` while an
/// overlay is open and trigger a wasted `compose_modal_frame`
/// repaint of the modal layer.
///
/// **Post-overlay correctness.** When the menu closes,
/// `mark_layout_and_all_widgets()` fires from the close handler
/// (see `src/dirty.rs:97-103`), so the post-close render uses
/// current `LiveData` and recomputes the layout from scratch.
/// The next pull after close runs with `render_ui = true`,
/// recomputes hints against the (possibly stale) cached value,
/// and marks layout dirty if anything actually drifted —
/// harmless duplicate of the close handler's mark.
fn maybe_mark_layout_dirty_from_hints_change(
    state: &mut AppState,
    config: &config::Config,
    render_ui: bool,
) {
    if !render_ui {
        return;
    }
    let new_hints = state.live.layout_hints(config, &state.view, &state.filter);
    if state
        .render
        .last_layout_hints
        .is_none_or(|hints| hints != new_hints)
    {
        state.render.dirty.mark_layout();
    }
    state.render.last_layout_hints = Some(new_hints);
}

pub(crate) fn reconcile_selected_net_iface(state: &mut AppState) {
    let Some(net) = state.live.net.as_ref() else {
        return;
    };
    state
        .network
        .reconcile(&net.nets, &state.view.net_iface, &mut state.render.dirty);
}

pub(crate) fn reconcile_selected_gpu_iface(state: &mut AppState) {
    state.gpu.reconcile(
        &state.live.gpu,
        &state.view.gpu_iface,
        &mut state.render.dirty,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn net_snap(names: &[&str]) -> Arc<runner::NetSnapshot> {
        Arc::new(runner::NetSnapshot {
            nets: names
                .iter()
                .map(|n| crate::domain::network::NetInfo {
                    name: (*n).into(),
                    ..Default::default()
                })
                .collect(),
            status: crate::collect::CollectStatus::Ok,
        })
    }

    #[test]
    fn reconcile_selected_net_iface_selects_first_available_interface() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        state.render.clear_dirty();
        state.live.net = Some(net_snap(&["Ethernet", "Wi-Fi"]));

        reconcile_selected_net_iface(&mut state);

        assert_eq!(state.network.selected_iface, "Ethernet");
        assert!(state.render.dirty.is_widget_dirty(WidgetKind::Net));
    }

    #[test]
    fn reconcile_uses_preferred_iface_when_present() {
        // RuntimeView.net_iface holds the persisted/preferred
        // interface; reconcile must honour it when the iface is
        // currently in the live list.
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        state.view.net_iface = "Wi-Fi".into();
        state.live.net = Some(net_snap(&["Ethernet", "Wi-Fi"]));

        reconcile_selected_net_iface(&mut state);

        assert_eq!(state.network.selected_iface, "Wi-Fi");
    }

    #[test]
    fn reconcile_falls_back_when_preferred_iface_missing_but_preserves_preference() {
        // Saved preference is `Ethernet` but the live list only
        // has `Wi-Fi`. The displayed iface falls back to `Wi-Fi`,
        // BUT the persisted `RuntimeView.net_iface` must remain
        // `Ethernet` so the user's preference re-asserts on the
        // next process restart (when Ethernet may be back).
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        state.view.net_iface = "Ethernet".into();
        state.live.net = Some(net_snap(&["Wi-Fi"]));

        reconcile_selected_net_iface(&mut state);

        assert_eq!(state.network.selected_iface, "Wi-Fi");
        assert_eq!(
            state.view.net_iface, "Ethernet",
            "preferred iface must NOT be overwritten by auto-fallback"
        );
    }

    #[test]
    fn reconcile_keeps_existing_selection_when_still_present() {
        // Once `selected_iface` is non-empty and present in the
        // live list, reconcile must not switch to a different
        // iface — even if the preferred iface reappears mid-
        // session (the user's saved preference re-asserts only
        // at the next restart).
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        state.network.selected_iface = "Wi-Fi".into();
        state.view.net_iface = "Ethernet".into();
        state.live.net = Some(net_snap(&["Ethernet", "Wi-Fi"]));

        reconcile_selected_net_iface(&mut state);

        assert_eq!(state.network.selected_iface, "Wi-Fi");
        assert_eq!(state.view.net_iface, "Ethernet");
    }

    // ─────────────────────────────────────────────────────────────
    // maybe_mark_layout_dirty_from_hints_change — render_ui gate.
    //
    // While a dimming overlay is open the pull path must stay
    // write-free: widget snapshots ingest into LiveData but no
    // dirty marks fire and no terminal writes happen. The
    // layout-hints comparison is part of that regime.
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn hints_change_marks_layout_dirty_when_render_ui_true() {
        // Cached hints = None (initial state) → any current hints
        // count as a "change" → mark_layout fires.
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        state.render.clear_dirty();
        assert!(state.render.last_layout_hints.is_none());

        maybe_mark_layout_dirty_from_hints_change(&mut state, &config, true);

        assert!(
            state.render.dirty.needs_layout(),
            "first call with render_ui=true must mark layout dirty",
        );
        assert!(
            state.render.last_layout_hints.is_some(),
            "cache must be populated after the call",
        );

        // Second call with the same hints (no LiveData mutation
        // between calls) → no change → no additional mark. Reset
        // dirty first to make the assertion meaningful.
        state.render.clear_dirty();
        maybe_mark_layout_dirty_from_hints_change(&mut state, &config, true);
        assert!(
            !state.render.dirty.needs_layout(),
            "stable hints must not re-trigger mark_layout",
        );
    }

    #[test]
    fn hints_change_does_not_mark_layout_dirty_when_render_ui_false() {
        // Cached hints = None, current hints != None — would
        // normally fire mark_layout. With render_ui=false (a
        // dimming overlay is active), it must NOT fire and must
        // NOT update the cache (so the change is detected on the
        // next render_ui=true pull).
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        state.render.clear_dirty();
        assert!(state.render.last_layout_hints.is_none());

        maybe_mark_layout_dirty_from_hints_change(&mut state, &config, false);

        assert!(
            !state.render.dirty.needs_layout(),
            "render_ui=false must not mark layout dirty",
        );
        assert!(
            state.render.last_layout_hints.is_none(),
            "render_ui=false must not update the cache",
        );
    }

    /// Build a memory snapshot whose `swap_total` is non-zero so
    /// that `LayoutHints::has_swap` resolves to `true` (the gate
    /// also requires `config.mem.show_swap`, which defaults to
    /// `true`). Used by drift tests that need a hint to flip
    /// during the test body.
    fn mem_snap_with_swap() -> Arc<runner::MemSnapshot> {
        let mut info = crate::domain::memory::MemInfo::default();
        info.stats.swap_total = 4 * 1024 * 1024 * 1024;
        Arc::new(runner::MemSnapshot {
            info,
            status: crate::collect::CollectStatus::Ok,
        })
    }

    #[test]
    fn hints_drift_during_overlay_marks_dirty_after_overlay_closes() {
        // Pre-overlay: cache populated with current hints. No mem
        // snapshot yet → has_swap = false.
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        maybe_mark_layout_dirty_from_hints_change(&mut state, &config, true);
        let pre_overlay = state
            .render
            .last_layout_hints
            .expect("cache populated by previous call");
        state.render.clear_dirty();

        // Overlay opens; meanwhile the memory collector publishes
        // its first snapshot revealing a non-zero swap partition,
        // which flips `LayoutHints::has_swap` from false to true.
        // With render_ui=false this drift must not be observed.
        state.live.mem = Some(mem_snap_with_swap());
        let drifted = state.live.layout_hints(&config, &state.view, &state.filter);
        assert_ne!(
            pre_overlay, drifted,
            "test setup precondition: swap appearing must flip has_swap",
        );

        maybe_mark_layout_dirty_from_hints_change(&mut state, &config, false);
        assert!(
            !state.render.dirty.needs_layout(),
            "drift during overlay must not mark dirty",
        );
        assert_eq!(
            state.render.last_layout_hints,
            Some(pre_overlay),
            "cache must still hold the pre-overlay value",
        );

        // Overlay closes. The next pull runs with render_ui=true
        // and the drift is finally observed.
        maybe_mark_layout_dirty_from_hints_change(&mut state, &config, true);
        assert!(
            state.render.dirty.needs_layout(),
            "drift accumulated during overlay must mark dirty on first \
             post-close pull",
        );
    }
}
