//! Per-frame application state owned by the event loop.
//!
//! `AppState` bundles the structs that live for the entire process
//! lifetime and are mutated by the input handlers and the data-pull
//! pipeline. Each sub-struct is a focused container; the split is
//! purely about borrow scopes and ownership clarity, not policy.

use crate::config;
use crate::dirty::Dirty;
use crate::domain::process::ProcDisplayEntry;
use crate::domain::widget_kind::WidgetKind;
use crate::domain::widget_set::WidgetSet;
use crate::draw;
use crate::handlers::MenuState;
use crate::runner;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

pub(crate) struct AppState {
    pub(crate) runtime: RuntimeState,
    pub(crate) render: RenderState,
    pub(crate) live: LiveData,
    pub(crate) overlay: OverlayState,
    pub(crate) process: ProcessViewState,
    pub(crate) network: NetworkViewState,
    pub(crate) filter: WidgetFilter,
}

impl AppState {
    pub(crate) fn new(config: &config::Config, _now: Instant) -> Self {
        Self {
            runtime: RuntimeState::new(config),
            render: RenderState::new(),
            live: LiveData::new(),
            overlay: OverlayState::new(),
            process: ProcessViewState::new(),
            network: NetworkViewState::new(),
            // Restore the persisted view filter at startup. Toggle
            // gestures during the previous session are picked up
            // here; `Shift+R` cleared the filter and saved an
            // empty set, so a "reset and restart clean" cycle
            // works without manual config editing.
            filter: WidgetFilter {
                hidden: config.hidden_widgets.clone(),
            },
        }
    }

    /// Compose the engine's per-frame `hidden` [`WidgetSet`] from
    /// every visibility source: hardware absence (GPUs without a
    /// backing device, derived from `LiveData`'s detected GPU count)
    /// unioned with the user's runtime view filter.
    ///
    /// The engine consumes the result without caring why a widget
    /// is hidden — there is one source of truth per frame, built
    /// here.
    pub(crate) fn compose_hidden(&self, config: &config::Config) -> WidgetSet {
        let hints = self.live.layout_hints(config);
        let mut hidden = WidgetSet::new();
        for n in (hints.gpu_count as u8)..(crate::config::MAX_GPUS as u8) {
            hidden.insert(WidgetKind::Gpu(n));
        }
        hidden.extend_from(&self.filter.hidden);
        hidden
    }
}

pub(crate) struct RuntimeState {
    pub(crate) rounded: bool,
    pub(crate) update_ms: u64,
}

impl RuntimeState {
    fn new(config: &config::Config) -> Self {
        Self {
            rounded: config.rounded_corners,
            update_ms: config.update_ms as u64,
        }
    }
}

pub(crate) struct RenderState {
    pub(crate) dirty: Dirty,
    pub(crate) cached_layout: Option<draw::layout::Layout>,
    pub(crate) last_layout_hints: Option<draw::layout::LayoutHints>,
}

impl RenderState {
    fn new() -> Self {
        Self {
            dirty: Dirty::FULL,
            cached_layout: None,
            last_layout_hints: None,
        }
    }

    pub(crate) fn mark_resize(&mut self) {
        self.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
    }

    pub(crate) fn clear_dirty(&mut self) {
        self.dirty = Dirty::empty();
    }
}

/// Runtime view filter — the set of widgets the user has chosen to
/// hide via the toggle keys (`1`-`9`, `0`).
///
/// Lives in [`AppState`] (not [`config::Config`]) because it's a
/// transient view operation, not a persisted layout edit. Hiding a
/// widget never mutates the active preset; the user's saved layout
/// stays exactly as they wrote it. `Shift+R` clears the filter.
///
/// The filter is global across preset cycles: "hide this" means
/// "hide this", regardless of which preset is active. If a hidden
/// widget isn't in the active layout, the entry is harmless — it
/// only takes effect if the user later cycles to a preset that
/// includes the widget.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WidgetFilter {
    pub(crate) hidden: WidgetSet,
}

