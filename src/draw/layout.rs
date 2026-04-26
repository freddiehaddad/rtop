/// Dimensions and position of a UI box.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoxDimensions {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// Complete layout of all UI boxes.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub cpu: Option<BoxDimensions>,
    pub mem: Option<BoxDimensions>,
    pub disk: Option<BoxDimensions>,
    pub net: Option<BoxDimensions>,
    pub proc_box: Option<BoxDimensions>,
    pub gpu: Vec<BoxDimensions>,
}

/// Minimum box dimensions (matching btop).
pub const MIN_CPU_HEIGHT: usize = 8;
/// Minimum height for the memory box.
pub const MIN_MEM_HEIGHT: usize = 10;
/// Minimum width for the memory box.
pub const MIN_MEM_WIDTH: usize = 36;
/// Minimum height for the network box.
pub const MIN_NET_HEIGHT: usize = 6;
/// Minimum width for the network box.
pub const MIN_NET_WIDTH: usize = 20;
/// Minimum width for the process box.
pub const MIN_PROC_WIDTH: usize = 44;
/// Minimum height for a GPU box.
pub const MIN_GPU_HEIGHT: usize = 4;
/// Minimum height for the disk box.
pub const MIN_DISK_HEIGHT: usize = 4;
/// Percentage of terminal width allocated to the proc box (right column).
const PROC_WIDTH_PCT: usize = 55;
/// Percentage of remaining height allocated to the mem box when both mem+net are shown.
const MEM_HEIGHT_PCT: usize = 55;

/// Configuration for layout calculation.
pub struct LayoutConfig<'a> {
    pub term_width: usize,
    pub term_height: usize,
    pub shown_boxes: &'a [String],
    pub cpu_bottom: bool,
    pub mem_below_net: bool,
    pub proc_left: bool,
    pub core_count: usize,
    pub gpu_count: usize,
}

