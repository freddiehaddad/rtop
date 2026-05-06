//! Layout presets — a fixed set of curated configurations the user
//! can cycle through with `p` / `P`.
//!
//! Presets are read-only and ship with rtop. There is no runtime
//! "save preset" or "delete preset" — users who need a different
//! layout edit the `custom_*` fields in `rtop.toml` directly or
//! cycle to the custom preset (the slot beyond `BuiltinPreset::COUNT`).
//! The only preset state persisted across runs is `Config::preset`,
//! a [`PresetField`] storing the active cursor by canonical name.

use crate::domain::widget_kind::{WidgetKind, WidgetList};
use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Identity of one of the curated, immutable layout presets that
/// ship with rtop. The associated layout data lives in a private
/// const table; methods on `BuiltinPreset` expose it.
///
/// Adding a variant is a three-step change: extend the enum, extend
/// the const data table, and bump [`Self::COUNT`] / [`Self::ALL`].
/// The const assert below guarantees the table and the enum stay
/// aligned at compile time. Variant declaration order MUST match
/// the order of entries in [`BUILTIN_PRESETS`] because [`Self::data`]
/// indexes the table with `self as usize`.
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
    /// CPU + memory + disk — passive system-utilisation view with
    /// no process noise and no network.
    CpuMemDisk,
}

/// Per-preset layout data. Private to this module — callers go
/// through [`BuiltinPreset`]'s accessors so the data table and the
/// enum can't drift apart.
struct PresetData {
    name: &'static str,
    widgets: &'static [WidgetKind],
    cpu_bottom: bool,
    mem_below_net: bool,
    proc_left: bool,
    /// `true` collapses the layout into a single full-width column.
    /// Left-column widgets stack at their preferred heights from the
    /// top; proc (when present) absorbs the remaining height at the
    /// bottom. Used for presets whose widgets have small intrinsic
    /// heights and would otherwise stretch to fill an oversized
    /// column in the default 2-column layout.
    stack_vertical: bool,
}

/// Widget list for the `all` builtin: every widget rtop knows
/// about (the five base widgets plus a `Gpu(N)` entry for every
/// supported index). Constructed at compile time so `MAX_GPUS`
/// is the single source of truth — adjust it and the array
/// resizes automatically.
const PRESET_ALL_WIDGETS: &[WidgetKind] = {
    const BASE_LEN: usize = 5;
    const TOTAL: usize = BASE_LEN + crate::config::MAX_GPUS;
    const fn build() -> [WidgetKind; TOTAL] {
        let mut arr = [WidgetKind::Cpu; TOTAL];
        arr[0] = WidgetKind::Cpu;
        arr[1] = WidgetKind::Mem;
        arr[2] = WidgetKind::Net;
        arr[3] = WidgetKind::Proc;
        arr[4] = WidgetKind::Disk;
        let mut i = 0;
        while i < crate::config::MAX_GPUS {
            arr[BASE_LEN + i] = WidgetKind::Gpu(i as u8);
            i += 1;
        }
        arr
    }
    const ARR: [WidgetKind; TOTAL] = build();
    &ARR
};

/// Widget list for the `cpu+gpu+proc` builtin: CPU at the top,
/// every supported GPU index in the middle, processes on the
/// right (or wherever proc_left places it). Like
/// [`PRESET_ALL_WIDGETS`], the GPU run is built at compile time
/// so `MAX_GPUS` stays the single source of truth, and the layout
/// engine drops `Gpu(N)` entries whose index exceeds the detected
/// GPU count.
const PRESET_CPU_GPU_PROC_WIDGETS: &[WidgetKind] = {
    const TOTAL: usize = 2 + crate::config::MAX_GPUS;
    const fn build() -> [WidgetKind; TOTAL] {
        let mut arr = [WidgetKind::Cpu; TOTAL];
        arr[0] = WidgetKind::Cpu;
        let mut i = 0;
        while i < crate::config::MAX_GPUS {
            arr[1 + i] = WidgetKind::Gpu(i as u8);
            i += 1;
        }
        arr[1 + crate::config::MAX_GPUS] = WidgetKind::Proc;
        arr
    }
    const ARR: [WidgetKind; TOTAL] = build();
    &ARR
};

const BUILTIN_PRESETS: [PresetData; BuiltinPreset::COUNT] = [
    PresetData {
        name: "all",
        widgets: PRESET_ALL_WIDGETS,
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
        stack_vertical: false,
    },
    PresetData {
        name: "cpu+proc",
        widgets: &[WidgetKind::Cpu, WidgetKind::Proc],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
        stack_vertical: false,
    },
    PresetData {
        name: "mem+proc",
        widgets: &[WidgetKind::Mem, WidgetKind::Proc],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
        stack_vertical: true,
    },
    PresetData {
        name: "disk+proc",
        widgets: &[WidgetKind::Disk, WidgetKind::Proc],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
        stack_vertical: true,
    },
    PresetData {
        name: "cpu+net+proc",
        widgets: &[WidgetKind::Cpu, WidgetKind::Net, WidgetKind::Proc],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
        stack_vertical: false,
    },
    PresetData {
        name: "cpu+gpu+proc",
        widgets: PRESET_CPU_GPU_PROC_WIDGETS,
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
        stack_vertical: true,
    },
    PresetData {
        name: "cpu+mem+disk",
        widgets: &[WidgetKind::Cpu, WidgetKind::Mem, WidgetKind::Disk],
        cpu_bottom: false,
        mem_below_net: false,
        proc_left: false,
        stack_vertical: true,
    },
];

