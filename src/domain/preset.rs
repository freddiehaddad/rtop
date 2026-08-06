//! Layout presets — a fixed set of curated configurations the user
//! can cycle through with `p` / `P`.
//!
//! Presets are read-only and ship with rtop. There is no runtime
//! "save preset" or "delete preset" — users who need a different
//! layout edit the `custom_layout = "..."` DSL string in
//! `rtop.toml` directly (or via the options-menu `custom_layout`
//! key) or cycle to the custom preset (the slot beyond
//! `BuiltinPreset::COUNT`). The only preset state persisted across
//! runs is `Config::preset`, a [`PresetField`] storing the active
//! cursor by canonical name.

use crate::domain::layout_spec::{HStackChild, Slot};
use crate::domain::widget_kind::WidgetKind;
use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::NonZeroU8;

/// Width-share weight for the proc column in two-column layouts
/// (matches the long-standing 60% used by btop's `all` view).
const PROC_WEIGHT: NonZeroU8 = match NonZeroU8::new(60) {
    Some(n) => n,
    None => panic!("60 is non-zero by construction"),
};
/// Width-share weight for the non-proc column in two-column layouts.
const LEFT_WEIGHT: NonZeroU8 = match NonZeroU8::new(40) {
    Some(n) => n,
    None => panic!("40 is non-zero by construction"),
};

/// Identity of one of the curated, immutable layout presets that
/// ship with rtop. Adding a variant is a three-step change: extend
/// the enum, extend [`Self::ALL`], and add a match arm in
/// [`Self::name`] / [`Self::layout_spec`].
///
/// Cycle order (also the variant declaration order) visits the
/// dashboard first, then the four resource diagnostics paired with
/// `proc`, then the gaming/ML focus, then the no-process system
/// status view, then `Custom`. Each step is a distinct user mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinPreset {
    /// Everything: the six base widgets including the cycling
    /// GPU widget. The dashboard / overview position.
    All,
    /// CPU + processes — "what's eating my CPU?"
    CpuProc,
    /// Memory + processes — "what's eating my RAM?"
    MemProc,
    /// Disk + processes — "what's hammering my disk?" Pairs with
    /// the disk widget's `i` toggle to enter IO view.
    DiskProc,
    /// CPU + network + processes — "what's saturating my bandwidth
    /// and is the CPU keeping up?"
    CpuNetProc,
    /// CPU + every supported GPU + processes — gaming / ML focus.
    CpuGpuProc,
    /// CPU + network + memory + disk — passive system-utilisation
    /// view with no process noise. Net absorbs slack so the column
    /// fills the screen without empty space.
    CpuNetMemDisk,
}

impl BuiltinPreset {
    /// Total number of built-in presets.
    pub const COUNT: usize = 7;

    /// All built-in presets in cycle order. The order also defines
    /// the cycle position via [`ActivePreset::next`] / [`ActivePreset::prev`]
    /// — `Custom` follows the last builtin, then wraps to the first.
    pub const ALL: [Self; Self::COUNT] = [
        Self::All,
        Self::CpuProc,
        Self::MemProc,
        Self::DiskProc,
        Self::CpuNetProc,
        Self::CpuGpuProc,
        Self::CpuNetMemDisk,
    ];

