//! Typed identifier for the named layout widgets (cpu, mem, net,
//! proc, disk, gpu). Replaces the prior `Vec<String>` /
//! `&[&'static str]` representation throughout layout, preset,
//! and toggle code so that:
//!
//! - The set of valid widgets is checked by the type system rather
//!   than by a runtime `is_valid_box_name` helper.
//! - Builtin presets and the user's custom layout share one
//!   element type so the `Config` runtime cache can be removed.
//! - Adding a new variant fails-loud at every `match` site.
//!
//! TOML serialisation produces plain strings ("cpu", "gpu", …)
//! via the bespoke [`Serialize`]/[`Deserialize`] impls below; the
//! enum's variant shape is an internal concern.

use std::fmt::{self, Display};
use std::str::FromStr;

use serde::de::{self, Deserializer, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

/// One layout widget. Each variant is a singleton — including
/// `Gpu`, which represents a single user-cycled GPU widget that
/// renders one of the per-device snapshots at a time. The
/// per-device data still flows through `N` collector threads (one
/// per discovered device); the runtime cursor in
/// [`crate::app::GpuViewState`] picks which device the singleton
/// widget displays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WidgetKind {
    Cpu,
    Mem,
    Net,
    Proc,
    Disk,
    Gpu,
    /// Borderless 1-row widget rendered as part of a layout (typically
    /// the last child of the outermost vertical stack). Hosts the
    /// menu / preset / update-interval hints (left section) and
    /// uptime / clock (right section). Driven by the dedicated
    /// statusbar collector at a fixed 1 Hz cadence.
    Statusbar,
}

/// How a widget interacts with its enclosing container's slack along
/// the major axis (vertical stack height; in a horizontal stack the
/// major axis is width but no widget currently distinguishes there).
///
/// Lives on [`WidgetKind`] as an intrinsic property so the layout
/// engine never asks "is this widget net?" or "is this widget proc?"
/// to decide who absorbs leftover space. Adding a future widget only
/// requires answering "Preferred or Fill?" once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetSizing {
    /// Widget renders at a fixed/preferred height derived from data
    /// hints (core count, swap visibility, disk count, …). Surplus
    /// space goes to `Fill` siblings (or stays empty if none).
    Preferred,
    /// Widget absorbs slack along the parent stack's axis. Has a
    /// minimum height; otherwise grows to fill. Multiple `Fill`
    /// siblings in one container share equally.
    Fill,
}

impl WidgetKind {
    /// Numeric keybind that toggles this widget's visibility from
    /// the normal view. `1`-`6` for the six base widgets;
    /// `Statusbar` returns `None` — it has no number keybind and
    /// is toggled exclusively via the options-menu `statusbar`
    /// tab. Mirrors
    /// [`crate::handlers::normal::toggle_widget_main_action`].
    pub const fn toggle_key(self) -> Option<char> {
        match self {
            Self::Cpu => Some('1'),
            Self::Mem => Some('2'),
            Self::Net => Some('3'),
            Self::Proc => Some('4'),
            Self::Disk => Some('5'),
            Self::Gpu => Some('6'),
            Self::Statusbar => None,
        }
    }

    /// Iterate over every supported [`WidgetKind`] in canonical
    /// order: the six base widgets, then `Statusbar`. Used by
    /// [`crate::dirty::RenderDirty`] for "all widgets" operations
    /// and by anywhere else that needs to walk the full universe of
    /// widget kinds.
    pub fn all() -> impl Iterator<Item = WidgetKind> {
        const ALL: [WidgetKind; 7] = [
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
            WidgetKind::Disk,
            WidgetKind::Gpu,
            WidgetKind::Statusbar,
        ];
        ALL.into_iter()
    }

    /// Intrinsic sizing classification — see [`WidgetSizing`].
    ///
    /// This is the **only** place the layout engine asks a widget
    /// "do you have a fixed preferred height, or do you absorb slack?".
    /// Adding a new widget kind requires answering this once; the
    /// engine handles every distribution decision uniformly from there.
    pub const fn sizing(self) -> WidgetSizing {
        match self {
            Self::Cpu | Self::Mem | Self::Disk | Self::Gpu | Self::Statusbar => {
                WidgetSizing::Preferred
            }
            Self::Net | Self::Proc => WidgetSizing::Fill,
        }
    }
}

