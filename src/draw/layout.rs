use crate::config::MAX_GPUS;
use crate::domain::widget_kind::{PerWidget, WidgetKind};

/// Dimensions and position of a UI widget.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WidgetDimensions {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// Snapshot-derived sizing inputs that widgets and the layout
/// engine consult when computing per-widget heights.
///
/// Built once per frame from `LiveData` + the current `Config`,
/// reused for both layout-change detection (in
/// `app::pull_subsystem_data`) and the actual `calc_sizes` call.
/// Each field is the *user-visible* derived value: `has_swap`
/// already accounts for `config.show_swap`, `disk_count` is the
/// post-`disks_filter` count, etc.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayoutHints {
    pub core_count: usize,
    pub gpu_count: usize,
    pub disk_count: usize,
    pub has_swap: bool,
    pub has_cpu_temp: bool,
    pub has_cpu_watts: bool,
    /// Rows the disk widget reserves for each disk in the active
    /// view. The disk widget renders either 1 row per disk
    /// (capacity-only usage view, or combined IO graph) or 2 rows
    /// (usage view with the inline IO stat row, or split-graph IO
    /// view with separate read/write rows). Computed at the
    /// `LiveData::layout_hints` boundary so the disk widget's
    /// `preferred_height` doesn't have to peek at config flags.
    pub disk_rows_per_unit: u8,
}

/// Complete layout of all UI widgets.
///
/// Widget dimensions are stored keyed by [`WidgetKind`]. GPU widget
/// slots are addressed by their actual index `n` (from
/// [`WidgetKind::Gpu(n)`]) — preserving identity end-to-end so a
/// sparse GPU layout (e.g. only `gpu1` enabled) renders the
/// correct device's data with the correct title and toggle key.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    dims: PerWidget<Option<WidgetDimensions>>,
}

impl Layout {
    /// Borrow the dimensions assigned to `kind`, if the widget is
    /// laid out this frame.
    pub fn dims_for(&self, kind: WidgetKind) -> Option<&WidgetDimensions> {
        self.dims.get(kind).as_ref()
    }

    /// Assign dimensions to `kind` for this frame.
    fn set(&mut self, kind: WidgetKind, dim: WidgetDimensions) {
        *self.dims.get_mut(kind) = Some(dim);
    }
}

/// Minimum widget dimensions (matching btop).
pub const MIN_CPU_HEIGHT: usize = 8;
/// Minimum width for the memory widget.
pub const MIN_MEM_WIDTH: usize = 36;
/// Minimum height for the network widget.
pub const MIN_NET_HEIGHT: usize = 6;
/// Minimum width for the network widget.
pub const MIN_NET_WIDTH: usize = 20;
/// Minimum width for the process widget.
pub const MIN_PROC_WIDTH: usize = 44;
/// Minimum height for a GPU widget (5 content rows + 2 borders).
pub const MIN_GPU_HEIGHT: usize = 7;
/// Minimum height for the disk widget.
pub const MIN_DISK_HEIGHT: usize = 4;
/// Minimum height for the proc widget.
///
/// Used as the floor for the proc column when computing the
/// minimum terminal size. Real placement gives proc whatever space
/// remains after CPU and the left column; this is just the smallest
/// value at which the header + a few rows are still legible.
pub const MIN_PROC_HEIGHT: usize = 8;
/// Percentage of terminal width allocated to the proc widget (right column).
const PROC_WIDTH_PCT: usize = 60;

/// Resolved per-frame description of which widgets land in which
/// region of the screen.
///
/// `LayoutPlan::from(&LayoutConfig)` walks the widget list and
/// orientation flags exactly once. Both [`calc_sizes`] (which
/// computes pixel-perfect dimensions) and [`min_terminal_size`]
/// (which computes the smallest terminal that can fit those
/// dimensions) consume the plan, so the "what goes where" knowledge
/// lives in one place.
#[derive(Debug, Clone)]
pub struct LayoutPlan {
    pub term_size: (usize, usize),
    pub hints: LayoutHints,
    pub has_cpu: bool,
    pub has_proc: bool,
    pub left_column: LeftColumn,
    pub cpu_bottom: bool,
    pub mem_below_net: bool,
    pub proc_left: bool,
}