    /// Stable, user-visible identifier for the preset (used in
    /// the CPU widget bottom hint and in TOML serialisation).
    pub fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::CpuProc => "cpu+proc",
            Self::MemProc => "mem+proc",
            Self::DiskProc => "disk+proc",
            Self::CpuNetProc => "cpu+net+proc",
            Self::CpuGpuProc => "cpu+gpu+proc",
            Self::CpuNetMemDisk => "cpu+net+mem+disk",
        }
    }

    /// Resolve a preset by its canonical [`Self::name`].
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == s)
    }

    /// Static [`Slot`] tree for this preset.
    ///
    /// Each preset's tree is a literal `Slot::VStack` /
    /// `Slot::HStack` shape. Every preset's outermost shape is a
    /// [`Slot::VStack`]; the statusbar leaf is appended to that
    /// VStack as the last child so it always lands on the bottom
    /// row. The helper [`with_statusbar`] takes care of the append
    /// (and gracefully wraps a future non-VStack root in a new
    /// outer VStack).
    pub fn layout_spec(self) -> Slot {
        let body = match self {
            Self::All => all_layout_spec(),
            Self::CpuProc => Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Proc),
            ]),
            Self::MemProc => Slot::VStack(vec![
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Proc),
            ]),
            Self::DiskProc => Slot::VStack(vec![
                Slot::Widget(WidgetKind::Disk),
                Slot::Widget(WidgetKind::Proc),
            ]),
            Self::CpuNetProc => Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::HStack(vec![
                    HStackChild::new(Slot::Widget(WidgetKind::Net), LEFT_WEIGHT),
                    HStackChild::new(Slot::Widget(WidgetKind::Proc), PROC_WEIGHT),
                ]),
            ]),
            Self::CpuGpuProc => cpu_gpu_proc_layout_spec(),
            Self::CpuNetMemDisk => Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Net),
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Disk),
            ]),
        };
        with_statusbar(body)
    }
}

/// Append [`WidgetKind::Statusbar`] as the last leaf of the
/// outermost vertical stack. When `root` is already a
/// [`Slot::VStack`] the statusbar is pushed to its children
/// (preserving existing height-distribution semantics — wrapping
/// in another VStack would re-introduce a slack-distribution
/// boundary that doesn't exist today). When `root` is anything
/// else (an `HStack` or a `Widget` leaf) the helper wraps it in
/// a new outer VStack with the statusbar as the second child.
fn with_statusbar(root: Slot) -> Slot {
    match root {
        Slot::VStack(mut children) => {
            children.push(Slot::Widget(WidgetKind::Statusbar));
            Slot::VStack(children)
        }
        other => Slot::VStack(vec![other, Slot::Widget(WidgetKind::Statusbar)]),
    }
}

/// Build the `all` preset's slot tree: CPU on top, then a
/// two-column body with [GPU, Mem, Net, Disk] on the left and
/// Proc on the right.
fn all_layout_spec() -> Slot {
    Slot::VStack(vec![
        Slot::Widget(WidgetKind::Cpu),
        Slot::HStack(vec![
            HStackChild::new(
                Slot::VStack(vec![
                    Slot::Widget(WidgetKind::Gpu),
                    Slot::Widget(WidgetKind::Mem),
                    Slot::Widget(WidgetKind::Net),
                    Slot::Widget(WidgetKind::Disk),
                ]),
                LEFT_WEIGHT,
            ),
            HStackChild::new(Slot::Widget(WidgetKind::Proc), PROC_WEIGHT),
        ]),
    ])
}

/// Build the `cpu+gpu+proc` preset's slot tree: CPU on top, GPU
/// in the middle at preferred height, Proc absorbing the rest.
fn cpu_gpu_proc_layout_spec() -> Slot {
    Slot::VStack(vec![
        Slot::Widget(WidgetKind::Cpu),
        Slot::Widget(WidgetKind::Gpu),
        Slot::Widget(WidgetKind::Proc),
    ])
}

/// Cursor identifying which preset is currently active. The
/// builtins are shaped data; `Custom` is the user's mutable slot
/// whose layout lives in `Config::custom_layout` (a [`Slot`] tree).
///
/// The cycle order (`Self::next` / `Self::prev`) visits every
/// builtin in [`BuiltinPreset::ALL`] order, then `Custom`, then
/// wraps. [`Self::CYCLE_LEN`] positions total
/// (`BuiltinPreset::COUNT + 1`).
///
/// First-launch default is [`BuiltinPreset::All`] (the dashboard
/// view) so the very first `p` keypress visibly cycles to a
/// different preset, and the cursor name matches what the user is
/// looking at. Custom is reached only by cycling — there is no
/// auto-promotion from any layout-editing path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivePreset {
    Builtin(BuiltinPreset),
    Custom,
}

