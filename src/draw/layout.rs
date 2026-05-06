//! Layout engine: turns a `LayoutConfig` (widget set + orientation
//! flags) into per-widget rectangles in `Layout`.
//!
//! Internally the engine is a recursive tree-walking algorithm over
//! a `Slot` tree (`VStack` / `HStack` / `Widget` leaves). The legacy
//! `LayoutConfig` API with its orientation flags is converted into
//! a `Slot` tree by [`build_slot`]; from there the engine never
//! special-cases any widget by name. Widget-specific rules (slack
//! absorption, the CPU height clamp) are intrinsic properties of
//! [`WidgetKind`] consulted uniformly.
//!
//! Future commits in this layout-redesign series will promote `Slot`
//! to the public surface and drop the orientation-flag fields from
//! `LayoutConfig`. This commit replaces the engine's internals
//! without changing any caller.

use crate::config::MAX_GPUS;
use crate::domain::widget_kind::{PerWidget, WidgetKind, WidgetSizing};

// ─────────────────────────────────────────────────────────────────
// Public types — output, hints, configuration, constants
// ─────────────────────────────────────────────────────────────────

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
/// Percentage of terminal width allocated to the proc-bearing
/// column in a two-column split.
const PROC_WIDTH_PCT: usize = 60;
/// Percentage of terminal width allocated to the non-proc column
/// in a two-column split. By construction equals `100 -
/// PROC_WIDTH_PCT`.
const LEFT_WIDTH_PCT: usize = 100 - PROC_WIDTH_PCT;

/// Configuration for layout calculation.
pub struct LayoutConfig<'a> {
    pub term_width: usize,
    pub term_height: usize,
    pub widgets: &'a [WidgetKind],
    pub cpu_bottom: bool,
    pub mem_below_net: bool,
    pub proc_left: bool,
    /// `true` collapses the layout into a single full-width column
    /// (CPU on top — or bottom per `cpu_bottom` — followed by the
    /// remaining widgets stacked vertically with a Fill widget
    /// absorbing slack).
    pub stack_vertical: bool,
    /// Snapshot-derived sizing inputs (core_count, disk_count,
    /// has_swap, …) that widgets consume via their per-widget
    /// `preferred_height` helpers.
    pub hints: LayoutHints,
}

// ─────────────────────────────────────────────────────────────────
// Public entry points
// ─────────────────────────────────────────────────────────────────

/// Calculate widget sizes and positions based on terminal
/// dimensions and config. Returns an empty layout on degenerate
/// inputs (term smaller than 2x2, or no widgets).
pub fn calc_sizes(cfg: &LayoutConfig) -> Layout {
    let mut layout = Layout::default();
    if cfg.term_width < 2 || cfg.term_height < 2 {
        return layout;
    }
    let Some(slot) = build_slot(cfg) else {
        return layout;
    };
    let area = Rect {
        x: 0,
        y: 0,
        width: cfg.term_width,
        height: cfg.term_height,
    };
    let ctx = PlaceCtx {
        hints: &cfg.hints,
        term_height: cfg.term_height,
    };
    place(&slot, area, &ctx, &mut layout);
    layout
}

/// Smallest terminal size at which the active layout fits without
/// truncation, given the current widget set and snapshot hints.
///
/// Returns the `(width, height)` shown in the "Terminal too small.
/// Need WxH." message, and the value used by the `is_too_small`
/// gate in the event loop.
pub fn min_terminal_size(cfg: &LayoutConfig) -> (usize, usize) {
    let Some(slot) = build_slot(cfg) else {
        return (0, 0);
    };
    let ctx = MinCtx { hints: &cfg.hints };
    let (min_w, mut min_h) = slot_min_size(&slot, &ctx);
    // CPU's preferred-height clamp (`min(preferred, term_height/3)`)
    // means the terminal must be at least three times CPU's
    // preferred height for CPU to render at its preferred size
    // without being clamped. Encoded here at the top level rather
    // than inside the recursion because the clamp is anchored to
    // *terminal* height, not the immediate parent container.
    if has_widget(&slot, WidgetKind::Cpu) {
        let cpu_pref = crate::ui::cpu_widget::preferred_height(&cfg.hints).max(MIN_CPU_HEIGHT);
        min_h = min_h.max(3 * cpu_pref);
    }
    (min_w, min_h)
}

// ─────────────────────────────────────────────────────────────────
// Internal: the new layout primitives
// ─────────────────────────────────────────────────────────────────