pub(crate) struct LiveData {
    pub(crate) cpu: Option<Arc<runner::CpuSnapshot>>,
    pub(crate) mem: Option<Arc<runner::MemSnapshot>>,
    pub(crate) disk: Option<Arc<runner::DiskSnapshot>>,
    pub(crate) net: Option<Arc<runner::NetSnapshot>>,
    pub(crate) gpu: Option<Arc<runner::GpuSnapshot>>,
    pub(crate) proc_data: Option<Arc<runner::ProcSnapshot>>,
    /// Cached core count for proc widget (stable hardware constant).
    pub(crate) core_count: usize,
    /// Cached total physical memory for proc widget (stable hardware constant).
    pub(crate) total_mem: u64,
}

impl LiveData {
    fn new() -> Self {
        Self {
            cpu: None,
            mem: None,
            disk: None,
            net: None,
            gpu: None,
            proc_data: None,
            core_count: 0,
            total_mem: 0,
        }
    }

    /// True when all subsystems have provided at least one data point.
    pub(crate) fn is_ready(&self) -> bool {
        self.cpu.is_some()
            && self.mem.is_some()
            && self.disk.is_some()
            && self.net.is_some()
            && self.gpu.is_some()
            && self.proc_data.is_some()
    }

    pub(crate) fn layout_hints(&self, config: &config::Config) -> draw::layout::LayoutHints {
        // Disk widget rows-per-disk depends on which view is active:
        //   * Usage view (default): 2 rows when `show_io_stat` adds
        //     the inline read/write/busy row under each capacity
        //     meter; 1 row otherwise.
        //   * IO view (`io_mode` toggle or persistent
        //     `disk_io_mode`): 2 rows for separate read+write
        //     graphs; 1 row when `io_graph_combined` merges them.
        let io_view = config.io_mode || config.disk_io_mode;
        let disk_rows_per_unit = if io_view {
            if config.io_graph_combined { 1 } else { 2 }
        } else if config.show_io_stat {
            2
        } else {
            1
        };
        draw::layout::LayoutHints {
            core_count: self.core_count,
            gpu_count: self.gpu.as_ref().map_or(0, |g| g.gpus.len()),
            disk_count: filtered_disk_count(self.disk.as_deref(), config),
            has_swap: config.show_swap
                && self
                    .mem
                    .as_ref()
                    .is_some_and(|m| m.info.stats.swap_total > 0),
            has_cpu_temp: config.check_temp
                && self.cpu.as_ref().is_some_and(|c| !c.info.temp.is_empty()),
            has_cpu_watts: config.show_cpu_watts
                && self
                    .cpu
                    .as_ref()
                    .is_some_and(|c| c.info.cpu_watts.is_some()),
            disk_rows_per_unit,
        }
    }
}

/// Count the disks that pass the user's `disks_filter`.
///
/// Used by both layout sizing (`calculate_layout` and `LayoutHints`) and
/// dirty-flag change detection so the disk widget height tracks the
/// post-filter row count, not the raw drive count. Returns 0 when no
/// disk snapshot is available.
pub(crate) fn filtered_disk_count(
    disk: Option<&runner::DiskSnapshot>,
    config: &config::Config,
) -> usize {
    let filter = crate::domain::disk::DisksFilter::parse(&config.disks_filter);
    disk.map_or(0, |d| {
        d.info
            .disks
            .iter()
            .filter(|info| filter.matches(&info.name))
            .count()
    })
}

pub(crate) struct OverlayState {
    pub(crate) menu_state: MenuState,
    pub(crate) menu_return_to: MenuState,
    pub(crate) main_menu_selected: usize,
    pub(crate) options_cat: usize,
    pub(crate) options_selected: usize,
    pub(crate) options_page: usize,
    /// Mutable buffer for an in-progress inline option edit.
    ///
    /// Invariant maintained by [`Self::enter_option_edit`] and
    /// [`Self::exit_option_edit`]: this is `Some` if and only if
    /// `menu_state == MenuState::OptionsEdit`. The check is also
    /// re-asserted in [`Self::set_menu_state`] to catch any future
    /// caller that bypasses the helpers.
    option_edit: Option<crate::handlers::options_edit::OptionEditState>,
}