// Compile-time invariants on the widget sizing classification. These
// pin the contract documented on `WidgetKind::sizing`.
const _: () = {
    assert!(matches!(WidgetKind::Cpu.sizing(), WidgetSizing::Preferred));
    assert!(matches!(WidgetKind::Mem.sizing(), WidgetSizing::Preferred));
    assert!(matches!(WidgetKind::Disk.sizing(), WidgetSizing::Preferred));
    assert!(matches!(WidgetKind::Gpu.sizing(), WidgetSizing::Preferred));
    assert!(matches!(WidgetKind::Net.sizing(), WidgetSizing::Fill));
    assert!(matches!(WidgetKind::Proc.sizing(), WidgetSizing::Fill));
    assert!(matches!(
        WidgetKind::Statusbar.sizing(),
        WidgetSizing::Preferred
    ));
};

impl Display for WidgetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu => f.write_str("cpu"),
            Self::Mem => f.write_str("mem"),
            Self::Net => f.write_str("net"),
            Self::Proc => f.write_str("proc"),
            Self::Disk => f.write_str("disk"),
            Self::Gpu => f.write_str("gpu"),
            Self::Statusbar => f.write_str("statusbar"),
        }
    }
}

/// Error returned when a string cannot be parsed into a
/// [`WidgetKind`]. Carries the offending input so callers can surface
/// a useful warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseWidgetKindError(pub String);

impl Display for ParseWidgetKindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid widget name '{}'", self.0)
    }
}

impl std::error::Error for ParseWidgetKindError {}

impl FromStr for WidgetKind {
    type Err = ParseWidgetKindError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "cpu" => Ok(Self::Cpu),
            "mem" => Ok(Self::Mem),
            "net" => Ok(Self::Net),
            "proc" => Ok(Self::Proc),
            "disk" => Ok(Self::Disk),
            "gpu" => Ok(Self::Gpu),
            "statusbar" => Ok(Self::Statusbar),
            other => Err(ParseWidgetKindError(other.to_string())),
        }
    }
}

impl Serialize for WidgetKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for WidgetKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct WidgetKindVisitor;

        impl Visitor<'_> for WidgetKindVisitor {
            type Value = WidgetKind;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "a widget name (\"cpu\", \"mem\", \"net\", \"proc\", \"disk\", \"gpu\", or \"statusbar\")",
                )
            }

            fn visit_str<E: de::Error>(self, s: &str) -> Result<WidgetKind, E> {
                s.parse().map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_str(WidgetKindVisitor)
    }
}

/// Typed indexed container with one slot per [`WidgetKind`].
///
/// Provides exhaustive enum-keyed storage for per-widget values
/// without runtime hashing or `Option` lookup misses. Used by
/// `draw::layout::Layout` and [`crate::domain::widget_set::WidgetSet`].
///
/// Each variant occupies a single field — including `Gpu`, which
/// is a singleton (the per-device GPU snapshots live on
/// [`crate::app::LiveData::gpu`] as a `Vec<Option<Arc<...>>>`,
/// independent of the widget surface).
///
/// `Copy`-derived when `T: Copy` so containers built on top
/// (e.g. [`crate::dirty::RenderDirty`]) can be cheaply passed by
/// value through the per-frame render pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerWidget<T> {
    cpu: T,
    mem: T,
    net: T,
    process: T,
    disk: T,
    gpu: T,
    statusbar: T,
}

impl<T: Default> Default for PerWidget<T> {
    fn default() -> Self {
        Self {
            cpu: T::default(),
            mem: T::default(),
            net: T::default(),
            process: T::default(),
            disk: T::default(),
            gpu: T::default(),
            statusbar: T::default(),
        }
    }
}

impl<T> PerWidget<T> {
    /// Borrow the slot for `kind`.
    pub fn get(&self, kind: WidgetKind) -> &T {
        match kind {
            WidgetKind::Cpu => &self.cpu,
            WidgetKind::Mem => &self.mem,
            WidgetKind::Net => &self.net,
            WidgetKind::Proc => &self.process,
            WidgetKind::Disk => &self.disk,
            WidgetKind::Gpu => &self.gpu,
            WidgetKind::Statusbar => &self.statusbar,
        }
    }

