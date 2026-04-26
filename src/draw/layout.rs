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
    pub net: Option<BoxDimensions>,
    pub proc_box: Option<BoxDimensions>,
    pub gpu: Vec<BoxDimensions>,
}

/// Minimum box dimensions (matching btop).
pub const MIN_CPU_HEIGHT: usize = 8;
pub const MIN_MEM_HEIGHT: usize = 10;
pub const MIN_MEM_WIDTH: usize = 36;
pub const MIN_NET_HEIGHT: usize = 6;
pub const MIN_NET_WIDTH: usize = 20;
#[allow(dead_code)] // used in tests
pub const MIN_PROC_HEIGHT: usize = 10;
pub const MIN_PROC_WIDTH: usize = 44;
pub const MIN_GPU_HEIGHT: usize = 4;

/// Calculate box sizes and positions based on terminal dimensions and config.
#[allow(clippy::too_many_arguments)]
pub fn calc_sizes(
    term_width: usize,
    term_height: usize,
    shown_boxes: &[String],
    cpu_bottom: bool,
    mem_below_net: bool,
    proc_left: bool,
    core_count: usize,
    gpu_count: usize,
) -> Layout {
    let has_cpu = shown_boxes.iter().any(|b| b == "cpu");
    let has_mem = shown_boxes.iter().any(|b| b == "mem");
    let has_net = shown_boxes.iter().any(|b| b == "net");
    let has_proc = shown_boxes.iter().any(|b| b == "proc");

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
        (term_width * 55 / 100).max(MIN_PROC_WIDTH).min(term_width)
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

    // MEM and NET heights
    let (mem_height, net_height) = if has_mem && has_net {
        let mh = (remaining_height * 55 / 100).max(MIN_MEM_HEIGHT);
        let nh = remaining_height.saturating_sub(mh).max(MIN_NET_HEIGHT);
        (mh, nh)
    } else if has_mem {
        (remaining_height, 0)
    } else if has_net {
        (0, remaining_height)
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

    #[test]
    fn calc_sizes_all_boxes_shown() {
        let layout = calc_sizes(120, 40, &boxes(&["cpu", "mem", "net", "proc"]), false, false, false, 8, 0);
        assert!(layout.cpu.is_some());
        assert!(layout.mem.is_some());
        assert!(layout.net.is_some());
        assert!(layout.proc_box.is_some());
    }

    #[test]
    fn calc_sizes_cpu_only() {
        let layout = calc_sizes(80, 24, &boxes(&["cpu"]), false, false, false, 4, 0);
        assert!(layout.cpu.is_some());
        assert!(layout.mem.is_none());
        assert!(layout.net.is_none());
        assert!(layout.proc_box.is_none());
    }

    #[test]
    fn calc_sizes_proc_only() {
        let layout = calc_sizes(80, 24, &boxes(&["proc"]), false, false, false, 4, 0);
        assert!(layout.proc_box.is_some());
        assert!(layout.cpu.is_none());
    }

    #[test]
    fn calc_sizes_cpu_bottom() {
        let layout_top = calc_sizes(80, 40, &boxes(&["cpu", "mem"]), false, false, false, 4, 0);
        let layout_bot = calc_sizes(80, 40, &boxes(&["cpu", "mem"]), true, false, false, 4, 0);
        assert!(layout_top.cpu.as_ref().unwrap().y < layout_bot.cpu.as_ref().unwrap().y);
    }

    #[test]
    fn calc_sizes_proc_left() {
        let layout = calc_sizes(120, 40, &boxes(&["cpu", "mem", "net", "proc"]), false, false, true, 4, 0);
        let proc_x = layout.proc_box.as_ref().unwrap().x;
        let mem_x = layout.mem.as_ref().unwrap().x;
        assert!(proc_x < mem_x); // proc on left, mem on right
    }

    #[test]
    fn calc_sizes_mem_below_net() {
        let layout_above = calc_sizes(80, 40, &boxes(&["cpu", "mem", "net"]), false, false, false, 4, 0);
        let layout_below = calc_sizes(80, 40, &boxes(&["cpu", "mem", "net"]), false, true, false, 4, 0);
        assert!(
            layout_above.mem.as_ref().unwrap().y < layout_above.net.as_ref().unwrap().y
        );
        assert!(
            layout_below.mem.as_ref().unwrap().y > layout_below.net.as_ref().unwrap().y
        );
    }

    #[test]
    fn calc_sizes_minimum_terminal_size() {
        let layout = calc_sizes(10, 5, &boxes(&["cpu", "mem", "net", "proc"]), false, false, false, 2, 0);
        // Should not panic, boxes may have 0-size or be missing
        let _ = layout;
    }

    #[test]
    fn calc_sizes_respects_minimum_dimensions() {
        let layout = calc_sizes(200, 60, &boxes(&["cpu", "mem", "net", "proc"]), false, false, false, 16, 0);
        if let Some(mem) = &layout.mem {
            assert!(mem.width >= MIN_MEM_WIDTH);
            assert!(mem.height >= MIN_MEM_HEIGHT);
        }
        if let Some(proc_b) = &layout.proc_box {
            assert!(proc_b.width >= MIN_PROC_WIDTH);
        }
    }
}
