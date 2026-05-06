//! Layout engine: turns a `LayoutConfig` (terminal size + hints + a
//! [`Slot`] tree) into per-widget rectangles in [`Layout`].
//!
//! The engine is a recursive walk over the [`Slot`] tree
//! ([`Slot::VStack`] / [`Slot::HStack`] / [`Slot::Widget`] leaves).
//! It never special-cases any widget by name — widget-specific rules
//! (slack absorption, the CPU height clamp) are intrinsic properties
//! of [`WidgetKind`] consulted uniformly.
//!
//! Visibility — invisible widgets (currently only `Gpu(n)` where `n
//! >= hints.gpu_count`) contribute zero to every size aggregation
//! and are skipped at placement time. Containers redistribute the
//! freed space across their visible siblings. Builtin presets can
//! therefore declare static `Slot` trees that include all GPU slots
//! and rely on the engine to fold absent devices out, regardless of
//! where a `Gpu(n)` leaf appears in the tree.

use crate::domain::layout_spec::{HStackChild, Slot};
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

/// Configuration for layout calculation.
pub struct LayoutConfig {
    pub term_width: usize,
    pub term_height: usize,
    /// Root of the slot tree to render. Built per frame from
    /// `Config::layout_spec()`.
    pub root: Slot,
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
/// inputs (term smaller than 2x2).
pub fn calc_sizes(cfg: &LayoutConfig) -> Layout {
    let mut layout = Layout::default();
    if cfg.term_width < 2 || cfg.term_height < 2 {
        return layout;
    }
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
    place(&cfg.root, area, &ctx, &mut layout);
    layout
}

/// Smallest terminal size at which the active layout fits without
/// truncation, given the current slot tree and snapshot hints.
///
/// Returns the `(width, height)` shown in the "Terminal too small.
/// Need WxH." message, and the value used by the `is_too_small`
/// gate in the event loop.
pub fn min_terminal_size(cfg: &LayoutConfig) -> (usize, usize) {
    let ctx = MinCtx { hints: &cfg.hints };
    let (min_w, mut min_h) = slot_min_size(&cfg.root, &ctx);
    // CPU's preferred-height clamp (`min(preferred, term_height/3)`)
    // means the terminal must be at least three times CPU's
    // preferred height for CPU to render at its preferred size
    // without being clamped. Encoded here at the top level rather
    // than inside the recursion because the clamp is anchored to
    // *terminal* height, not the immediate parent container.
    if cfg.root.contains(WidgetKind::Cpu) {
        let cpu_pref = crate::ui::cpu_widget::preferred_height(&cfg.hints).max(MIN_CPU_HEIGHT);
        min_h = min_h.max(3 * cpu_pref);
    }
    (min_w, min_h)
}

// ─────────────────────────────────────────────────────────────────
// Internal types
// ─────────────────────────────────────────────────────────────────

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
// Place — assign x/y/w/h to every widget reachable from the slot
// ─────────────────────────────────────────────────────────────────

fn place(slot: &Slot, area: Rect, ctx: &PlaceCtx, layout: &mut Layout) {
    match slot {
        Slot::Widget(kind) => {
            if !widget_is_visible(*kind, ctx.hints) {
                return;
            }
            // Apply per-widget width floor. In normal terminals the
            // floor is below the allocated area width and this is a
            // no-op; in pathologically narrow allocations the
            // widget overflows its column rather than truncating
            // its content.
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
            let widths = hstack_distribute_widths(children, ctx.hints, area.width);
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
/// invisible children get 0; `Preferred` children get their
/// preferred height; `Fill` children share the remainder equally
/// with rounding leftover going to the earliest-listed `Fill`
/// child (so the heights sum to exactly `total`).
fn vstack_distribute_heights(children: &[Slot], ctx: &PlaceCtx, total: usize) -> Vec<usize> {
    let mut heights = vec![0usize; children.len()];
    let mut sum_preferred = 0usize;
    let mut fill_indices: Vec<usize> = Vec::new();
    for (i, child) in children.iter().enumerate() {
        if !slot_is_visible(child, ctx.hints) {
            continue;
        }
        if slot_is_fill(child, ctx.hints) {
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
/// proportionally to weights. Invisible children get 0 and their
/// weight is excluded from the denominator. The last visible child
/// absorbs rounding leftover so the widths sum to exactly `total`.
fn hstack_distribute_widths(
    children: &[HStackChild],
    hints: &LayoutHints,
    total: usize,
) -> Vec<usize> {
    let mut widths = vec![0usize; children.len()];
    let visible: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| slot_is_visible(&c.slot, hints))
        .map(|(i, _)| i)
        .collect();
    if visible.is_empty() {
        return widths;
    }
    let total_weight: usize = visible
        .iter()
        .map(|&i| children[i].weight.get() as usize)
        .sum();
    let last = *visible.last().expect("visible is non-empty");
    let mut allocated = 0usize;
    for &i in &visible {
        let w = if i == last {
            total.saturating_sub(allocated)
        } else {
            total * children[i].weight.get() as usize / total_weight
        };
        widths[i] = w;
        allocated += w;
    }
    widths
}

// ─────────────────────────────────────────────────────────────────
// Sizing — slot-level aggregation built on per-widget queries
// ─────────────────────────────────────────────────────────────────

/// Whether a slot has any visible descendant `Fill` widget.
/// Invisible subtrees contribute nothing — a `Fill` widget that
/// won't render this frame must not claim slack.
fn slot_is_fill(slot: &Slot, hints: &LayoutHints) -> bool {
    match slot {
        Slot::Widget(kind) => {
            widget_is_visible(*kind, hints) && matches!(kind.sizing(), WidgetSizing::Fill)
        }
        Slot::VStack(children) => children.iter().any(|c| slot_is_fill(c, hints)),
        Slot::HStack(children) => children.iter().any(|c| slot_is_fill(&c.slot, hints)),
    }
}

/// Whether a slot will render anything this frame. A slot with no
/// visible leaves contributes zero to every aggregation and is
/// skipped at placement time.
fn slot_is_visible(slot: &Slot, hints: &LayoutHints) -> bool {
    match slot {
        Slot::Widget(kind) => widget_is_visible(*kind, hints),
        Slot::VStack(children) => children.iter().any(|c| slot_is_visible(c, hints)),
        Slot::HStack(children) => children.iter().any(|c| slot_is_visible(&c.slot, hints)),
    }
}

/// Preferred height of a Preferred slot. (Calling on a Fill slot
/// returns the sum/max of its children's preferred heights, which
/// is meaningful as a lower bound but not the actual rendered
/// height — Fill slots get their heights from the parent's
/// `vstack_distribute_heights`.) Invisible children contribute 0.
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
/// terminal height` cap). Returns 0 for invisible widgets.
fn widget_preferred_height(kind: WidgetKind, ctx: &PlaceCtx) -> usize {
    if !widget_is_visible(kind, ctx.hints) {
        return 0;
    }
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
/// aggregate (VStack: max width across visible children, sum height;
/// HStack: weight-aware min width across visible children, max
/// height). Invisible subtrees contribute zero.
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
                if !slot_is_visible(child, ctx.hints) {
                    continue;
                }
                let (cw, ch) = slot_min_size(child, ctx);
                max_w = max_w.max(cw);
                sum_h += ch;
            }
            (max_w, sum_h)
        }
        Slot::HStack(children) => {
            let visible: Vec<&HStackChild> = children
                .iter()
                .filter(|c| slot_is_visible(&c.slot, ctx.hints))
                .collect();
            if visible.is_empty() {
                return (0, 0);
            }
            let total_weight: usize = visible
                .iter()
                .map(|c| c.weight.get() as usize)
                .sum::<usize>();
            let mut max_min_w = 0usize;
            let mut max_h = 0usize;
            for child in visible {
                let (cw, ch) = slot_min_size(&child.slot, ctx);
                let weight = child.weight.get() as usize;
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
    if !widget_is_visible(kind, hints) {
        return 0;
    }
    match kind {
        WidgetKind::Cpu => crate::ui::cpu_widget::min_width(hints),
        WidgetKind::Net => MIN_NET_WIDTH,
        WidgetKind::Proc => MIN_PROC_WIDTH,
        WidgetKind::Mem | WidgetKind::Disk | WidgetKind::Gpu(_) => MIN_MEM_WIDTH,
    }
}

fn widget_min_height(kind: WidgetKind, hints: &LayoutHints) -> usize {
    if !widget_is_visible(kind, hints) {
        return 0;
    }
    match kind {
        WidgetKind::Cpu => crate::ui::cpu_widget::preferred_height(hints).max(MIN_CPU_HEIGHT),
        WidgetKind::Mem => crate::ui::mem_widget::preferred_height(hints),
        WidgetKind::Disk => crate::ui::disk_widget::preferred_height(hints),
        WidgetKind::Gpu(_) => crate::ui::gpu_widget::preferred_height(),
        WidgetKind::Net => MIN_NET_HEIGHT,
        WidgetKind::Proc => MIN_PROC_HEIGHT,
    }
}

/// Whether a widget kind renders this frame. Currently only
/// [`WidgetKind::Gpu`] varies — indices beyond `hints.gpu_count`
/// have no backing device. The other widgets are always visible
/// when present in the tree.
fn widget_is_visible(kind: WidgetKind, hints: &LayoutHints) -> bool {
    match kind {
        WidgetKind::Gpu(n) => (n as usize) < hints.gpu_count,
        _ => true,
    }
}

// ─────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU8;

    fn nz(n: u8) -> NonZeroU8 {
        NonZeroU8::new(n).expect("test weight must be non-zero")
    }

    fn hints() -> LayoutHints {
        LayoutHints {
            core_count: 4,
            gpu_count: 0,
            disk_count: 2,
            has_swap: false,
            has_cpu_temp: false,
            has_cpu_watts: false,
            disk_rows_per_unit: 2,
        }
    }

    fn lc(tw: usize, th: usize, root: Slot) -> LayoutConfig {
        LayoutConfig {
            term_width: tw,
            term_height: th,
            root,
            hints: hints(),
        }
    }

    fn lc_with_hints(
        tw: usize,
        th: usize,
        root: Slot,
        f: impl FnOnce(&mut LayoutHints),
    ) -> LayoutConfig {
        let mut cfg = lc(tw, th, root);
        f(&mut cfg.hints);
        cfg
    }

    fn two_col(left: Slot, right: Slot) -> Slot {
        Slot::HStack(vec![
            HStackChild::new(left, nz(40)),
            HStackChild::new(right, nz(60)),
        ])
    }

    // ────────────────────────────────────────────────────────────
    // place — top-level shapes
    // ────────────────────────────────────────────────────────────

    #[test]
    fn calc_sizes_single_widget_fills_terminal() {
        let layout = calc_sizes(&lc(100, 30, Slot::Widget(WidgetKind::Proc)));
        let proc = layout.dims_for(WidgetKind::Proc).expect("proc placed");
        assert_eq!(proc.x, 0);
        assert_eq!(proc.y, 0);
        assert_eq!(proc.width, 100);
        assert_eq!(proc.height, 30);
    }

    #[test]
    fn calc_sizes_returns_empty_layout_for_too_small_terminal() {
        let layout = calc_sizes(&lc(1, 30, Slot::Widget(WidgetKind::Proc)));
        assert!(layout.dims_for(WidgetKind::Proc).is_none());
        let layout = calc_sizes(&lc(100, 1, Slot::Widget(WidgetKind::Proc)));
        assert!(layout.dims_for(WidgetKind::Proc).is_none());
    }

    #[test]
    fn calc_sizes_cpu_full_width_then_two_columns() {
        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            two_col(
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Proc),
            ),
        ]);
        let layout = calc_sizes(&lc_with_hints(120, 40, root, |h| h.core_count = 8));
        let cpu = layout.dims_for(WidgetKind::Cpu).expect("cpu placed");
        let mem = layout.dims_for(WidgetKind::Mem).expect("mem placed");
        let proc = layout.dims_for(WidgetKind::Proc).expect("proc placed");
        assert_eq!(cpu.x, 0);
        assert_eq!(cpu.y, 0);
        assert_eq!(cpu.width, 120);
        // Mem and proc are side-by-side below CPU.
        assert_eq!(mem.x, 0);
        assert_eq!(mem.y, cpu.height);
        assert!(proc.x > 0);
        assert_eq!(mem.x + mem.width, proc.x);
        assert_eq!(mem.width + proc.width, 120);
    }

    #[test]
    fn calc_sizes_vstack_cpu_top_vs_cpu_bottom() {
        let cpu_top = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
        ]);
        let cpu_bot = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Cpu),
        ]);
        let layout_top = calc_sizes(&lc(80, 40, cpu_top));
        let layout_bot = calc_sizes(&lc(80, 40, cpu_bot));
        assert!(
            layout_top.dims_for(WidgetKind::Cpu).expect("cpu placed").y
                < layout_bot.dims_for(WidgetKind::Cpu).expect("cpu placed").y
        );
    }

    #[test]
    fn calc_sizes_hstack_proc_on_left_when_listed_first() {
        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            two_col(
                Slot::Widget(WidgetKind::Proc),
                Slot::Widget(WidgetKind::Mem),
            ),
        ]);
        let layout = calc_sizes(&lc(120, 40, root));
        let proc_x = layout.dims_for(WidgetKind::Proc).expect("proc placed").x;
        let mem_x = layout.dims_for(WidgetKind::Mem).expect("mem placed").x;
        assert!(proc_x < mem_x);
    }

    #[test]
    fn calc_sizes_widths_sum_to_terminal_width_in_hstack() {
        let root = two_col(
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Proc),
        );
        let layout = calc_sizes(&lc(120, 40, root));
        let mem = layout.dims_for(WidgetKind::Mem).expect("mem placed");
        let proc = layout.dims_for(WidgetKind::Proc).expect("proc placed");
        assert_eq!(mem.width + proc.width, 120);
    }

    // ────────────────────────────────────────────────────────────
    // VStack height distribution: Preferred vs Fill
    // ────────────────────────────────────────────────────────────

    #[test]
    fn vstack_preferred_then_fill_proc_absorbs_remainder() {
        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Proc),
        ]);
        let cfg = lc(120, 40, root);
        let mem_pref = crate::ui::mem_widget::preferred_height(&cfg.hints);
        let layout = calc_sizes(&cfg);
        let mem = layout.dims_for(WidgetKind::Mem).expect("mem placed");
        let proc = layout.dims_for(WidgetKind::Proc).expect("proc placed");
        assert_eq!(mem.height, mem_pref);
        assert_eq!(proc.height, 40 - mem_pref);
        assert_eq!(mem.y + mem.height, proc.y);
    }

    #[test]
    fn vstack_two_fill_siblings_share_remainder() {
        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Net),
            Slot::Widget(WidgetKind::Proc),
        ]);
        let cfg = lc(120, 40, root);
        let mem_pref = crate::ui::mem_widget::preferred_height(&cfg.hints);
        let layout = calc_sizes(&cfg);
        let net = layout.dims_for(WidgetKind::Net).expect("net placed");
        let proc = layout.dims_for(WidgetKind::Proc).expect("proc placed");
        let remainder = 40 - mem_pref;
        // Two Fill children share equally; rounding leftover goes to the first one (net).
        assert!(net.height + proc.height == remainder);
        assert!(net.height >= proc.height);
        assert!(net.height - proc.height <= 1);
    }

    #[test]
    fn vstack_only_preferred_children_leave_remainder_unallocated() {
        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Disk),
        ]);
        let cfg = lc(120, 40, root);
        let layout = calc_sizes(&cfg);
        let mem = layout.dims_for(WidgetKind::Mem).expect("mem placed");
        let disk = layout.dims_for(WidgetKind::Disk).expect("disk placed");
        assert_eq!(
            mem.height,
            crate::ui::mem_widget::preferred_height(&cfg.hints)
        );
        assert_eq!(
            disk.height,
            crate::ui::disk_widget::preferred_height(&cfg.hints)
        );
    }

    // ────────────────────────────────────────────────────────────
    // CPU height clamp (1/3 terminal height)
    // ────────────────────────────────────────────────────────────

    #[test]
    fn cpu_clamped_to_one_third_of_terminal_height() {
        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Proc),
        ]);
        // With many cores, CPU's preferred height is large. Clamp
        // to term_height/3 = 60/3 = 20.
        let cfg = lc_with_hints(200, 60, root, |h| h.core_count = 64);
        let layout = calc_sizes(&cfg);
        let cpu = layout.dims_for(WidgetKind::Cpu).expect("cpu placed");
        assert!(cpu.height <= 60 / 3);
    }

    // ────────────────────────────────────────────────────────────
    // GPU visibility — engine treats absent GPUs as zero-size
    // ────────────────────────────────────────────────────────────

    #[test]
    fn invisible_gpu_in_vstack_contributes_zero_height() {
        // Static tree includes Gpu(0..7); only 2 are detected. The
        // engine should treat Gpu(2..7) as 0-height and pack
        // Mem/Net/Disk into the remaining space.
        let mut left = Vec::new();
        for n in 0..8u8 {
            left.push(Slot::Widget(WidgetKind::Gpu(n)));
        }
        left.push(Slot::Widget(WidgetKind::Mem));
        left.push(Slot::Widget(WidgetKind::Disk));
        left.push(Slot::Widget(WidgetKind::Net));
        let cfg = lc_with_hints(120, 60, Slot::VStack(left), |h| {
            h.gpu_count = 2;
            h.core_count = 8;
        });
        let layout = calc_sizes(&cfg);
        let g0 = layout.dims_for(WidgetKind::Gpu(0)).expect("gpu0 placed");
        let g1 = layout.dims_for(WidgetKind::Gpu(1)).expect("gpu1 placed");
        let mem = layout.dims_for(WidgetKind::Mem).expect("mem placed");
        let gpu_pref = crate::ui::gpu_widget::preferred_height();
        assert_eq!(g0.y, 0);
        assert_eq!(g0.height, gpu_pref);
        assert_eq!(g1.y, gpu_pref);
        assert_eq!(g1.height, gpu_pref);
        // Mem follows immediately after the two visible GPUs (no
        // gap from invisible Gpu(2..7)).
        assert_eq!(mem.y, 2 * gpu_pref);
        // Invisible GPU slots aren't placed.
        for n in 2..8u8 {
            assert!(layout.dims_for(WidgetKind::Gpu(n)).is_none());
        }
    }

    #[test]
    fn invisible_gpu_in_hstack_yields_width_to_visible_sibling() {
        // HStack with [Gpu(5), Proc] when no GPUs are detected.
        // The Gpu child is invisible; Proc gets the full width.
        let root = Slot::HStack(vec![
            HStackChild::new(Slot::Widget(WidgetKind::Gpu(5)), nz(40)),
            HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
        ]);
        let cfg = lc_with_hints(100, 30, root, |h| h.gpu_count = 0);
        let layout = calc_sizes(&cfg);
        assert!(layout.dims_for(WidgetKind::Gpu(5)).is_none());
        let proc = layout.dims_for(WidgetKind::Proc).expect("proc placed");
        assert_eq!(proc.x, 0);
        assert_eq!(proc.width, 100);
    }

    #[test]
    fn sparse_gpu_layout_preserves_indices() {
        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Gpu(1)),
            Slot::Widget(WidgetKind::Gpu(3)),
        ]);
        let layout = calc_sizes(&lc_with_hints(120, 50, root, |h| {
            h.core_count = 8;
            h.gpu_count = 4;
        }));
        assert!(layout.dims_for(WidgetKind::Gpu(0)).is_none());
        assert!(layout.dims_for(WidgetKind::Gpu(1)).is_some());
        assert!(layout.dims_for(WidgetKind::Gpu(2)).is_none());
        assert!(layout.dims_for(WidgetKind::Gpu(3)).is_some());
        let gpu1 = layout.dims_for(WidgetKind::Gpu(1)).expect("gpu1 placed");
        let gpu3 = layout.dims_for(WidgetKind::Gpu(3)).expect("gpu3 placed");
        assert!(gpu1.y < gpu3.y);
    }

    #[test]
    fn invisible_only_tree_yields_empty_layout() {
        // Tree contains only invisible widgets — no terminal-too-small
        // triggers, but nothing renders either.
        let root = Slot::Widget(WidgetKind::Gpu(5));
        let cfg = lc_with_hints(120, 40, root, |h| h.gpu_count = 0);
        let layout = calc_sizes(&cfg);
        assert!(layout.dims_for(WidgetKind::Gpu(5)).is_none());
    }

    // ────────────────────────────────────────────────────────────
    // hstack_distribute_widths
    // ────────────────────────────────────────────────────────────

    #[test]
    fn hstack_distribute_widths_sum_equals_total() {
        let children = vec![
            HStackChild::new(Slot::Widget(WidgetKind::Mem), nz(40)),
            HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
        ];
        let h = hints();
        for total in [10usize, 50, 100, 113, 1000] {
            let widths = hstack_distribute_widths(&children, &h, total);
            let sum: usize = widths.iter().sum();
            assert_eq!(sum, total, "sum != total for total={total}");
        }
    }

    #[test]
    fn hstack_distribute_widths_skips_invisible_children() {
        let children = vec![
            HStackChild::new(Slot::Widget(WidgetKind::Gpu(5)), nz(40)),
            HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
        ];
        let mut h = hints();
        h.gpu_count = 0;
        let widths = hstack_distribute_widths(&children, &h, 100);
        assert_eq!(widths[0], 0);
        assert_eq!(widths[1], 100);
    }

    #[test]
    fn hstack_distribute_widths_all_invisible_yields_zeros() {
        let children = vec![
            HStackChild::new(Slot::Widget(WidgetKind::Gpu(5)), nz(40)),
            HStackChild::new(Slot::Widget(WidgetKind::Gpu(6)), nz(60)),
        ];
        let mut h = hints();
        h.gpu_count = 0;
        let widths = hstack_distribute_widths(&children, &h, 100);
        assert_eq!(widths, vec![0, 0]);
    }

    // ────────────────────────────────────────────────────────────
    // min_terminal_size
    // ────────────────────────────────────────────────────────────

    #[test]
    fn min_terminal_size_proc_only_uses_proc_minimums() {
        let (w, h) = min_terminal_size(&lc(0, 0, Slot::Widget(WidgetKind::Proc)));
        assert_eq!(w, MIN_PROC_WIDTH);
        assert_eq!(h, MIN_PROC_HEIGHT);
    }

    #[test]
    fn min_terminal_size_left_only_uses_widest_visible_widget() {
        let (w, _) = min_terminal_size(&lc(0, 0, Slot::Widget(WidgetKind::Net)));
        assert_eq!(w, MIN_NET_WIDTH);

        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Net),
        ]);
        let (w, _) = min_terminal_size(&lc(0, 0, root));
        assert_eq!(w, MIN_MEM_WIDTH);
    }

    #[test]
    fn min_terminal_size_hstack_satisfies_pct_split() {
        let root = two_col(
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Proc),
        );
        let (w, _) = min_terminal_size(&lc(0, 0, root));
        // Both columns must fit at their respective minimums under
        // the 40/60 weight split.
        assert!(w * 60 / 100 >= MIN_PROC_WIDTH);
        assert!(w - w * 60 / 100 >= MIN_MEM_WIDTH);
    }

    #[test]
    fn min_terminal_size_vstack_height_sums_visible_children() {
        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Proc),
        ]);
        let cfg = lc(0, 0, root);
        let (_, h) = min_terminal_size(&cfg);
        let mem_pref = crate::ui::mem_widget::preferred_height(&cfg.hints);
        assert_eq!(h, mem_pref + MIN_PROC_HEIGHT);
    }

    #[test]
    fn min_terminal_size_grows_with_more_disks_and_cores() {
        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            two_col(
                Slot::VStack(vec![
                    Slot::Widget(WidgetKind::Mem),
                    Slot::Widget(WidgetKind::Net),
                    Slot::Widget(WidgetKind::Disk),
                ]),
                Slot::Widget(WidgetKind::Proc),
            ),
        ]);
        let make = |cores, disks| {
            lc_with_hints(0, 0, root.clone(), |h| {
                h.core_count = cores;
                h.disk_count = disks;
                h.disk_rows_per_unit = 2;
            })
        };
        let small = min_terminal_size(&make(4, 1));
        let large = min_terminal_size(&make(32, 4));
        assert!(
            large.1 > small.1,
            "more cores/disks should require taller terminal: small={small:?}, large={large:?}",
        );
    }

    #[test]
    fn min_terminal_size_includes_cpu_clamp_constraint() {
        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Proc),
        ]);
        let (_, h) = min_terminal_size(&lc(0, 0, root));
        let cpu_pref =
            crate::ui::cpu_widget::preferred_height(&LayoutHints::default()).max(MIN_CPU_HEIGHT);
        assert!(h >= 3 * cpu_pref);
    }

    #[test]
    fn min_terminal_size_invisible_widgets_contribute_zero() {
        // Single-widget tree of an invisible Gpu reports (0, 0).
        let cfg = lc_with_hints(0, 0, Slot::Widget(WidgetKind::Gpu(5)), |h| h.gpu_count = 0);
        let (w, h) = min_terminal_size(&cfg);
        assert_eq!((w, h), (0, 0));
    }

    // ────────────────────────────────────────────────────────────
    // slot_is_fill
    // ────────────────────────────────────────────────────────────

    #[test]
    fn slot_is_fill_treats_invisible_fill_as_not_fill() {
        // No Fill widgets are currently invisible-able, but verify
        // the contract: an invisible Fill contributes nothing,
        // including not claiming a slack share.
        // Test via Gpu — Preferred — for shape coverage; the
        // semantic check is the visibility gate.
        let h = hints();
        assert!(!slot_is_fill(&Slot::Widget(WidgetKind::Gpu(5)), &h));
        // A Fill widget in a tree alongside an invisible widget is
        // still Fill (the tree contains a visible Fill).
        let mut h2 = h;
        h2.gpu_count = 0;
        let root = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Gpu(5)),
            Slot::Widget(WidgetKind::Proc),
        ]);
        assert!(slot_is_fill(&root, &h2));
    }
}
