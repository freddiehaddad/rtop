//! Layout presets — a fixed set of curated configurations the user
//! can cycle through with `p` / `P`.
//!
//! Presets are read-only and ship with rtop. There is no runtime
//! "save preset" or "delete preset" — users who need a different
//! layout edit the `[layout] shape = "..."` DSL string in
//! `rtop.toml` directly (or via the options-menu `shape` key) or
//! cycle to the custom preset (the slot beyond `BuiltinPreset::COUNT`).
//! The only preset state persisted across runs is `Config::preset`,
//! a [`PresetField`] storing the active cursor by canonical name.

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
    /// Everything: the five base widgets plus a `Gpu(N)` for every
    /// supported index. The dashboard / overview position.
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
    /// Each preset's tree is hand-written below as a literal
    /// `Slot::VStack` / `Slot::HStack` shape — every widget the
    /// preset *might* display appears in the tree, including
    /// every supported GPU index. The layout engine collapses
    /// invisible widgets (currently only `Gpu(n)` where `n >=
    /// hints.gpu_count`) to zero size so the same static tree
    /// produces correct output on every hardware configuration.
    pub fn layout_spec(self) -> Slot {
        match self {
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
        }
    }
}

/// Build the `all` preset's slot tree: CPU on top, then a
/// two-column body with [GPUs, Mem, Net, Disk] on the left and
/// Proc on the right.
fn all_layout_spec() -> Slot {
    let mut left: Vec<Slot> = Vec::with_capacity(crate::config::MAX_GPUS + 3);
    for n in 0..crate::config::MAX_GPUS as u8 {
        left.push(Slot::Widget(WidgetKind::Gpu(n)));
    }
    left.push(Slot::Widget(WidgetKind::Mem));
    left.push(Slot::Widget(WidgetKind::Net));
    left.push(Slot::Widget(WidgetKind::Disk));
    Slot::VStack(vec![
        Slot::Widget(WidgetKind::Cpu),
        Slot::HStack(vec![
            HStackChild::new(Slot::VStack(left), LEFT_WEIGHT),
            HStackChild::new(Slot::Widget(WidgetKind::Proc), PROC_WEIGHT),
        ]),
    ])
}

/// Build the `cpu+gpu+proc` preset's slot tree: CPU on top, every
/// supported GPU stacked below at preferred height, Proc absorbing
/// the remaining height.
fn cpu_gpu_proc_layout_spec() -> Slot {
    let mut col: Vec<Slot> = Vec::with_capacity(crate::config::MAX_GPUS + 2);
    col.push(Slot::Widget(WidgetKind::Cpu));
    for n in 0..crate::config::MAX_GPUS as u8 {
        col.push(Slot::Widget(WidgetKind::Gpu(n)));
    }
    col.push(Slot::Widget(WidgetKind::Proc));
    Slot::VStack(col)
}

/// Cursor identifying which preset is currently active. The
/// builtins are shaped data; `Custom` is the user's mutable slot
/// whose layout lives in `Config::custom` (a [`CustomLayout`]).
///
/// The cycle order (`Self::next` / `Self::prev`) visits every
/// builtin in [`BuiltinPreset::ALL`] order, then `Custom`, then
/// wraps. [`Self::CYCLE_LEN`] positions total
/// (`BuiltinPreset::COUNT + 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ActivePreset {
    Builtin(BuiltinPreset),
    #[default]
    Custom,
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
    pub fn new(active: ActivePreset) -> Self {
        Self {
            active,
            invalid: None,
        }
    }

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

/// The persisted layout for the custom preset slot.
///
/// Stored as a single [`Slot`] tree — the canonical layout
/// representation. Serialised as a TOML `[layout]` table with one
/// `shape` field carrying the DSL string form of the tree:
///
/// ```toml
/// [layout]
/// shape = "vstack(cpu, hstack(40:vstack(mem, net, disk), 60:proc))"
/// ```
///
/// On first launch (no `[layout]` table in `rtop.toml`) the default
/// is `BuiltinPreset::All`'s tree, matching the previous default of
/// "every widget visible".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomLayout {
    /// The custom layout's [`Slot`] tree. Persisted under the
    /// `shape` TOML key (the user-facing name); the field name
    /// `root` is the type-internal moniker.
    #[serde(rename = "shape")]
    pub root: Slot,
}

impl Default for CustomLayout {
    /// First-launch default: clone the `all` preset's tree so the
    /// user sees the dashboard view straight away. Toggling
    /// individual widgets off (`1`-`9`) edits this tree in place;
    /// the user can also rewrite the `shape` DSL string directly.
    fn default() -> Self {
        Self {
            root: BuiltinPreset::All.layout_spec(),
        }
    }
}

impl CustomLayout {
    /// Borrow this custom layout's [`Slot`] tree as an owned clone
    /// for engine consumption. The engine takes the tree by value;
    /// custom layouts can be edited in place between frames.
    pub fn layout_spec(&self) -> Slot {
        self.root.clone()
    }
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