/// Widgets that live in the left column, in stack order (gpus on
/// top, then mem/net per `mem_below_net`, then disk on the bottom).
#[derive(Debug, Clone, Default)]
pub struct LeftColumn {
    /// Detected GPU indices to render this frame, preserving
    /// `WidgetKind::Gpu(n)` identity. Sparse (e.g. only `gpu1`
    /// enabled) is supported end-to-end so the renderer pulls the
    /// right device data for each slot.
    pub gpu_indices: Vec<u8>,
    pub has_mem: bool,
    pub has_net: bool,
    pub has_disk: bool,
}

impl LeftColumn {
    pub fn is_empty(&self) -> bool {
        self.gpu_indices.is_empty() && !self.has_mem && !self.has_net && !self.has_disk
    }
}

impl From<&LayoutConfig<'_>> for LayoutPlan {
    fn from(cfg: &LayoutConfig<'_>) -> Self {
        let widgets = cfg.widgets;
        let hints = cfg.hints;
        let gpu_indices: Vec<u8> = (0..MAX_GPUS as u8)
            .filter(|n| (*n as usize) < hints.gpu_count && widgets.contains(&WidgetKind::Gpu(*n)))
            .collect();
        Self {
            term_size: (cfg.term_width, cfg.term_height),
            hints,
            has_cpu: widgets.contains(&WidgetKind::Cpu),
            has_proc: widgets.contains(&WidgetKind::Proc),
            left_column: LeftColumn {
                gpu_indices,
                has_mem: widgets.contains(&WidgetKind::Mem),
                has_net: widgets.contains(&WidgetKind::Net),
                has_disk: widgets.contains(&WidgetKind::Disk),
            },
            cpu_bottom: cfg.cpu_bottom,
            mem_below_net: cfg.mem_below_net,
            proc_left: cfg.proc_left,
        }
    }
}

/// Smallest terminal size at which the active layout fits without
/// truncation, given the current widget set and snapshot hints.
///
/// Width is derived from the proc/left column split (`proc_width =
/// term_width * PROC_WIDTH_PCT / 100` with a `MIN_PROC_WIDTH`
/// floor; the left column gets the remainder with a `MIN_MEM_WIDTH`
/// floor). Height is derived from the per-widget `preferred_height`
/// helpers: `cpu_pref + max(left_column_pref, proc_pref)`. The CPU
/// widget is also constrained by `term_height/3`, so we additionally
/// require `term_height >= 3 * cpu_pref` to let the CPU widget reach
/// its preferred height.
///
/// The returned size is exactly what the user would see in the
/// "Terminal too small" message, and the value used by the
/// `is_too_small` gate in the event loop.
pub fn min_terminal_size(cfg: &LayoutConfig) -> (usize, usize) {
    let plan = LayoutPlan::from(cfg);
    (min_width_for(&plan), min_height_for(&plan))
}

