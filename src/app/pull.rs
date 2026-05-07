//! Drain ready collector slots into [`AppState::live`] and mark
//! the corresponding widgets dirty.
//!
//! This is the "policy" half of the per-frame data path: it owns
//! per-subsystem ingest (where each snapshot lands, and what side
//! effects its arrival triggers) and the layout-hint change check.
//! The "mechanism" half (the loop, the channel) lives in `app::run`.

use crate::app::state::AppState;
use crate::config;
use crate::dirty::Dirty;
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
    for kind in SubsystemKind::ALL {
        if !*ready.get(kind) {
            continue;
        }
        match kind {
            SubsystemKind::Cpu => {
                if let Some(snap) = manager.cpu_slot.latest() {
                    state.live.core_count = snap.info.core_count;
                    state.live.cpu = Some(snap);
                    if render_ui {
                        state.render.dirty |= Dirty::CPU_WIDGET;
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
                        state.render.dirty |= Dirty::MEM_WIDGET;
                    }
                }
            }
            SubsystemKind::Disk => {
                if let Some(snap) = manager.disk_slot.latest() {
                    state.live.disk = Some(snap);
                    if render_ui {
                        state.render.dirty |= Dirty::DISK_WIDGET;
                    }
                }
            }
            SubsystemKind::Net => {
                if let Some(snap) = manager.net_slot.latest() {
                    state.live.net = Some(snap);
                    if render_ui {
                        state.render.dirty |= Dirty::NET_WIDGET;
                    }
                }
            }
            SubsystemKind::Gpu => {
                if let Some(snap) = manager.gpu_slot.latest() {
                    state.live.gpu = Some(snap);
                    if render_ui {
                        state.render.dirty |= Dirty::GPU_WIDGET;
                    }
                }
            }
            SubsystemKind::Proc => {
                if let Some(snap) = manager.proc_slot.latest() {
                    state
                        .process
                        .update_stale_procs(&snap.procs, config.proc.keep_dead_proc_usage);
                    state.live.proc_data = Some(snap);
                    if render_ui {
                        state.render.dirty |= Dirty::PROC_WIDGET | Dirty::PROC_LIST;
                    }
                }
            }
        }
    }

    // Check layout hints for changes.
    let new_hints = state.live.layout_hints(config, &state.view);
    if state
        .render
        .last_layout_hints
        .is_none_or(|hints| hints != new_hints)
    {
        state.render.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
    }
    state.render.last_layout_hints = Some(new_hints);

    reconcile_selected_iface(state);
}

pub(crate) fn reconcile_selected_iface(state: &mut AppState) {
    let Some(net) = state.live.net.as_ref() else {
        return;
    };
    state
        .network
        .reconcile(&net.nets, &state.view.net_iface, &mut state.render.dirty);
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
    fn reconcile_selected_iface_selects_first_available_interface() {
        let config = config::Config::new();
        let mut state = AppState::new(&config);
        state.render.clear_dirty();
        state.live.net = Some(net_snap(&["Ethernet", "Wi-Fi"]));

        reconcile_selected_iface(&mut state);

        assert_eq!(state.network.selected_iface, "Ethernet");
        assert!(state.render.dirty.contains(Dirty::NET_WIDGET));
    }

    #[test]
    fn reconcile_uses_preferred_iface_when_present() {
        // RuntimeView.net_iface holds the persisted/preferred
        // interface; reconcile must honour it when the iface is
        // currently in the live list.
        let config = config::Config::new();
        let mut state = AppState::new(&config);
        state.view.net_iface = "Wi-Fi".into();
        state.live.net = Some(net_snap(&["Ethernet", "Wi-Fi"]));

        reconcile_selected_iface(&mut state);

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
        let mut state = AppState::new(&config);
        state.view.net_iface = "Ethernet".into();
        state.live.net = Some(net_snap(&["Wi-Fi"]));

        reconcile_selected_iface(&mut state);

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
        let mut state = AppState::new(&config);
        state.network.selected_iface = "Wi-Fi".into();
        state.view.net_iface = "Ethernet".into();
        state.live.net = Some(net_snap(&["Ethernet", "Wi-Fi"]));

        reconcile_selected_iface(&mut state);

        assert_eq!(state.network.selected_iface, "Wi-Fi");
        assert_eq!(state.view.net_iface, "Ethernet");
    }
}