/// Calculate box sizes and positions based on terminal dimensions and config.
pub fn calc_sizes(cfg: &LayoutConfig) -> Layout {
    let term_width = cfg.term_width;
    let term_height = cfg.term_height;
    let shown_boxes = cfg.shown_boxes;
    let cpu_bottom = cfg.cpu_bottom;
    let mem_below_net = cfg.mem_below_net;
    let proc_left = cfg.proc_left;
    let core_count = cfg.core_count;
    let gpu_count = cfg.gpu_count;
    let has_cpu = shown_boxes.iter().any(|b| b == "cpu");
    let has_mem = shown_boxes.iter().any(|b| b == "mem");
    let has_net = shown_boxes.iter().any(|b| b == "net");
    let has_proc = shown_boxes.iter().any(|b| b == "proc");
    let has_disk = shown_boxes.iter().any(|b| b == "disk");

    // Count how many gpu boxes are shown
    let gpu_shown: Vec<usize> = (0..gpu_count)
        .filter(|i| shown_boxes.iter().any(|b| b == &format!("gpu{i}")))
        .collect();

    let mut layout = Layout::default();

    if term_width < 2 || term_height < 2 {
        return layout;
    }

    // GPU boxes — each takes MIN_GPU_HEIGHT, stacked below CPU
    let total_gpu_height = gpu_shown.len() * MIN_GPU_HEIGHT;

    // CPU box height based on core count
    let cpu_height = if has_cpu {
        let rows = core_count.div_ceil(2); // 2 cores per row
        let max_h = (term_height / 3).max(MIN_CPU_HEIGHT);
        (rows + 5).clamp(MIN_CPU_HEIGHT, max_h)
    } else {
        0
    };

    // Top section height (CPU + GPU boxes)
    let top_section = cpu_height + total_gpu_height;

    // Proc box width (right side, ~55%)
    let proc_width = if has_proc {
        (term_width * PROC_WIDTH_PCT / 100).max(MIN_PROC_WIDTH).min(term_width)
    } else {
        0
    };

    // Left column width (MEM + NET)
    let left_width = if has_proc {
        term_width - proc_width
    } else {
        term_width
    };

    // Remaining height after CPU + GPU
    let remaining_height = term_height.saturating_sub(top_section);

    // Reserve disk height if visible
    let disk_height = if has_disk {
        MIN_DISK_HEIGHT.max(remaining_height / 4).min(remaining_height / 2)
    } else {
        0
    };
    let left_remaining = remaining_height.saturating_sub(disk_height);

    // MEM and NET heights from the remaining left column space
    let (mem_height, net_height) = if has_mem && has_net {
        let mh = (left_remaining * MEM_HEIGHT_PCT / 100).max(MIN_MEM_HEIGHT);
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
        layout.cpu = Some(BoxDimensions {
            x: 0,
            y: cpu_y,
            width: term_width,
            height: cpu_height,
        });
    }

    // GPU boxes stacked below CPU
    let gpu_start_y = if cpu_bottom {
        // GPU above the bottom CPU
        term_height.saturating_sub(top_section) + cpu_height
    } else {
        cpu_height
    };
    for (i, &gpu_idx) in gpu_shown.iter().enumerate() {
        let _ = gpu_idx;
        layout.gpu.push(BoxDimensions {
            x: 0,
            y: gpu_start_y + i * MIN_GPU_HEIGHT,
            width: term_width,
            height: MIN_GPU_HEIGHT,
        });
    }

    // Left column Y start (after CPU + GPU if at top)
    let left_y_start = if cpu_bottom { 0 } else { top_section };

    // MEM and NET positions
    let (mem_y, net_y) = if mem_below_net {
        (left_y_start + net_height, left_y_start)
    } else {
        (left_y_start, left_y_start + mem_height)
    };

    let left_x = if proc_left { proc_width } else { 0 };

    if has_mem {
        layout.mem = Some(BoxDimensions {
            x: left_x,
            y: mem_y,
            width: left_width.max(MIN_MEM_WIDTH),
            height: mem_height,
        });
    }

    if has_net {
        layout.net = Some(BoxDimensions {
            x: left_x,
            y: net_y,
            width: left_width.max(MIN_NET_WIDTH),
            height: net_height,
        });
    }

    // Disk box — below mem+net in the left column
    if has_disk {
        let disk_y = left_y_start + mem_height + net_height;
        layout.disk = Some(BoxDimensions {
            x: left_x,
            y: disk_y,
            width: left_width.max(MIN_MEM_WIDTH),
            height: disk_height,
        });
    }

    // PROC position
    if has_proc {
        let proc_x = if proc_left { 0 } else { left_width };
        layout.proc_box = Some(BoxDimensions {
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

    fn boxes(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn lc(tw: usize, th: usize, shown: &[String]) -> LayoutConfig<'_> {
        LayoutConfig { term_width: tw, term_height: th, shown_boxes: shown, cpu_bottom: false, mem_below_net: false, proc_left: false, core_count: 4, gpu_count: 0 }
    }

    #[test]
    fn calc_sizes_all_boxes_shown() {
        let b = boxes(&["cpu", "mem", "net", "proc"]);
        let layout = calc_sizes(&LayoutConfig { core_count: 8, ..lc(120, 40, &b) });
        assert!(layout.cpu.is_some());
        assert!(layout.mem.is_some());
        assert!(layout.net.is_some());
        assert!(layout.proc_box.is_some());
    }

    #[test]
    fn calc_sizes_cpu_only() {
        let b = boxes(&["cpu"]);
        let layout = calc_sizes(&lc(80, 24, &b));
        assert!(layout.cpu.is_some());
        assert!(layout.mem.is_none());
        assert!(layout.net.is_none());
        assert!(layout.proc_box.is_none());
    }

    #[test]
    fn calc_sizes_proc_only() {
        let b = boxes(&["proc"]);
        let layout = calc_sizes(&lc(80, 24, &b));
        assert!(layout.proc_box.is_some());
        assert!(layout.cpu.is_none());
    }

    #[test]
    fn calc_sizes_cpu_bottom() {
        let b = boxes(&["cpu", "mem"]);
        let layout_top = calc_sizes(&lc(80, 40, &b));
        let layout_bot = calc_sizes(&LayoutConfig { cpu_bottom: true, ..lc(80, 40, &b) });
        assert!(layout_top.cpu.as_ref().unwrap().y < layout_bot.cpu.as_ref().unwrap().y);
    }

    #[test]
    fn calc_sizes_proc_left() {
        let b = boxes(&["cpu", "mem", "net", "proc"]);
        let layout = calc_sizes(&LayoutConfig { proc_left: true, ..lc(120, 40, &b) });
        let proc_x = layout.proc_box.as_ref().unwrap().x;
        let mem_x = layout.mem.as_ref().unwrap().x;
        assert!(proc_x < mem_x); // proc on left, mem on right
    }

    #[test]
    fn calc_sizes_mem_below_net() {
        let b = boxes(&["cpu", "mem", "net"]);
        let layout_above = calc_sizes(&lc(80, 40, &b));
        let layout_below = calc_sizes(&LayoutConfig { mem_below_net: true, ..lc(80, 40, &b) });
        assert!(
            layout_above.mem.as_ref().unwrap().y < layout_above.net.as_ref().unwrap().y
        );
        assert!(
            layout_below.mem.as_ref().unwrap().y > layout_below.net.as_ref().unwrap().y
        );
    }

    #[test]
    fn calc_sizes_minimum_terminal_size() {
        let b = boxes(&["cpu", "mem", "net", "proc"]);
        let layout = calc_sizes(&LayoutConfig { core_count: 2, ..lc(10, 5, &b) });
        // Should not panic, boxes may have 0-size or be missing
        let _ = layout;
    }

    #[test]
    fn calc_sizes_respects_minimum_dimensions() {
        let b = boxes(&["cpu", "mem", "net", "proc"]);
        let layout = calc_sizes(&LayoutConfig { core_count: 16, ..lc(200, 60, &b) });
        if let Some(mem) = &layout.mem {
            assert!(mem.width >= MIN_MEM_WIDTH);
            assert!(mem.height >= MIN_MEM_HEIGHT);
        }
        if let Some(proc_b) = &layout.proc_box {
            assert!(proc_b.width >= MIN_PROC_WIDTH);
        }
    }

    #[test]
    fn calc_sizes_disk_box_when_shown() {
        let b = boxes(&["cpu", "mem", "net", "proc", "disk"]);
        let layout = calc_sizes(&LayoutConfig { core_count: 8, ..lc(120, 50, &b) });
        assert!(layout.disk.is_some(), "disk box should be present");
        let disk = layout.disk.as_ref().unwrap();
        assert!(disk.height >= MIN_DISK_HEIGHT);
        // Disk should be below mem and net in the left column
        if let Some(mem) = &layout.mem {
            assert!(disk.y >= mem.y + mem.height);
        }
    }

    #[test]
    fn calc_sizes_no_disk_box_when_hidden() {
        let b = boxes(&["cpu", "mem", "net", "proc"]);
        let layout = calc_sizes(&lc(120, 50, &b));
        assert!(layout.disk.is_none(), "disk box should be absent");
    }
}
