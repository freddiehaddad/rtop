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
//! ## DSL
//!
//! `Slot` round-trips through a small textual DSL persisted in
//! `rtop.toml` as a single `shape` string:
//!
//! ```text
//! cpu
//! vstack(cpu, mem)
//! hstack(40:cpu, 60:proc)
//! vstack(cpu, hstack(40:vstack(mem, net, disk), 60:proc))
//! ```
//!
//! The DSL is parsed by [`Slot::from_str`] / emitted by
//! [`Slot::fmt`], and the same forms are used by the custom
//! [`Serialize`] / [`Deserialize`] impls (single string in TOML).
//! Validation rejects duplicate widget kinds and non-positive
//! weights at parse time so invalid layouts cannot enter the
//! engine.
//!
//! ## Module location
//!
//! Lives in `domain/` because both `domain::preset` (the producer)
//! and `draw::layout` (the consumer) reference it; per the
//! architecture rules `domain/` may not depend on `draw/`, so the
//! type must sit in or below `domain/`.

use std::collections::HashSet;
use std::fmt::{self, Display};
use std::num::NonZeroU8;
use std::str::FromStr;

use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

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

    /// Validate the global invariants of a slot tree:
    ///
    /// * No widget kind appears more than once. The engine's per-widget
    ///   layout map is keyed by `WidgetKind`; duplicates would silently
    ///   overwrite each other's dimensions.
    ///
    /// Container non-emptiness and per-weight non-zero are enforced at
    /// parse time (the grammar can't express empty containers and
    /// `NonZeroU8` rejects zero weights), so this method only checks
    /// the cross-tree uniqueness rule.
    pub fn validate(&self) -> Result<(), SlotParseError> {
        let mut seen: HashSet<WidgetKind> = HashSet::new();
        let mut stack: Vec<&Slot> = vec![self];
        while let Some(s) = stack.pop() {
            match s {
                Slot::Widget(kind) => {
                    if !seen.insert(*kind) {
                        return Err(SlotParseError::DuplicateWidget(*kind));
                    }
                }
                Slot::VStack(children) => stack.extend(children.iter()),
                Slot::HStack(children) => stack.extend(children.iter().map(|c| &c.slot)),
            }
        }
        Ok(())
    }

    /// Return a copy of the tree with every occurrence of `kind`
    /// removed. Containers that lose all their children collapse
    /// (single-child containers flatten to that child). Returns
    /// `None` if pruning would leave the tree empty — callers
    /// should treat that as "refuse the operation" rather than
    /// allowing the user to construct an empty layout.
    pub fn pruned(self, kind: WidgetKind) -> Option<Self> {
        match self {
            Slot::Widget(k) if k == kind => None,
            Slot::Widget(_) => Some(self),
            Slot::VStack(children) => {
                let kept: Vec<Slot> = children
                    .into_iter()
                    .filter_map(|c| c.pruned(kind))
                    .collect();
                collapse(kept).map(Slot::VStack).map(Slot::flatten)
            }
            Slot::HStack(children) => {
                let kept: Vec<HStackChild> = children
                    .into_iter()
                    .filter_map(|c| {
                        c.slot
                            .pruned(kind)
                            .map(|slot| HStackChild::new(slot, c.weight))
                    })
                    .collect();
                if kept.is_empty() {
                    None
                } else if kept.len() == 1 {
                    Some(kept.into_iter().next().expect("len==1").slot)
                } else {
                    Some(Slot::HStack(kept))
                }
            }
        }
    }

    /// Append `kind` as a leaf at the bottom of the tree's outermost
    /// VStack, wrapping the tree in a new VStack first if necessary.
    /// No-op if `kind` is already present (preserves the
    /// no-duplicates invariant).
    pub fn append_widget(self, kind: WidgetKind) -> Self {
        if self.contains(kind) {
            return self;
        }
        match self {
            Slot::VStack(mut children) => {
                children.push(Slot::Widget(kind));
                Slot::VStack(children)
            }
            other => Slot::VStack(vec![other, Slot::Widget(kind)]),
        }
    }

    /// Flatten a degenerate single-child container to its inner slot.
    /// `pruned` uses this to collapse `VStack[Widget]` (which has the
    /// same render shape as bare `Widget`) into the simpler form.
    fn flatten(self) -> Self {
        match self {
            Slot::VStack(children) if children.len() == 1 => {
                children.into_iter().next().expect("len==1")
            }
            other => other,
        }
    }
}

