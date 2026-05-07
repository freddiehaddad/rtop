//! Disk widget sizing: constants and intrinsic-height policy.
//!
//! All width budgeting for the per-disk row layouts (label, meter,
//! graphs, value columns) lives here so the width math is in one place
//! and the orchestration in `mod.rs` only consumes the resulting cells.

use std::collections::VecDeque;

use crate::draw::layout::{LayoutHints, MIN_DISK_HEIGHT};

/// Right-aligned value column width for normal mode ("274G/1.6T" = up to 10 chars).
pub(super) const USAGE_VAL_W: usize = 10;

/// Right-aligned value column for IO combined rows
/// (1 space + max "R1023B/s W1023B/s" = 18 chars).
pub(super) const IO_COMBINED_VAL_W: usize = 19;

/// Right-aligned width for a shortened per-second speed value in
/// the IO display modes (perf-row mode and IO separate-row mode).
/// `floating_humanizer(_, shorten=true, _, _, true, _)` returns at
/// most 6 characters (e.g. `0.0B/s`, `999K/s`); the `+1` guarantees
/// at least one leading space between the `R`/`W` letter and the
/// value, mirroring the `B` column's `max_pct_width + 1` rjust
/// target. Without this fixed width the graphs visually shift when
/// the speed value's character count changes.
pub(super) const IO_SPEED_W: usize = 7;

/// Minimum graph/meter width.
pub(super) const MIN_METER_W: usize = 5;

/// Minimum graph width in IO mode.
pub(super) const MIN_IO_GRAPH_W: usize = 3;

/// Preferred intrinsic height for the disk widget given the
/// snapshot hints, in rows (including borders).
///
/// Each disk reserves [`hints.disk_rows_per_unit`] rows (1 for
/// capacity-only or combined-IO; 2 for capacity-with-inline-IO or
/// split-graph IO view). Plus 2 border rows, with a floor of
/// [`MIN_DISK_HEIGHT`].
pub fn preferred_height(hints: &LayoutHints) -> usize {
    let content_rows = hints.disk_count * hints.disk_rows_per_unit as usize;
    (content_rows + 2).max(MIN_DISK_HEIGHT)
}

/// Maximum value visible in the rightmost `width` samples of `history`,
/// floored at the `current` reading and at 1 (so percentage math never
/// divides by zero). Mirrors the per-graph denominator the IO view and
/// inline perf row both need.
pub(super) fn visible_graph_max(history: &VecDeque<i64>, width: usize, current: u64) -> i64 {
    history
        .iter()
        .rev()
        .take(width.max(1))
        .copied()
        .max()
        .unwrap_or(0)
        .max(current as i64)
        .max(1)
}
