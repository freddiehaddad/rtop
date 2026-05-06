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
                        .update_stale_procs(&snap.procs, config.keep_dead_proc_usage);
                    state.live.proc_data = Some(snap);
                    if render_ui {
                        state.render.dirty |= Dirty::PROC_WIDGET | Dirty::PROC_LIST;
                    }
                }
            }
        }
    }

    // Check layout hints for changes.
    let new_hints = state.live.layout_hints(config);
    if state
        .render
        .last_layout_hints
        .is_none_or(|hints| hints != new_hints)
    {
        state.render.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
    }
    state.render.last_layout_hints = Some(new_hints);

    reconcile_selected_iface(state, config);
}

pub(crate) fn reconcile_selected_iface(state: &mut AppState, config: &config::Config) {
    let Some(net) = state.live.net.as_ref() else {
        return;
    };
    state
        .network
        .reconcile(&net.nets, &config.net_iface, &mut state.render.dirty);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Instant;

    #[test]
    fn reconcile_selected_iface_selects_first_available_interface() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());
        state.render.clear_dirty();
        state.live.net = Some(Arc::new(runner::NetSnapshot {
            nets: vec![
                crate::domain::network::NetInfo {
                    name: "Ethernet".into(),
                    ..Default::default()
                },
                crate::domain::network::NetInfo {
                    name: "Wi-Fi".into(),
                    ..Default::default()
                },
            ],
            status: crate::collect::CollectStatus::Ok,
        }));

        reconcile_selected_iface(&mut state, &config);

        assert_eq!(state.network.selected_iface, "Ethernet");
        assert!(state.render.dirty.contains(Dirty::NET_WIDGET));
    }
}
