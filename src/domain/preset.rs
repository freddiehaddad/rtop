//! Layout presets — a fixed set of curated configurations the user
//! can cycle through with `p` / `P`.
//!
//! Presets are read-only and ship with rtop. There is no runtime
//! "save preset" or "delete preset" — users who need a different
//! layout edit the `custom_*` fields in `rtop.toml` directly or
//! cycle to the custom preset (the slot beyond `BuiltinPreset::COUNT`).
//! The only preset state persisted across runs is `Config::preset`,
//! a [`PresetField`] storing the active cursor by canonical name.

use crate::domain::layout_spec::{HStackChild, Slot};
use crate::domain::widget_kind::{WidgetKind, WidgetList};
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
    /// CPU + network + memory + disk — passive system-utilisation
    /// view with no process noise. Net absorbs slack so the column
    /// fills the screen without empty space.
    CpuNetMemDisk,
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
        name: "cpu+net+mem+disk",
        widgets: &[
            WidgetKind::Cpu,
            WidgetKind::Net,
            WidgetKind::Mem,
            WidgetKind::Disk,
        ],
        cpu_bottom: false,
        // Net renders above mem in the left column so the visual
        // order matches the preset name `cpu+net+mem+disk`.
        mem_below_net: true,
        proc_left: false,
        // Use the 2-column path (which collapses to a single
        // full-width column when proc is absent) so net's existing
        // slack-absorbing behaviour fills the available height.
        stack_vertical: false,
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
        Self::CpuNetMemDisk,
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

impl CustomLayout {
    /// Build a [`Slot`] tree from the legacy widget-list +
    /// orientation-flag representation. Returns `None` when no
    /// widget would render.
    ///
    /// This is a transitional helper: a future commit migrates
    /// [`CustomLayout`] to store a [`Slot`] tree directly (with
    /// DSL serialisation), at which point the orientation-flag
    /// fields and this method go away. Until then, custom layouts
    /// are converted on each frame so the engine consumes the
    /// same canonical input shape as builtin presets.
    pub fn layout_spec(&self) -> Option<Slot> {
        legacy_to_slot(
            self.widgets.as_slice(),
            self.cpu_bottom,
            self.mem_below_net,
            self.proc_left,
            self.stack_vertical,
        )
    }
}

