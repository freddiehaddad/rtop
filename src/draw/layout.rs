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
/// Percentage of terminal width allocated to the proc widget (right column).
const PROC_WIDTH_PCT: usize = 60;

/// Minimum terminal width for the default layout (left column + proc column).
///
/// Derived from: `MIN_MEM_WIDTH (36) + MIN_PROC_WIDTH (44) = 80`.
/// When both a left-column widget (mem/net/disk) and proc are visible, the
/// terminal must be wide enough for both columns.
pub const MIN_TERM_WIDTH: usize = MIN_MEM_WIDTH + MIN_PROC_WIDTH;

/// Minimum terminal height for the default layout.
///
/// Derived from: `MIN_CPU_HEIGHT (8) + MIN_NET_HEIGHT (6) + MIN_DISK_HEIGHT (4) = 18`.
/// This is the smallest height that can fit cpu (top) plus the shortest
/// combination of left-column widgets beneath it.
pub const MIN_TERM_HEIGHT: usize = MIN_CPU_HEIGHT + MIN_NET_HEIGHT + MIN_DISK_HEIGHT;

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
pub fn calc_sizes(cfg: &LayoutConfig) -> Layout {
    let term_width = cfg.term_width;
    let term_height = cfg.term_height;
    let widgets = cfg.widgets;
    let cpu_bottom = cfg.cpu_bottom;
    let mem_below_net = cfg.mem_below_net;
    let proc_left = cfg.proc_left;
    let hints = &cfg.hints;
    let has_cpu = widgets.contains(&WidgetKind::Cpu);
    let has_mem = widgets.contains(&WidgetKind::Mem);
    let has_net = widgets.contains(&WidgetKind::Net);
    let has_proc = widgets.contains(&WidgetKind::Proc);
    let has_disk = widgets.contains(&WidgetKind::Disk);

    // Collect the actual GPU indices to render this frame, preserving
    // identity. Filter against `gpu_count` (devices detected) and the
    // user's widget list. The order in this Vec is the layout
    // placement order — GPU widgets are placed top-to-bottom in their
    // index order, but each placement carries its true `n`.
    let gpu_indices_shown: Vec<u8> = (0..MAX_GPUS as u8)
        .filter(|n| (*n as usize) < hints.gpu_count && widgets.contains(&WidgetKind::Gpu(*n)))
        .collect();
    let gpu_count_shown = gpu_indices_shown.len();

    let mut layout = Layout::default();

    if term_width < 2 || term_height < 2 {
        return layout;
    }

    // GPU widgets — fixed height per device (queried from the widget itself).
    let gpu_unit_height = crate::ui::gpu_widget::preferred_height();
    let total_gpu_height = gpu_count_shown * gpu_unit_height;

    // CPU widget height: widget computes its preferred intrinsic
    // height; layout clamps it against `[MIN_CPU_HEIGHT, term_height/3]`.
    let cpu_height = if has_cpu {
        let max_h = (term_height / 3).max(MIN_CPU_HEIGHT);
        crate::ui::cpu_widget::preferred_height(hints).clamp(MIN_CPU_HEIGHT, max_h)
    } else {
        0
    };

    // Top section is CPU only (GPU moves to left column)
    let top_section = cpu_height;

    // Whether there are any left-column widgets (now includes GPU)
    let has_gpu = gpu_count_shown > 0;
    let has_left = has_mem || has_net || has_disk || has_gpu;

    // Proc widget width (right side, ~55% — or full width if no left-column widgets)
    let proc_width = if has_proc {
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

    // Left column width (MEM + NET + DISK)
    let left_width = if has_proc && has_left {
        term_width - proc_width
    } else if has_left {
        term_width
    } else {
        0
    };

    // Remaining height after CPU
    let remaining_height = term_height.saturating_sub(top_section);

    // Reserve GPU height in left column
    let gpu_height_total = if has_gpu { total_gpu_height } else { 0 };

    // Disk widget — preferred height comes from the widget itself.
    let disk_height = if has_disk {
        crate::ui::disk_widget::preferred_height(hints)
    } else {
        0
    };
    let left_remaining = remaining_height
        .saturating_sub(disk_height)
        .saturating_sub(gpu_height_total);

    // MEM preferred height comes from the widget itself.
    let mem_fixed = crate::ui::mem_widget::preferred_height(hints);

    // MEM and NET heights from the remaining left column space
    let (mem_height, net_height) = if has_mem && has_net {
        let mh = mem_fixed.min(left_remaining);
        let nh = left_remaining.saturating_sub(mh).max(MIN_NET_HEIGHT);
        (mh, nh)
    } else if has_mem {
        (left_remaining, 0)
    } else if has_net {
        (0, left_remaining)
    } else {
        (0, 0)
    };

    // CPU position
    let cpu_y = if cpu_bottom {
        term_height.saturating_sub(top_section)
    } else {
        0
    };

    if has_cpu {
        layout.set(
            WidgetKind::Cpu,
            WidgetDimensions {
                x: 0,
                y: cpu_y,
                width: term_width,
                height: cpu_height,
            },
        );
    }

    // Left column positioning
    let left_y_start = if cpu_bottom { 0 } else { top_section };
    let left_x = if proc_left { proc_width } else { 0 };

    // GPU widgets in the left column, above mem. Each enabled GPU
    // widget is keyed by its actual index `n` (from
    // `WidgetKind::Gpu(n)`); the `placement_i` only drives the
    // vertical position so widgets stack top-to-bottom in
    // declaration order without leaving gaps when a low-index GPU
    // is disabled.
    let gpu_start_y = left_y_start;
    for (placement_i, n) in gpu_indices_shown.iter().enumerate() {
        layout.set(
            WidgetKind::Gpu(*n),
            WidgetDimensions {
                x: left_x,
                y: gpu_start_y + placement_i * gpu_unit_height,
                width: left_width.max(MIN_MEM_WIDTH),
                height: gpu_unit_height,
            },
        );
    }

    // Shift left column content below GPU
    let left_content_y = left_y_start + gpu_height_total;

    // MEM and NET positions (below GPU in left column)
    let (mem_y, net_y) = if mem_below_net {
        (left_content_y + net_height, left_content_y)
    } else {
        (left_content_y, left_content_y + mem_height)
    };

    if has_mem {
        layout.set(
            WidgetKind::Mem,
            WidgetDimensions {
                x: left_x,
                y: mem_y,
                width: left_width.max(MIN_MEM_WIDTH),
                height: mem_height,
            },
        );
    }

    if has_net {
        layout.set(
            WidgetKind::Net,
            WidgetDimensions {
                x: left_x,
                y: net_y,
                width: left_width.max(MIN_NET_WIDTH),
                height: net_height,
            },
        );
    }

    // Disk widget — below mem+net in the left column
    if has_disk {
        let disk_y = left_content_y + mem_height + net_height;
        layout.set(
            WidgetKind::Disk,
            WidgetDimensions {
                x: left_x,
                y: disk_y,
                width: left_width.max(MIN_MEM_WIDTH),
                height: disk_height,
            },
        );
    }

    // PROC position
    if has_proc {
        let proc_x = if proc_left { 0 } else { left_width };
        layout.set(
            WidgetKind::Proc,
            WidgetDimensions {
                x: proc_x,
                y: left_y_start,
                width: proc_width,
                height: remaining_height,
            },
        );
    }

    layout
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
}