impl OverlayState {
    fn new() -> Self {
        Self {
            menu_state: MenuState::None,
            menu_return_to: MenuState::None,
            main_menu_selected: 0,
            options_cat: 0,
            options_selected: 0,
            options_page: 0,
            option_edit: None,
        }
    }

    pub(crate) fn set_menu_state(&mut self, new: MenuState) {
        debug_assert!(
            self.menu_state.can_transition_to(new),
            "invalid menu transition: {:?} → {:?}",
            self.menu_state,
            new,
        );
        // The buffer-invariant check is `assert!`, not `debug_assert!`,
        // because release builds must crash on a dangling
        // `option_edit` rather than silently render with inconsistent
        // state. Cost is one boolean per menu transition (handled on
        // user keystrokes, not per frame), which is negligible.
        assert!(
            new == MenuState::OptionsEdit || self.option_edit.is_none(),
            "option_edit must be cleared before transitioning to {:?}",
            new,
        );
        self.menu_state = new;
    }

    /// Begin an inline option edit: store the buffer state and
    /// transition to [`MenuState::OptionsEdit`] in one atomic step.
    pub(crate) fn enter_option_edit(
        &mut self,
        state: crate::handlers::options_edit::OptionEditState,
    ) {
        // Set the buffer first so the debug_assert in
        // set_menu_state sees the new invariant satisfied.
        self.option_edit = Some(state);
        self.set_menu_state(MenuState::OptionsEdit);
    }

    /// End an inline option edit: discard the buffer and return to
    /// [`MenuState::Options`] in one atomic step. Returns the
    /// discarded state for callers who need to inspect it (e.g.
    /// log the cancelled value).
    pub(crate) fn exit_option_edit(
        &mut self,
    ) -> Option<crate::handlers::options_edit::OptionEditState> {
        let edit = self.option_edit.take();
        self.set_menu_state(MenuState::Options);
        edit
    }

    pub(crate) fn option_edit(&self) -> Option<&crate::handlers::options_edit::OptionEditState> {
        self.option_edit.as_ref()
    }

    pub(crate) fn option_edit_mut(
        &mut self,
    ) -> Option<&mut crate::handlers::options_edit::OptionEditState> {
        self.option_edit.as_mut()
    }

    pub(crate) fn render_ui(&self) -> bool {
        self.menu_state == MenuState::None || self.menu_state == MenuState::Filter
    }
}

/// Tracks recently-dead processes for one additional display cycle.
///
/// When `keep_dead_proc_usage` is enabled, processes that disappear
/// between collection cycles are preserved with their last-known
/// CPU/memory values so they don't flicker out of the display.
struct StaleProcessTracker {
    /// Recently-dead process entries (PID → last known data).
    stale: HashMap<u32, crate::domain::process::ProcInfo>,
    /// PIDs from the previous collection cycle.
    prev_pids: HashSet<u32>,
    /// ProcInfo cache from the previous cycle for stale lookup.
    prev_cache: HashMap<u32, crate::domain::process::ProcInfo>,
}

impl StaleProcessTracker {
    fn new() -> Self {
        Self {
            stale: HashMap::new(),
            prev_pids: HashSet::new(),
            prev_cache: HashMap::new(),
        }
    }

    /// Update stale tracking with the current live process list.
    ///
    /// Detects PIDs that were in the previous cycle but are now gone,
    /// and preserves their last-known data for one display cycle.
    fn update(&mut self, live_procs: &[crate::domain::process::ProcInfo], keep_dead: bool) {
        let active_pids: HashSet<u32> = live_procs.iter().map(|p| p.pid).collect();

        self.stale.clear();
        if keep_dead {
            for &pid in &self.prev_pids {
                if !active_pids.contains(&pid)
                    && let Some(info) = self.prev_cache.get(&pid)
                {
                    self.stale.insert(pid, info.clone());
                }
            }
        }

        self.prev_pids = active_pids;
        self.prev_cache.clear();
        for proc in live_procs {
            self.prev_cache.insert(proc.pid, proc.clone());
        }
    }

