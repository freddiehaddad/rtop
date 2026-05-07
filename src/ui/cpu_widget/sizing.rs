//! Sizing helpers for the CPU widget.
//!
//! Holds the per-widget intrinsic-size policy (preferred height,
//! min width, core-grid layout) and the constants that define the
//! widget's structural overhead. Separate from `mod.rs` so the
//! sizing logic can be reasoned about (and tested) without the
//! rendering machinery.

use std::collections::VecDeque;

/// Label width for stats meter rows (matches GPU widget).
pub(super) const STATS_LABEL_W: usize = 6;
/// Right-aligned value column width for stats meter rows (matches GPU widget).
pub(super) const STATS_VAL_W: usize = 10;
/// Preferred number of core rows per column.
pub(super) const CORES_PER_COL: usize = 8;
/// Width of the percentage field per core (" 42%" with leading space).
pub(super) const CORE_PCT_W: usize = 5;
/// Width of the temperature field per core (" 100°C" with leading space), or 0 when hidden.
pub(super) const CORE_TEMP_W: usize = 6;
/// Minimum width for the mini-graph per core. Below this, the graph
/// is dropped entirely (Compact / Minimal tiers).
pub(super) const CORE_GRAPH_MIN: usize = 3;
/// Inter-column gap (1 space between columns).
pub(super) const CORE_COL_GAP: usize = 1;
/// Structural overhead of the CPU widget around the core panel:
/// 2 borders + 1 vertical divider + 1 cell of left padding + 1 cell
/// of right padding inside the core panel.
pub(super) const CPU_STRUCTURAL_OVERHEAD: usize = 5;

/// Count how many stats rows will be rendered for a given data state.
pub(super) fn stats_row_count(has_temp: bool, has_watts: bool) -> usize {
    let mut n = 2; // CPU + Load (always)
    if has_temp {
        n += 1;
    }
    if has_watts {
        n += 1;
    }
    n
}

/// Preferred intrinsic height for the CPU widget given the snapshot
/// hints, in rows (including borders). The layout engine clamps this
/// to `[MIN_CPU_HEIGHT, term_height/3]` when placing the widget.
///
/// Formula: `core_rows + stats_rows + 2 (load detail row + section
/// divider) + 2 (top + bottom borders)`. The widget owns this
/// formula so the layout engine no longer needs to know about
/// `core_grid_shape` or `stats_row_count`.
pub fn preferred_height(hints: &crate::draw::layout::LayoutHints) -> usize {
    let (core_rows, _) = core_grid_shape(hints.core_count);
    let stats_rows = stats_row_count(hints.has_cpu_temp, hints.has_cpu_watts);
    let panel_overhead = stats_rows + 2; // load detail row + section divider
    core_rows + panel_overhead + 2 // + top/bottom borders
}

/// Smallest total widget width at which the CPU widget can render
/// the core panel at its **Minimal** tier (label + percent only, no
/// per-core graph, no per-core temperature).
///
/// Below this threshold the widget cannot render the core panel,
/// so the global "too small" gate in `min_terminal_size` should
/// trigger and the user sees the standard "Terminal too small"
/// message.
///
/// Formula: `cols * minimal_col_w + (cols - 1) * gap +
/// CPU_STRUCTURAL_OVERHEAD`. The main graph is allowed to shrink
/// to 0 at this threshold; widening the terminal further restores
/// it (and unlocks the Compact / Comfortable tiers).
pub fn min_width(hints: &crate::draw::layout::LayoutHints) -> usize {
    let core_count = hints.core_count;
    if core_count == 0 {
        return CPU_STRUCTURAL_OVERHEAD;
    }
    let (_, cols) = core_grid_shape(core_count);
    let label_w = CoreGridLayout::label_width(core_count);
    let minimal_col_w = label_w + CORE_PCT_W;
    let core_panel_inner = cols * minimal_col_w + cols.saturating_sub(1) * CORE_COL_GAP;
    core_panel_inner + CPU_STRUCTURAL_OVERHEAD
}

/// Compute the y-axis maximum for a CPU graph row.
///
/// When `auto_scale` is `false` (default) returns 100 — the natural
/// upper bound for CPU%. When `true`, returns the largest value in
/// the most recent `width` data points (matches the net widget's
/// `net_auto` algorithm). The `.max(1)` floor avoids a degenerate
/// max=0 reading on an all-zero window; `Graph::new` would replace
/// max=0 with 100 internally, but flooring at 1 keeps the
/// per-frame semantic stable when data first becomes non-zero.
pub(super) fn graph_max(data: &VecDeque<i64>, width: usize, auto_scale: bool) -> i64 {
    if !auto_scale {
        return 100;
    }
    data.iter()
        .rev()
        .take(width.max(1))
        .copied()
        .max()
        .unwrap_or(0)
        .max(1)
}

