//! Per-frame application state owned by the event loop.
//!
//! `AppState` bundles the structs that live for the entire process
//! lifetime and are mutated by the input handlers and the data-pull
//! pipeline. Each sub-struct is a focused container grouped by
//! responsibility — collected snapshots ([`LiveData`]),
//! runtime-toggle view state ([`RuntimeView`]), per-frame render
//! cache ([`RenderState`]), overlay/menu state ([`OverlayState`]),
//! process-list view state ([`ProcessViewState`]), the active
//! network interface ([`NetworkViewState`]), and the user's
//! widget-visibility filter ([`WidgetFilter`]).
//!
//! Note that [`RuntimeView`] holds the **persisted/preferred**
//! interface name in `net_iface`, while [`NetworkViewState`] holds
//! the **effective/displayed** interface in `selected_iface` —
//! these can diverge when the saved interface isn't currently
//! present (e.g. unplugged Ethernet, disabled Wi-Fi). See
//! [`NetworkViewState`] for the full policy.

use crate::config;
use crate::dirty::RenderDirty;
use crate::domain::process::ProcDisplayEntry;
use crate::domain::widget_kind::WidgetKind;
use crate::domain::widget_set::WidgetSet;
use crate::draw;
use crate::handlers::MenuState;
use crate::runner;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(crate) struct AppState {
    pub(crate) view: RuntimeView,
    pub(crate) render: RenderState,
    pub(crate) live: LiveData,
    pub(crate) overlay: OverlayState,
    pub(crate) process: ProcessViewState,
    pub(crate) network: NetworkViewState,
    pub(crate) filter: WidgetFilter,
}

impl AppState {
    pub(crate) fn new(config: &config::Config) -> Self {
        Self {
            view: RuntimeView::from_config(&config.view),
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
                hidden: config.hidden_widgets,
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
        let hints = self.live.layout_hints(config, &self.view);
        let mut hidden = WidgetSet::new();
        for n in (hints.gpu_count as u8)..(crate::config::MAX_GPUS as u8) {
            hidden.insert(WidgetKind::Gpu(n));
        }
        hidden.extend_from(&self.filter.hidden);
        hidden
    }
}

/// Runtime view-state — the user's current toggle/select gestures
/// for fields that mirror back to [`crate::config::ViewConfig`] on
/// save.
///
/// **Sync contract** (see [`crate::config::ViewConfig`] for the
/// rationale):
///
/// 1. `AppState::new` initialises this from `config.view`
///    ([`RuntimeView::from_config`]).
/// 2. Opening the options menu copies `RuntimeView -> config.view`
///    ([`RuntimeView::sync_to_config`]) so the menu shows current
///    values.
/// 3. Committing an options-menu edit copies
///    `config.view -> RuntimeView`
///    ([`RuntimeView::sync_from_config`]) so the runtime picks up
///    the user's change.
/// 4. Process exit runs `sync_to_config` before serialising so the
///    on-disk form reflects the current runtime values.
///
/// Handler runtime toggles (`e`, `r`, `c`, Left/Right, `i`, `a`,
/// `s`, Tab, `f`/`/`) mutate this struct only — they never reach
/// `&mut Config`.
#[derive(Debug, Clone)]
pub(crate) struct RuntimeView {
    pub(crate) proc_tree: bool,
    pub(crate) proc_reversed: bool,
    pub(crate) proc_per_core: bool,
    pub(crate) proc_sorting: crate::collect::process_display::ProcSort,
    pub(crate) proc_filter: String,
    pub(crate) io_mode: bool,
    pub(crate) net_auto: bool,
    pub(crate) net_sync: bool,
    pub(crate) net_iface: String,
}

impl RuntimeView {
    /// Initialise from a freshly loaded `ViewConfig` (typically at
    /// `AppState::new` or after `Config::reload`).
    pub(crate) fn from_config(view: &crate::config::ViewConfig) -> Self {
        Self {
            proc_tree: view.proc_tree,
            proc_reversed: view.proc_reversed,
            proc_per_core: view.proc_per_core,
            proc_sorting: view.proc_sorting,
            proc_filter: view.proc_filter.clone(),
            io_mode: view.io_mode,
            net_auto: view.net_auto,
            net_sync: view.net_sync,
            net_iface: view.net_iface.clone(),
        }
    }

