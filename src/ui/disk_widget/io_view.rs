//! Per-disk row rendering for the IO view (`io_mode || disk_io_mode`).
//!
//! Two layouts are supported, selected by `DiskFrame::io_graph_combined`:
//! * **Combined**: one row per disk with a single graph fed by
//!   `read + write`, ending in an `R<rspeed> W<wspeed>` value column.
//! * **Separate**: two rows per disk (read, then write), each with its
//!   own graph plus a fixed-width `R`/`W` letter and rjust speed.
//!
//! Both helpers return how many rows they actually drew so the
//! orchestrator can advance its cursor and respect `inner_h`.

use crate::domain::disk::DiskInfo;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::Graph;
use crate::theme::{Theme, gradient_color};
use crate::theme_keys as tc;
use crate::tools;

use super::DiskFrame;
use super::sizing::{IO_COMBINED_VAL_W, IO_SPEED_W, MIN_IO_GRAPH_W, visible_graph_max};

pub(super) struct IoRowParams<'a> {
    pub content_x: usize,
    pub row_y: usize,
    pub inner_w: usize,
    pub inner_h_remaining: usize,
    pub theme: &'a Theme,
    pub settings: &'a DiskFrame,
    pub read_grad: &'a [String],
    pub write_grad: &'a [String],
}

/// Render the combined-graph IO row for a disk. Always one row tall;
/// returns 1 if drawn, 0 if `inner_h_remaining == 0`.
pub(super) fn draw_combined_row(
    buf: &mut AnsiBuffer,
    disk: &DiskInfo,
    params: &IoRowParams<'_>,
) -> usize {
    if params.inner_h_remaining == 0 {
        return 0;
    }

    let fg = params.theme.color(tc::MAIN_FG);

    let label = format!("{} IO ", disk.name);
    let label_len = tools::ulen(&label, false);
    let graph_w = params
        .inner_w
        .saturating_sub(label_len + IO_COMBINED_VAL_W)
        .max(MIN_IO_GRAPH_W);

    let speed_r = tools::floating_humanizer(
        disk.read_bytes_per_sec,
        true,
        0,
        false,
        true,
        params.settings.base_10,
    );
    let speed_w = tools::floating_humanizer(
        disk.write_bytes_per_sec,
        true,
        0,
        false,
        true,
        params.settings.base_10,
    );
    let value = format!("R{} W{}", speed_r, speed_w);

    let combined: std::collections::VecDeque<i64> = {
        let rlen = disk.read_history.len();
        let wlen = disk.write_history.len();
        let max_len = rlen.max(wlen);
        let mut combined = std::collections::VecDeque::with_capacity(max_len);
        for i in 0..max_len {
            let r = if i < rlen { disk.read_history[i] } else { 0 };
            let w = if i < wlen { disk.write_history[i] } else { 0 };
            combined.push_back(r + w);
        }
        combined
    };

    let max = visible_graph_max(
        &combined,
        graph_w,
        disk.read_bytes_per_sec + disk.write_bytes_per_sec,
    );
    let mut graph = Graph::new(graph_w, 1, params.settings.graph_symbol, false, max, 0);
    let graph_row = graph.render_row(&combined, params.read_grad);

    let combined_speed = disk.read_bytes_per_sec + disk.write_bytes_per_sec;
    let combined_pct = ((combined_speed as i64).saturating_mul(100) / max).min(100) as i32;
    buf.mv(params.content_x, params.row_y)
        .color(fg)
        .text(&label)
        .text(&graph_row)
        .color(gradient_color(params.read_grad, combined_pct))
        .text(&tools::rjust(&value, IO_COMBINED_VAL_W, false));
    1
}

/// Render the separate read+write rows for a disk. Up to two rows
/// tall; returns the number of rows actually drawn (0, 1, or 2).
pub(super) fn draw_separate_rows(
    buf: &mut AnsiBuffer,
    disk: &DiskInfo,
    params: &IoRowParams<'_>,
) -> usize {
    if params.inner_h_remaining == 0 {
        return 0;
    }

    let fg = params.theme.color(tc::MAIN_FG);

    // Separate read and write graph rows. The R/W letter sits on
    // the right next to the speed value (matching the perf-row
    // column order); the graph fills the variable space between
    // drive label and ` R`/` W` letter. The 2-char gap (one
    // trailing space after the drive label, one leading space
    // before the letter) provides visual separation.
    let label = format!("{} ", disk.name);
    let label_len = tools::ulen(&label, false);
    // Right column: " R" or " W" (2 chars) + rjust(speed, IO_SPEED_W).
    let value_col_w = 2 + IO_SPEED_W;
    let graph_w = params
        .inner_w
        .saturating_sub(label_len + value_col_w)
        .max(MIN_IO_GRAPH_W);

    // Read row.
    let speed_r = tools::floating_humanizer(
        disk.read_bytes_per_sec,
        true,
        0,
        false,
        true,
        params.settings.base_10,
    );

    let read_max = visible_graph_max(&disk.read_history, graph_w, disk.read_bytes_per_sec);
    let mut rg = Graph::new(graph_w, 1, params.settings.graph_symbol, false, read_max, 0);
    let rg_row = rg.render_row(&disk.read_history, params.read_grad);

    let read_pct =
        ((disk.read_bytes_per_sec as i64).saturating_mul(100) / read_max).min(100) as i32;
    buf.mv(params.content_x, params.row_y)
        .color(fg)
        .text(&label)
        .text(&rg_row)
        .color(fg)
        .text(" R")
        .color(gradient_color(params.read_grad, read_pct))
        .text(&tools::rjust(&speed_r, IO_SPEED_W, false));

    if params.inner_h_remaining < 2 {
        return 1;
    }

    // Write row.
    let speed_w = tools::floating_humanizer(
        disk.write_bytes_per_sec,
        true,
        0,
        false,
        true,
        params.settings.base_10,
    );

    let write_max = visible_graph_max(&disk.write_history, graph_w, disk.write_bytes_per_sec);
    let mut wg = Graph::new(
        graph_w,
        1,
        params.settings.graph_symbol,
        false,
        write_max,
        0,
    );
    let wg_row = wg.render_row(&disk.write_history, params.write_grad);

    let write_pct =
        ((disk.write_bytes_per_sec as i64).saturating_mul(100) / write_max).min(100) as i32;
    buf.mv(params.content_x, params.row_y + 1)
        .color(fg)
        .text(&label)
        .text(&wg_row)
        .color(fg)
        .text(" W")
        .color(gradient_color(params.write_grad, write_pct))
        .text(&tools::rjust(&speed_w, IO_SPEED_W, false));

    2
}