/// Smallest terminal width that fits the columns described by `plan`.
fn min_width_for(plan: &LayoutPlan) -> usize {
    let left = &plan.left_column;
    // Widest widget that lives in the left column (mem floor
    // dominates for mem/disk/gpu; net floor only matters when net
    // is the sole left-column widget).
    let left_min_width = if left.has_mem || left.has_disk || !left.gpu_indices.is_empty() {
        MIN_MEM_WIDTH
    } else if left.has_net {
        MIN_NET_WIDTH
    } else {
        0
    };

    let layout_min_width = if plan.has_proc && !left.is_empty() {
        // Both columns must fit at their own minimums simultaneously
        // under the PROC_WIDTH_PCT split. Solve for the smallest
        // term_width that satisfies both:
        //   term_width * PROC_WIDTH_PCT / 100 >= MIN_PROC_WIDTH
        //   term_width - term_width * PROC_WIDTH_PCT / 100 >= left_min_width
        let from_proc = (MIN_PROC_WIDTH * 100).div_ceil(PROC_WIDTH_PCT);
        let from_left = (left_min_width * 100).div_ceil(100 - PROC_WIDTH_PCT);
        from_proc.max(from_left)
    } else if plan.has_proc {
        MIN_PROC_WIDTH
    } else {
        left_min_width
    };

    // The CPU widget always spans the full terminal width when
    // present, so it must also fit at its own minimum (the
    // `Minimal` core-panel tier — label + percent only — for the
    // detected core count). On many-core machines this dominates
    // the column-split minimum.
    let cpu_min_width = if plan.has_cpu {
        crate::ui::cpu_widget::min_width(&plan.hints)
    } else {
        0
    };
    layout_min_width.max(cpu_min_width)
}

/// Smallest terminal height that fits the rows described by `plan`.
fn min_height_for(plan: &LayoutPlan) -> usize {
    let cpu_pref = if plan.has_cpu {
        crate::ui::cpu_widget::preferred_height(&plan.hints).max(MIN_CPU_HEIGHT)
    } else {
        0
    };
    let left_pref = left_column_preferred_height(plan);
    let proc_pref = if plan.has_proc { MIN_PROC_HEIGHT } else { 0 };

    let layout_height = cpu_pref + left_pref.max(proc_pref);
    // CPU is clamped to term_height / 3 in `calc_sizes`, so to let
    // the CPU widget reach its preferred height the terminal must
    // also be at least three times that preferred height.
    let cpu_clamp_height = if plan.has_cpu { 3 * cpu_pref } else { 0 };
    layout_height.max(cpu_clamp_height)
}

/// Sum of preferred heights of every widget in the left column.
fn left_column_preferred_height(plan: &LayoutPlan) -> usize {
    let left = &plan.left_column;
    let gpu_total = left.gpu_indices.len() * crate::ui::gpu_widget::preferred_height();
    let mem = if left.has_mem {
        crate::ui::mem_widget::preferred_height(&plan.hints)
    } else {
        0
    };
    let net = if left.has_net { MIN_NET_HEIGHT } else { 0 };
    let disk = if left.has_disk {
        crate::ui::disk_widget::preferred_height(&plan.hints)
    } else {
        0
    };
    gpu_total + mem + net + disk
}

/// Configuration for layout calculation.
pub struct LayoutConfig<'a> {
    pub term_width: usize,
    pub term_height: usize,
    pub widgets: &'a [WidgetKind],
    pub cpu_bottom: bool,
    pub mem_below_net: bool,
    pub proc_left: bool,
    /// Snapshot-derived sizing inputs (core_count, disk_count,
    /// has_swap, …) that widgets consume via their per-widget
    /// `preferred_height` helpers.
    pub hints: LayoutHints,
}

/// Calculate widget sizes and positions based on terminal dimensions and config.
///
/// Walks the active widget set into a [`LayoutPlan`] (single source
/// of truth for "what goes where"), then dispatches to placement
/// helpers that own one region each. The same `LayoutPlan` shape
/// drives [`min_terminal_size`].
pub fn calc_sizes(cfg: &LayoutConfig) -> Layout {
    let plan = LayoutPlan::from(cfg);
    let mut layout = Layout::default();
    let (term_w, term_h) = plan.term_size;
    if term_w < 2 || term_h < 2 {
        return layout;
    }

    let v = split_vertical(&plan, term_h);
    let c = split_columns(&plan, term_w);

    place_cpu(&plan, term_w, &v, &mut layout);
    place_left_column(&plan, &c, &v, &mut layout);
    place_proc(&plan, &c, &v, &mut layout);

    layout
}

/// Vertical split: where CPU sits and how much height the bottom
/// region (left column + proc) gets.
struct VerticalSplit {
    cpu_y: usize,
    cpu_height: usize,
    /// First row of the bottom region (left column + proc).
    bottom_y: usize,
    /// Height of the bottom region.
    bottom_height: usize,
}

