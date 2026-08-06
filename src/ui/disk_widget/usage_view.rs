//! Per-disk row rendering for the usage view (default mode).
//!
//! Two pieces:
//! * [`draw_usage_row`]: the capacity meter row (`<drive> <fs> [meter] <used>/<total>`).
//! * [`draw_perf_row`]: the optional second row (`R <speed> [graph] W <speed> [graph] B <pct>%`)
//!   shown when `DiskFrame::show_io_stat` is on.

use crate::domain::disk::DiskInfo;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::Graph;
use crate::draw::meter::Meter;
use crate::theme::{Theme, gradient_color};
use crate::theme_keys as tc;
use crate::tools;

use super::DiskFrame;
use super::sizing::{IO_SPEED_W, MIN_METER_W, USAGE_VAL_W, visible_graph_max};

pub(super) struct UsageRowParams<'a> {
    pub content_x: usize,
    pub row_y: usize,
    pub inner_w: usize,
    pub theme: &'a Theme,
    pub settings: &'a DiskFrame,
    pub avail_grad: &'a [String],
    pub meter_bg: &'a str,
}

/// Render the capacity meter row for one disk. Always one row tall;
/// returns 1 to mirror the IO helpers' "rows drawn" contract.
pub(super) fn draw_usage_row(
    buf: &mut AnsiBuffer,
    disk: &DiskInfo,
    params: &UsageRowParams<'_>,
) -> usize {
    let fg = params.theme.color(tc::MAIN_FG);

    let du = tools::floating_humanizer(disk.used, true, 0, false, false, params.settings.base_10);
    let dt = tools::floating_humanizer(disk.total, true, 0, false, false, params.settings.base_10);
    let value = format!("{}/{}", du, dt);

    let label = if disk.fstype.is_empty() {
        format!("{} ", disk.name)
    } else {
        format!("{} {} ", disk.name, disk.fstype)
    };
    let label_len = label.len();
    let meter_w = params
        .inner_w
        .saturating_sub(label_len + USAGE_VAL_W)
        .max(MIN_METER_W);
    let disk_meter = Meter::new(meter_w, params.avail_grad, params.meter_bg);

    buf.mv(params.content_x, params.row_y)
        .color(fg)
        .text(&label)
        .text(disk_meter.render(disk.used_percent))
        .color(gradient_color(params.avail_grad, disk.used_percent))
        .text(&tools::rjust(&value, USAGE_VAL_W, false));

    1
}

pub(super) struct PerfRowParams<'a> {
    pub content_x: usize,
    pub row_y: usize,
    pub inner_w: usize,
    pub theme: &'a Theme,
    pub settings: &'a DiskFrame,
    pub read_grad: &'a [String],
    pub write_grad: &'a [String],
    pub busy_grad: &'a [String],
}

/// Render the inline IO perf row beneath a usage row. One row tall;
/// returns 1.
pub(super) fn draw_perf_row(
    buf: &mut AnsiBuffer,
    disk: &DiskInfo,
    params: &PerfRowParams<'_>,
) -> usize {
    let x = params.content_x;
    let y = params.row_y;
    let width = params.inner_w;
    if width == 0 {
        return 0;
    }

    let base_10 = params.settings.base_10;
    let fg = params.theme.color(tc::MAIN_FG);
    let read_speed =
        tools::floating_humanizer(disk.read_bytes_per_sec, true, 0, false, true, base_10);
    let write_speed =
        tools::floating_humanizer(disk.write_bytes_per_sec, true, 0, false, true, base_10);
    let busy = disk.busy_percent.clamp(0, 100);
    let busy_w: usize = 6; // "B" + 5-char right-justified value
    let busy_x = x + width.saturating_sub(busy_w);
    let left_w = width.saturating_sub(busy_w);

    // Constant column widths so the graph never shifts when the
    // speed-string width changes. Mirrors the `B` column pattern:
    // letter + rjust(value, max_value_width + 1) — the +1 inside
    // IO_SPEED_W guarantees at least one leading space.
    let read_w = 1 + IO_SPEED_W;
    let write_w = 1 + IO_SPEED_W;
    let fixed_left = read_w + write_w + 4; // labels plus spaces around the two graphs
    let graph_total = left_w.saturating_sub(fixed_left);
    let read_graph_w = graph_total / 2;
    let write_graph_w = graph_total.saturating_sub(read_graph_w);

    let read_graph_max =
        visible_graph_max(&disk.read_history, read_graph_w, disk.read_bytes_per_sec);
    let read_pct =
        ((disk.read_bytes_per_sec as i64).saturating_mul(100) / read_graph_max).min(100) as i32;
    let read_color = gradient_color(params.read_grad, read_pct);

    let mut col = x;
    buf.mv(col, y)
        .color(fg)
        .text("R")
        .color(read_color)
        .text(&tools::rjust(&read_speed, IO_SPEED_W, false));
    col += read_w;

    if read_graph_w > 0 {
        buf.text(" ");
        col += 1;
        let mut graph = Graph::new(
            read_graph_w,
            1,
            params.settings.graph_symbol,
            false,
            read_graph_max,
            0,
        );
        let graph_row = graph.render_row(&disk.read_history, params.read_grad);
        buf.text(&graph_row);
        col += read_graph_w;
    }

    if col + write_w < busy_x {
        buf.text(" ");
        col += 1;
    }

    let available_write_graph_w = write_graph_w.min(busy_x.saturating_sub(col + 1));
    let write_graph_max = if available_write_graph_w > 0 {
        visible_graph_max(
            &disk.write_history,
            available_write_graph_w,
            disk.write_bytes_per_sec,
        )
    } else {
        // No room for the graph — still need a sane denominator for value
        // coloring. Match the "lifetime peak when no window" idiom by using
        // the visible-window max over the full history.
        visible_graph_max(
            &disk.write_history,
            disk.write_history.len().max(1),
            disk.write_bytes_per_sec,
        )
    };
    let write_pct =
        ((disk.write_bytes_per_sec as i64).saturating_mul(100) / write_graph_max).min(100) as i32;
    let write_color = gradient_color(params.write_grad, write_pct);

    buf.color(fg)
        .text("W")
        .color(write_color)
        .text(&tools::rjust(&write_speed, IO_SPEED_W, false));

    if available_write_graph_w > 0 {
        buf.text(" ");
        let mut graph = Graph::new(
            available_write_graph_w,
            1,
            params.settings.graph_symbol,
            false,
            write_graph_max,
            0,
        );
        let graph_row = graph.render_row(&disk.write_history, params.write_grad);
        buf.text(&graph_row);
    }

    let busy_color = gradient_color(params.busy_grad, busy);
    buf.mv(busy_x, y)
        .color(fg)
        .text("B")
        .color(busy_color)
        .text(&format!("{:>5}", format!("{busy}%")));

    1
}