/// Helper for `pruned`: returns `None` for empty `Vec`, the only
/// element for singleton, or wrapped vec otherwise (caller chooses
/// the wrapper variant).
fn collapse(mut children: Vec<Slot>) -> Option<Vec<Slot>> {
    match children.len() {
        0 => None,
        1 => {
            // Caller will wrap as VStack and then flatten; return as
            // a single-child Vec so the caller maps it through
            // `Slot::flatten`.
            Some(vec![children.pop().expect("len==1")])
        }
        _ => Some(children),
    }
}

// ─────────────────────────────────────────────────────────────────
// DSL: Display (emitter)
// ─────────────────────────────────────────────────────────────────

impl Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Slot::Widget(kind) => write!(f, "{kind}"),
            Slot::VStack(children) => {
                f.write_str("vstack(")?;
                for (i, c) in children.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{c}")?;
                }
                f.write_str(")")
            }
            Slot::HStack(children) => {
                f.write_str("hstack(")?;
                for (i, c) in children.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}:{}", c.weight, c.slot)?;
                }
                f.write_str(")")
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────
// DSL: FromStr (parser)
// ─────────────────────────────────────────────────────────────────

/// Error returned by [`Slot::from_str`] / DSL deserialisation.
///
/// Messages are human-friendly (suitable for inline display in the
/// options-menu shape editor); no machine-readable error code is
/// provided because the DSL is small and authored by humans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotParseError {
    /// An empty input was supplied. The DSL has no empty production
    /// — every layout has at least one widget.
    Empty,
    /// Expected a widget name or a container keyword (`vstack` /
    /// `hstack`) at `pos`, found something else.
    ExpectedSlot { pos: usize },
    /// Expected the literal character `expected` at `pos`.
    Expected { expected: char, pos: usize },
    /// Expected a positive integer (1..=255) at `pos`.
    ExpectedNumber { pos: usize },
    /// A weight token was syntactically a number but did not fit in
    /// `NonZeroU8` (zero or > 255).
    InvalidWeight { text: String },
    /// A widget identifier did not match any [`WidgetKind`] (e.g.
    /// `gpu99`, typo).
    UnknownWidget { name: String },
    /// Trailing input after a complete slot was parsed.
    TrailingInput { pos: usize },
    /// The same widget kind appears more than once in the tree.
    DuplicateWidget(WidgetKind),
}

impl Display for SlotParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("layout shape is empty"),
            Self::ExpectedSlot { pos } => {
                write!(f, "expected widget name or vstack/hstack at position {pos}")
            }
            Self::Expected { expected, pos } => {
                write!(f, "expected '{expected}' at position {pos}")
            }
            Self::ExpectedNumber { pos } => write!(f, "expected weight number at position {pos}"),
            Self::InvalidWeight { text } => {
                write!(f, "invalid weight '{text}' (must be 1..=255)")
            }
            Self::UnknownWidget { name } => write!(f, "unknown widget '{name}'"),
            Self::TrailingInput { pos } => {
                write!(f, "unexpected trailing input at position {pos}")
            }
            Self::DuplicateWidget(kind) => write!(f, "widget '{kind}' appears more than once"),
        }
    }
}

impl std::error::Error for SlotParseError {}