    /// Returns the stale process entries from the previous cycle.
    fn stale_procs(&self) -> &HashMap<u32, crate::domain::process::ProcInfo> {
        &self.stale
    }
}

pub(crate) struct ProcessViewState {
    pub(crate) start: usize,
    pub(crate) selected: usize,
    pub(crate) detailed_pid: u32,
    pub(crate) selected_pid: u32,
    pub(crate) followed_pid: u32,
    pub(crate) filter_text: String,
    pub(crate) entries: Vec<ProcDisplayEntry>,
    /// Combined procs list (live + stale) that entries' `proc_index` refers to.
    /// Only populated when `keep_dead_proc_usage` adds stale entries.
    pub(crate) display_procs: Option<Vec<crate::domain::process::ProcInfo>>,
    /// Armed terminate state: (pid, process_name, force_kill).
    /// Set on first `t`/`T`, cleared on second press or any other key.
    pub(crate) armed_terminate: Option<(u32, String, bool)>,
    stale_tracker: StaleProcessTracker,
}

impl ProcessViewState {
    fn new() -> Self {
        Self {
            start: 0,
            selected: 0,
            detailed_pid: 0,
            selected_pid: 0,
            followed_pid: 0,
            filter_text: String::new(),
            entries: Vec::new(),
            display_procs: None,
            armed_terminate: None,
            stale_tracker: StaleProcessTracker::new(),
        }
    }

    pub(crate) fn update_stale_procs(
        &mut self,
        procs: &[crate::domain::process::ProcInfo],
        keep_dead: bool,
    ) {
        self.stale_tracker.update(procs, keep_dead);
    }

    pub(crate) fn rebuild_entries(
        &mut self,
        procs: Option<&[crate::domain::process::ProcInfo]>,
        config: &config::Config,
    ) {
        let Some(live_procs) = procs else {
            self.entries.clear();
            self.display_procs = None;
            return;
        };

        // Build combined procs list with stale entries if keep_dead_proc_usage.
        let stale = self.stale_tracker.stale_procs();
        let has_stale = config.keep_dead_proc_usage && !stale.is_empty();
        if has_stale {
            let combined: Vec<crate::domain::process::ProcInfo> = live_procs
                .iter()
                .cloned()
                .chain(stale.values().cloned())
                .collect();
            self.display_procs = Some(combined);
        } else {
            self.display_procs = None;
        }
        let procs: &[crate::domain::process::ProcInfo] =
            self.display_procs.as_deref().unwrap_or(live_procs);

        let sort_by = config.proc_sorting;
        let reversed = config.proc_reversed;
        let filter = &config.proc_filter;
        let tree_mode = config.proc_tree;
        let aggregate = config.proc_aggregate;
        self.entries = crate::collect::process_display::build_proc_display_entries(
            procs, sort_by, reversed, filter, tree_mode, aggregate,
        );

        // Keep selection on the same row index (btop behavior).
        // Only track by PID when Follow mode is active.
        if !self.entries.is_empty() {
            self.selected = self.selected.min(self.entries.len() - 1);
        } else {
            self.selected = 0;
        }
        // Update selected_pid to reflect whatever process is now on this row.
        self.selected_pid = self
            .entries
            .get(self.selected)
            .and_then(|e| procs.get(e.proc_index))
            .map_or(0, |p| p.pid);

        // Auto-scroll to followed process.
        let followed = self.followed_pid;
        if followed > 0 {
            if let Some(idx) = self
                .entries
                .iter()
                .position(|e| procs.get(e.proc_index).is_some_and(|p| p.pid == followed))
            {
                self.selected = idx;
                self.selected_pid = followed;
            } else {
                // Followed process died — unfollow.
                self.followed_pid = 0;
            }
        }
    }
}

pub(crate) struct NetworkViewState {
    pub(crate) selected_iface: String,
}

impl NetworkViewState {
    fn new() -> Self {
        Self {
            selected_iface: String::new(),
        }
    }