/// Grid layout for the per-core display area.
///
/// Computes rows, columns, and per-column widths from core count and
/// available panel width. Uses `core_grid_shape()` as the single source
/// of truth for grid dimensions.
pub struct CoreGridLayout {
    /// Number of rows per column.
    pub rows: usize,
    /// Number of columns.
    pub cols: usize,
    /// Character width per column (label + graph + pct + temp + gap).
    /// The gap on the last column serves as right-side padding.
    pub col_w: usize,
    /// Character width of the core label (`"0 "` = 2, `"31 "` = 3,
    /// `"127 "` = 4) — digits in the largest index plus one trailing
    /// space.
    pub label_w: usize,
    /// Character width of the mini-graph (flex element).
    pub graph_w: usize,
    /// Whether per-core temperature is shown.
    pub show_temp: bool,
}

/// Core grid shape: (rows_per_column, columns) for a given core count.
///
/// Shared by the height sizing in [`preferred_height`] and the
/// core panel renderer (for drawing). This is the single source of
/// truth — the layout engine queries `preferred_height` rather than
/// reaching into this function directly.
pub(super) fn core_grid_shape(core_count: usize) -> (usize, usize) {
    match core_count {
        0 => (0, 0),
        1..=8 => (core_count, 1),
        9..=12 => (2, core_count.div_ceil(2)),
        _ => (CORES_PER_COL, core_count.div_ceil(CORES_PER_COL)),
    }
}

impl CoreGridLayout {
    /// Compute the grid layout from core count and available width.
    ///
    /// `panel_inner_w` is the usable content width (already excludes
    /// the divider and 1-space padding on each side).
    ///
    /// The grid picks the highest tier that fits the per-column
    /// budget:
    /// * **Comfortable**: `label + graph + pct + temp` per column
    ///   (where `temp` is included only when `show_coretemp` is on
    ///   and the user enabled per-core temps).
    /// * **Compact**: `label + pct + temp` per column — drops the
    ///   per-core mini-graph.
    /// * **Minimal**: `label + pct` per column — also drops the
    ///   per-core temperature.
    /// * **None** (`cols = 0`): the budget is too small for even the
    ///   Minimal tier; the renderer skips the core panel entirely.
    ///
    /// `col_w` is set to *exactly* the chosen tier's width so the
    /// renderer's column-offset arithmetic matches what gets drawn.
    pub fn new(core_count: usize, panel_inner_w: usize, show_coretemp: bool) -> Self {
        let (rows, cols) = core_grid_shape(core_count);
        let label_w = Self::label_width(core_count);
        let temp_w: usize = if show_coretemp { CORE_TEMP_W } else { 0 };

        // Per-column budget after subtracting inter-column gaps.
        let total_gaps = cols.saturating_sub(1) * CORE_COL_GAP;
        let avail = panel_inner_w.saturating_sub(total_gaps);
        let col_budget = avail / cols.max(1);

        // Pick the highest tier whose column width fits the budget.
        let comfortable_min = label_w + CORE_GRAPH_MIN + CORE_PCT_W + temp_w;
        let compact_min = label_w + CORE_PCT_W + temp_w;
        let minimal_min = label_w + CORE_PCT_W;

        let (graph_w, show_temp_used) = if col_budget >= comfortable_min {
            // Comfortable: graph_w grows to fill any extra budget.
            let graph_w = col_budget - (label_w + CORE_PCT_W + temp_w);
            (graph_w, show_coretemp)
        } else if show_coretemp && col_budget >= compact_min {
            (0, true)
        } else if col_budget >= minimal_min {
            (0, false)
        } else {
            // Even Minimal doesn't fit. Signal "no core panel" so
            // the renderer skips it. The "too small" gate in
            // `min_terminal_size` should catch this case earlier
            // for the active hardware, but we also gracefully
            // degrade if the layout undershoots.
            return Self {
                rows: 0,
                cols: 0,
                col_w: 0,
                label_w,
                graph_w: 0,
                show_temp: false,
            };
        };

        let temp_w_used = if show_temp_used { CORE_TEMP_W } else { 0 };
        let col_w = label_w + graph_w + CORE_PCT_W + temp_w_used;

        Self {
            rows,
            cols,
            col_w,
            label_w,
            graph_w,
            show_temp: show_temp_used,
        }
    }

    /// Label width based on core count.
    ///
    /// Width is `digits_in_max_index + 1 (trailing space)`. The
    /// trailing space provides visual separation from the mini-graph.
    fn label_width(core_count: usize) -> usize {
        let max_idx = core_count.saturating_sub(1);
        let digits = if max_idx >= 100 {
            3
        } else if max_idx >= 10 {
            2
        } else {
            1
        };
        digits + 1 // + trailing space
    }

    /// Format the label for a given core index.
    ///
    /// Indices are zero-padded to the width of the largest index
    /// so columns line up. The `C` prefix shown by btop is omitted —
    /// the section divider above the grid already labels the panel.
    pub fn format_label(&self, index: usize) -> String {
        // label_w = digits + 1 (trailing space), so digits = label_w - 1.
        let digits = self.label_w.saturating_sub(1);
        format!("{:0width$} ", index, width = digits)
    }

    /// X position for a core in the given column, relative to content_x.
    pub fn col_offset(&self, col: usize) -> usize {
        col * (self.col_w + CORE_COL_GAP)
    }

    /// Exact width the grid occupies (including last column's trailing gap).
    pub fn grid_width(&self) -> usize {
        if self.cols == 0 {
            return 0;
        }
        self.cols * self.col_w + self.cols.saturating_sub(1) * CORE_COL_GAP
    }
}