    /// Mutably borrow the slot for `kind`.
    pub fn get_mut(&mut self, kind: WidgetKind) -> &mut T {
        match kind {
            WidgetKind::Cpu => &mut self.cpu,
            WidgetKind::Mem => &mut self.mem,
            WidgetKind::Net => &mut self.net,
            WidgetKind::Proc => &mut self.process,
            WidgetKind::Disk => &mut self.disk,
            WidgetKind::Gpu => &mut self.gpu,
            WidgetKind::Statusbar => &mut self.statusbar,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizing_classifies_preferred_widgets() {
        assert_eq!(WidgetKind::Cpu.sizing(), WidgetSizing::Preferred);
        assert_eq!(WidgetKind::Mem.sizing(), WidgetSizing::Preferred);
        assert_eq!(WidgetKind::Disk.sizing(), WidgetSizing::Preferred);
        assert_eq!(WidgetKind::Gpu.sizing(), WidgetSizing::Preferred);
        assert_eq!(WidgetKind::Statusbar.sizing(), WidgetSizing::Preferred);
    }

    #[test]
    fn sizing_classifies_net_and_proc_as_fill() {
        assert_eq!(WidgetKind::Net.sizing(), WidgetSizing::Fill);
        assert_eq!(WidgetKind::Proc.sizing(), WidgetSizing::Fill);
    }

    #[test]
    fn sizing_is_const_callable() {
        const _PROC: WidgetSizing = WidgetKind::Proc.sizing();
        const _CPU: WidgetSizing = WidgetKind::Cpu.sizing();
        const _GPU: WidgetSizing = WidgetKind::Gpu.sizing();
        const _SB: WidgetSizing = WidgetKind::Statusbar.sizing();
    }

    #[test]
    fn toggle_keys_cover_six_base_widgets_only() {
        assert_eq!(WidgetKind::Cpu.toggle_key(), Some('1'));
        assert_eq!(WidgetKind::Mem.toggle_key(), Some('2'));
        assert_eq!(WidgetKind::Net.toggle_key(), Some('3'));
        assert_eq!(WidgetKind::Proc.toggle_key(), Some('4'));
        assert_eq!(WidgetKind::Disk.toggle_key(), Some('5'));
        assert_eq!(WidgetKind::Gpu.toggle_key(), Some('6'));
        // Statusbar is reachable only via the options-menu `statusbar`
        // tab; it intentionally has no number-key keybind.
        assert_eq!(WidgetKind::Statusbar.toggle_key(), None);
    }

    #[test]
    fn all_iterator_lists_seven_widgets_in_canonical_order() {
        let kinds: Vec<WidgetKind> = WidgetKind::all().collect();
        assert_eq!(
            kinds,
            vec![
                WidgetKind::Cpu,
                WidgetKind::Mem,
                WidgetKind::Net,
                WidgetKind::Proc,
                WidgetKind::Disk,
                WidgetKind::Gpu,
                WidgetKind::Statusbar,
            ]
        );
    }

    #[test]
    fn display_round_trips_for_every_variant() {
        for variant in WidgetKind::all() {
            let s = variant.to_string();
            assert_eq!(s.parse::<WidgetKind>().unwrap(), variant);
        }
    }

    #[test]
    fn from_str_rejects_unknown_names() {
        assert!("foo".parse::<WidgetKind>().is_err());
        assert!("Cpu".parse::<WidgetKind>().is_err()); // case-sensitive
        assert!("".parse::<WidgetKind>().is_err());
        // Pre-refactor, `gpu0`..`gpu7` were valid widget names.
        // Cycling-GPU collapses them to a single `gpu` — the
        // indexed forms must now reject.
        assert!("gpu0".parse::<WidgetKind>().is_err());
        assert!("gpu1".parse::<WidgetKind>().is_err());
    }

    #[test]
    fn per_widget_default_is_default_for_every_slot() {
        let p = PerWidget::<bool>::default();
        for kind in WidgetKind::all() {
            assert!(!*p.get(kind));
        }
    }

    #[test]
    fn per_widget_slots_are_independent() {
        let mut p = PerWidget::<u32>::default();
        for (i, kind) in WidgetKind::all().enumerate() {
            *p.get_mut(kind) = i as u32 + 1;
        }
        assert_eq!(*p.get(WidgetKind::Cpu), 1);
        assert_eq!(*p.get(WidgetKind::Mem), 2);
        assert_eq!(*p.get(WidgetKind::Net), 3);
        assert_eq!(*p.get(WidgetKind::Proc), 4);
        assert_eq!(*p.get(WidgetKind::Disk), 5);
        assert_eq!(*p.get(WidgetKind::Gpu), 6);
        assert_eq!(*p.get(WidgetKind::Statusbar), 7);
    }

    #[test]
    fn per_widget_is_copy_when_t_is_copy() {
        // Pin `Copy` so containers built on top (RenderDirty,
        // Layout) can be passed by value through the per-frame
        // render pipeline without churning their signatures.
        fn assert_copy<T: Copy>() {}
        assert_copy::<PerWidget<bool>>();
        assert_copy::<PerWidget<i32>>();
    }
}