    pub(crate) fn reconcile(
        &mut self,
        nets: &[crate::domain::network::NetInfo],
        preferred: &str,
        dirty: &mut Dirty,
    ) {
        if nets.is_empty() {
            if !self.selected_iface.is_empty() {
                self.selected_iface.clear();
                *dirty |= Dirty::NET_WIDGET;
            }
            return;
        }

        // If we have no selection yet, try the preferred interface from config
        if self.selected_iface.is_empty()
            && preferred != "auto"
            && !preferred.is_empty()
            && nets.iter().any(|n| n.name == preferred)
        {
            self.selected_iface = preferred.to_string();
            *dirty |= Dirty::NET_WIDGET;
            return;
        }

        if self.selected_iface.is_empty() || !nets.iter().any(|n| n.name == self.selected_iface) {
            self.selected_iface = nets[0].name.clone();
            *dirty |= Dirty::NET_WIDGET;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_initializes_from_config() {
        let mut config = config::Config::new();
        config.rounded_corners = false;
        config.update_ms = 1_500;
        let now = Instant::now();

        let state = AppState::new(&config, now);

        assert!(!state.runtime.rounded);
        assert_eq!(state.runtime.update_ms, 1_500);
        assert!(state.overlay.menu_state == MenuState::None);
        assert_eq!(state.render.dirty, Dirty::FULL);
        assert!(state.render.cached_layout.is_none());
        assert!(!state.live.is_ready());
        assert!(state.process.entries.is_empty());
        assert!(state.network.selected_iface.is_empty());
    }

    #[test]
    fn app_state_render_ui_only_for_normal_and_filter() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());

        state.overlay.menu_state = MenuState::None;
        assert!(state.overlay.render_ui());

        state.overlay.menu_state = MenuState::Filter;
        assert!(state.overlay.render_ui());

        state.overlay.menu_state = MenuState::Main;
        assert!(!state.overlay.render_ui());

        state.overlay.menu_state = MenuState::Help;
        assert!(!state.overlay.render_ui());

        state.overlay.menu_state = MenuState::Options;
        assert!(!state.overlay.render_ui());
    }

    #[test]
    fn layout_hints_disk_rows_per_unit_covers_all_four_view_modes() {
        let mut config = config::Config::new();
        let state = AppState::new(&config, Instant::now());

        // Usage view + show_io_stat on → 2 rows per disk.
        config.io_mode = false;
        config.disk_io_mode = false;
        config.show_io_stat = true;
        assert_eq!(state.live.layout_hints(&config).disk_rows_per_unit, 2);

        // Usage view + show_io_stat off → 1 row per disk.
        config.show_io_stat = false;
        assert_eq!(state.live.layout_hints(&config).disk_rows_per_unit, 1);

        // IO view + split graphs → 2 rows per disk regardless of show_io_stat.
        config.io_mode = true;
        config.io_graph_combined = false;
        assert_eq!(state.live.layout_hints(&config).disk_rows_per_unit, 2);

        // IO view + combined graph → 1 row per disk.
        config.io_graph_combined = true;
        assert_eq!(state.live.layout_hints(&config).disk_rows_per_unit, 1);

        // Persistent disk_io_mode behaves the same as runtime io_mode.
        config.io_mode = false;
        config.disk_io_mode = true;
        config.io_graph_combined = false;
        assert_eq!(state.live.layout_hints(&config).disk_rows_per_unit, 2);
    }

    #[test]
    fn enter_option_edit_transitions_and_stores_buffer() {
        use crate::config::{ConfigKey, StringKey};
        use crate::handlers::options_edit::{EditKind, OptionEditState};
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());

        // Start in Options (precondition for entering edit mode).
        state.overlay.set_menu_state(MenuState::Main);
        state.overlay.set_menu_state(MenuState::Options);
        assert_eq!(state.overlay.menu_state, MenuState::Options);
        assert!(state.overlay.option_edit().is_none());