    /// Copy `self` into `view` — the persisted snapshot. Called
    /// before opening the options menu (so the menu shows current
    /// values) and before saving.
    pub(crate) fn sync_to_config(&self, view: &mut crate::config::ViewConfig) {
        view.proc_tree = self.proc_tree;
        view.proc_reversed = self.proc_reversed;
        view.proc_per_core = self.proc_per_core;
        view.proc_sorting = self.proc_sorting;
        view.proc_filter.clone_from(&self.proc_filter);
        view.io_mode = self.io_mode;
        view.net_auto = self.net_auto;
        view.net_sync = self.net_sync;
        view.net_iface.clone_from(&self.net_iface);
    }

    /// Copy `view` into `self`. Called after the user commits an
    /// options-menu edit so the runtime picks up the change.
    pub(crate) fn sync_from_config(&mut self, view: &crate::config::ViewConfig) {
        self.proc_tree = view.proc_tree;
        self.proc_reversed = view.proc_reversed;
        self.proc_per_core = view.proc_per_core;
        self.proc_sorting = view.proc_sorting;
        self.proc_filter.clone_from(&view.proc_filter);
        self.io_mode = view.io_mode;
        self.net_auto = view.net_auto;
        self.net_sync = view.net_sync;
        self.net_iface.clone_from(&view.net_iface);
    }
}

pub(crate) struct RenderState {
    pub(crate) dirty: RenderDirty,
    pub(crate) cached_layout: Option<draw::layout::Layout>,
    pub(crate) last_layout_hints: Option<draw::layout::LayoutHints>,
}

impl RenderState {
    fn new() -> Self {
        Self {
            dirty: RenderDirty::full(),
            cached_layout: None,
            last_layout_hints: None,
        }
    }

    pub(crate) fn mark_resize(&mut self) {
        self.dirty.mark_layout();
    }