fn split_vertical(plan: &LayoutPlan, term_height: usize) -> VerticalSplit {
    let cpu_height = if plan.has_cpu {
        let max_h = (term_height / 3).max(MIN_CPU_HEIGHT);
        crate::ui::cpu_widget::preferred_height(&plan.hints).clamp(MIN_CPU_HEIGHT, max_h)
    } else {
        0
    };
    let cpu_y = if plan.cpu_bottom {
        term_height.saturating_sub(cpu_height)
    } else {
        0
    };
    let bottom_y = if plan.cpu_bottom { 0 } else { cpu_height };
    let bottom_height = term_height.saturating_sub(cpu_height);
    VerticalSplit {
        cpu_y,
        cpu_height,
        bottom_y,
        bottom_height,
    }
}

/// Horizontal split: x position and width of the proc column and
/// the left column. Either may be zero-width if its contents are
/// absent.
struct ColumnSplit {
    proc_x: usize,
    proc_width: usize,
    left_x: usize,
    left_width: usize,
}

fn split_columns(plan: &LayoutPlan, term_width: usize) -> ColumnSplit {
    let has_left = !plan.left_column.is_empty();
    let proc_width = if plan.has_proc {
        if has_left {
            (term_width * PROC_WIDTH_PCT / 100)
                .max(MIN_PROC_WIDTH)
                .min(term_width)
        } else {
            term_width
        }
    } else {
        0
    };
    let left_width = if plan.has_proc && has_left {
        term_width - proc_width
    } else if has_left {
        term_width
    } else {
        0
    };
    let (left_x, proc_x) = if plan.proc_left {
        (proc_width, 0)
    } else {
        (0, left_width)
    };
    ColumnSplit {
        proc_x,
        proc_width,
        left_x,
        left_width,
    }
}

fn place_cpu(plan: &LayoutPlan, term_width: usize, v: &VerticalSplit, layout: &mut Layout) {
    if !plan.has_cpu {
        return;
    }
    layout.set(
        WidgetKind::Cpu,
        WidgetDimensions {
            x: 0,
            y: v.cpu_y,
            width: term_width,
            height: v.cpu_height,
        },
    );
}

fn place_left_column(plan: &LayoutPlan, c: &ColumnSplit, v: &VerticalSplit, layout: &mut Layout) {
    let left = &plan.left_column;
    if left.is_empty() {
        return;
    }

    let mem_width = c.left_width.max(MIN_MEM_WIDTH);
    let net_width = c.left_width.max(MIN_NET_WIDTH);

    // GPU widgets stack at the top of the left column. Each enabled
    // GPU widget is keyed by its actual index `n` (from
    // `WidgetKind::Gpu(n)`); the `placement_i` only drives the
    // vertical position so widgets stack top-to-bottom in
    // declaration order without leaving gaps when a low-index GPU
    // is disabled.
    let gpu_unit_height = crate::ui::gpu_widget::preferred_height();
    let total_gpu_height = left.gpu_indices.len() * gpu_unit_height;
    for (placement_i, n) in left.gpu_indices.iter().enumerate() {
        layout.set(
            WidgetKind::Gpu(*n),
            WidgetDimensions {
                x: c.left_x,
                y: v.bottom_y + placement_i * gpu_unit_height,
                width: mem_width,
                height: gpu_unit_height,
            },
        );
    }

    // Disk gets its preferred height; mem and net split the rest.
    let after_gpu_height = v.bottom_height.saturating_sub(total_gpu_height);
    let disk_height = if left.has_disk {
        crate::ui::disk_widget::preferred_height(&plan.hints)
    } else {
        0
    };
    let mem_net_budget = after_gpu_height.saturating_sub(disk_height);

    let mem_pref = if left.has_mem {
        crate::ui::mem_widget::preferred_height(&plan.hints)
    } else {
        0
    };
    let (mem_height, net_height) = if left.has_mem && left.has_net {
        let mh = mem_pref.min(mem_net_budget);
        let nh = mem_net_budget.saturating_sub(mh).max(MIN_NET_HEIGHT);
        (mh, nh)
    } else if left.has_mem {
        (mem_net_budget, 0)
    } else if left.has_net {
        (0, mem_net_budget)
    } else {
        (0, 0)
    };

    let mem_net_top_y = v.bottom_y + total_gpu_height;
    let (mem_y, net_y) = if plan.mem_below_net {
        (mem_net_top_y + net_height, mem_net_top_y)
    } else {
        (mem_net_top_y, mem_net_top_y + mem_height)
    };

    if left.has_mem {
        layout.set(
            WidgetKind::Mem,
            WidgetDimensions {
                x: c.left_x,
                y: mem_y,
                width: mem_width,
                height: mem_height,
            },
        );
    }
    if left.has_net {
        layout.set(
            WidgetKind::Net,
            WidgetDimensions {
                x: c.left_x,
                y: net_y,
                width: net_width,
                height: net_height,
            },
        );
    }
    if left.has_disk {
        let disk_y = mem_net_top_y + mem_height + net_height;
        layout.set(
            WidgetKind::Disk,
            WidgetDimensions {
                x: c.left_x,
                y: disk_y,
                width: mem_width,
                height: disk_height,
            },
        );
    }
}