        let edit = OptionEditState::new(
            ConfigKey::String(StringKey::ClockFormat),
            EditKind::Text,
            "%H:%M".into(),
        );
        state.overlay.enter_option_edit(edit);
        assert_eq!(state.overlay.menu_state, MenuState::OptionsEdit);
        let stored = state
            .overlay
            .option_edit()
            .expect("option_edit must be Some after enter_option_edit");
        assert_eq!(stored.buffer, "%H:%M");
        assert_eq!(stored.key, ConfigKey::String(StringKey::ClockFormat));
    }

    #[test]
    fn exit_option_edit_clears_buffer_and_returns_to_options() {
        use crate::config::{ConfigKey, StringKey};
        use crate::handlers::options_edit::{EditKind, OptionEditState};
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());

        state.overlay.set_menu_state(MenuState::Main);
        state.overlay.set_menu_state(MenuState::Options);
        state.overlay.enter_option_edit(OptionEditState::new(
            ConfigKey::String(StringKey::ProcFilter),
            EditKind::Text,
            String::new(),
        ));
        let returned = state.overlay.exit_option_edit();
        assert!(returned.is_some(), "exit must return the discarded buffer");
        assert_eq!(state.overlay.menu_state, MenuState::Options);
        assert!(state.overlay.option_edit().is_none());
    }

    #[test]
    #[should_panic(expected = "option_edit must be cleared")]
    fn set_menu_state_panics_if_option_edit_is_dangling() {
        use crate::config::{ConfigKey, StringKey};
        use crate::handlers::options_edit::{EditKind, OptionEditState};
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());

        state.overlay.set_menu_state(MenuState::Main);
        state.overlay.set_menu_state(MenuState::Options);
        state.overlay.enter_option_edit(OptionEditState::new(
            ConfigKey::String(StringKey::ProcFilter),
            EditKind::Text,
            String::new(),
        ));
        // Bypassing exit_option_edit (which would clear option_edit
        // first) must trip the invariant assertion in set_menu_state.
        state.overlay.set_menu_state(MenuState::Options);
    }

    #[test]
    fn mark_resize_sets_layout_and_all_widgets() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, Instant::now());
        state.render.clear_dirty();

        state.render.mark_resize();

        assert!(state.render.dirty.contains(Dirty::LAYOUT));
        assert!(state.render.dirty.intersects(Dirty::ALL_WIDGETS));
    }

    fn make_proc(pid: u32) -> crate::domain::process::ProcInfo {
        crate::domain::process::ProcInfo {
            pid,
            name: format!("proc{pid}"),
            ..Default::default()
        }
    }

    #[test]
    fn stale_tracker_no_stale_on_first_update() {
        let mut tracker = StaleProcessTracker::new();
        tracker.update(&[make_proc(1), make_proc(2)], true);
        assert!(tracker.stale_procs().is_empty());
    }

    #[test]
    fn stale_tracker_detects_dead_process() {
        let mut tracker = StaleProcessTracker::new();
        tracker.update(&[make_proc(1), make_proc(2)], true);
        tracker.update(&[make_proc(1)], true);
        assert_eq!(tracker.stale_procs().len(), 1);
        assert!(tracker.stale_procs().contains_key(&2));
    }

    #[test]
    fn stale_tracker_one_cycle_retention() {
        let mut tracker = StaleProcessTracker::new();
        tracker.update(&[make_proc(1), make_proc(2)], true);
        tracker.update(&[make_proc(1)], true);
        assert!(tracker.stale_procs().contains_key(&2));
        // Third cycle: pid 2 is no longer in prev_pids, so it drops out.
        tracker.update(&[make_proc(1)], true);
        assert!(tracker.stale_procs().is_empty());
    }

    #[test]
    fn stale_tracker_keep_dead_false_clears_stale() {
        let mut tracker = StaleProcessTracker::new();
        tracker.update(&[make_proc(1), make_proc(2)], true);
        tracker.update(&[make_proc(1)], false);
        assert!(tracker.stale_procs().is_empty());
    }

    #[test]
    fn stale_tracker_all_processes_die() {
        let mut tracker = StaleProcessTracker::new();
        tracker.update(&[make_proc(1), make_proc(2)], true);
        tracker.update(&[], true);
        assert_eq!(tracker.stale_procs().len(), 2);
    }
}