    // -- BuiltinPreset::layout_spec --

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
        assert_eq!(children.len(), 2);
        assert!(matches!(children[0], Slot::Widget(WidgetKind::Cpu)));
        let Slot::HStack(hstack) = &children[1] else {
            panic!("`all` body should be an HStack, got {:?}", children[1]);
        };
        assert_eq!(hstack.len(), 2);
        // Right column is always proc.
        assert!(matches!(hstack[1].slot, Slot::Widget(WidgetKind::Proc)));
        // Left column is a VStack containing every GPU + mem + net + disk.
        let Slot::VStack(left) = &hstack[0].slot else {
            panic!("`all` left column should be a VStack");
        };
        assert_eq!(left.len(), crate::config::MAX_GPUS + 3);
    }

    #[test]
    fn builtin_cpu_proc_layout_spec_is_two_widget_vstack() {
        let spec = BuiltinPreset::CpuProc.layout_spec();
        assert_eq!(
            spec,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Proc),
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
            ])
        );
        assert_eq!(
            BuiltinPreset::DiskProc.layout_spec(),
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Disk),
                Slot::Widget(WidgetKind::Proc),
            ])
        );
    }

    #[test]
    fn builtin_cpu_net_proc_layout_spec_is_cpu_then_two_column_net_proc() {
        let spec = BuiltinPreset::CpuNetProc.layout_spec();
        let Slot::VStack(children) = &spec else {
            panic!("`cpu+net+proc` should be a VStack");
        };
        assert_eq!(children.len(), 2);
        assert!(matches!(children[0], Slot::Widget(WidgetKind::Cpu)));
        let Slot::HStack(hstack) = &children[1] else {
            panic!("`cpu+net+proc` body should be an HStack");
        };
        assert_eq!(hstack.len(), 2);
        assert!(matches!(hstack[0].slot, Slot::Widget(WidgetKind::Net)));
        assert!(matches!(hstack[1].slot, Slot::Widget(WidgetKind::Proc)));
    }

    #[test]
    fn builtin_cpu_gpu_proc_layout_spec_is_single_column() {
        let spec = BuiltinPreset::CpuGpuProc.layout_spec();
        let Slot::VStack(children) = &spec else {
            panic!("`cpu+gpu+proc` should be a VStack");
        };
        // CPU + every GPU + proc, all in one column.
        assert_eq!(children.len(), 2 + crate::config::MAX_GPUS);
        assert!(matches!(children[0], Slot::Widget(WidgetKind::Cpu)));
        for n in 0..crate::config::MAX_GPUS as u8 {
            assert!(matches!(
                children[1 + n as usize],
                Slot::Widget(WidgetKind::Gpu(m)) if m == n,
            ));
        }
        assert!(matches!(
            children[children.len() - 1],
            Slot::Widget(WidgetKind::Proc)
        ));
    }

    #[test]
    fn builtin_cpu_net_mem_disk_layout_spec_is_single_column_in_visual_order() {
        let spec = BuiltinPreset::CpuNetMemDisk.layout_spec();
        // The preset name encodes the visual order top-to-bottom.
        assert_eq!(
            spec,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Net),
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Disk),
            ])
        );
    }

    // -- ActivePreset --

    #[test]
    fn active_preset_default_is_custom() {
        assert_eq!(ActivePreset::default(), ActivePreset::Custom);
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

    // -- PresetField --

    #[test]
    fn preset_field_serialises_as_canonical_name() {
        for &b in &BuiltinPreset::ALL {
            let field = PresetField::new(ActivePreset::Builtin(b));
            let value = toml::Value::try_from(&field).unwrap();
            assert_eq!(value, toml::Value::String(b.name().to_string()));
        }
        let field = PresetField::new(ActivePreset::Custom);
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
            let field = PresetField::new(active);
            let serialised = toml::Value::try_from(&field).unwrap();
            let mut deserialised: PresetField = serialised.try_into().unwrap();
            assert_eq!(deserialised.active(), active);
            assert_eq!(deserialised.take_invalid(), None);
        }
    }

    // -- CustomLayout --

    #[test]
    fn custom_layout_default_is_all_preset_tree() {
        let layout = CustomLayout::default();
        assert_eq!(layout.root, BuiltinPreset::All.layout_spec());
    }

    #[test]
    fn custom_layout_round_trips_through_toml_via_dsl_string() {
        let layout = CustomLayout {
            root: Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Mem),
            ]),
        };
        let value = toml::Value::try_from(&layout).unwrap();
        // Persisted as `shape = "vstack(cpu, mem)"` — a TOML table
        // with one string field.
        let table = value
            .as_table()
            .expect("CustomLayout serialises as a table");
        assert_eq!(
            table.get("shape").and_then(|v| v.as_str()),
            Some("vstack(cpu, mem)"),
        );
        let loaded: CustomLayout = value.try_into().unwrap();
        assert_eq!(loaded, layout);
    }

    #[test]
    fn custom_layout_layout_spec_returns_root_clone() {
        let layout = CustomLayout {
            root: Slot::Widget(WidgetKind::Cpu),
        };
        assert_eq!(layout.layout_spec(), Slot::Widget(WidgetKind::Cpu));
    }
}
