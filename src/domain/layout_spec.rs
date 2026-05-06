//! `Slot` — the canonical layout specification.
//!
//! A `Slot` tree describes *where* widgets render: a recursive
//! composition of vertical stacks (`VStack`), horizontal stacks
//! (`HStack`), and widget leaves (`Widget`). The
//! [`crate::draw::layout`] engine consumes a `Slot` and produces a
//! [`crate::draw::layout::Layout`] of per-widget rectangles.
//!
//! Every widget is a uniform leaf in the tree. The engine never
//! special-cases by widget identity; per-widget intrinsic
//! properties live on [`crate::domain::widget_kind::WidgetKind`]
//! (`sizing()` for slack absorption, the per-widget `min_*` /
//! `preferred_*` helpers in `ui/`).
//!
//! Lives in `domain/` because:
//!  * Both `domain::preset` (the producer) and `draw::layout` (the
//!    consumer) reference it; per the architecture rules
//!    `domain/` may not depend on `draw/`, so the type must sit in
//!    or below `domain/`.
//!  * Future commits add a DSL parser/emitter and `Serialize` /
//!    `Deserialize` impls — those naturally belong next to the type.

use std::num::NonZeroU8;

use crate::domain::widget_kind::WidgetKind;

/// A rectangular region that holds either a widget, a vertical
/// stack of slots, or a horizontal stack of slots.
///
/// `Slot` is the canonical input to the layout engine. Builtin
/// presets and the user's custom layout both produce a `Slot`
/// tree which the engine walks recursively to assign rectangles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    /// A widget leaf. The widget's intrinsic
    /// [`crate::domain::widget_kind::WidgetSizing`] determines
    /// whether it absorbs slack along the parent axis.
    Widget(WidgetKind),
    /// Children stacked top-to-bottom, each spanning the full width
    /// of the container. Heights are distributed by the engine:
    /// `Preferred` children get their preferred height, `Fill`
    /// children share the remainder equally.
    VStack(Vec<Slot>),
    /// Children laid out left-to-right, each spanning the full
    /// height of the container. Widths are distributed
    /// proportionally to per-child weights.
    HStack(Vec<HStackChild>),
}

/// A child of an [`Slot::HStack`] carrying its relative width
/// weight.
///
/// Total available width is divided proportionally across visible
/// siblings: child `i` receives `total * weights[i] / sum(weights)`.
/// Weights are [`NonZeroU8`] so a zero share — which would either
/// allocate nothing visible to a child or produce a divide-by-zero —
/// is unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HStackChild {
    pub slot: Slot,
    pub weight: NonZeroU8,
}

impl HStackChild {
    /// Construct an `HStackChild`. `weight` must be non-zero;
    /// callers that have a `u8` should `try_into()` and surface
    /// the error explicitly.
    pub const fn new(slot: Slot, weight: NonZeroU8) -> Self {
        Self { slot, weight }
    }
}

impl Slot {
    /// `true` iff `target` appears anywhere in this tree. Used by
    /// callers that need to ask "does this layout include widget X?"
    /// without rebuilding their own tree-walking helpers.
    pub fn contains(&self, target: WidgetKind) -> bool {
        match self {
            Self::Widget(kind) => *kind == target,
            Self::VStack(children) => children.iter().any(|c| c.contains(target)),
            Self::HStack(children) => children.iter().any(|c| c.slot.contains(target)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nz(n: u8) -> NonZeroU8 {
        NonZeroU8::new(n).expect("test weight must be non-zero")
    }

    #[test]
    fn contains_finds_widget_at_root() {
        let s = Slot::Widget(WidgetKind::Cpu);
        assert!(s.contains(WidgetKind::Cpu));
        assert!(!s.contains(WidgetKind::Mem));
    }

    #[test]
    fn contains_finds_widget_in_nested_vstack() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::VStack(vec![Slot::Widget(WidgetKind::Mem)]),
        ]);
        assert!(s.contains(WidgetKind::Cpu));
        assert!(s.contains(WidgetKind::Mem));
        assert!(!s.contains(WidgetKind::Net));
    }

    #[test]
    fn contains_finds_widget_in_hstack_children() {
        let s = Slot::HStack(vec![
            HStackChild::new(Slot::Widget(WidgetKind::Mem), nz(40)),
            HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
        ]);
        assert!(s.contains(WidgetKind::Mem));
        assert!(s.contains(WidgetKind::Proc));
        assert!(!s.contains(WidgetKind::Cpu));
    }

    #[test]
    fn contains_distinguishes_gpu_indices() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Gpu(0)),
            Slot::Widget(WidgetKind::Gpu(3)),
        ]);
        assert!(s.contains(WidgetKind::Gpu(0)));
        assert!(s.contains(WidgetKind::Gpu(3)));
        assert!(!s.contains(WidgetKind::Gpu(1)));
        assert!(!s.contains(WidgetKind::Gpu(2)));
    }

    #[test]
    fn equality_compares_structure_and_weights() {
        let a = Slot::HStack(vec![HStackChild::new(
            Slot::Widget(WidgetKind::Mem),
            nz(40),
        )]);
        let b = Slot::HStack(vec![HStackChild::new(
            Slot::Widget(WidgetKind::Mem),
            nz(40),
        )]);
        let c = Slot::HStack(vec![HStackChild::new(
            Slot::Widget(WidgetKind::Mem),
            nz(50),
        )]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