/// Convert the legacy widget-list + orientation-flag representation
/// (used by both [`CustomLayout`] and the persisted `rtop.toml`
/// custom block) into a canonical [`Slot`] tree. Returns `None`
/// when no widget is present.
///
/// GPU indices are preserved verbatim — every `Gpu(n)` listed in
/// `widgets` lands in the tree regardless of detected count. The
/// engine collapses absent devices to zero size at render time.
pub(crate) fn legacy_to_slot(
    widgets: &[WidgetKind],
    cpu_bottom: bool,
    mem_below_net: bool,
    proc_left: bool,
    stack_vertical: bool,
) -> Option<Slot> {
    let has_cpu = widgets.contains(&WidgetKind::Cpu);
    let has_mem = widgets.contains(&WidgetKind::Mem);
    let has_net = widgets.contains(&WidgetKind::Net);
    let has_proc = widgets.contains(&WidgetKind::Proc);
    let has_disk = widgets.contains(&WidgetKind::Disk);
    let gpu_indices: Vec<u8> = (0..crate::config::MAX_GPUS as u8)
        .filter(|n| widgets.contains(&WidgetKind::Gpu(*n)))
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
    if mem_below_net {
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

    let body = if stack_vertical && has_left {
        let mut col = left_col;
        if has_proc {
            col.push(Slot::Widget(WidgetKind::Proc));
        }
        collapse_vstack(col)
    } else if has_proc && has_left {
        let left_slot = collapse_vstack(left_col).expect("has_left implies non-empty left_col");
        let proc_slot = Slot::Widget(WidgetKind::Proc);
        let (first, second) = if proc_left {
            (
                HStackChild::new(proc_slot, PROC_WEIGHT),
                HStackChild::new(left_slot, LEFT_WEIGHT),
            )
        } else {
            (
                HStackChild::new(left_slot, LEFT_WEIGHT),
                HStackChild::new(proc_slot, PROC_WEIGHT),
            )
        };
        Some(Slot::HStack(vec![first, second]))
    } else if has_proc {
        Some(Slot::Widget(WidgetKind::Proc))
    } else {
        collapse_vstack(left_col)
    };

    if has_cpu {
        let cpu = Slot::Widget(WidgetKind::Cpu);
        Some(match body {
            Some(b) if cpu_bottom => Slot::VStack(vec![b, cpu]),
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

    // -- BuiltinPreset::layout_spec --

    fn count_widget_leaves(slot: &Slot) -> usize {
        match slot {
            Slot::Widget(_) => 1,
            Slot::VStack(children) => children.iter().map(count_widget_leaves).sum(),
            Slot::HStack(children) => children.iter().map(|c| count_widget_leaves(&c.slot)).sum(),
        }
    }

    #[test]
    fn builtin_layout_spec_includes_every_widget_listed_in_widgets() {
        // Every widget in `BuiltinPreset::widgets()` must appear in
        // the `layout_spec()` tree at least once. This is the
        // structural contract guaranteeing the static tree and the
        // legacy widget list stay in sync.
        for &preset in &BuiltinPreset::ALL {
            let spec = preset.layout_spec();
            for &kind in preset.widgets() {
                assert!(
                    spec.contains(kind),
                    "{}: layout_spec() missing widget {kind}",
                    preset.name(),
                );
            }
            // And the count of leaves equals the widget list length
            // (no extras, no duplicates).
            assert_eq!(
                count_widget_leaves(&spec),
                preset.widgets().len(),
                "{}: leaf count must equal widgets() length",
                preset.name(),
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
        assert!(matches!(hstack[1].slot, Slot::Widget(WidgetKind::Proc),));
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

    // -- legacy_to_slot --

    #[test]
    fn legacy_to_slot_empty_widget_list_returns_none() {
        assert!(legacy_to_slot(&[], false, false, false, false).is_none());
    }

    #[test]
    fn legacy_to_slot_proc_only_yields_widget_leaf() {
        assert_eq!(
            legacy_to_slot(&[WidgetKind::Proc], false, false, false, false),
            Some(Slot::Widget(WidgetKind::Proc)),
        );
    }

    #[test]
    fn legacy_to_slot_cpu_bottom_swaps_root_vstack_order() {
        let widgets = [WidgetKind::Cpu, WidgetKind::Mem];
        let top = legacy_to_slot(&widgets, false, false, false, false).unwrap();
        let bot = legacy_to_slot(&widgets, true, false, false, false).unwrap();
        assert_eq!(
            top,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Cpu),
                Slot::Widget(WidgetKind::Mem),
            ])
        );
        assert_eq!(
            bot,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Cpu),
            ])
        );
    }

    #[test]
    fn legacy_to_slot_mem_below_net_swaps_left_column_mem_net_order() {
        let widgets = [WidgetKind::Mem, WidgetKind::Net];
        let mem_above_net = legacy_to_slot(&widgets, false, false, false, false).unwrap();
        let mem_below_net = legacy_to_slot(&widgets, false, true, false, false).unwrap();
        assert_eq!(
            mem_above_net,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Net),
            ])
        );
        assert_eq!(
            mem_below_net,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Net),
                Slot::Widget(WidgetKind::Mem),
            ])
        );
    }

    #[test]
    fn legacy_to_slot_proc_left_reverses_hstack_order() {
        let widgets = [WidgetKind::Mem, WidgetKind::Proc];
        let proc_right = legacy_to_slot(&widgets, false, false, false, false).unwrap();
        let proc_left_root = legacy_to_slot(&widgets, false, false, true, false).unwrap();
        let Slot::HStack(right_children) = &proc_right else {
            panic!("expected HStack root for [mem, proc]");
        };
        assert!(matches!(
            right_children[0].slot,
            Slot::Widget(WidgetKind::Mem)
        ));
        assert!(matches!(
            right_children[1].slot,
            Slot::Widget(WidgetKind::Proc)
        ));
        let Slot::HStack(left_children) = &proc_left_root else {
            panic!("expected HStack root with proc_left=true");
        };
        assert!(matches!(
            left_children[0].slot,
            Slot::Widget(WidgetKind::Proc)
        ));
        assert!(matches!(
            left_children[1].slot,
            Slot::Widget(WidgetKind::Mem)
        ));
    }

    #[test]
    fn legacy_to_slot_stack_vertical_collapses_two_column_to_single_vstack() {
        let widgets = [WidgetKind::Mem, WidgetKind::Proc];
        let stacked = legacy_to_slot(&widgets, false, false, false, true).unwrap();
        assert_eq!(
            stacked,
            Slot::VStack(vec![
                Slot::Widget(WidgetKind::Mem),
                Slot::Widget(WidgetKind::Proc),
            ])
        );
    }

    #[test]
    fn legacy_to_slot_includes_all_listed_gpus_regardless_of_index_count() {
        // The conversion no longer filters by `gpu_count` — the
        // engine handles GPU presence at render time.
        let widgets = [
            WidgetKind::Cpu,
            WidgetKind::Gpu(0),
            WidgetKind::Gpu(3),
            WidgetKind::Gpu(7),
            WidgetKind::Proc,
        ];
        let spec = legacy_to_slot(&widgets, false, false, false, false).unwrap();
        assert!(spec.contains(WidgetKind::Gpu(0)));
        assert!(spec.contains(WidgetKind::Gpu(3)));
        assert!(spec.contains(WidgetKind::Gpu(7)));
    }

    #[test]
    fn custom_layout_spec_default_includes_every_listed_widget() {
        let custom = CustomLayout::default();
        let spec = custom.layout_spec().expect("default custom is non-empty");
        for &kind in custom.widgets.as_slice() {
            assert!(
                spec.contains(kind),
                "default custom layout_spec missing {kind}",
            );
        }
    }
}