    pub(crate) fn clear_dirty(&mut self) {
        self.dirty.clear();
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

    pub(crate) fn layout_hints(
        &self,
        config: &config::Config,
        view: &RuntimeView,
    ) -> draw::layout::LayoutHints {
        // Disk widget rows-per-disk depends on which view is active:
        //   * Usage view (default): 2 rows when `show_io_stat` adds
        //     the inline read/write/busy row under each capacity
        //     meter; 1 row otherwise.
        //   * IO view (`view.io_mode` runtime toggle or persistent
        //     `disk.disk_io_mode`): 2 rows for separate read+write
        //     graphs; 1 row when `io_graph_combined` merges them.
        let io_view = view.io_mode || config.disk.disk_io_mode;
        let disk_rows_per_unit = if io_view {
            if config.disk.io_graph_combined { 1 } else { 2 }
        } else if config.disk.show_io_stat {
            2
        } else {
            1
        };
        draw::layout::LayoutHints {
            core_count: self.core_count,
            gpu_count: self.gpu.as_ref().map_or(0, |g| g.gpus.len()),
            disk_count: crate::domain::disk::DisksFilter::parse(&config.disk.disks_filter)
                .count_matching(self.disk.as_ref().map_or(&[], |d| &d.info.disks)),
            has_swap: config.mem.show_swap
                && self
                    .mem
                    .as_ref()
                    .is_some_and(|m| m.info.stats.swap_total > 0),
            has_cpu_temp: config.cpu.check_temp
                && self.cpu.as_ref().is_some_and(|c| !c.info.temp.is_empty()),
            has_cpu_watts: config.cpu.show_cpu_watts
                && self
                    .cpu
                    .as_ref()
                    .is_some_and(|c| c.info.cpu_watts.is_some()),
            disk_rows_per_unit,
        }
    }
}

// `filtered_disk_count` was inlined into the only remaining caller
// (`LiveData::layout_hints`) — see the `disk_count:` field above.

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
        view: &RuntimeView,
    ) {
        let Some(live_procs) = procs else {
            self.entries.clear();
            self.display_procs = None;
            return;
        };

        // Build combined procs list with stale entries if keep_dead_proc_usage.
        let stale = self.stale_tracker.stale_procs();
        let has_stale = config.proc.keep_dead_proc_usage && !stale.is_empty();
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

        let sort_by = view.proc_sorting;
        let reversed = view.proc_reversed;
        let filter = &view.proc_filter;
        let tree_mode = view.proc_tree;
        let aggregate = config.proc.proc_aggregate;
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

/// Effective network-interface selection — the interface
/// currently being **displayed**.
///
/// This is **not** always the interface the user has saved as
/// their preference. Two roles must be distinguished:
///
/// * [`RuntimeView::net_iface`] — the **persisted/preferred**
///   interface name. Initialised from `config.view.net_iface`,
///   mirrored back on save. Mutated by `cycle_iface_*_action`
///   handlers when the user explicitly cycles to a new interface.
/// * [`NetworkViewState::selected_iface`] — the **effective**
///   interface for this frame. Mutated by
///   [`Self::reconcile`] when the preferred interface isn't
///   currently present (cable unplugged, Wi-Fi disabled), and by
///   the same cycle handlers (which keep both fields in sync).
///
/// User-visible policy this implements:
///
/// * Saved `Ethernet`, unplug Ethernet → display falls back to
///   Wi-Fi; `rtop.toml` still says `Ethernet`. Plug Ethernet back
///   in *next session* → it auto-selects.
/// * Cycle to Wi-Fi explicitly → both fields update; `rtop.toml`
///   on quit says `Wi-Fi`.
///
/// In-session "fall forward" (preferred iface reappears →
/// auto-switch back) is **not** implemented — once
/// `selected_iface` is non-empty and present in the live list,
/// reconcile leaves it alone. The user's saved preference re-asserts
/// only at the next process restart.
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
        dirty: &mut RenderDirty,
    ) {
        if nets.is_empty() {
            if !self.selected_iface.is_empty() {
                self.selected_iface.clear();
                dirty.mark_widget(WidgetKind::Net);
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
            dirty.mark_widget(WidgetKind::Net);
            return;
        }

        if self.selected_iface.is_empty() || !nets.iter().any(|n| n.name == self.selected_iface) {
            self.selected_iface = nets[0].name.clone();
            dirty.mark_widget(WidgetKind::Net);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_initializes_from_config() {
        let mut config = config::Config::new();
        config.ui.rounded_corners = false;
        config.refresh.update_ms = 1_500;

        let state = AppState::new(&config);

        assert!(state.overlay.menu_state == MenuState::None);
        assert_eq!(state.render.dirty, RenderDirty::full());
        assert!(state.render.cached_layout.is_none());
        assert!(!state.live.is_ready());
        assert!(state.process.entries.is_empty());
        assert!(state.network.selected_iface.is_empty());
    }

    #[test]
    fn runtime_view_initialises_from_config_view() {
        // AppState::new should mirror config.view into state.view at
        // startup so the runtime mirror matches the loaded TOML.
        let mut config = config::Config::new();
        config.view.proc_tree = true;
        config.view.proc_filter = "chrome".to_string();
        config.view.io_mode = true;
        config.view.net_iface = "Ethernet".to_string();

        let state = AppState::new(&config);

        assert!(state.view.proc_tree);
        assert_eq!(state.view.proc_filter, "chrome");
        assert!(state.view.io_mode);
        assert_eq!(state.view.net_iface, "Ethernet");
    }

    #[test]
    fn runtime_view_sync_to_config_mirrors_back() {
        // After handler runtime toggles, sync_to_config copies the
        // RuntimeView state back into config.view so the persisted
        // form matches the runtime values (used at save).
        let mut config = config::Config::new();
        let mut view = RuntimeView::from_config(&config.view);

        // Simulate handler-side runtime toggles.
        view.proc_tree = true;
        view.proc_reversed = true;
        view.proc_per_core = true;
        view.proc_sorting = crate::collect::process_display::ProcSort::Memory;
        view.proc_filter = "rtop".to_string();
        view.io_mode = true;
        view.net_auto = false;
        view.net_sync = true;
        view.net_iface = "Wi-Fi".to_string();

        view.sync_to_config(&mut config.view);

        assert!(config.view.proc_tree);
        assert!(config.view.proc_reversed);
        assert!(config.view.proc_per_core);
        assert_eq!(
            config.view.proc_sorting,
            crate::collect::process_display::ProcSort::Memory
        );
        assert_eq!(config.view.proc_filter, "rtop");
        assert!(config.view.io_mode);
        assert!(!config.view.net_auto);
        assert!(config.view.net_sync);
        assert_eq!(config.view.net_iface, "Wi-Fi");
    }

    #[test]
    fn runtime_view_sync_from_config_picks_up_menu_edit() {
        // After the user edits a runtime-toggle field via the
        // options menu (which mutates config.view via the typed-
        // key API), sync_from_config copies the new value into
        // RuntimeView so handlers see it.
        let mut config = config::Config::new();
        let mut view = RuntimeView::from_config(&config.view);

        // Simulate options-menu edits (BoolKey::toggle, etc.).
        config.view.proc_tree = true;
        config.view.io_mode = true;
        config.view.net_iface = "Ethernet".to_string();

        view.sync_from_config(&config.view);

        assert!(view.proc_tree);
        assert!(view.io_mode);
        assert_eq!(view.net_iface, "Ethernet");
    }

    #[test]
    fn app_state_render_ui_only_for_normal_and_filter() {
        let config = config::Config::new();
        let mut state = AppState::new(&config);

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
        let mut state = AppState::new(&config);

        // Usage view + show_io_stat on → 2 rows per disk.
        // (`io_mode` lives on `RuntimeView` not `Config` after the
        //  view-state extraction; mutate both so the layout-hint
        //  arithmetic sees the same value.)
        state.view.io_mode = false;
        config.disk.disk_io_mode = false;
        config.disk.show_io_stat = true;
        assert_eq!(
            state
                .live
                .layout_hints(&config, &state.view)
                .disk_rows_per_unit,
            2
        );

        // Usage view + show_io_stat off → 1 row per disk.
        config.disk.show_io_stat = false;
        assert_eq!(
            state
                .live
                .layout_hints(&config, &state.view)
                .disk_rows_per_unit,
            1
        );

        // IO view + split graphs → 2 rows per disk regardless of show_io_stat.
        state.view.io_mode = true;
        config.disk.io_graph_combined = false;
        assert_eq!(
            state
                .live
                .layout_hints(&config, &state.view)
                .disk_rows_per_unit,
            2
        );

        // IO view + combined graph → 1 row per disk.
        config.disk.io_graph_combined = true;
        assert_eq!(
            state
                .live
                .layout_hints(&config, &state.view)
                .disk_rows_per_unit,
            1
        );

        // Persistent disk_io_mode behaves the same as runtime io_mode.
        state.view.io_mode = false;
        config.disk.disk_io_mode = true;
        config.disk.io_graph_combined = false;
        assert_eq!(
            state
                .live
                .layout_hints(&config, &state.view)
                .disk_rows_per_unit,
            2
        );
    }

    #[test]
    fn enter_option_edit_transitions_and_stores_buffer() {
        use crate::config::{ConfigKey, StringKey};
        use crate::handlers::options_edit::{EditKind, OptionEditState};
        let config = config::Config::new();
        let mut state = AppState::new(&config);

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
        let mut state = AppState::new(&config);

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
        let mut state = AppState::new(&config);

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
        let mut state = AppState::new(&config);
        state.render.clear_dirty();

        state.render.mark_resize();

        assert!(state.render.dirty.needs_layout());
        assert!(state.render.dirty.is_any_widget_dirty());
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
