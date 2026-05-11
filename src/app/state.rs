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
use crate::domain::process::{ProcDisplayEntry, ProcInfo};
use crate::domain::widget_kind::WidgetKind;
use crate::domain::widget_set::WidgetSet;
use crate::draw;
use crate::overlay::ActiveModal;
use crate::runner;
use std::collections::HashSet;
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
    /// Construct fresh app state.
    ///
    /// `gpu_count` is the number of GPUs discovered by
    /// [`crate::runner::CollectorManager::start`]. Discovery is
    /// one-shot at startup so this value is fixed for the lifetime
    /// of the `AppState`; it threads through to
    /// [`LiveData::gpu_count`] and from there into
    /// `compose_hidden`, `is_ready`, and `layout_hints`. Test
    /// fixtures pass `0` because they do not exercise GPU
    /// behaviour.
    pub(crate) fn new(config: &config::Config, gpu_count: u8) -> Self {
        Self {
            view: RuntimeView::from_config(&config.view),
            render: RenderState::new(),
            live: LiveData::new(gpu_count),
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
    /// backing device, derived from `LiveData`'s detected GPU count),
    /// the statusbar's master `show_statusbar` config toggle, and
    /// the user's runtime view filter.
    ///
    /// The engine consumes the result without caring why a widget
    /// is hidden — there is one source of truth per frame, built
    /// here.
    pub(crate) fn compose_hidden(&self, config: &config::Config) -> WidgetSet {
        // Inline the single integer this function needs from
        // `LiveData` rather than calling `layout_hints` — the full
        // hints struct triggers the statusbar's label-width
        // pre-computation (string allocations + `format_clock_width`
        // for every call), and `compose_hidden` runs twice per
        // layout-dirty frame.
        let gpu_count = self.live.gpu_count;
        let mut hidden = WidgetSet::new();
        for n in gpu_count..(crate::config::MAX_GPUS as u8) {
            hidden.insert(WidgetKind::Gpu(n));
        }
        // Master `show_statusbar` toggle. When off, route the
        // statusbar through the same engine path as any other
        // hidden widget so its row is reclaimed by the layout
        // above (rather than left as a dead band).
        if !config.statusbar.show_statusbar {
            hidden.insert(WidgetKind::Statusbar);
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

/// Cached dimmed snapshot of the widget layer plus the
/// previous-frame dim boundary. Packaged together so that the
/// "snapshot" and "boundary moved since last frame" data points
/// cannot drift apart, and so the reconciliation rule lives in one
/// method instead of being open-coded at the call site.
#[derive(Debug, Default)]
struct DimUnderlayCache {
    /// Cached dimmed snapshot of the widget layer. Populated on
    /// the first render after a centered modal opens, held until
    /// the modal closes or the terminal resizes.
    snapshot: Option<String>,
    /// The previous frame's `dims_underlay()` result. Together with
    /// the snapshot this lets `reconcile_boundary` notice modal
    /// open/close transitions and invalidate reactively.
    last_dims: bool,
}

impl DimUnderlayCache {
    /// Note the current modal-dim state. If the boundary moved since
    /// the previous call, drop the cached snapshot. Returns the
    /// (now-reconciled) current dim state for caller convenience.
    fn reconcile_boundary(&mut self, dims_now: bool) -> bool {
        if dims_now != self.last_dims {
            self.snapshot = None;
            self.last_dims = dims_now;
        }
        dims_now
    }

    /// Borrow the cached snapshot, if any.
    fn snapshot(&self) -> Option<&str> {
        self.snapshot.as_deref()
    }

    /// Replace the cached snapshot.
    fn store(&mut self, s: String) {
        self.snapshot = Some(s);
    }

    /// Drop the snapshot. Called by terminal-resize handling.
    fn invalidate(&mut self) {
        self.snapshot = None;
    }
}

pub(crate) struct RenderState {
    pub(crate) dirty: RenderDirty,
    cached_layout: Option<draw::layout::Layout>,
    pub(crate) last_layout_hints: Option<draw::layout::LayoutHints>,
    /// Dimmed-underlay cache + boundary tracker. Encapsulated so
    /// that no caller can read the snapshot without going through
    /// [`RenderState::reconcile_dim_boundary`], and the snapshot
    /// cannot drift past a modal open/close transition.
    dim_cache: DimUnderlayCache,
}

impl RenderState {
    fn new() -> Self {
        Self {
            dirty: RenderDirty::full(),
            cached_layout: None,
            last_layout_hints: None,
            dim_cache: DimUnderlayCache::default(),
        }
    }

    pub(crate) fn mark_resize(&mut self) {
        self.dirty.mark_layout();
        // Resize while a modal is open invalidates the cached
        // dimmed underlay (its layout no longer matches the
        // terminal). The next overlay-active render will rebuild
        // it at the new size.
        self.dim_cache.invalidate();
    }

    pub(crate) fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// Borrow the cached active layout, if one has been computed
    /// this session. The layout is recomputed lazily by
    /// [`crate::app::dirty_exec::execute_dirty_work`] when
    /// `dirty.needs_layout()` is set or when this returns `None`.
    pub(crate) fn cached_layout(&self) -> Option<&draw::layout::Layout> {
        self.cached_layout.as_ref()
    }

    /// Replace the cached active layout with a freshly-computed one.
    pub(crate) fn set_cached_layout(&mut self, layout: draw::layout::Layout) {
        self.cached_layout = Some(layout);
    }

    /// Reconcile the dimmed-underlay cache against the current modal
    /// boundary. If the boundary moved since the previous frame,
    /// drop the cached snapshot. Returns the (now-reconciled)
    /// current dim state — `true` if a centered modal is active and
    /// the caller should compose, `false` otherwise.
    pub(crate) fn reconcile_dim_boundary(&mut self, dims_now: bool) -> bool {
        self.dim_cache.reconcile_boundary(dims_now)
    }

    /// Borrow the cached dimmed underlay snapshot, if one is
    /// populated. Read by the modal compose path.
    pub(crate) fn cached_dimmed_underlay(&self) -> Option<&str> {
        self.dim_cache.snapshot()
    }

    /// Store a freshly-built dimmed underlay snapshot. Called by the
    /// modal compose path immediately after rendering and dimming
    /// the widget layer.
    pub(crate) fn store_dimmed_underlay(&mut self, s: String) {
        self.dim_cache.store(s);
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
    /// Per-device GPU snapshots, one slot per discovered device.
    /// Slots `n >= gpu_count` stay empty for the lifetime of the
    /// process; slots `n < gpu_count` populate from their per-device
    /// collector thread's [`crate::runner::CollectorManager::gpu_slots`]
    /// entry on each pull.
    pub(crate) gpu: [Option<Arc<runner::GpuSnapshot>>; crate::config::MAX_GPUS],
    pub(crate) proc_data: Option<Arc<runner::ProcSnapshot>>,
    /// Latest statusbar snapshot (uptime). Refreshed at the
    /// statusbar collector's hardcoded 1 Hz cadence; the
    /// statusbar widget reads this when rendering.
    ///
    /// Intentionally **not** part of [`LiveData::is_ready`] — the
    /// statusbar's absence does not justify delaying the first
    /// frame, and the bar gracefully renders zero uptime until
    /// the first snapshot arrives.
    pub(crate) statusbar: Option<Arc<runner::StatusbarSnapshot>>,
    /// Cached core count for proc widget (stable hardware constant).
    pub(crate) core_count: usize,
    /// Cached total physical memory for proc widget (stable hardware constant).
    pub(crate) total_mem: u64,
    /// Number of GPU devices discovered at startup. Bound at
    /// construction via [`Self::new`] from
    /// [`crate::runner::CollectorManager::gpu_count`]; immutable
    /// thereafter — discovery is one-shot, so this is a hardware
    /// constant for the lifetime of the process. Drives the
    /// layout-hint `gpu_count` and the
    /// `compose_hidden`/`is_ready` GPU iterations.
    pub(crate) gpu_count: u8,
}

impl LiveData {
    /// Construct an empty `LiveData` for a system with `gpu_count`
    /// detected GPUs. `gpu_count` is fixed for the lifetime of
    /// the value; per-device snapshot slots `n < gpu_count`
    /// populate from their per-device collector thread on each
    /// pull, slots `n >= gpu_count` stay empty forever.
    fn new(gpu_count: u8) -> Self {
        Self {
            cpu: None,
            mem: None,
            disk: None,
            net: None,
            gpu: std::array::from_fn(|_| None),
            proc_data: None,
            statusbar: None,
            core_count: 0,
            total_mem: 0,
            gpu_count,
        }
    }

    /// True when every base subsystem has provided at least one
    /// data point AND every discovered GPU has too. A system with
    /// zero detected GPUs trivially satisfies the GPU half (the
    /// `0..0` range is empty).
    pub(crate) fn is_ready(&self) -> bool {
        self.cpu.is_some()
            && self.mem.is_some()
            && self.disk.is_some()
            && self.net.is_some()
            && self.proc_data.is_some()
            && (0..self.gpu_count as usize).all(|n| self.gpu[n].is_some())
    }

    pub(crate) fn layout_hints(
        &self,
        config: &config::Config,
        view: &RuntimeView,
        filter: &WidgetFilter,
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

        // ── Statusbar label widths ────────────────────────────────
        //
        // Pre-compute every label's visible-cell width so the
        // statusbar widget's `min_width` is a pure sum and the
        // engine's `min_terminal_size` integrates the bar correctly
        // for the existing "too small" gate. Each width is zero
        // when its item is hidden so the sum naturally matches the
        // visible content.
        //
        // The plain-text helpers in `crate::ui::statusbar_widget`
        // (`MENU_LABEL`, `preset_label`, `update_label`,
        // `uptime_label`, `format_clock_width`) are the single
        // source of truth for each item's structure; the renderer's
        // `format_*_item` functions are pinned to match these by
        // synchronisation tests so the layout-engine's `min_width`
        // math cannot drift from the renderer's actual output.
        let sb = &config.statusbar;
        let filter_active = !filter.hidden.is_empty();
        let preset_name = config.active_preset().name();
        let statusbar_preset_label_width = if sb.statusbar_show_preset {
            crate::tools::ulen(
                &crate::ui::statusbar_widget::preset_label(preset_name, filter_active),
                false,
            )
        } else {
            0
        };
        let statusbar_update_label_width = if sb.statusbar_show_update_interval {
            crate::tools::ulen(
                &crate::ui::statusbar_widget::update_label(config.refresh.update_ms as u64),
                false,
            )
        } else {
            0
        };
        let statusbar_uptime_label_width = if sb.statusbar_show_uptime {
            // Read the latest snapshot if present; before the first
            // statusbar tick the field is zero (the bar renders an
            // empty uptime that frame, then catches up on the next
            // tick).
            let secs = self.statusbar.as_ref().map_or(0, |s| s.info.uptime_seconds);
            crate::tools::ulen(&crate::ui::statusbar_widget::uptime_label(secs), false)
        } else {
            0
        };
        let statusbar_clock_label_width = if sb.statusbar_show_clock {
            // `format_clock_width` is a pure function of the format
            // string — never reads the wall clock. An empty format
            // returns 0, which matches `format_clock` returning ""
            // and preserves the "empty format = no clock" behaviour.
            crate::tools::format_clock_width(&sb.statusbar_clock_format)
        } else {
            0
        };

        draw::layout::LayoutHints {
            core_count: self.core_count,
            gpu_count: self.gpu_count as usize,
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

            statusbar_show_menu: sb.statusbar_show_menu,
            statusbar_show_preset: sb.statusbar_show_preset,
            statusbar_show_update_interval: sb.statusbar_show_update_interval,
            statusbar_show_uptime: sb.statusbar_show_uptime,
            statusbar_show_clock: sb.statusbar_show_clock,
            statusbar_preset_label_width,
            statusbar_update_label_width,
            statusbar_uptime_label_width,
            statusbar_clock_label_width,
        }
    }
}

// `filtered_disk_count` was inlined into the only remaining caller
// (`LiveData::layout_hints`) — see the `disk_count:` field above.

/// Overlay/menu state for the application.
///
/// [`ActiveModal`] is the single source of truth for "what overlay
/// is open and what is its state." Per-overlay state lives inside
/// the appropriate [`ActiveModal`] variant.
pub(crate) struct OverlayState {
    pub(crate) active: ActiveModal,
}

impl OverlayState {
    fn new() -> Self {
        Self {
            active: ActiveModal::None,
        }
    }

    /// `true` when the widget layer should render at full
    /// brightness (no centered modal active). Filter mode does not
    /// dim — it's an inline prompt — so it returns `true` too.
    pub(crate) fn render_ui(&self) -> bool {
        !self.active.dims_underlay()
    }

    /// Snapshot of the active overlay used by render and dispatch
    /// paths that need an [`ActiveModal`] reference. The `_filter_text`
    /// argument is reserved for the future state-consolidation
    /// step that moves the filter input into [`FilterState`].
    pub(crate) fn active(&self, _filter_text: &str) -> &ActiveModal {
        &self.active
    }
}

pub(crate) struct ProcessViewState {
    pub(crate) start: usize,
    pub(crate) selected: usize,
    /// Detail panel state.
    ///
    /// Replaces an earlier `detailed_pid: u32` field whose `0`
    /// sentinel meant "panel closed". The enum makes the
    /// open/closed split explicit at the type level and lets the
    /// `Open` variant carry the cached `ProcInfo` needed to keep
    /// rendering after the watched process exits — see
    /// [`DetailPanel`] for the full invariant.
    pub(crate) detail: DetailPanel,
    pub(crate) selected_pid: u32,
    pub(crate) followed_pid: u32,
    pub(crate) filter_text: String,
    pub(crate) entries: Vec<ProcDisplayEntry>,
    /// Armed terminate state: (pid, process_name, force_kill).
    /// Set on first `t`/`T`, cleared on second press or any other key.
    pub(crate) armed_terminate: Option<(u32, String, bool)>,
    /// Pause state for the process list.
    ///
    /// When `Some`, the proc widget renders from `pause.snapshot`
    /// instead of from `LiveData::proc_data`. Live collection
    /// continues — `LiveData::proc_data` still updates each cycle so
    /// resume is instant — but the on-screen list is frozen at the
    /// PIDs and values that existed at pause time.
    ///
    /// `pause.dead_pids` tracks PIDs from the frozen snapshot that
    /// no longer appear in the latest live snapshot. The proc widget
    /// renders those rows with the dead-row theme color
    /// (`dead_proc_fg`) and a `✗ ` prefix on the name column so the
    /// user can tell which rows in the snapshot represent processes
    /// that have since exited.
    ///
    /// Pause is *runtime only* — it does not survive restart.
    /// Persisting it would risk launching rtop with a frozen list
    /// from a previous session that the user has forgotten about.
    /// Other view-state toggles (`proc_tree`, `io_mode`, …) persist
    /// because their behavior is obvious from the rendered output;
    /// pause's "frozen UI" is more easily mistaken for a hung
    /// program.
    pub(crate) pause: Option<PauseState>,
}

/// Process detail panel state.
///
/// Two-state machine that controls whether the proc widget renders
/// the detail panel above the process list.
///
/// * `Closed` — no panel; layout reserves no detail rows.
/// * `Open` — a panel is visible for the PID inside [`OpenDetail`].
///   The variant carries a cached `ProcInfo` (`last_seen`) that the
///   renderer uses as the data source. The cache is refreshed every
///   cycle the active procs source contains the PID and is preserved
///   as-is once the PID disappears, so the panel keeps rendering its
///   last-known values with a `✗ Process exited` annotation in both
///   live (non-paused) and paused modes.
///
/// Together with the `dead` flag computed at render time
/// (`dead = !live_snapshot.contains(pid)`) this gives a single
/// unified rule across paused and live modes: the panel persists
/// once opened and surfaces the dead state through the same status
/// line in both modes.
#[derive(Debug, Clone)]
pub(crate) enum DetailPanel {
    Closed,
    Open(OpenDetail),
}

/// Inner state for [`DetailPanel::Open`].
#[derive(Debug, Clone)]
pub(crate) struct OpenDetail {
    /// PID the panel is currently inspecting.
    pub(crate) pid: u32,
    /// Most recent [`ProcInfo`] observed for `pid` from the active
    /// procs source (paused snapshot when paused, live snapshot
    /// otherwise). Refreshed by
    /// [`ProcessViewState::refresh_detail_cache`] every time the
    /// source contains the PID; preserved as-is when the source
    /// omits it.
    pub(crate) last_seen: ProcInfo,
}

/// Pause state captured the moment the user toggled pause on. Lives
/// on [`ProcessViewState`]; see the `pause` field's doc comment for
/// the full snapshot-freeze invariant.
#[derive(Debug, Clone)]
pub(crate) struct PauseState {
    /// Process snapshot captured at pause time. The proc widget
    /// renders from `snapshot.procs` while this is held; values in
    /// every cell are the values at pause time, not live values.
    pub(crate) snapshot: Arc<runner::ProcSnapshot>,
    /// PIDs from `snapshot` that are no longer present in the most
    /// recent live snapshot. Recomputed on every live-data arrival
    /// while paused; used by the proc widget to render dead-row
    /// styling and by the terminate action to reject doomed
    /// syscalls.
    pub(crate) dead_pids: HashSet<u32>,
}

impl ProcessViewState {
    fn new() -> Self {
        Self {
            start: 0,
            selected: 0,
            detail: DetailPanel::Closed,
            selected_pid: 0,
            followed_pid: 0,
            filter_text: String::new(),
            entries: Vec::new(),
            armed_terminate: None,
            pause: None,
        }
    }

    /// Open the detail panel for `info.pid`, seeding `last_seen`
    /// from `info`. Caller is responsible for marking the proc
    /// widget dirty.
    ///
    /// The caller resolves the `ProcInfo` from the active procs
    /// source (typically the row under the cursor), so this method
    /// never has to look up by PID and cannot fail. This keeps the
    /// open-detail path total — no `expect`, no `Option` shuffle.
    pub(crate) fn open_detail(&mut self, info: ProcInfo) {
        let pid = info.pid;
        self.detail = DetailPanel::Open(OpenDetail {
            pid,
            last_seen: info,
        });
    }

    /// Close the detail panel. No-op when already closed.
    pub(crate) fn close_detail(&mut self) {
        self.detail = DetailPanel::Closed;
    }

    /// `true` if `pid` is the currently open detail PID.
    pub(crate) fn detail_pid_is(&self, pid: u32) -> bool {
        matches!(&self.detail, DetailPanel::Open(d) if d.pid == pid)
    }

    /// Borrow the resolved detail proc info, or `None` when closed.
    /// The returned reference points at the cached `last_seen`
    /// value, which mirrors the latest observation from the active
    /// procs source (or the value at the moment the process exited
    /// when the source no longer contains the PID).
    pub(crate) fn detail_info(&self) -> Option<&ProcInfo> {
        match &self.detail {
            DetailPanel::Closed => None,
            DetailPanel::Open(d) => Some(&d.last_seen),
        }
    }

    /// Refresh `last_seen` from `source` if the source contains the
    /// open PID. No-op when closed or when the source does not
    /// contain the PID — in that case the cached value is preserved
    /// so the panel continues rendering the last-known values for an
    /// exited process.
    pub(crate) fn refresh_detail_cache(&mut self, source: &[ProcInfo]) {
        let DetailPanel::Open(open) = &mut self.detail else {
            return;
        };
        if let Some(info) = source.iter().find(|p| p.pid == open.pid) {
            open.last_seen = info.clone();
        }
    }

    /// Toggle the detail panel against `info`.
    ///
    /// If the panel is already open for `info.pid`, close it and
    /// clear the follow target (matches the manual Enter-to-close
    /// gesture's coupling: a dead PID is unfollowable, and closing
    /// the inspector also releases the follow lock). Otherwise
    /// open the panel for `info`.
    ///
    /// Returns the new open state (`true` = open after toggle) so
    /// callers can drive logging or further dirty-flag work; the
    /// caller is always responsible for marking the proc widget
    /// dirty.
    pub(crate) fn toggle_detail(&mut self, info: ProcInfo) -> bool {
        if self.detail_pid_is(info.pid) {
            self.close_detail();
            self.followed_pid = 0;
            false
        } else {
            self.open_detail(info);
            true
        }
    }

    /// Close the detail panel and clear the follow target.
    ///
    /// Returns `true` when the panel transitioned from open to
    /// closed — the caller should then mark the proc widget dirty
    /// to repaint without the panel rows. Returns `false` when the
    /// panel was already closed; the caller should consume the
    /// keystroke without dirtying anything.
    pub(crate) fn close_detail_and_unfollow(&mut self) -> bool {
        if matches!(self.detail, DetailPanel::Closed) {
            return false;
        }
        self.close_detail();
        self.followed_pid = 0;
        true
    }

    /// Toggle pause on or off. Returns the new pause state (`true`
    /// = paused after the toggle).
    ///
    /// On activation, captures `live.proc_data` as the frozen
    /// snapshot. If `live.proc_data` is `None` (first-frame race
    /// before any data has arrived), the activation is a silent
    /// no-op — there's nothing to freeze yet — and the function
    /// returns `false`. In practice this is unreachable because the
    /// "Collecting data..." gate consumes `Space` before key
    /// dispatch reaches the action.
    ///
    /// On deactivation, drops the frozen snapshot.
    pub(crate) fn toggle_pause(&mut self, live: &LiveData) -> bool {
        if self.pause.is_some() {
            self.pause = None;
            false
        } else if let Some(snapshot) = live.proc_data.clone() {
            self.pause = Some(PauseState {
                snapshot,
                dead_pids: HashSet::new(),
            });
            true
        } else {
            false
        }
    }

    /// Recompute `pause.dead_pids` from the current live snapshot.
    /// Returns `true` if the dead-PID set changed, signalling that
    /// the proc widget must redraw to update the dead-row styling.
    ///
    /// Called by the pull path on every live proc-data arrival
    /// while paused. No-op if pause is not active.
    pub(crate) fn refresh_dead_pids(&mut self, live: &runner::ProcSnapshot) -> bool {
        let Some(pause) = self.pause.as_mut() else {
            return false;
        };
        let live_pids: HashSet<u32> = live.procs.iter().map(|p| p.pid).collect();
        let new_dead: HashSet<u32> = pause
            .snapshot
            .procs
            .iter()
            .map(|p| p.pid)
            .filter(|pid| !live_pids.contains(pid))
            .collect();
        if new_dead == pause.dead_pids {
            false
        } else {
            pause.dead_pids = new_dead;
            true
        }
    }

    /// `true` if `pid` is in the paused snapshot's dead-PID set. A
    /// shorthand the renderer and termination logic both use.
    pub(crate) fn is_dead(&self, pid: u32) -> bool {
        self.pause
            .as_ref()
            .is_some_and(|p| p.dead_pids.contains(&pid))
    }

    /// Borrow the slice of processes the current display is built
    /// from: the paused snapshot when paused, otherwise the live
    /// snapshot. Returns `None` if no snapshot is available
    /// (first-frame race, before any data has arrived).
    pub(crate) fn procs_source<'a>(
        &'a self,
        live: &'a LiveData,
    ) -> Option<&'a [crate::domain::process::ProcInfo]> {
        if let Some(pause) = self.pause.as_ref() {
            Some(pause.snapshot.procs.as_slice())
        } else {
            live.proc_data.as_ref().map(|s| s.procs.as_slice())
        }
    }

    pub(crate) fn rebuild_entries(
        &mut self,
        config: &config::Config,
        view: &RuntimeView,
        live: &LiveData,
    ) {
        // Resolve the procs source inline rather than via the
        // procs_source helper: the helper borrows all of `&self`,
        // which conflicts with the mutations to `self.entries` and
        // `self.selected` below. Direct field access lets the
        // borrow checker prove the borrows touch disjoint fields.
        let procs: &[crate::domain::process::ProcInfo] = match self.pause.as_ref() {
            Some(p) => p.snapshot.procs.as_slice(),
            None => match live.proc_data.as_ref() {
                Some(s) => s.procs.as_slice(),
                None => {
                    self.entries.clear();
                    return;
                }
            },
        };

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

        // Auto-scroll to followed process. While paused this is
        // computed against the snapshot's PID set, so a followed
        // PID that has died will not auto-disengage until pause is
        // released — the user is studying the snapshot and the
        // cursor should stay anchored.
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
                // Followed process gone from the displayed source —
                // unfollow.
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

        let state = AppState::new(&config, 0);

        assert!(matches!(state.overlay.active, ActiveModal::None));
        assert_eq!(state.render.dirty, RenderDirty::full());
        assert!(state.render.cached_layout().is_none());
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

        let state = AppState::new(&config, 0);

        assert!(state.view.proc_tree);
        assert_eq!(state.view.proc_filter, "chrome");
        assert!(state.view.io_mode);
        assert_eq!(state.view.net_iface, "Ethernet");
    }

    // ────────────────────────────────────────────────────────────
    // compose_hidden — the single source of truth for the engine's
    // per-frame `hidden` widget set.
    // ────────────────────────────────────────────────────────────

    #[test]
    fn compose_hidden_excludes_statusbar_when_show_statusbar_is_true() {
        // Default `show_statusbar = true` → the engine receives no
        // `Statusbar` in `hidden`, so the bar is laid out at its
        // preferred 1-row height.
        let config = config::Config::new();
        let state = AppState::new(&config, 0);
        let hidden = state.compose_hidden(&config);
        assert!(
            !hidden.contains(WidgetKind::Statusbar),
            "default config should not hide the statusbar",
        );
    }

    #[test]
    fn compose_hidden_includes_statusbar_when_show_statusbar_is_false() {
        // Master toggle off → the engine treats the statusbar as
        // hidden via the same code path as any other hidden widget.
        // Parents reclaim the freed row through the normal vstack
        // distribution; the visual side-effect is verified by the
        // engine-level test in `draw/layout.rs`.
        let mut config = config::Config::new();
        config.statusbar.show_statusbar = false;
        let state = AppState::new(&config, 0);
        let hidden = state.compose_hidden(&config);
        assert!(
            hidden.contains(WidgetKind::Statusbar),
            "show_statusbar=false must add Statusbar to compose_hidden",
        );
    }

    #[test]
    fn compose_hidden_unions_master_toggle_with_widget_filter() {
        // Both the master toggle and the runtime widget filter are
        // independent visibility sources; the composition unions
        // them so neither path can shadow the other.
        let mut config = config::Config::new();
        config.statusbar.show_statusbar = false;
        let mut state = AppState::new(&config, 0);
        state.filter.hidden.insert(WidgetKind::Mem);

        let hidden = state.compose_hidden(&config);

        assert!(
            hidden.contains(WidgetKind::Statusbar),
            "master toggle source still applies",
        );
        assert!(
            hidden.contains(WidgetKind::Mem),
            "runtime widget filter source still applies",
        );
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
        use crate::overlay::{
            ReturnTarget, filter::FilterState, help::HelpState, main_menu::MainMenuState,
            options::OptionsState,
        };
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);

        state.overlay.active = ActiveModal::None;
        assert!(state.overlay.render_ui());

        state.overlay.active = ActiveModal::Filter(FilterState);
        assert!(state.overlay.render_ui());

        state.overlay.active = ActiveModal::Main(MainMenuState::new());
        assert!(!state.overlay.render_ui());

        state.overlay.active = ActiveModal::Help(HelpState::new(ReturnTarget::Normal));
        assert!(!state.overlay.render_ui());

        state.overlay.active = ActiveModal::Options(OptionsState::new(ReturnTarget::Normal));
        assert!(!state.overlay.render_ui());
    }

    #[test]
    fn layout_hints_disk_rows_per_unit_covers_all_four_view_modes() {
        let mut config = config::Config::new();
        let mut state = AppState::new(&config, 0);

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
                .layout_hints(&config, &state.view, &state.filter)
                .disk_rows_per_unit,
            2
        );

        // Usage view + show_io_stat off → 1 row per disk.
        config.disk.show_io_stat = false;
        assert_eq!(
            state
                .live
                .layout_hints(&config, &state.view, &state.filter)
                .disk_rows_per_unit,
            1
        );

        // IO view + split graphs → 2 rows per disk regardless of show_io_stat.
        state.view.io_mode = true;
        config.disk.io_graph_combined = false;
        assert_eq!(
            state
                .live
                .layout_hints(&config, &state.view, &state.filter)
                .disk_rows_per_unit,
            2
        );

        // IO view + combined graph → 1 row per disk.
        config.disk.io_graph_combined = true;
        assert_eq!(
            state
                .live
                .layout_hints(&config, &state.view, &state.filter)
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
                .layout_hints(&config, &state.view, &state.filter)
                .disk_rows_per_unit,
            2
        );
    }

    #[test]
    fn mark_resize_sets_layout_and_all_widgets() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        state.render.clear_dirty();

        state.render.mark_resize();

        assert!(state.render.dirty.needs_layout());
        assert!(state.render.dirty.is_any_widget_dirty());
    }

    // ─────────────────────────────────────────────────────────────
    // Pause feature
    // ─────────────────────────────────────────────────────────────

    fn snap(pids: &[u32]) -> Arc<runner::ProcSnapshot> {
        Arc::new(runner::ProcSnapshot {
            procs: pids
                .iter()
                .map(|&pid| crate::domain::process::ProcInfo {
                    pid,
                    name: format!("proc{pid}"),
                    ..Default::default()
                })
                .collect(),
            status: crate::collect::CollectStatus::Ok,
        })
    }

    #[test]
    fn pause_activation_captures_live_snapshot() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        let live_snap = snap(&[1, 2, 3]);
        state.live.proc_data = Some(Arc::clone(&live_snap));

        let now_paused = state.process.toggle_pause(&state.live);

        assert!(now_paused);
        let pause = state.process.pause.as_ref().expect("pause should be set");
        assert!(Arc::ptr_eq(&pause.snapshot, &live_snap));
        assert!(pause.dead_pids.is_empty());
    }

    #[test]
    fn pause_activation_noop_when_no_live_data() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        // live.proc_data is None.

        let now_paused = state.process.toggle_pause(&state.live);

        assert!(!now_paused);
        assert!(state.process.pause.is_none());
    }

    #[test]
    fn pause_deactivation_drops_snapshot() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        state.live.proc_data = Some(snap(&[1, 2]));

        assert!(state.process.toggle_pause(&state.live));
        assert!(state.process.pause.is_some());

        let now_paused = state.process.toggle_pause(&state.live);
        assert!(!now_paused);
        assert!(state.process.pause.is_none());
    }

    #[test]
    fn dead_pids_recomputes_on_live_update() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        state.live.proc_data = Some(snap(&[1, 2, 3]));
        state.process.toggle_pause(&state.live);

        // Live update: PID 2 has died.
        let next_live = snap(&[1, 3]);
        let changed = state.process.refresh_dead_pids(&next_live);
        assert!(changed);
        let pause = state.process.pause.as_ref().unwrap();
        assert_eq!(pause.dead_pids.len(), 1);
        assert!(pause.dead_pids.contains(&2));

        // Resurrection: same PID 2 reappears in next live update.
        let after = snap(&[1, 2, 3]);
        let changed = state.process.refresh_dead_pids(&after);
        assert!(changed);
        let pause = state.process.pause.as_ref().unwrap();
        assert!(pause.dead_pids.is_empty());
    }

    #[test]
    fn dead_pids_only_includes_paused_snapshot_pids() {
        // A PID that never appeared in the paused snapshot must not
        // count as "dead" — it's a brand new live process, not a
        // departed one.
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        state.live.proc_data = Some(snap(&[1, 2]));
        state.process.toggle_pause(&state.live);

        // Live update: PID 99 appears (new), PID 2 still alive, PID 1 still alive.
        let next_live = snap(&[1, 2, 99]);
        state.process.refresh_dead_pids(&next_live);
        let pause = state.process.pause.as_ref().unwrap();
        assert!(pause.dead_pids.is_empty());
    }

    #[test]
    fn refresh_dead_pids_returns_false_when_set_unchanged() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        state.live.proc_data = Some(snap(&[1, 2, 3]));
        state.process.toggle_pause(&state.live);

        // First update: PID 2 dies.
        let live1 = snap(&[1, 3]);
        assert!(state.process.refresh_dead_pids(&live1));

        // Second update: same PID 2 still missing, no change.
        let live2 = snap(&[1, 3]);
        assert!(!state.process.refresh_dead_pids(&live2));
    }

    #[test]
    fn is_dead_returns_false_when_not_paused() {
        let config = config::Config::new();
        let state = AppState::new(&config, 0);
        assert!(!state.process.is_dead(123));
    }

    #[test]
    fn procs_source_returns_paused_when_paused_else_live() {
        let config = config::Config::new();
        let mut state = AppState::new(&config, 0);
        let live_snap = snap(&[10, 20]);
        state.live.proc_data = Some(Arc::clone(&live_snap));

        // Not paused: source = live.
        let src = state.process.procs_source(&state.live).unwrap();
        assert_eq!(src.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![10, 20]);

        // Pause, then mutate live: source remains the snapshot.
        state.process.toggle_pause(&state.live);
        state.live.proc_data = Some(snap(&[10, 20, 30]));
        let src = state.process.procs_source(&state.live).unwrap();
        assert_eq!(src.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![10, 20]);
    }

    // ─────────────────────────────────────────────────────────────
    // Detail panel state — DetailPanel enum and helpers
    // ─────────────────────────────────────────────────────────────

    fn proc_with(pid: u32, name: &str) -> crate::domain::process::ProcInfo {
        crate::domain::process::ProcInfo {
            pid,
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn detail_panel_starts_closed() {
        let mut state = ProcessViewState::new();
        assert!(matches!(state.detail, DetailPanel::Closed));
        assert!(state.detail_info().is_none());
        assert!(!state.detail_pid_is(0));
        assert!(!state.detail_pid_is(42));
        // Closing a closed panel is a no-op and stays Closed.
        state.close_detail();
        assert!(matches!(state.detail, DetailPanel::Closed));
    }

    #[test]
    fn open_detail_seeds_last_seen_and_records_pid() {
        let mut state = ProcessViewState::new();
        state.open_detail(proc_with(100, "alpha"));

        let info = state
            .detail_info()
            .expect("panel should be open after open_detail");
        assert_eq!(info.pid, 100);
        assert_eq!(info.name, "alpha");
        assert!(state.detail_pid_is(100));
        assert!(!state.detail_pid_is(200));
    }

    #[test]
    fn refresh_detail_cache_updates_when_source_contains_pid() {
        let mut state = ProcessViewState::new();
        state.open_detail(proc_with(100, "alpha"));

        // New observation: same PID, different name (e.g. process
        // self-renamed, or initial info had a stale value).
        let source = vec![proc_with(100, "alpha-renamed"), proc_with(200, "beta")];
        state.refresh_detail_cache(&source);

        let info = state.detail_info().expect("panel still open");
        assert_eq!(info.pid, 100);
        assert_eq!(info.name, "alpha-renamed");
    }

    #[test]
    fn refresh_detail_cache_preserves_last_seen_when_source_omits_pid() {
        // The "process just exited" case: the live snapshot no
        // longer contains the open PID. The cache must keep the
        // previous values so the panel can render the last-known
        // state with the dead annotation.
        let mut state = ProcessViewState::new();
        state.open_detail(proc_with(100, "alpha"));

        let source_without = vec![proc_with(200, "beta")];
        state.refresh_detail_cache(&source_without);

        let info = state.detail_info().expect("panel still open");
        assert_eq!(info.pid, 100);
        assert_eq!(
            info.name, "alpha",
            "last_seen must survive when source omits the PID"
        );
    }

    #[test]
    fn refresh_detail_cache_is_noop_when_closed() {
        let mut state = ProcessViewState::new();
        let source = vec![proc_with(100, "alpha")];
        state.refresh_detail_cache(&source);
        assert!(matches!(state.detail, DetailPanel::Closed));
    }

    #[test]
    fn close_detail_clears_panel() {
        let mut state = ProcessViewState::new();
        state.open_detail(proc_with(100, "alpha"));
        assert!(matches!(state.detail, DetailPanel::Open(_)));

        state.close_detail();
        assert!(matches!(state.detail, DetailPanel::Closed));
        assert!(state.detail_info().is_none());
        assert!(!state.detail_pid_is(100));
    }

    #[test]
    fn open_detail_replaces_previous_open() {
        // Opening for a different PID swaps the cached info wholesale.
        let mut state = ProcessViewState::new();
        state.open_detail(proc_with(100, "alpha"));
        state.open_detail(proc_with(200, "beta"));

        assert!(state.detail_pid_is(200));
        let info = state.detail_info().unwrap();
        assert_eq!(info.pid, 200);
        assert_eq!(info.name, "beta");
    }

    // ─────────────────────────────────────────────────────────────
    // Detail panel — toggle and close-and-unfollow (handler logic
    // exposed as state methods so the action handlers can stay
    // thin and these can be tested without InputContext mocking).
    // ─────────────────────────────────────────────────────────────

    #[test]
    fn toggle_detail_opens_when_closed() {
        let mut state = ProcessViewState::new();
        state.followed_pid = 42;
        let opened = state.toggle_detail(proc_with(100, "alpha"));
        assert!(opened);
        assert!(state.detail_pid_is(100));
        assert_eq!(
            state.followed_pid, 42,
            "opening the panel must NOT clear an existing follow target"
        );
    }

    #[test]
    fn toggle_detail_closes_when_open_for_same_pid_and_clears_follow() {
        let mut state = ProcessViewState::new();
        state.open_detail(proc_with(100, "alpha"));
        state.followed_pid = 100;
        let opened = state.toggle_detail(proc_with(100, "alpha"));
        assert!(!opened);
        assert!(matches!(state.detail, DetailPanel::Closed));
        assert_eq!(state.followed_pid, 0, "closing must clear follow target");
    }

    #[test]
    fn toggle_detail_replaces_when_open_for_different_pid() {
        // Opening detail for a different PID swaps the panel
        // without touching follow state.
        let mut state = ProcessViewState::new();
        state.open_detail(proc_with(100, "alpha"));
        state.followed_pid = 100;
        let opened = state.toggle_detail(proc_with(200, "beta"));
        assert!(opened);
        assert!(state.detail_pid_is(200));
        assert_eq!(
            state.followed_pid, 100,
            "swapping panel target must NOT clear follow"
        );
    }

    #[test]
    fn close_detail_and_unfollow_returns_false_when_already_closed() {
        let mut state = ProcessViewState::new();
        state.followed_pid = 7;
        let changed = state.close_detail_and_unfollow();
        assert!(!changed);
        assert_eq!(
            state.followed_pid, 7,
            "no-op path must NOT touch follow state"
        );
    }

    #[test]
    fn close_detail_and_unfollow_closes_open_panel_and_clears_follow() {
        let mut state = ProcessViewState::new();
        state.open_detail(proc_with(100, "alpha"));
        state.followed_pid = 100;
        let changed = state.close_detail_and_unfollow();
        assert!(changed);
        assert!(matches!(state.detail, DetailPanel::Closed));
        assert_eq!(state.followed_pid, 0);
    }
}