/// A rectangular region that holds either a widget, a vertical
/// stack of slots, or a horizontal stack of slots. The engine's
/// internal representation; future commits promote it to the
/// public surface.
#[derive(Debug, Clone)]
enum Slot {
    Widget(WidgetKind),
    VStack(Vec<Slot>),
    HStack(Vec<HStackChild>),
}

/// A child of an `HStack` carrying its relative width weight.
/// Total available width is divided proportionally to weights.
#[derive(Debug, Clone)]
struct HStackChild {
    slot: Slot,
    weight: u8,
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

/// Context threaded through `place` recursion. Carries the data
/// hints widgets need for `preferred_height(hints)` calls plus the
/// terminal height needed for CPU's container-relative clamp.
struct PlaceCtx<'a> {
    hints: &'a LayoutHints,
    term_height: usize,
}

/// Context threaded through `slot_min_size` recursion. Only needs
/// hints — terminal dimensions are unknown at min-size compute time
/// (that's what we're computing).
struct MinCtx<'a> {
    hints: &'a LayoutHints,
}

// ─────────────────────────────────────────────────────────────────
// Build — convert legacy LayoutConfig to a Slot tree
// ─────────────────────────────────────────────────────────────────

/// Convert the legacy orientation-flag-based `LayoutConfig` into a
/// `Slot` tree. Returns `None` when no widgets are visible.
///
/// This is the bridge between the old public API and the new
/// internal representation. Future commits will promote `Slot` to
/// the public API and drop this function.
fn build_slot(cfg: &LayoutConfig) -> Option<Slot> {
    let widgets = cfg.widgets;
    let has_cpu = widgets.contains(&WidgetKind::Cpu);
    let has_mem = widgets.contains(&WidgetKind::Mem);
    let has_net = widgets.contains(&WidgetKind::Net);
    let has_proc = widgets.contains(&WidgetKind::Proc);
    let has_disk = widgets.contains(&WidgetKind::Disk);
    let gpu_indices: Vec<u8> = (0..MAX_GPUS as u8)
        .filter(|n| (*n as usize) < cfg.hints.gpu_count && widgets.contains(&WidgetKind::Gpu(*n)))
        .collect();
    let has_left = has_mem || has_net || has_disk || !gpu_indices.is_empty();

    if !has_cpu && !has_proc && !has_left {
        return None;
    }

    // Build the left-column widget order: GPUs first, then mem/net
    // (per `mem_below_net`), then disk last.
    let mut left_col: Vec<Slot> = Vec::new();
    for n in &gpu_indices {
        left_col.push(Slot::Widget(WidgetKind::Gpu(*n)));
    }
    if cfg.mem_below_net {
        if has_net {
            left_col.push(Slot::Widget(WidgetKind::Net));
        }
        if has_mem {
            left_col.push(Slot::Widget(WidgetKind::Mem));
        }
    } else {
        if has_mem {
            left_col.push(Slot::Widget(WidgetKind::Mem));
        }
        if has_net {
            left_col.push(Slot::Widget(WidgetKind::Net));
        }
    }
    if has_disk {
        left_col.push(Slot::Widget(WidgetKind::Disk));
    }

    // Body = everything that goes below (or above, with `cpu_bottom`)
    // the CPU widget. Three shapes:
    //   * stack_vertical && has_left : single full-width column
    //     containing the left-column widgets and proc.
    //   * !stack_vertical && has_proc && has_left : two-column HStack
    //     with the left-column widgets in one column and proc in the
    //     other (placement controlled by `proc_left`).
    //   * has_proc only : proc fills the body alone.
    //   * has_left only : left-column widgets fill the body alone.
    let body = if cfg.stack_vertical && has_left {
        let mut col = left_col;
        if has_proc {
            col.push(Slot::Widget(WidgetKind::Proc));
        }
        collapse_vstack(col)
    } else if has_proc && has_left {
        let left_slot = collapse_vstack(left_col).expect("has_left implies non-empty left_col");
        let proc_slot = Slot::Widget(WidgetKind::Proc);
        let (first, second) = if cfg.proc_left {
            (
                HStackChild {
                    slot: proc_slot,
                    weight: PROC_WIDTH_PCT as u8,
                },
                HStackChild {
                    slot: left_slot,
                    weight: LEFT_WIDTH_PCT as u8,
                },
            )
        } else {
            (
                HStackChild {
                    slot: left_slot,
                    weight: LEFT_WIDTH_PCT as u8,
                },
                HStackChild {
                    slot: proc_slot,
                    weight: PROC_WIDTH_PCT as u8,
                },
            )
        };
        Some(Slot::HStack(vec![first, second]))
    } else if has_proc {
        Some(Slot::Widget(WidgetKind::Proc))
    } else {
        collapse_vstack(left_col)
    };

    // Wrap with CPU on top or bottom, if present.
    if has_cpu {
        let cpu = Slot::Widget(WidgetKind::Cpu);
        Some(match body {
            Some(b) if cfg.cpu_bottom => Slot::VStack(vec![b, cpu]),
            Some(b) => Slot::VStack(vec![cpu, b]),
            None => cpu,
        })
    } else {
        body
    }
}