impl Default for ActivePreset {
    fn default() -> Self {
        Self::Builtin(BuiltinPreset::All)
    }
}

impl ActivePreset {
    /// Canonical TOML name for the [`Self::Custom`] variant.
    pub const NAME_CUSTOM: &'static str = "custom";

    /// Number of distinct cursor positions the cycle visits.
    pub const CYCLE_LEN: usize = BuiltinPreset::COUNT + 1;

    /// Canonical TOML / display name for this cursor.
    pub fn name(self) -> &'static str {
        match self {
            Self::Builtin(b) => b.name(),
            Self::Custom => Self::NAME_CUSTOM,
        }
    }

    /// Resolve a cursor by its canonical name. Returns `None` when
    /// the string doesn't match any builtin or `"custom"`.
    pub fn from_name(s: &str) -> Option<Self> {
        if s == Self::NAME_CUSTOM {
            Some(Self::Custom)
        } else {
            BuiltinPreset::from_name(s).map(Self::Builtin)
        }
    }

    /// Move the cursor one position forward in the cycle (wrapping).
    pub fn next(self) -> Self {
        let i = (self.cycle_index() + 1) % Self::CYCLE_LEN;
        Self::from_cycle_index(i)
    }

    /// Move the cursor one position backward in the cycle (wrapping).
    pub fn prev(self) -> Self {
        let i = (self.cycle_index() + Self::CYCLE_LEN - 1) % Self::CYCLE_LEN;
        Self::from_cycle_index(i)
    }

    fn cycle_index(self) -> usize {
        match self {
            Self::Builtin(b) => b as usize,
            Self::Custom => BuiltinPreset::COUNT,
        }
    }

    fn from_cycle_index(i: usize) -> Self {
        if i < BuiltinPreset::COUNT {
            Self::Builtin(BuiltinPreset::ALL[i])
        } else {
            Self::Custom
        }
    }
}

/// Persisted preset cursor with deserialise-time error capture.
///
/// Holds the typed value plus a side channel for the offending
/// string when a hand-edited TOML names a preset that doesn't
/// exist. `Config::validate` drains the side channel into
/// warnings via [`Self::take_invalid`] and clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresetField {
    active: ActivePreset,
    /// Offending name from the last deserialise pass that didn't
    /// match a known preset. Cleared by [`Self::take_invalid`].
    invalid: Option<String>,
}

impl PresetField {
    pub fn active(&self) -> ActivePreset {
        self.active
    }

    pub fn set(&mut self, active: ActivePreset) {
        self.active = active;
    }

    /// Drain the captured invalid name (deserialise-time parse
    /// failure) and return it. Used by `Config::validate` to fold
    /// it into the warning list once and then clear so repeated
    /// `validate` calls don't re-report.
    pub fn take_invalid(&mut self) -> Option<String> {
        self.invalid.take()
    }
}

impl Serialize for PresetField {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Canonical wire form is the cursor name. Invalid entries
        // are dropped so the saved file matches what the runtime
        // actually used.
        serializer.collect_str(self.active.name())
    }
}

impl<'de> Deserialize<'de> for PresetField {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PresetFieldVisitor;

        impl de::Visitor<'_> for PresetFieldVisitor {
            type Value = PresetField;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "a preset name (\"{}\" or one of the builtins)",
                    ActivePreset::NAME_CUSTOM,
                )
            }

            fn visit_str<E: de::Error>(self, s: &str) -> Result<PresetField, E> {
                Ok(match ActivePreset::from_name(s) {
                    Some(active) => PresetField {
                        active,
                        invalid: None,
                    },
                    None => PresetField {
                        active: ActivePreset::Custom,
                        invalid: Some(s.to_string()),
                    },
                })
            }
        }

        deserializer.deserialize_str(PresetFieldVisitor)
    }
}