impl FromStr for Slot {
    type Err = SlotParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(SlotParseError::Empty);
        }
        let mut parser = Parser { src: s, pos: 0 };
        parser.skip_ws();
        let slot = parser.parse_slot()?;
        parser.skip_ws();
        if parser.pos != s.len() {
            return Err(SlotParseError::TrailingInput { pos: parser.pos });
        }
        slot.validate()?;
        Ok(slot)
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while let Some(b) = self.src.as_bytes().get(self.pos)
            && b.is_ascii_whitespace()
        {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn consume(&mut self, expected: u8) -> Result<(), SlotParseError> {
        self.skip_ws();
        if self.peek() == Some(expected) {
            self.pos += 1;
            Ok(())
        } else {
            Err(SlotParseError::Expected {
                expected: expected as char,
                pos: self.pos,
            })
        }
    }

    fn read_identifier(&mut self) -> Result<&'a str, SlotParseError> {
        self.skip_ws();
        let start = self.pos;
        while let Some(b) = self.peek()
            && (b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(SlotParseError::ExpectedSlot { pos: self.pos });
        }
        Ok(&self.src[start..self.pos])
    }

    fn read_number(&mut self) -> Result<u8, SlotParseError> {
        self.skip_ws();
        let start = self.pos;
        while let Some(b) = self.peek()
            && b.is_ascii_digit()
        {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(SlotParseError::ExpectedNumber { pos: self.pos });
        }
        let text = &self.src[start..self.pos];
        text.parse::<u8>()
            .map_err(|_| SlotParseError::InvalidWeight {
                text: text.to_string(),
            })
    }

    fn parse_slot(&mut self) -> Result<Slot, SlotParseError> {
        let ident = self.read_identifier()?;
        match ident {
            "vstack" => {
                self.consume(b'(')?;
                let children = self.parse_vstack_args()?;
                self.consume(b')')?;
                Ok(Slot::VStack(children))
            }
            "hstack" => {
                self.consume(b'(')?;
                let children = self.parse_hstack_args()?;
                self.consume(b')')?;
                Ok(Slot::HStack(children))
            }
            other => other
                .parse::<WidgetKind>()
                .map(Slot::Widget)
                .map_err(|e| SlotParseError::UnknownWidget { name: e.0 }),
        }
    }

    fn parse_vstack_args(&mut self) -> Result<Vec<Slot>, SlotParseError> {
        let mut children = Vec::new();
        children.push(self.parse_slot()?);
        loop {
            self.skip_ws();
            if self.peek() == Some(b',') {
                self.pos += 1;
                children.push(self.parse_slot()?);
            } else {
                break;
            }
        }
        Ok(children)
    }

    fn parse_hstack_args(&mut self) -> Result<Vec<HStackChild>, SlotParseError> {
        let mut children = Vec::new();
        children.push(self.parse_hstack_child()?);
        loop {
            self.skip_ws();
            if self.peek() == Some(b',') {
                self.pos += 1;
                children.push(self.parse_hstack_child()?);
            } else {
                break;
            }
        }
        Ok(children)
    }

    fn parse_hstack_child(&mut self) -> Result<HStackChild, SlotParseError> {
        let raw = self.read_number()?;
        let weight = NonZeroU8::new(raw).ok_or_else(|| SlotParseError::InvalidWeight {
            text: "0".to_string(),
        })?;
        self.consume(b':')?;
        let slot = self.parse_slot()?;
        Ok(HStackChild::new(slot, weight))
    }
}

// ─────────────────────────────────────────────────────────────────
// Serde — single string going through Display / FromStr
// ─────────────────────────────────────────────────────────────────

impl Serialize for Slot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Slot {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse::<Slot>().map_err(de::Error::custom)
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

    // ────────────────────────────────────────────────────────────
    // validate
    // ────────────────────────────────────────────────────────────