/// Wrap a list of slots in a `VStack`, but flatten singleton lists
/// to the inner slot. Avoids degenerate one-child stacks in the tree.
fn collapse_vstack(mut col: Vec<Slot>) -> Option<Slot> {
    match col.len() {
        0 => None,
        1 => col.pop(),
        _ => Some(Slot::VStack(col)),
    }
}

// ─────────────────────────────────────────────────────────────────
// Place — assign x/y/w/h to every widget reachable from the slot
// ─────────────────────────────────────────────────────────────────

fn place(slot: &Slot, area: Rect, ctx: &PlaceCtx, layout: &mut Layout) {
    match slot {
        Slot::Widget(kind) => {
            // Skip GPU widgets whose index is beyond the detected
            // count. `build_slot` already filters, but defensive
            // here so the engine remains correct if a future caller
            // hands us a tree directly without going through
            // `build_slot`.
            if let WidgetKind::Gpu(n) = kind
                && (*n as usize) >= ctx.hints.gpu_count
            {
                return;
            }
            // Apply per-widget width floor. In normal terminals the
            // floor is below the allocated area width and this is a
            // no-op; in pathologically narrow allocations the
            // widget overflows its column rather than truncating
            // its content. Matches the legacy `width.max(MIN_*_WIDTH)`
            // calls in the old place_left_column.
            let width = area.width.max(widget_min_width(*kind, ctx.hints));
            layout.set(
                *kind,
                WidgetDimensions {
                    x: area.x,
                    y: area.y,
                    width,
                    height: area.height,
                },
            );
        }
        Slot::VStack(children) => {
            let heights = vstack_distribute_heights(children, ctx, area.height);
            let mut y = area.y;
            for (child, child_h) in children.iter().zip(heights.iter()) {
                place(
                    child,
                    Rect {
                        x: area.x,
                        y,
                        width: area.width,
                        height: *child_h,
                    },
                    ctx,
                    layout,
                );
                y += child_h;
            }
        }
        Slot::HStack(children) => {
            let widths = hstack_distribute_widths(children, area.width);
            let mut x = area.x;
            for (child, child_w) in children.iter().zip(widths.iter()) {
                place(
                    &child.slot,
                    Rect {
                        x,
                        y: area.y,
                        width: *child_w,
                        height: area.height,
                    },
                    ctx,
                    layout,
                );
                x += child_w;
            }
        }
    }
}

/// Distribute a VStack's vertical space across its children:
/// Preferred children get their preferred height; Fill children
/// share the remainder equally with rounding leftover going to the
/// earliest-listed Fill child (so the total matches `total` exactly).
fn vstack_distribute_heights(children: &[Slot], ctx: &PlaceCtx, total: usize) -> Vec<usize> {
    let mut heights = vec![0usize; children.len()];
    let mut sum_preferred = 0usize;
    let mut fill_indices: Vec<usize> = Vec::new();
    for (i, child) in children.iter().enumerate() {
        if slot_is_fill(child) {
            fill_indices.push(i);
        } else {
            heights[i] = slot_preferred_height(child, ctx);
            sum_preferred += heights[i];
        }
    }
    if !fill_indices.is_empty() {
        let remaining = total.saturating_sub(sum_preferred);
        let per_fill = remaining / fill_indices.len();
        let leftover = remaining % fill_indices.len();
        for (k, &i) in fill_indices.iter().enumerate() {
            heights[i] = per_fill + if k < leftover { 1 } else { 0 };
        }
    }
    heights
}