/// First-launch default for the custom preset's layout tree.
/// Clones the `all` preset's tree so the user sees the dashboard
/// view straight away. Replaces the previous `CustomLayout`
/// wrapper struct — the wrapper held only one field and existed
/// solely to host a `#[serde(rename = "shape")]` attribute that
/// no longer applies (the field is now `Config.custom_layout: Slot`
/// at the top level).
pub fn default_custom_layout() -> Slot {
    BuiltinPreset::All.layout_spec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_constant_lists_every_variant() {
        assert_eq!(BuiltinPreset::ALL.len(), BuiltinPreset::COUNT);
        let mut names: Vec<&str> = BuiltinPreset::ALL.iter().map(|p| p.name()).collect();
        names.sort_unstable();
        let mut deduped = names.clone();
        deduped.dedup();
        assert_eq!(names, deduped, "preset names must be unique");
    }

    #[test]
    fn names_round_trip_via_from_name() {
        for &preset in &BuiltinPreset::ALL {
            assert_eq!(BuiltinPreset::from_name(preset.name()), Some(preset));
        }
    }

    #[test]
    fn from_name_rejects_unknown() {
        assert_eq!(BuiltinPreset::from_name("nope"), None);
        assert_eq!(BuiltinPreset::from_name(""), None);
        // Names are case-sensitive — the canonical form is lowercase.
        assert_eq!(BuiltinPreset::from_name("All"), None);
    }

    fn collect_widgets(slot: &Slot) -> Vec<WidgetKind> {
        let mut out = Vec::new();
        let mut stack: Vec<&Slot> = vec![slot];
        while let Some(s) = stack.pop() {
            match s {
                Slot::Widget(kind) => out.push(*kind),
                Slot::VStack(children) => stack.extend(children.iter().rev()),
                Slot::HStack(children) => stack.extend(children.iter().map(|c| &c.slot).rev()),
            }
        }
        out
    }

    #[test]
    fn layout_spec_validates_for_every_builtin() {
        // Each builtin's static tree must satisfy the global
        // invariants Slot::validate checks (no duplicate widget
        // kinds). This catches future regressions where someone
        // accidentally lists a widget twice in a hand-written tree.
        for &preset in &BuiltinPreset::ALL {
            preset.layout_spec().validate().unwrap_or_else(|e| {
                panic!("{}: layout_spec failed validation: {e}", preset.name())
            });
        }
    }

    #[test]
    fn layout_spec_no_duplicate_widget_in_any_builtin() {
        for &preset in &BuiltinPreset::ALL {
            let widgets = collect_widgets(&preset.layout_spec());
            let mut seen = std::collections::HashSet::new();
            for kind in &widgets {
                assert!(
                    seen.insert(*kind),
                    "{}: widget {kind} appears twice",
                    preset.name(),
                );
            }
        }
    }

    #[test]
    fn layout_spec_round_trips_through_dsl() {
        // Every builtin tree must be expressible in the DSL — its
        // canonical Display form must parse back to the same tree.
        for &preset in &BuiltinPreset::ALL {
            let original = preset.layout_spec();
            let dsl = original.to_string();
            let parsed: Slot = dsl
                .parse()
                .unwrap_or_else(|e| panic!("{}: DSL did not round-trip: {e}", preset.name()));
            assert_eq!(
                parsed,
                original,
                "{}: DSL round-trip altered tree",
                preset.name()
            );
        }
    }

    #[test]
    fn builtin_all_layout_spec_is_cpu_top_two_column_body() {
        let spec = BuiltinPreset::All.layout_spec();
        let Slot::VStack(children) = &spec else {
            panic!("`all` should be a VStack at the root, got {spec:?}");
        };
        // Cpu, two-column body, statusbar.
        assert_eq!(children.len(), 3);
        assert!(matches!(children[0], Slot::Widget(WidgetKind::Cpu)));
        let Slot::HStack(hstack) = &children[1] else {
            panic!("`all` body should be an HStack, got {:?}", children[1]);
        };
        assert_eq!(hstack.len(), 2);
        // Right column is always proc.
        assert!(matches!(hstack[1].slot, Slot::Widget(WidgetKind::Proc)));
        // Left column is a VStack containing the singleton GPU + mem + net + disk.
        let Slot::VStack(left) = &hstack[0].slot else {
            panic!("`all` left column should be a VStack");
        };
        assert_eq!(
            left,
            &vec![
                Slot::Widget(WidgetKind::Gpu),
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Net),
                Slot::Widget(WidgetKind::Disk),
            ]
        );
        // Statusbar is the last leaf.
        assert!(matches!(children[2], Slot::Widget(WidgetKind::Statusbar)));
    }

    #[test]
    fn builtin_cpu_proc_layout_spec_is_two_widget_vstack() {
        let spec = BuiltinPreset::CpuProc.layout_spec();
        assert_eq!(
            spec,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Proc),
                Slot::Widget(WidgetKind::Statusbar),
            ])
        );
    }

    #[test]
    fn builtin_mem_proc_and_disk_proc_stack_widget_then_proc() {
        assert_eq!(
            BuiltinPreset::MemProc.layout_spec(),
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Proc),
                Slot::Widget(WidgetKind::Statusbar),
            ])
        );
        assert_eq!(
            BuiltinPreset::DiskProc.layout_spec(),
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Disk),
                Slot::Widget(WidgetKind::Proc),
                Slot::Widget(WidgetKind::Statusbar),
            ])
        );
    }

    #[test]
    fn builtin_cpu_net_proc_layout_spec_is_cpu_then_two_column_net_proc() {
        let spec = BuiltinPreset::CpuNetProc.layout_spec();
        let Slot::VStack(children) = &spec else {
            panic!("`cpu+net+proc` should be a VStack");
        };
        // Cpu, two-column body, statusbar.
        assert_eq!(children.len(), 3);
        assert!(matches!(children[0], Slot::Widget(WidgetKind::Cpu)));
        let Slot::HStack(hstack) = &children[1] else {
            panic!("`cpu+net+proc` body should be an HStack");
        };
        assert_eq!(hstack.len(), 2);
        assert!(matches!(hstack[0].slot, Slot::Widget(WidgetKind::Net)));
        assert!(matches!(hstack[1].slot, Slot::Widget(WidgetKind::Proc)));
        assert!(matches!(children[2], Slot::Widget(WidgetKind::Statusbar)));
    }

    #[test]
    fn builtin_cpu_gpu_proc_layout_spec_is_single_column() {
        let spec = BuiltinPreset::CpuGpuProc.layout_spec();
        // CPU + singleton GPU + proc + statusbar, all in one column.
        assert_eq!(
            spec,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Gpu),
                Slot::Widget(WidgetKind::Proc),
                Slot::Widget(WidgetKind::Statusbar),
            ])
        );
    }

    #[test]
    fn builtin_cpu_net_mem_disk_layout_spec_is_single_column_in_visual_order() {
        let spec = BuiltinPreset::CpuNetMemDisk.layout_spec();
        // The preset name encodes the visual order top-to-bottom;
        // the statusbar lands at the bottom.
        assert_eq!(
            spec,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Net),
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Disk),
                Slot::Widget(WidgetKind::Statusbar),
            ])
        );
    }

    #[test]
    fn active_preset_default_is_all_builtin() {
        // First-launch cursor lands on the `all` builtin so the
        // user's first `p` press visibly cycles to a different
        // preset, and the cursor name matches the visible layout.
        assert_eq!(
            ActivePreset::default(),
            ActivePreset::Builtin(BuiltinPreset::All)
        );
    }

    #[test]
    fn active_preset_name_round_trips_through_from_name() {
        for &b in &BuiltinPreset::ALL {
            let active = ActivePreset::Builtin(b);
            assert_eq!(ActivePreset::from_name(active.name()), Some(active));
        }
        assert_eq!(
            ActivePreset::from_name(ActivePreset::Custom.name()),
            Some(ActivePreset::Custom)
        );
    }

    #[test]
    fn active_preset_from_name_rejects_unknown() {
        assert_eq!(ActivePreset::from_name("nope"), None);
        assert_eq!(ActivePreset::from_name(""), None);
        assert_eq!(ActivePreset::from_name("Custom"), None); // case-sensitive
    }

    #[test]
    fn active_preset_next_walks_full_cycle_then_wraps() {
        let mut order: Vec<ActivePreset> = BuiltinPreset::ALL
            .iter()
            .map(|&b| ActivePreset::Builtin(b))
            .collect();
        order.push(ActivePreset::Custom);
        assert_eq!(order.len(), ActivePreset::CYCLE_LEN);

        let mut cursor = order[0];
        for &expected_next in order.iter().cycle().skip(1).take(order.len()) {
            cursor = cursor.next();
            assert_eq!(cursor, expected_next);
        }
        assert_eq!(cursor, order[0]);
    }

    #[test]
    fn active_preset_prev_walks_full_cycle_then_wraps() {
        let mut forward: Vec<ActivePreset> = BuiltinPreset::ALL
            .iter()
            .map(|&b| ActivePreset::Builtin(b))
            .collect();
        forward.push(ActivePreset::Custom);
        let order: Vec<ActivePreset> = forward.iter().rev().copied().collect();
        assert_eq!(order.len(), ActivePreset::CYCLE_LEN);

        let mut cursor = order[0];
        for &expected_prev in order.iter().cycle().skip(1).take(order.len()) {
            cursor = cursor.prev();
            assert_eq!(cursor, expected_prev);
        }
        assert_eq!(cursor, order[0]);
    }

    #[test]
    fn preset_field_serialises_as_canonical_name() {
        for &b in &BuiltinPreset::ALL {
            let mut field = PresetField::default();
            field.set(ActivePreset::Builtin(b));
            let value = toml::Value::try_from(&field).unwrap();
            assert_eq!(value, toml::Value::String(b.name().to_string()));
        }
        let mut field = PresetField::default();
        field.set(ActivePreset::Custom);
        let value = toml::Value::try_from(&field).unwrap();
        assert_eq!(value, toml::Value::String("custom".to_string()));
    }

    #[test]
    fn preset_field_deserialises_known_name() {
        let value = toml::Value::String("cpu+proc".to_string());
        let mut field: PresetField = value.try_into().unwrap();
        assert_eq!(
            field.active(),
            ActivePreset::Builtin(BuiltinPreset::CpuProc)
        );
        assert_eq!(field.take_invalid(), None);
    }

    #[test]
    fn preset_field_deserialises_custom() {
        let value = toml::Value::String("custom".to_string());
        let mut field: PresetField = value.try_into().unwrap();
        assert_eq!(field.active(), ActivePreset::Custom);
        assert_eq!(field.take_invalid(), None);
    }

    #[test]
    fn preset_field_unknown_name_falls_back_to_custom_and_captures_invalid() {
        let value = toml::Value::String("cpu+pro".to_string());
        let mut field: PresetField = value.try_into().unwrap();
        assert_eq!(field.active(), ActivePreset::Custom);
        assert_eq!(field.take_invalid().as_deref(), Some("cpu+pro"));
        assert_eq!(field.take_invalid(), None);
    }

    #[test]
    fn preset_field_round_trips_through_toml_for_all_active_values() {
        let mut cases: Vec<ActivePreset> = BuiltinPreset::ALL
            .iter()
            .map(|&b| ActivePreset::Builtin(b))
            .collect();
        cases.push(ActivePreset::Custom);
        for active in cases {
            let mut field = PresetField::default();
            field.set(active);
            let serialised = toml::Value::try_from(&field).unwrap();
            let mut deserialised: PresetField = serialised.try_into().unwrap();
            assert_eq!(deserialised.active(), active);
            assert_eq!(deserialised.take_invalid(), None);
        }
    }

    #[test]
    fn default_custom_layout_is_all_preset_tree() {
        assert_eq!(default_custom_layout(), BuiltinPreset::All.layout_spec());
    }

    /// The statusbar is added to every builtin preset (and the
    /// custom layout default) at the bottom of the outer VStack.
    /// `Slot::contains` walks the entire tree; this exercise pins
    /// that the leaf is reachable from every preset and from
    /// `default_custom_layout`.
    #[test]
    fn every_builtin_preset_contains_statusbar_widget() {
        for &preset in &BuiltinPreset::ALL {
            assert!(
                preset.layout_spec().contains(WidgetKind::Statusbar),
                "{}: layout_spec must contain WidgetKind::Statusbar",
                preset.name(),
            );
        }
    }

    #[test]
    fn default_custom_layout_contains_statusbar_widget() {
        assert!(default_custom_layout().contains(WidgetKind::Statusbar));
    }

    /// Beyond mere presence, the statusbar must be the **last**
    /// leaf of the **outermost** vertical stack so it always
    /// renders on the bottom row regardless of how the rest of
    /// the layout reflows. `with_statusbar` enforces this for
    /// VStack roots; this test pins the contract end-to-end so a
    /// future preset that uses an HStack root cannot accidentally
    /// drop the bottom-row guarantee.
    #[test]
    fn statusbar_is_last_leaf_of_outer_vstack_for_every_builtin() {
        for &preset in &BuiltinPreset::ALL {
            let spec = preset.layout_spec();
            match spec {
                Slot::VStack(children) => {
                    let last = children
                        .last()
                        .unwrap_or_else(|| panic!("{}: VStack has no children", preset.name()));
                    assert_eq!(
                        last,
                        &Slot::Widget(WidgetKind::Statusbar),
                        "{}: statusbar must be last child of outer VStack",
                        preset.name(),
                    );
                }
                other => panic!(
                    "{}: outer slot is {other:?}, expected VStack so the statusbar lands on the bottom row",
                    preset.name(),
                ),
            }
        }
    }

    /// `with_statusbar` must NOT introduce a nested VStack when
    /// the input is already a VStack — that would re-introduce a
    /// slack-distribution boundary the existing presets don't
    /// have. Verified by walking the outer children: every
    /// child of the outermost VStack from `with_statusbar(VStack(_))`
    /// is the original child or the appended statusbar leaf, NOT
    /// a nested VStack wrapping them.
    #[test]
    fn with_statusbar_appends_to_existing_vstack_without_nesting() {
        let inner = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Proc),
        ]);
        let wrapped = with_statusbar(inner);
        match wrapped {
            Slot::VStack(children) => {
                assert_eq!(children.len(), 3);
                assert_eq!(children[0], Slot::Widget(WidgetKind::Cpu));
                assert_eq!(children[1], Slot::Widget(WidgetKind::Proc));
                assert_eq!(children[2], Slot::Widget(WidgetKind::Statusbar));
            }
            other => panic!("expected VStack, got {other:?}"),
        }
    }

    /// `with_statusbar` must wrap a non-VStack root in a new
    /// outer VStack so the statusbar still lands at the bottom.
    /// (No builtin preset uses this branch today; the test pins
    /// the contract for future custom layouts edited via the DSL.)
    #[test]
    fn with_statusbar_wraps_non_vstack_root_in_new_vstack() {
        let leaf = Slot::Widget(WidgetKind::Cpu);
        let wrapped = with_statusbar(leaf.clone());
        assert_eq!(
            wrapped,
            Slot::VStack(vec![leaf, Slot::Widget(WidgetKind::Statusbar)]),
        );
    }
}