// Compile-time guarantee that the data table and the enum stay in
// sync. If a variant is added or removed, this assert forces the
// table to be widened/narrowed in the same change.
const _: () = assert!(BUILTIN_PRESETS.len() == BuiltinPreset::COUNT);

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
        Self::CpuMemDisk,
    ];

    /// Stable, user-visible identifier for the preset (used in
    /// the CPU widget bottom hint and in TOML serialisation).
    pub fn name(self) -> &'static str {
        self.data().name
    }

    /// Widget list for this preset, in display order.
    pub fn widgets(self) -> &'static [WidgetKind] {
        self.data().widgets
    }

    /// `true` to render CPU at the bottom of the screen.
    pub fn cpu_bottom(self) -> bool {
        self.data().cpu_bottom
    }

    /// `true` to position memory below network in the left column.
    pub fn mem_below_net(self) -> bool {
        self.data().mem_below_net
    }

    /// `true` to render the process widget on the left.
    pub fn proc_left(self) -> bool {
        self.data().proc_left
    }

    /// `true` collapses the layout into a single full-width column —
    /// left-column widgets at preferred heights, proc absorbs slack
    /// at the bottom. See [`PresetData::stack_vertical`].
    pub fn stack_vertical(self) -> bool {
        self.data().stack_vertical
    }

    /// Resolve a preset by its canonical [`Self::name`].
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == s)
    }

    fn data(self) -> &'static PresetData {
        // The `as usize` cast is sound because `BuiltinPreset` has
        // implicit discriminants that start at 0 and step by 1, and
        // the const assert above guarantees the table covers every
        // variant.
        &BUILTIN_PRESETS[self as usize]
    }
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
/// Mirrors [`crate::domain::widget_kind::WidgetList`]'s pattern:
/// holds the typed value plus a side channel for the offending
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

/// The persisted layout for the custom preset slot — the four
/// orientation-and-visibility fields rolled into one block. The
/// builtin presets carry their layout as static [`PresetData`];
/// this struct is the owned, mutable counterpart that
/// `Config::custom` stores and `Config::layout()` borrows from
/// when the cursor is on [`ActivePreset::Custom`].
///
/// Serialised as a TOML `[layout]` table (named via
/// `#[serde(rename = "layout")]` on the `Config::custom` field).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomLayout {
    pub widgets: WidgetList,
    pub cpu_bottom: bool,
    pub mem_below_net: bool,
    pub proc_left: bool,
    /// Mirrors [`PresetData::stack_vertical`] for custom layouts.
    /// Defaults to `false` (the standard 2-column layout) so old
    /// configs without the field continue to work.
    pub stack_vertical: bool,
}

impl Default for CustomLayout {
    /// First-launch default: every widget rtop knows about (the
    /// five base widgets plus a `Gpu(N)` entry for every supported
    /// index). The layout engine drops Gpu entries whose index is
    /// `>= detected gpu_count`, so the list can safely include all
    /// of them — widgets only render when both listed and backed
    /// by hardware. New GPUs plugged in later are picked up
    /// automatically because their index is already in the list.
    fn default() -> Self {
        let mut widgets = vec![
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
            WidgetKind::Disk,
        ];
        widgets.extend((0..crate::config::MAX_GPUS).filter_map(WidgetKind::gpu));
        Self {
            widgets: WidgetList::from_kinds(widgets),
            cpu_bottom: false,
            mem_below_net: false,
            proc_left: false,
            stack_vertical: false,
        }
    }
}

