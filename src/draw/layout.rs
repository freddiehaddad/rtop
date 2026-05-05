/// Dimensions and position of a UI widget.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WidgetDimensions {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// Complete layout of all UI widgets.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub cpu: Option<WidgetDimensions>,
    pub mem: Option<WidgetDimensions>,
    pub disk: Option<WidgetDimensions>,
    pub net: Option<WidgetDimensions>,
    pub proc_widget: Option<WidgetDimensions>,
    pub gpu: Vec<WidgetDimensions>,
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
    pub widgets: &'a [crate::domain::widget_kind::WidgetKind],
    pub cpu_bottom: bool,
    pub mem_below_net: bool,
    pub proc_left: bool,
    pub core_count: usize,
    pub gpu_count: usize,
    /// Number of disks to display (drives content height).
    pub disk_count: usize,
    /// Whether swap is active (adds 3 rows to mem height).
    pub has_swap: bool,
    /// CPU core panel overhead rows (stats meters + load detail + divider).
    pub cpu_panel_overhead: usize,
}

/// Calculate widget sizes and positions based on terminal dimensions and config.
pub fn calc_sizes(cfg: &LayoutConfig) -> Layout {
    use crate::domain::widget_kind::WidgetKind;

    let term_width = cfg.term_width;
    let term_height = cfg.term_height;
    let widgets = cfg.widgets;
    let cpu_bottom = cfg.cpu_bottom;
    let mem_below_net = cfg.mem_below_net;
    let proc_left = cfg.proc_left;
    let core_count = cfg.core_count;
    let gpu_count = cfg.gpu_count;
    let has_cpu = widgets.contains(&WidgetKind::Cpu);
    let has_mem = widgets.contains(&WidgetKind::Mem);
    let has_net = widgets.contains(&WidgetKind::Net);
    let has_proc = widgets.contains(&WidgetKind::Proc);
    let has_disk = widgets.contains(&WidgetKind::Disk);

    // Count how many gpu widgets are shown
    let gpu_count_shown = (0..gpu_count)
        .filter_map(WidgetKind::gpu)
        .filter(|kind| widgets.contains(kind))
        .count();

    let mut layout = Layout::default();

    if term_width < 2 || term_height < 2 {
        return layout;
    }

    // GPU widgets — each takes MIN_GPU_HEIGHT, placed in the left column
    let total_gpu_height = gpu_count_shown * MIN_GPU_HEIGHT;

    // CPU widget height: core grid rows + panel overhead + 2 border rows.
    let cpu_height = if has_cpu {
        let max_h = (term_height / 3).max(MIN_CPU_HEIGHT);
        let (core_rows, _) = crate::ui::cpu_widget::core_grid_shape(core_count);
        (core_rows + cfg.cpu_panel_overhead + 2).clamp(MIN_CPU_HEIGHT, max_h)
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

    // Reserve disk height if visible — capacity + IO row per disk, plus borders
    let disk_height = if has_disk {
        let content_rows = cfg.disk_count * 2;
        (content_rows + 2).max(MIN_DISK_HEIGHT)
    } else {
        0
    };
    let left_remaining = remaining_height
        .saturating_sub(disk_height)
        .saturating_sub(gpu_height_total);

    // MEM height: 4 base rows + 3 if swap active + 2 borders
    let mem_content = 4 + if cfg.has_swap { 1 } else { 0 };
    let mem_fixed = mem_content + 2;

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
        layout.cpu = Some(WidgetDimensions {
            x: 0,
            y: cpu_y,
            width: term_width,
            height: cpu_height,
        });
    }

    // Left column positioning
    let left_y_start = if cpu_bottom { 0 } else { top_section };
    let left_x = if proc_left { proc_width } else { 0 };

    // GPU widgets in the left column, above mem
    let gpu_start_y = left_y_start;
    for i in 0..gpu_count_shown {
        layout.gpu.push(WidgetDimensions {
            x: left_x,
            y: gpu_start_y + i * MIN_GPU_HEIGHT,
            width: left_width.max(MIN_MEM_WIDTH),
            height: MIN_GPU_HEIGHT,
        });
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
        layout.mem = Some(WidgetDimensions {
            x: left_x,
            y: mem_y,
            width: left_width.max(MIN_MEM_WIDTH),
            height: mem_height,
        });
    }

    if has_net {
        layout.net = Some(WidgetDimensions {
            x: left_x,
            y: net_y,
            width: left_width.max(MIN_NET_WIDTH),
            height: net_height,
        });
    }

    // Disk widget — below mem+net in the left column
    if has_disk {
        let disk_y = left_content_y + mem_height + net_height;
        layout.disk = Some(WidgetDimensions {
            x: left_x,
            y: disk_y,
            width: left_width.max(MIN_MEM_WIDTH),
            height: disk_height,
        });
    }

    // PROC position
    if has_proc {
        let proc_x = if proc_left { 0 } else { left_width };
        layout.proc_widget = Some(WidgetDimensions {
            x: proc_x,
            y: left_y_start,
            width: proc_width,
            height: remaining_height,
        });
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
            core_count: 4,
            gpu_count: 0,
            disk_count: 2,
            has_swap: false,
            cpu_panel_overhead: 4, // 2 stats + load detail + divider
        }
    }

    #[test]
    fn calc_sizes_all_widgets_shown() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
        ]);
        let layout = calc_sizes(&LayoutConfig {
            core_count: 8,
            ..lc(120, 40, &b)
        });
        assert!(layout.cpu.is_some());
        assert!(layout.mem.is_some());
        assert!(layout.net.is_some());
        assert!(layout.proc_widget.is_some());
    }

    #[test]
    fn calc_sizes_cpu_only() {
        let b = widgets(&[WidgetKind::Cpu]);
        let layout = calc_sizes(&lc(80, 24, &b));
        assert!(layout.cpu.is_some());
        assert!(layout.mem.is_none());
        assert!(layout.net.is_none());
        assert!(layout.proc_widget.is_none());
    }

    #[test]
    fn calc_sizes_proc_only() {
        let b = widgets(&[WidgetKind::Proc]);
        let layout = calc_sizes(&lc(80, 24, &b));
        assert!(layout.proc_widget.is_some());
        assert!(layout.cpu.is_none());
    }

    #[test]
    fn calc_sizes_cpu_bottom() {
        let b = widgets(&[WidgetKind::Cpu, WidgetKind::Mem]);
        let layout_top = calc_sizes(&lc(80, 40, &b));
        let layout_bot = calc_sizes(&LayoutConfig {
            cpu_bottom: true,
            ..lc(80, 40, &b)
        });
        assert!(layout_top.cpu.as_ref().unwrap().y < layout_bot.cpu.as_ref().unwrap().y);
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
        let proc_x = layout.proc_widget.as_ref().unwrap().x;
        let mem_x = layout.mem.as_ref().unwrap().x;
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
        assert!(layout_above.mem.as_ref().unwrap().y < layout_above.net.as_ref().unwrap().y);
        assert!(layout_below.mem.as_ref().unwrap().y > layout_below.net.as_ref().unwrap().y);
    }

    #[test]
    fn calc_sizes_minimum_terminal_size() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
        ]);
        let layout = calc_sizes(&LayoutConfig {
            core_count: 2,
            ..lc(10, 5, &b)
        });
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
        let layout = calc_sizes(&LayoutConfig {
            core_count: 16,
            ..lc(200, 60, &b)
        });
        if let Some(mem) = &layout.mem {
            assert!(mem.width >= MIN_MEM_WIDTH);
            assert!(mem.height >= 6); // minimum: 4 rows + 2 borders
        }
        if let Some(proc_b) = &layout.proc_widget {
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
        let layout = calc_sizes(&LayoutConfig {
            core_count: 8,
            ..lc(120, 50, &b)
        });
        assert!(layout.disk.is_some(), "disk widget should be present");
        let disk = layout.disk.as_ref().unwrap();
        assert!(disk.height >= 2 * 2 + 2);
        // Disk should be below mem and net in the left column
        if let Some(mem) = &layout.mem {
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
        assert!(layout.disk.is_none(), "disk widget should be absent");
    }
}