fn place_proc(plan: &LayoutPlan, c: &ColumnSplit, v: &VerticalSplit, layout: &mut Layout) {
    if !plan.has_proc {
        return;
    }
    layout.set(
        WidgetKind::Proc,
        WidgetDimensions {
            x: c.proc_x,
            y: v.bottom_y,
            width: c.proc_width,
            height: v.bottom_height,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::widget_kind::WidgetKind;

    fn widgets(kinds: &[WidgetKind]) -> Vec<WidgetKind> {
        kinds.to_vec()
    }

    fn lc(tw: usize, th: usize, shown: &[WidgetKind]) -> LayoutConfig<'_> {
        LayoutConfig {
            term_width: tw,
            term_height: th,
            widgets: shown,
            cpu_bottom: false,
            mem_below_net: false,
            proc_left: false,
            hints: LayoutHints {
                core_count: 4,
                gpu_count: 0,
                disk_count: 2,
                has_swap: false,
                has_cpu_temp: false,
                has_cpu_watts: false,
                disk_rows_per_unit: 2,
            },
        }
    }

    /// Build a `LayoutConfig` from `lc(...)` and apply a hints
    /// override callback. Avoids the verbose
    /// `LayoutConfig { hints: LayoutHints { ..lc(...).hints }, ... }`
    /// at every test site.
    fn lc_with_hints(
        tw: usize,
        th: usize,
        shown: &[WidgetKind],
        f: impl FnOnce(&mut LayoutHints),
    ) -> LayoutConfig<'_> {
        let mut cfg = lc(tw, th, shown);
        f(&mut cfg.hints);
        cfg
    }

    #[test]
    fn calc_sizes_all_widgets_shown() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
        ]);
        let layout = calc_sizes(&lc_with_hints(120, 40, &b, |h| h.core_count = 8));
        assert!(layout.dims_for(WidgetKind::Cpu).is_some());
        assert!(layout.dims_for(WidgetKind::Mem).is_some());
        assert!(layout.dims_for(WidgetKind::Net).is_some());
        assert!(layout.dims_for(WidgetKind::Proc).is_some());
    }

    #[test]
    fn calc_sizes_cpu_only() {
        let b = widgets(&[WidgetKind::Cpu]);
        let layout = calc_sizes(&lc(80, 24, &b));
        assert!(layout.dims_for(WidgetKind::Cpu).is_some());
        assert!(layout.dims_for(WidgetKind::Mem).is_none());
        assert!(layout.dims_for(WidgetKind::Net).is_none());
        assert!(layout.dims_for(WidgetKind::Proc).is_none());
    }

    #[test]
    fn calc_sizes_proc_only() {
        let b = widgets(&[WidgetKind::Proc]);
        let layout = calc_sizes(&lc(80, 24, &b));
        assert!(layout.dims_for(WidgetKind::Proc).is_some());
        assert!(layout.dims_for(WidgetKind::Cpu).is_none());
    }

    #[test]
    fn calc_sizes_cpu_bottom() {
        let b = widgets(&[WidgetKind::Cpu, WidgetKind::Mem]);
        let layout_top = calc_sizes(&lc(80, 40, &b));
        let layout_bot = calc_sizes(&LayoutConfig {
            cpu_bottom: true,
            ..lc(80, 40, &b)
        });
        assert!(
            layout_top.dims_for(WidgetKind::Cpu).unwrap().y
                < layout_bot.dims_for(WidgetKind::Cpu).unwrap().y
        );
    }

    #[test]
    fn calc_sizes_proc_left() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
        ]);
        let layout = calc_sizes(&LayoutConfig {
            proc_left: true,
            ..lc(120, 40, &b)
        });
        let proc_x = layout.dims_for(WidgetKind::Proc).unwrap().x;
        let mem_x = layout.dims_for(WidgetKind::Mem).unwrap().x;
        assert!(proc_x < mem_x); // proc on left, mem on right
    }

    #[test]
    fn calc_sizes_mem_below_net() {
        let b = widgets(&[WidgetKind::Cpu, WidgetKind::Mem, WidgetKind::Net]);
        let layout_above = calc_sizes(&lc(80, 40, &b));
        let layout_below = calc_sizes(&LayoutConfig {
            mem_below_net: true,
            ..lc(80, 40, &b)
        });
        assert!(
            layout_above.dims_for(WidgetKind::Mem).unwrap().y
                < layout_above.dims_for(WidgetKind::Net).unwrap().y
        );
        assert!(
            layout_below.dims_for(WidgetKind::Mem).unwrap().y
                > layout_below.dims_for(WidgetKind::Net).unwrap().y
        );
    }

    #[test]
    fn calc_sizes_minimum_terminal_size() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
        ]);
        let layout = calc_sizes(&lc_with_hints(10, 5, &b, |h| h.core_count = 2));
        // Should not panic, widgets may have 0-size or be missing
        let _ = layout;
    }

    #[test]
    fn calc_sizes_respects_minimum_dimensions() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
        ]);
        let layout = calc_sizes(&lc_with_hints(200, 60, &b, |h| h.core_count = 16));
        if let Some(mem) = layout.dims_for(WidgetKind::Mem) {
            assert!(mem.width >= MIN_MEM_WIDTH);
            assert!(mem.height >= 6); // minimum: 4 rows + 2 borders
        }
        if let Some(proc_b) = layout.dims_for(WidgetKind::Proc) {
            assert!(proc_b.width >= MIN_PROC_WIDTH);
        }
    }

    #[test]
    fn calc_sizes_disk_widget_when_shown() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
            WidgetKind::Disk,
        ]);
        let layout = calc_sizes(&lc_with_hints(120, 50, &b, |h| h.core_count = 8));
        let disk = layout
            .dims_for(WidgetKind::Disk)
            .expect("disk widget should be present");
        assert!(disk.height >= 2 * 2 + 2);
        // Disk should be below mem and net in the left column
        if let Some(mem) = layout.dims_for(WidgetKind::Mem) {
            assert!(disk.y >= mem.y + mem.height);
        }
    }

    #[test]
    fn calc_sizes_no_disk_widget_when_hidden() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
        ]);
        let layout = calc_sizes(&lc(120, 50, &b));
        assert!(
            layout.dims_for(WidgetKind::Disk).is_none(),
            "disk widget should be absent",
        );
    }

    /// Regression test for the GPU widget identity bug.
    ///
    /// Prior to keying `Layout` by `WidgetKind`, the layout engine
    /// stored GPU dimensions in a dense `Vec` and the renderer
    /// indexed `gpu.gpus[gi]` by enumerate position. Toggling off
    /// `gpu0` while `gpu1` was enabled would render `gpu.gpus[0]`
    /// (the wrong device) with a `gpu0` title (the wrong label).
    ///
    /// With `PerWidget<Option<WidgetDimensions>>` keyed by
    /// `WidgetKind::Gpu(n)`, sparse layouts populate exactly the
    /// requested slots and the renderer iterates `0..MAX_GPUS`
    /// using the actual `n` for both the slot lookup and the
    /// `gpu.gpus[n]` index.
    #[test]
    fn calc_sizes_sparse_gpu_layout_preserves_indices() {
        let b = widgets(&[WidgetKind::Cpu, WidgetKind::Gpu(1), WidgetKind::Gpu(3)]);
        let layout = calc_sizes(&lc_with_hints(120, 50, &b, |h| {
            h.core_count = 8;
            h.gpu_count = 4;
        }));

        assert!(
            layout.dims_for(WidgetKind::Gpu(0)).is_none(),
            "gpu0 was not in the widget list",
        );
        assert!(
            layout.dims_for(WidgetKind::Gpu(1)).is_some(),
            "gpu1 was enabled and should be present",
        );
        assert!(
            layout.dims_for(WidgetKind::Gpu(2)).is_none(),
            "gpu2 was not in the widget list",
        );
        assert!(
            layout.dims_for(WidgetKind::Gpu(3)).is_some(),
            "gpu3 was enabled and should be present",
        );

        // Placement order preserved: gpu1 above gpu3 (lower index first).
        let gpu1 = layout.dims_for(WidgetKind::Gpu(1)).unwrap();
        let gpu3 = layout.dims_for(WidgetKind::Gpu(3)).unwrap();
        assert!(
            gpu1.y < gpu3.y,
            "lower GPU index should be placed above higher index",
        );
    }

    #[test]
    fn calc_sizes_skips_gpu_indices_beyond_detected_count() {
        // Enabled in widget list but no such device — must not be laid out.
        let b = widgets(&[WidgetKind::Cpu, WidgetKind::Gpu(0), WidgetKind::Gpu(5)]);
        let layout = calc_sizes(&lc_with_hints(120, 50, &b, |h| {
            h.core_count = 8;
            h.gpu_count = 1;
        }));
        assert!(layout.dims_for(WidgetKind::Gpu(0)).is_some());
        assert!(layout.dims_for(WidgetKind::Gpu(5)).is_none());
    }

    // ----------------------------------------------------------------
    // min_terminal_size
    // ----------------------------------------------------------------

    #[test]
    fn min_terminal_size_proc_only_uses_proc_minimums() {
        let b = widgets(&[WidgetKind::Proc]);
        let (w, h) = min_terminal_size(&lc(0, 0, &b));
        assert_eq!(w, MIN_PROC_WIDTH);
        assert_eq!(h, MIN_PROC_HEIGHT);
    }

    #[test]
    fn min_terminal_size_left_only_uses_widest_left_widget() {
        // Only net is in the left column -> width floor is MIN_NET_WIDTH.
        let b = widgets(&[WidgetKind::Net]);
        let (w, _) = min_terminal_size(&lc(0, 0, &b));
        assert_eq!(w, MIN_NET_WIDTH);

        // Mem in the left column -> MIN_MEM_WIDTH (the dominant floor).
        let b = widgets(&[WidgetKind::Mem, WidgetKind::Net]);
        let (w, _) = min_terminal_size(&lc(0, 0, &b));
        assert_eq!(w, MIN_MEM_WIDTH);
    }

    #[test]
    fn min_terminal_size_two_columns_satisfies_pct_split() {
        let b = widgets(&[WidgetKind::Mem, WidgetKind::Proc]);
        let (w, _) = min_terminal_size(&lc(0, 0, &b));
        // 60/40 split must give proc >= MIN_PROC_WIDTH and left >= MIN_MEM_WIDTH.
        assert!(w * PROC_WIDTH_PCT / 100 >= MIN_PROC_WIDTH);
        assert!(w - w * PROC_WIDTH_PCT / 100 >= MIN_MEM_WIDTH);
    }

    #[test]
    fn min_terminal_size_height_sums_left_column_widgets() {
        // CPU + Mem + Net + Disk in default layout. Height must fit
        // CPU's preferred height plus the sum of left-column
        // preferred heights.
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Disk,
            WidgetKind::Proc,
        ]);
        let cfg = lc_with_hints(0, 0, &b, |h| {
            h.core_count = 4;
            h.disk_count = 1;
            h.disk_rows_per_unit = 2;
        });
        let (_, height) = min_terminal_size(&cfg);
        let cpu_pref = crate::ui::cpu_widget::preferred_height(&cfg.hints);
        let left = crate::ui::mem_widget::preferred_height(&cfg.hints)
            + MIN_NET_HEIGHT
            + crate::ui::disk_widget::preferred_height(&cfg.hints);
        // Must satisfy both the layout sum and the cpu/3 clamp.
        assert!(height >= cpu_pref + left);
        assert!(height >= 3 * cpu_pref);
    }

    #[test]
    fn min_terminal_size_grows_with_more_disks_and_gpus() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Disk,
            WidgetKind::Gpu(0),
            WidgetKind::Proc,
        ]);
        let small = min_terminal_size(&lc_with_hints(0, 0, &b, |h| {
            h.core_count = 4;
            h.disk_count = 1;
            h.gpu_count = 1;
            h.disk_rows_per_unit = 2;
        }));
        let large = min_terminal_size(&lc_with_hints(0, 0, &b, |h| {
            h.core_count = 32;
            h.disk_count = 4;
            h.gpu_count = 1;
            h.has_cpu_temp = true;
            h.has_cpu_watts = true;
            h.disk_rows_per_unit = 2;
        }));
        assert!(
            large.1 > small.1,
            "more cores/disks/temps should require taller terminal: small={small:?}, large={large:?}",
        );
    }

    // ----------------------------------------------------------------
    // LayoutPlan
    // ----------------------------------------------------------------

    #[test]
    fn layout_plan_captures_widget_set_and_orientation_flags() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
            WidgetKind::Disk,
        ]);
        let mut cfg = lc_with_hints(120, 40, &b, |h| h.core_count = 8);
        cfg.cpu_bottom = true;
        cfg.mem_below_net = true;
        cfg.proc_left = true;
        let plan = LayoutPlan::from(&cfg);
        assert!(plan.has_cpu);
        assert!(plan.has_proc);
        assert!(plan.left_column.has_mem);
        assert!(plan.left_column.has_net);
        assert!(plan.left_column.has_disk);
        assert!(plan.left_column.gpu_indices.is_empty());
        assert!(plan.cpu_bottom);
        assert!(plan.mem_below_net);
        assert!(plan.proc_left);
        assert_eq!(plan.term_size, (120, 40));
    }

    #[test]
    fn layout_plan_filters_gpus_against_detected_count() {
        // gpu0, gpu1, gpu5 in widget list but only 2 devices detected
        // → only gpu0 and gpu1 land in the plan.
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Gpu(0),
            WidgetKind::Gpu(1),
            WidgetKind::Gpu(5),
        ]);
        let cfg = lc_with_hints(120, 40, &b, |h| {
            h.core_count = 8;
            h.gpu_count = 2;
        });
        let plan = LayoutPlan::from(&cfg);
        assert_eq!(plan.left_column.gpu_indices, vec![0, 1]);
    }

    #[test]
    fn layout_plan_left_column_is_empty_when_no_left_widgets_present() {
        let b = widgets(&[WidgetKind::Cpu, WidgetKind::Proc]);
        let plan = LayoutPlan::from(&lc(120, 40, &b));
        assert!(plan.left_column.is_empty());
    }
}