/// Distribute an HStack's horizontal space across its children
/// proportionally to weights. The last child absorbs rounding
/// leftover so the widths sum to exactly `total`.
fn hstack_distribute_widths(children: &[HStackChild], total: usize) -> Vec<usize> {
    if children.is_empty() {
        return Vec::new();
    }
    let total_weight: usize = children.iter().map(|c| c.weight as usize).sum();
    if total_weight == 0 {
        // Defensive: equal split if every weight is zero.
        let per = total / children.len();
        let mut widths = vec![per; children.len()];
        widths[children.len() - 1] += total - per * children.len();
        return widths;
    }
    let last = children.len() - 1;
    let mut widths = Vec::with_capacity(children.len());
    let mut allocated = 0usize;
    for (i, child) in children.iter().enumerate() {
        let w = if i == last {
            total.saturating_sub(allocated)
        } else {
            total * child.weight as usize / total_weight
        };
        widths.push(w);
        allocated += w;
    }
    widths
}

// ─────────────────────────────────────────────────────────────────
// Sizing — slot-level aggregation built on per-widget queries
// ─────────────────────────────────────────────────────────────────

/// Whether a slot has any descendant `Fill` widget. A slot with at
/// least one Fill descendant absorbs slack; otherwise it has a
/// fixed preferred height.
fn slot_is_fill(slot: &Slot) -> bool {
    match slot {
        Slot::Widget(kind) => matches!(kind.sizing(), WidgetSizing::Fill),
        Slot::VStack(children) => children.iter().any(slot_is_fill),
        Slot::HStack(children) => children.iter().any(|c| slot_is_fill(&c.slot)),
    }
}

/// Preferred height of a Preferred slot. (Calling on a Fill slot
/// returns the sum/max of its children's preferred heights, which
/// is meaningful as a lower bound but not the actual rendered
/// height — Fill slots get their heights from the parent's
/// `vstack_distribute_heights`.)
fn slot_preferred_height(slot: &Slot, ctx: &PlaceCtx) -> usize {
    match slot {
        Slot::Widget(kind) => widget_preferred_height(*kind, ctx),
        Slot::VStack(children) => children.iter().map(|c| slot_preferred_height(c, ctx)).sum(),
        Slot::HStack(children) => children
            .iter()
            .map(|c| slot_preferred_height(&c.slot, ctx))
            .max()
            .unwrap_or(0),
    }
}

/// Preferred height of a single widget kind, with per-widget
/// container-relative clamps applied (currently only CPU's `1/3 of
/// terminal height` cap).
fn widget_preferred_height(kind: WidgetKind, ctx: &PlaceCtx) -> usize {
    let raw = match kind {
        WidgetKind::Cpu => crate::ui::cpu_widget::preferred_height(ctx.hints).max(MIN_CPU_HEIGHT),
        WidgetKind::Mem => crate::ui::mem_widget::preferred_height(ctx.hints),
        WidgetKind::Disk => crate::ui::disk_widget::preferred_height(ctx.hints),
        WidgetKind::Gpu(_) => crate::ui::gpu_widget::preferred_height(),
        // Fill widgets shouldn't be queried for preferred height
        // during normal placement (`vstack_distribute_heights` only
        // calls this on non-Fill slots). If the call happens
        // anyway — e.g. as a min-size estimate inside a parent
        // VStack that's itself Fill — return their min height.
        WidgetKind::Net => MIN_NET_HEIGHT,
        WidgetKind::Proc => MIN_PROC_HEIGHT,
    };
    match kind {
        WidgetKind::Cpu => raw.clamp(MIN_CPU_HEIGHT, (ctx.term_height / 3).max(MIN_CPU_HEIGHT)),
        _ => raw,
    }
}

// ─────────────────────────────────────────────────────────────────
// Min size — smallest terminal that fits the slot tree
// ─────────────────────────────────────────────────────────────────