/// Borrowed view of the live layout (the layout currently in
/// effect on screen). Returned by `Config::layout()`.
///
/// The view is built on demand and is the same shape regardless
/// of whether the cursor is on a builtin (where `widgets` borrows
/// the `&'static` slice from [`PresetData`]) or `Custom` (where
/// `widgets` borrows from `Config::custom`). No allocation
/// happens on the read path: builtins return their static slice
/// unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveLayout<'a> {
    pub widgets: &'a [WidgetKind],
    pub cpu_bottom: bool,
    pub mem_below_net: bool,
    pub proc_left: bool,
    pub stack_vertical: bool,
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

    #[test]
    fn all_presets_have_at_least_one_widget() {
        for &preset in &BuiltinPreset::ALL {
            assert!(
                !preset.widgets().is_empty(),
                "preset '{}' must reference at least one widget",
                preset.name(),
            );
        }
    }

    #[test]
    fn first_preset_is_all_widgets() {
        let p = BuiltinPreset::All;
        assert_eq!(p.name(), "all");
        let widgets = p.widgets();
        // 5 base widgets + every supported GPU index. The layout
        // engine drops Gpu entries whose index is `>= detected
        // gpu_count`, so listing all 8 here is the right thing.
        assert_eq!(widgets.len(), 5 + crate::config::MAX_GPUS);
        assert!(widgets.contains(&WidgetKind::Cpu));
        assert!(widgets.contains(&WidgetKind::Mem));
        assert!(widgets.contains(&WidgetKind::Net));
        assert!(widgets.contains(&WidgetKind::Proc));
        assert!(widgets.contains(&WidgetKind::Disk));
        for i in 0..crate::config::MAX_GPUS {
            let gpu = WidgetKind::gpu(i).expect("0..MAX_GPUS is in range");
            assert!(widgets.contains(&gpu), "preset 'all' must list {gpu}");
        }
        assert!(!p.cpu_bottom());
        assert!(!p.mem_below_net());
        assert!(!p.proc_left());
    }

    #[test]
    fn cpu_gpu_proc_preset_lists_cpu_every_gpu_and_proc() {
        let p = BuiltinPreset::CpuGpuProc;
        assert_eq!(p.name(), "cpu+gpu+proc");
        let widgets = p.widgets();
        // CPU + every supported GPU + proc, in that order.
        assert_eq!(widgets.len(), 2 + crate::config::MAX_GPUS);
        assert_eq!(widgets[0], WidgetKind::Cpu);
        assert_eq!(widgets[widgets.len() - 1], WidgetKind::Proc);
        for i in 0..crate::config::MAX_GPUS {
            let gpu = WidgetKind::gpu(i).expect("0..MAX_GPUS is in range");
            assert!(
                widgets.contains(&gpu),
                "preset 'cpu+gpu+proc' must list {gpu}"
            );
        }
        // Mem/Net/Disk are intentionally absent.
        assert!(!widgets.contains(&WidgetKind::Mem));
        assert!(!widgets.contains(&WidgetKind::Net));
        assert!(!widgets.contains(&WidgetKind::Disk));
    }

    #[test]
    fn diagnostic_pair_presets_are_resource_plus_proc() {
        // The mem+proc and disk+proc presets are deliberately
        // two-widget pairs, mirroring cpu+proc. Each pairs one
        // primary resource with the process list for diagnostic
        // workflows.
        let mem_proc = BuiltinPreset::MemProc;
        assert_eq!(mem_proc.name(), "mem+proc");
        assert_eq!(mem_proc.widgets(), &[WidgetKind::Mem, WidgetKind::Proc]);

        let disk_proc = BuiltinPreset::DiskProc;
        assert_eq!(disk_proc.name(), "disk+proc");
        assert_eq!(disk_proc.widgets(), &[WidgetKind::Disk, WidgetKind::Proc]);
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
        // Forward cycle visits every builtin in BuiltinPreset::ALL
        // order, then Custom, then wraps. Derived from ALL rather
        // than hardcoded so the test stays correct as the cycle
        // grows or is reordered.
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
        // After CYCLE_LEN steps we're back where we started.
        assert_eq!(cursor, order[0]);
    }

    #[test]
    fn active_preset_prev_walks_full_cycle_then_wraps() {
        // Backward cycle is the reverse of the forward order.
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
        // Subsequent take returns None.
        assert_eq!(field.take_invalid(), None);
    }

    #[test]
    fn preset_field_round_trips_through_toml_for_all_active_values() {
        // Iterate ALL plus Custom rather than hardcoding the
        // variants — this test stays correct as the catalog grows.
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
    fn custom_layout_default_includes_every_supported_widget() {
        let layout = CustomLayout::default();
        let kinds = layout.widgets.as_slice();
        assert_eq!(kinds.len(), 5 + crate::config::MAX_GPUS);
        assert!(kinds.contains(&WidgetKind::Cpu));
        assert!(kinds.contains(&WidgetKind::Mem));
        assert!(kinds.contains(&WidgetKind::Net));
        assert!(kinds.contains(&WidgetKind::Proc));
        assert!(kinds.contains(&WidgetKind::Disk));
        for i in 0..crate::config::MAX_GPUS {
            let gpu = WidgetKind::gpu(i).expect("0..MAX_GPUS is in range");
            assert!(kinds.contains(&gpu), "default must list {gpu}");
        }
        assert!(!layout.cpu_bottom);
        assert!(!layout.mem_below_net);
        assert!(!layout.proc_left);
    }

    #[test]
    fn custom_layout_round_trips_through_toml() {
        let layout = CustomLayout {
            widgets: WidgetList::from_kinds([WidgetKind::Cpu, WidgetKind::Mem]),
            cpu_bottom: true,
            mem_below_net: false,
            proc_left: true,
            stack_vertical: true,
        };
        let value = toml::Value::try_from(&layout).unwrap();
        let loaded: CustomLayout = value.try_into().unwrap();
        assert_eq!(
            loaded.widgets.as_slice(),
            &[WidgetKind::Cpu, WidgetKind::Mem]
        );
        assert!(loaded.cpu_bottom);
        assert!(!loaded.mem_below_net);
        assert!(loaded.proc_left);
        assert!(loaded.stack_vertical);
    }
}