    #[test]
    fn validate_accepts_unique_widgets() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Gpu(0)),
            Slot::Widget(WidgetKind::Gpu(1)),
        ]);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn validate_rejects_duplicate_widget_kind() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Cpu),
        ]);
        assert_eq!(
            s.validate(),
            Err(SlotParseError::DuplicateWidget(WidgetKind::Cpu))
        );
    }

    #[test]
    fn validate_rejects_duplicate_gpu_index() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Gpu(2)),
            Slot::Widget(WidgetKind::Gpu(2)),
        ]);
        assert_eq!(
            s.validate(),
            Err(SlotParseError::DuplicateWidget(WidgetKind::Gpu(2)))
        );
    }

    // ────────────────────────────────────────────────────────────
    // pruned
    // ────────────────────────────────────────────────────────────

    #[test]
    fn pruned_returns_none_when_only_widget() {
        let s = Slot::Widget(WidgetKind::Cpu);
        assert_eq!(s.pruned(WidgetKind::Cpu), None);
    }

    #[test]
    fn pruned_keeps_other_widgets() {
        let s = Slot::Widget(WidgetKind::Cpu);
        assert_eq!(s.clone().pruned(WidgetKind::Mem), Some(s));
    }

    #[test]
    fn pruned_collapses_vstack_with_single_remaining_child() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
        ]);
        assert_eq!(
            s.pruned(WidgetKind::Mem),
            Some(Slot::Widget(WidgetKind::Cpu))
        );
    }

    #[test]
    fn pruned_keeps_vstack_with_multiple_remaining_children() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
            Slot::Widget(WidgetKind::Net),
        ]);
        assert_eq!(
            s.pruned(WidgetKind::Mem),
            Some(Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Net),
            ]))
        );
    }

    #[test]
    fn pruned_collapses_hstack_with_single_remaining_child() {
        let s = Slot::HStack(vec![
            HStackChild::new(Slot::Widget(WidgetKind::Mem), nz(40)),
            HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
        ]);
        // When proc is removed, the remaining HStack with just one
        // child collapses to that child (no point in a single-child
        // horizontal stack).
        assert_eq!(
            s.pruned(WidgetKind::Proc),
            Some(Slot::Widget(WidgetKind::Mem))
        );
    }

    #[test]
    fn pruned_recurses_into_nested_containers() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::HStack(vec![
                HStackChild::new(Slot::Widget(WidgetKind::Mem), nz(40)),
                HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
            ]),
        ]);
        let pruned = s.pruned(WidgetKind::Mem).unwrap();
        // Removing Mem collapses the HStack to bare Proc, then the
        // VStack remains [Cpu, Proc].
        assert_eq!(
            pruned,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Proc),
            ])
        );
    }

    #[test]
    fn pruned_returns_none_for_widget_not_present() {
        let s = Slot::Widget(WidgetKind::Cpu);
        // Not present: pruning produces unchanged tree.
        assert_eq!(s.clone().pruned(WidgetKind::Mem), Some(s));
    }

    // ────────────────────────────────────────────────────────────
    // append_widget
    // ────────────────────────────────────────────────────────────

    #[test]
    fn append_widget_wraps_bare_widget_in_vstack() {
        let s = Slot::Widget(WidgetKind::Cpu);
        let extended = s.append_widget(WidgetKind::Mem);
        assert_eq!(
            extended,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Mem),
            ])
        );
    }

    #[test]
    fn append_widget_extends_existing_vstack() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
        ]);
        let extended = s.append_widget(WidgetKind::Net);
        assert_eq!(
            extended,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Net),
            ])
        );
    }

    #[test]
    fn append_widget_wraps_hstack_in_vstack() {
        let s = Slot::HStack(vec![
            HStackChild::new(Slot::Widget(WidgetKind::Mem), nz(40)),
            HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
        ]);
        let extended = s.clone().append_widget(WidgetKind::Cpu);
        assert_eq!(
            extended,
            Slot::VStack(vec![s, Slot::Widget(WidgetKind::Cpu)])
        );
    }

    #[test]
    fn append_widget_no_op_when_already_present() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
        ]);
        assert_eq!(s.clone().append_widget(WidgetKind::Cpu), s);
    }

    // ────────────────────────────────────────────────────────────
    // Display
    // ────────────────────────────────────────────────────────────

    #[test]
    fn display_widget_emits_widget_name() {
        assert_eq!(Slot::Widget(WidgetKind::Cpu).to_string(), "cpu");
        assert_eq!(Slot::Widget(WidgetKind::Gpu(3)).to_string(), "gpu3");
    }

    #[test]
    fn display_vstack_uses_comma_space_separator() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
        ]);
        assert_eq!(s.to_string(), "vstack(cpu, mem)");
    }

    #[test]
    fn display_hstack_uses_weight_colon_slot() {
        let s = Slot::HStack(vec![
            HStackChild::new(Slot::Widget(WidgetKind::Mem), nz(40)),
            HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
        ]);
        assert_eq!(s.to_string(), "hstack(40:mem, 60:proc)");
    }

    #[test]
    fn display_nested_emits_canonical_form() {
        let s = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::HStack(vec![
                HStackChild::new(
                    Slot::VStack(vec![
                        Slot::Widget(WidgetKind::Mem),
                        Slot::Widget(WidgetKind::Net),
                    ]),
                    nz(40),
                ),
                HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
            ]),
        ]);
        assert_eq!(
            s.to_string(),
            "vstack(cpu, hstack(40:vstack(mem, net), 60:proc))"
        );
    }

    // ────────────────────────────────────────────────────────────
    // FromStr — happy path
    // ────────────────────────────────────────────────────────────

    #[test]
    fn parse_bare_widget() {
        assert_eq!("cpu".parse::<Slot>(), Ok(Slot::Widget(WidgetKind::Cpu)));
        assert_eq!("gpu5".parse::<Slot>(), Ok(Slot::Widget(WidgetKind::Gpu(5))));
    }

    #[test]
    fn parse_simple_vstack() {
        assert_eq!(
            "vstack(cpu, mem)".parse::<Slot>(),
            Ok(Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Mem),
            ]))
        );
    }

    #[test]
    fn parse_simple_hstack() {
        assert_eq!(
            "hstack(40:mem, 60:proc)".parse::<Slot>(),
            Ok(Slot::HStack(vec![
                HStackChild::new(Slot::Widget(WidgetKind::Mem), nz(40)),
                HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
            ]))
        );
    }

    #[test]
    fn parse_tolerates_extra_whitespace() {
        assert_eq!(
            "  vstack( cpu , mem )  ".parse::<Slot>(),
            Ok(Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Mem),
            ]))
        );
        assert_eq!(
            "hstack(40 : mem, 60 : proc)".parse::<Slot>(),
            Ok(Slot::HStack(vec![
                HStackChild::new(Slot::Widget(WidgetKind::Mem), nz(40)),
                HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
            ]))
        );
    }

    #[test]
    fn parse_complex_nested() {
        let input = "vstack(cpu, hstack(40:vstack(mem, net, disk), 60:proc))";
        let parsed: Slot = input.parse().unwrap();
        assert_eq!(parsed.to_string(), input);
    }

    // ────────────────────────────────────────────────────────────
    // FromStr — error cases
    // ────────────────────────────────────────────────────────────

    #[test]
    fn parse_rejects_empty_string() {
        assert_eq!("".parse::<Slot>(), Err(SlotParseError::Empty));
        assert_eq!("   ".parse::<Slot>(), Err(SlotParseError::Empty));
    }

    #[test]
    fn parse_rejects_unknown_widget() {
        assert!(matches!(
            "nope".parse::<Slot>(),
            Err(SlotParseError::UnknownWidget { name }) if name == "nope"
        ));
        assert!(matches!(
            "gpu99".parse::<Slot>(),
            Err(SlotParseError::UnknownWidget { name }) if name == "gpu99"
        ));
    }

    #[test]
    fn parse_rejects_zero_weight() {
        assert!(matches!(
            "hstack(0:mem, 60:proc)".parse::<Slot>(),
            Err(SlotParseError::InvalidWeight { text }) if text == "0"
        ));
    }

    #[test]
    fn parse_rejects_overflow_weight() {
        assert!(matches!(
            "hstack(256:mem, 1:proc)".parse::<Slot>(),
            Err(SlotParseError::InvalidWeight { text }) if text == "256"
        ));
    }

    #[test]
    fn parse_rejects_missing_open_paren() {
        assert!(matches!(
            "vstack cpu, mem)".parse::<Slot>(),
            Err(SlotParseError::Expected { expected: '(', .. })
        ));
    }

    #[test]
    fn parse_rejects_missing_close_paren() {
        assert!(matches!(
            "vstack(cpu, mem".parse::<Slot>(),
            Err(SlotParseError::Expected { expected: ')', .. })
        ));
    }

    #[test]
    fn parse_rejects_missing_colon_in_hstack_child() {
        assert!(matches!(
            "hstack(40 mem, 60:proc)".parse::<Slot>(),
            Err(SlotParseError::Expected { expected: ':', .. })
        ));
    }

    #[test]
    fn parse_rejects_trailing_input() {
        assert!(matches!(
            "cpu garbage".parse::<Slot>(),
            Err(SlotParseError::TrailingInput { .. })
        ));
    }

    #[test]
    fn parse_rejects_empty_container() {
        // Empty containers are unrepresentable in the grammar — the
        // first child is required, so this fails on the missing
        // identifier.
        assert!(matches!(
            "vstack()".parse::<Slot>(),
            Err(SlotParseError::ExpectedSlot { .. })
        ));
        assert!(matches!(
            "hstack()".parse::<Slot>(),
            Err(SlotParseError::ExpectedNumber { .. })
        ));
    }

    #[test]
    fn parse_rejects_duplicate_widget() {
        assert!(matches!(
            "vstack(cpu, cpu)".parse::<Slot>(),
            Err(SlotParseError::DuplicateWidget(WidgetKind::Cpu))
        ));
    }

    // ────────────────────────────────────────────────────────────
    // Round-trip
    // ────────────────────────────────────────────────────────────

    #[test]
    fn round_trip_through_display_and_parse() {
        // Several non-trivial trees survive Display -> FromStr.
        let trees: Vec<Slot> = vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Proc),
            ]),
            Slot::HStack(vec![
                HStackChild::new(Slot::Widget(WidgetKind::Mem), nz(40)),
                HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
            ]),
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::HStack(vec![
                    HStackChild::new(
                        Slot::VStack(vec![
                            Slot::Widget(WidgetKind::Gpu(0)),
                            Slot::Widget(WidgetKind::Gpu(1)),
                            Slot::Widget(WidgetKind::Mem),
                            Slot::Widget(WidgetKind::Net),
                            Slot::Widget(WidgetKind::Disk),
                        ]),
                        nz(40),
                    ),
                    HStackChild::new(Slot::Widget(WidgetKind::Proc), nz(60)),
                ]),
            ]),
        ];
        for tree in trees {
            let s = tree.to_string();
            let parsed: Slot = s.parse().expect("round-trip parse");
            assert_eq!(parsed, tree, "tree did not round-trip: {s}");
        }
    }

    #[test]
    fn round_trip_through_serde_toml_string() {
        let tree = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Mem),
        ]);
        let value = toml::Value::try_from(&tree).unwrap();
        // Serialised form is a TOML string.
        assert_eq!(value, toml::Value::String("vstack(cpu, mem)".to_string()));
        let loaded: Slot = value.try_into().unwrap();
        assert_eq!(loaded, tree);
    }

    #[test]
    fn deserialise_rejects_invalid_string() {
        let value = toml::Value::String("vstack(nope)".to_string());
        let result: Result<Slot, _> = value.try_into();
        assert!(result.is_err());
    }
}