/// Compute the `(min_width, min_height)` of a slot tree. Bottom-up
/// recursion: leaves report their per-widget minimums; containers
/// aggregate (VStack: max width, sum height; HStack: weight-aware
/// min width, max height).
fn slot_min_size(slot: &Slot, ctx: &MinCtx) -> (usize, usize) {
    match slot {
        Slot::Widget(kind) => (
            widget_min_width(*kind, ctx.hints),
            widget_min_height(*kind, ctx.hints),
        ),
        Slot::VStack(children) => {
            let mut max_w = 0usize;
            let mut sum_h = 0usize;
            for child in children {
                let (cw, ch) = slot_min_size(child, ctx);
                max_w = max_w.max(cw);
                sum_h += ch;
            }
            (max_w, sum_h)
        }
        Slot::HStack(children) => {
            let total_weight: usize = children
                .iter()
                .map(|c| c.weight as usize)
                .sum::<usize>()
                .max(1);
            let mut max_min_w = 0usize;
            let mut max_h = 0usize;
            for child in children {
                let (cw, ch) = slot_min_size(&child.slot, ctx);
                let weight = (child.weight as usize).max(1);
                // For a child with min width `cw` and weight share
                // `weight / total_weight`, the HStack must be at
                // least `cw * total_weight / weight` wide
                // (rounded up).
                let needed_total = cw.saturating_mul(total_weight).div_ceil(weight);
                max_min_w = max_min_w.max(needed_total);
                max_h = max_h.max(ch);
            }
            (max_min_w, max_h)
        }
    }
}

fn widget_min_width(kind: WidgetKind, hints: &LayoutHints) -> usize {
    match kind {
        WidgetKind::Cpu => crate::ui::cpu_widget::min_width(hints),
        WidgetKind::Net => MIN_NET_WIDTH,
        WidgetKind::Proc => MIN_PROC_WIDTH,
        WidgetKind::Mem | WidgetKind::Disk | WidgetKind::Gpu(_) => MIN_MEM_WIDTH,
    }
}

fn widget_min_height(kind: WidgetKind, hints: &LayoutHints) -> usize {
    match kind {
        WidgetKind::Cpu => crate::ui::cpu_widget::preferred_height(hints).max(MIN_CPU_HEIGHT),
        WidgetKind::Mem => crate::ui::mem_widget::preferred_height(hints),
        WidgetKind::Disk => crate::ui::disk_widget::preferred_height(hints),
        WidgetKind::Gpu(_) => crate::ui::gpu_widget::preferred_height(),
        WidgetKind::Net => MIN_NET_HEIGHT,
        WidgetKind::Proc => MIN_PROC_HEIGHT,
    }
}

/// Whether `target` appears anywhere in the slot tree. Used by
/// `min_terminal_size` to apply CPU's terminal-height-relative
/// clamp post-recursion.
fn has_widget(slot: &Slot, target: WidgetKind) -> bool {
    match slot {
        Slot::Widget(kind) => *kind == target,
        Slot::VStack(children) => children.iter().any(|c| has_widget(c, target)),
        Slot::HStack(children) => children.iter().any(|c| has_widget(&c.slot, target)),
    }
}

// ─────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────

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
            stack_vertical: false,
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
    /// override callback.
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

    // ────────────────────────────────────────────────────────────
    // calc_sizes — preset shapes
    // ────────────────────────────────────────────────────────────

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
    fn calc_sizes_cpu_full_width_other_widgets_in_columns() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
        ]);
        let layout = calc_sizes(&lc(120, 40, &b));
        let cpu = layout.dims_for(WidgetKind::Cpu).unwrap();
        let mem = layout.dims_for(WidgetKind::Mem).unwrap();
        let proc = layout.dims_for(WidgetKind::Proc).unwrap();
        assert_eq!(cpu.x, 0);
        assert_eq!(cpu.width, 120);
        // Mem and proc are side-by-side below CPU.
        assert_eq!(mem.x, 0);
        assert!(proc.x > 0);
        assert_eq!(mem.x + mem.width, proc.x);
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
        assert!(proc_x < mem_x);
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
        // Should not panic on tiny terminals; widgets may have
        // unusable dims but the engine returns a Layout.
        let _ = calc_sizes(&lc_with_hints(10, 5, &b, |h| h.core_count = 2));
    }

    #[test]
    fn calc_sizes_proc_only() {
        let b = widgets(&[WidgetKind::Proc]);
        let layout = calc_sizes(&lc(100, 30, &b));
        let proc = layout.dims_for(WidgetKind::Proc).unwrap();
        // Proc spans the full terminal when alone.
        assert_eq!(proc.x, 0);
        assert_eq!(proc.y, 0);
        assert_eq!(proc.width, 100);
        assert_eq!(proc.height, 30);
    }

    #[test]
    fn calc_sizes_no_disk() {
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

    #[test]
    fn calc_sizes_widths_sum_to_terminal_width_in_two_column_layout() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
        ]);
        let layout = calc_sizes(&lc(120, 40, &b));
        let mem = layout.dims_for(WidgetKind::Mem).unwrap();
        let proc = layout.dims_for(WidgetKind::Proc).unwrap();
        assert_eq!(mem.width + proc.width, 120);
    }

    /// Regression: sparse GPU layouts preserve `WidgetKind::Gpu(n)`
    /// identity end-to-end so the renderer pulls the right device
    /// data for each placed slot.
    #[test]
    fn calc_sizes_sparse_gpu_layout_preserves_indices() {
        let b = widgets(&[WidgetKind::Cpu, WidgetKind::Gpu(1), WidgetKind::Gpu(3)]);
        let layout = calc_sizes(&lc_with_hints(120, 50, &b, |h| {
            h.core_count = 8;
            h.gpu_count = 4;
        }));
        assert!(layout.dims_for(WidgetKind::Gpu(0)).is_none());
        assert!(layout.dims_for(WidgetKind::Gpu(1)).is_some());
        assert!(layout.dims_for(WidgetKind::Gpu(2)).is_none());
        assert!(layout.dims_for(WidgetKind::Gpu(3)).is_some());
        // Lower index renders above higher index in declaration order.
        let gpu1 = layout.dims_for(WidgetKind::Gpu(1)).unwrap();
        let gpu3 = layout.dims_for(WidgetKind::Gpu(3)).unwrap();
        assert!(gpu1.y < gpu3.y);
    }

    #[test]
    fn calc_sizes_skips_gpu_indices_beyond_detected_count() {
        let b = widgets(&[WidgetKind::Cpu, WidgetKind::Gpu(0), WidgetKind::Gpu(5)]);
        let layout = calc_sizes(&lc_with_hints(120, 50, &b, |h| {
            h.core_count = 8;
            h.gpu_count = 1;
        }));
        assert!(layout.dims_for(WidgetKind::Gpu(0)).is_some());
        assert!(layout.dims_for(WidgetKind::Gpu(5)).is_none());
    }

    // ────────────────────────────────────────────────────────────
    // stack_vertical (single-column body)
    // ────────────────────────────────────────────────────────────

    fn lc_stacked<'a>(tw: usize, th: usize, shown: &'a [WidgetKind]) -> LayoutConfig<'a> {
        LayoutConfig {
            stack_vertical: true,
            ..lc(tw, th, shown)
        }
    }

    #[test]
    fn stack_vertical_mem_proc_stacks_proc_below_mem_full_width() {
        let b = widgets(&[WidgetKind::Mem, WidgetKind::Proc]);
        let layout = calc_sizes(&lc_stacked(120, 40, &b));
        let mem = layout.dims_for(WidgetKind::Mem).unwrap();
        let proc = layout.dims_for(WidgetKind::Proc).unwrap();
        let mem_pref = crate::ui::mem_widget::preferred_height(&LayoutHints::default());
        assert_eq!(mem.x, 0);
        assert_eq!(mem.y, 0);
        assert_eq!(mem.width, 120);
        assert_eq!(mem.height, mem_pref);
        assert_eq!(proc.x, 0);
        assert_eq!(proc.y, mem_pref);
        assert_eq!(proc.width, 120);
        assert_eq!(proc.height, 40 - mem_pref);
    }

    #[test]
    fn stack_vertical_disk_proc_stacks_proc_below_disk_full_width() {
        let b = widgets(&[WidgetKind::Disk, WidgetKind::Proc]);
        let cfg = LayoutConfig {
            stack_vertical: true,
            ..lc_with_hints(120, 40, &b, |h| {
                h.disk_count = 2;
                h.disk_rows_per_unit = 2;
            })
        };
        let layout = calc_sizes(&cfg);
        let disk = layout.dims_for(WidgetKind::Disk).unwrap();
        let proc = layout.dims_for(WidgetKind::Proc).unwrap();
        let disk_pref = crate::ui::disk_widget::preferred_height(&cfg.hints);
        assert_eq!(disk.x, 0);
        assert_eq!(disk.y, 0);
        assert_eq!(disk.width, 120);
        assert_eq!(disk.height, disk_pref);
        assert_eq!(proc.x, 0);
        assert_eq!(proc.y, disk_pref);
        assert_eq!(proc.width, 120);
        assert_eq!(proc.height, 40 - disk_pref);
    }

    #[test]
    fn stack_vertical_cpu_mem_disk_no_proc_stacks_under_cpu() {
        let b = widgets(&[WidgetKind::Cpu, WidgetKind::Mem, WidgetKind::Disk]);
        let cfg = LayoutConfig {
            stack_vertical: true,
            ..lc_with_hints(120, 60, &b, |h| {
                h.core_count = 8;
                h.disk_count = 2;
                h.disk_rows_per_unit = 2;
            })
        };
        let layout = calc_sizes(&cfg);
        let cpu = layout.dims_for(WidgetKind::Cpu).unwrap();
        let mem = layout.dims_for(WidgetKind::Mem).unwrap();
        let disk = layout.dims_for(WidgetKind::Disk).unwrap();
        let mem_pref = crate::ui::mem_widget::preferred_height(&cfg.hints);
        let disk_pref = crate::ui::disk_widget::preferred_height(&cfg.hints);
        assert_eq!(cpu.x, 0);
        assert_eq!(cpu.y, 0);
        assert_eq!(cpu.width, 120);
        assert_eq!(mem.x, 0);
        assert_eq!(mem.y, cpu.height);
        assert_eq!(mem.width, 120);
        assert_eq!(mem.height, mem_pref);
        assert_eq!(disk.x, 0);
        assert_eq!(disk.y, cpu.height + mem_pref);
        assert_eq!(disk.width, 120);
        assert_eq!(disk.height, disk_pref);
    }

    #[test]
    fn stack_vertical_cpu_gpu_proc_stacks_each_gpu_at_preferred_height() {
        let b = widgets(&[
            WidgetKind::Cpu,
            WidgetKind::Gpu(0),
            WidgetKind::Gpu(1),
            WidgetKind::Proc,
        ]);
        let cfg = LayoutConfig {
            stack_vertical: true,
            ..lc_with_hints(160, 60, &b, |h| {
                h.core_count = 8;
                h.gpu_count = 2;
            })
        };
        let layout = calc_sizes(&cfg);
        let gpu_pref = crate::ui::gpu_widget::preferred_height();
        let cpu = layout.dims_for(WidgetKind::Cpu).unwrap();
        let g0 = layout.dims_for(WidgetKind::Gpu(0)).unwrap();
        let g1 = layout.dims_for(WidgetKind::Gpu(1)).unwrap();
        let proc = layout.dims_for(WidgetKind::Proc).unwrap();
        assert_eq!(cpu.x, 0);
        assert_eq!(cpu.width, 160);
        assert_eq!(g0.x, 0);
        assert_eq!(g0.y, cpu.height);
        assert_eq!(g0.width, 160);
        assert_eq!(g0.height, gpu_pref);
        assert_eq!(g1.x, 0);
        assert_eq!(g1.y, cpu.height + gpu_pref);
        assert_eq!(g1.height, gpu_pref);
        assert_eq!(proc.x, 0);
        assert_eq!(proc.y, cpu.height + 2 * gpu_pref);
        assert_eq!(proc.width, 160);
        assert_eq!(proc.height, 60 - cpu.height - 2 * gpu_pref);
    }

    // ────────────────────────────────────────────────────────────
    // min_terminal_size
    // ────────────────────────────────────────────────────────────

    #[test]
    fn min_terminal_size_proc_only_uses_proc_minimums() {
        let b = widgets(&[WidgetKind::Proc]);
        let (w, h) = min_terminal_size(&lc(0, 0, &b));
        assert_eq!(w, MIN_PROC_WIDTH);
        assert_eq!(h, MIN_PROC_HEIGHT);
    }

    #[test]
    fn min_terminal_size_left_only_uses_widest_left_widget() {
        let b = widgets(&[WidgetKind::Net]);
        let (w, _) = min_terminal_size(&lc(0, 0, &b));
        assert_eq!(w, MIN_NET_WIDTH);

        let b = widgets(&[WidgetKind::Mem, WidgetKind::Net]);
        let (w, _) = min_terminal_size(&lc(0, 0, &b));
        assert_eq!(w, MIN_MEM_WIDTH);
    }

    #[test]
    fn min_terminal_size_two_columns_satisfies_pct_split() {
        let b = widgets(&[WidgetKind::Mem, WidgetKind::Proc]);
        let (w, _) = min_terminal_size(&lc(0, 0, &b));
        // Both columns must fit at their respective minimums under
        // the proc/left percentage split.
        assert!(w * PROC_WIDTH_PCT / 100 >= MIN_PROC_WIDTH);
        assert!(w - w * PROC_WIDTH_PCT / 100 >= MIN_MEM_WIDTH);
    }

    #[test]
    fn min_terminal_size_height_sums_left_plus_proc_when_stacked() {
        let b = widgets(&[WidgetKind::Mem, WidgetKind::Proc]);
        let cfg = lc_stacked(0, 0, &b);
        let (_, h) = min_terminal_size(&cfg);
        let mem_pref = crate::ui::mem_widget::preferred_height(&cfg.hints);
        assert_eq!(h, mem_pref + MIN_PROC_HEIGHT);
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

    #[test]
    fn min_terminal_size_includes_cpu_clamp_constraint() {
        // Terminal must be >= 3 * cpu_preferred so CPU can render
        // at its preferred height under the term_height/3 clamp.
        let b = widgets(&[WidgetKind::Cpu, WidgetKind::Proc]);
        let (_, h) = min_terminal_size(&lc(0, 0, &b));
        let cpu_pref =
            crate::ui::cpu_widget::preferred_height(&LayoutHints::default()).max(MIN_CPU_HEIGHT);
        assert!(h >= 3 * cpu_pref);
    }

    // ────────────────────────────────────────────────────────────
    // hstack_distribute_widths
    // ────────────────────────────────────────────────────────────

    #[test]
    fn hstack_distribute_widths_sum_equals_total() {
        let children = vec![
            HStackChild {
                slot: Slot::Widget(WidgetKind::Mem),
                weight: 40,
            },
            HStackChild {
                slot: Slot::Widget(WidgetKind::Proc),
                weight: 60,
            },
        ];
        for total in [10usize, 50, 100, 113, 1000] {
            let widths = hstack_distribute_widths(&children, total);
            let sum: usize = widths.iter().sum();
            assert_eq!(sum, total, "sum != total for total={total}");
        }
    }

    #[test]
    fn hstack_distribute_widths_proportional_to_weights() {
        let children = vec![
            HStackChild {
                slot: Slot::Widget(WidgetKind::Mem),
                weight: 1,
            },
            HStackChild {
                slot: Slot::Widget(WidgetKind::Proc),
                weight: 3,
            },
        ];
        let widths = hstack_distribute_widths(&children, 100);
        assert_eq!(widths[0], 25);
        assert_eq!(widths[1], 75);
    }

    // ────────────────────────────────────────────────────────────
    // vstack_distribute_heights
    // ────────────────────────────────────────────────────────────

    #[test]
    fn vstack_distribute_heights_preferred_get_preferred_fill_gets_rest() {
        let children = vec![
            Slot::Widget(WidgetKind::Mem),  // Preferred: pref = 6
            Slot::Widget(WidgetKind::Proc), // Fill
        ];
        let ctx = PlaceCtx {
            hints: &LayoutHints::default(),
            term_height: 100,
        };
        let heights = vstack_distribute_heights(&children, &ctx, 30);
        let mem_pref = crate::ui::mem_widget::preferred_height(&LayoutHints::default());
        assert_eq!(heights[0], mem_pref);
        assert_eq!(heights[1], 30 - mem_pref);
        assert_eq!(heights.iter().sum::<usize>(), 30);
    }

    #[test]
    fn vstack_distribute_heights_multiple_fills_share_equally() {
        let children = vec![
            Slot::Widget(WidgetKind::Net),  // Fill
            Slot::Widget(WidgetKind::Proc), // Fill
        ];
        let ctx = PlaceCtx {
            hints: &LayoutHints::default(),
            term_height: 100,
        };
        let heights = vstack_distribute_heights(&children, &ctx, 30);
        assert_eq!(heights, vec![15, 15]);
    }

    #[test]
    fn vstack_distribute_heights_no_fill_leaves_remainder_empty() {
        let children = vec![
            Slot::Widget(WidgetKind::Mem),  // Preferred: pref = 6
            Slot::Widget(WidgetKind::Disk), // Preferred
        ];
        let ctx = PlaceCtx {
            hints: &LayoutHints {
                disk_count: 2,
                disk_rows_per_unit: 2,
                ..Default::default()
            },
            term_height: 100,
        };
        let heights = vstack_distribute_heights(&children, &ctx, 30);
        let mem_pref = crate::ui::mem_widget::preferred_height(ctx.hints);
        let disk_pref = crate::ui::disk_widget::preferred_height(ctx.hints);
        assert_eq!(heights, vec![mem_pref, disk_pref]);
        // Leftover space (30 - mem_pref - disk_pref) is intentionally
        // unallocated when no Fill widget is present.
        assert!(heights.iter().sum::<usize>() < 30);
    }
}
