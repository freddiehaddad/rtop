//! Typed identifier for the named layout widgets (cpu, mem, net,
//! proc, disk, gpuN). Replaces the prior `Vec<String>` /
//! `&[&'static str]` representation throughout layout, preset,
//! and toggle code so that:
//!
//! - The set of valid widgets is checked by the type system rather
//!   than by a runtime `is_valid_box_name` helper.
//! - Builtin presets and the user's custom layout share one
//!   element type so the `Config` runtime cache can be removed.
//! - Adding a new variant fails-loud at every `match` site.
//!
//! TOML serialisation produces plain strings ("cpu", "gpu0", …)
//! via the bespoke [`Serialize`]/[`Deserialize`] impls below; the
//! enum's variant shape is an internal concern.

use std::fmt::{self, Display};
use std::str::FromStr;

use serde::de::{self, Deserializer, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

use crate::config::MAX_GPUS;

/// One layout widget. The non-GPU variants are fixed; `Gpu(n)` is a
/// per-device widget where `n` is in `0..MAX_GPUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WidgetKind {
    Cpu,
    Mem,
    Net,
    Proc,
    Disk,
    Gpu(u8),
}

impl WidgetKind {
    /// Materialise a `Gpu(n)` for the given index. Returns `None`
    /// if `n >= MAX_GPUS`.
    pub fn gpu(n: usize) -> Option<Self> {
        if n < MAX_GPUS {
            Some(Self::Gpu(n as u8))
        } else {
            None
        }
    }
}

impl Display for WidgetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpu => f.write_str("cpu"),
            Self::Mem => f.write_str("mem"),
            Self::Net => f.write_str("net"),
            Self::Proc => f.write_str("proc"),
            Self::Disk => f.write_str("disk"),
            Self::Gpu(n) => write!(f, "gpu{n}"),
        }
    }
}

/// Error returned when a string cannot be parsed into a
/// [`WidgetKind`]. Carries the offending input so that callers
/// (notably `WidgetList`'s deserialiser) can surface a useful warning.
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
            other => other
                .strip_prefix("gpu")
                .and_then(|suffix| suffix.parse::<u8>().ok())
                .filter(|n| (*n as usize) < MAX_GPUS)
                .map(Self::Gpu)
                .ok_or_else(|| ParseWidgetKindError(other.to_string())),
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
                write!(
                    f,
                    "a widget name (\"cpu\", \"mem\", \"net\", \"proc\", \"disk\", or \"gpuN\" with 0 <= N < {MAX_GPUS})"
                )
            }

            fn visit_str<E: de::Error>(self, s: &str) -> Result<WidgetKind, E> {
                s.parse().map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_str(WidgetKindVisitor)
    }
}

/// A list of [`WidgetKind`]s with deserialise-time error capture.
///
/// `Vec<WidgetKind>` would reject the entire load on a single bad
/// string, which is too strict — we want to drop the bad entries,
/// keep the good ones, and surface a warning. `WidgetList` reads the
/// raw TOML as `Vec<String>`, parses each entry, and stores
/// failures separately so `Config::validate` can warn the user
/// without losing the rest of the layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WidgetList {
    items: Vec<WidgetKind>,
    /// String entries from the last deserialise pass that failed
    /// to parse as a `WidgetKind`. Cleared by `take_invalid` after
    /// `Config::validate` reports them.
    invalid: Vec<String>,
}

impl WidgetList {
    pub fn new(items: Vec<WidgetKind>) -> Self {
        Self {
            items,
            invalid: Vec::new(),
        }
    }

    pub fn from_kinds<I: IntoIterator<Item = WidgetKind>>(iter: I) -> Self {
        Self::new(iter.into_iter().collect())
    }

    pub fn as_slice(&self) -> &[WidgetKind] {
        &self.items
    }

    pub fn push(&mut self, kind: WidgetKind) {
        self.items.push(kind);
    }

    pub fn remove_kind(&mut self, kind: WidgetKind) -> bool {
        if let Some(pos) = self.items.iter().position(|b| *b == kind) {
            self.items.remove(pos);
            true
        } else {
            false
        }
    }

    /// Drain the captured invalid entries (deserialise-time
    /// parse failures) and return them. Used by `Config::validate`
    /// to fold them into the warning list once and then clear so
    /// repeated `validate` calls don't re-report.
    pub fn take_invalid(&mut self) -> Vec<String> {
        std::mem::take(&mut self.invalid)
    }
}

impl Serialize for WidgetList {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // TOML output is the canonical string form; invalid
        // entries are dropped so the saved file matches what the
        // runtime actually used.
        self.items.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WidgetList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Vec::<String>::deserialize(deserializer)?;
        let mut items = Vec::with_capacity(raw.len());
        let mut invalid = Vec::new();
        for s in raw {
            match s.parse::<WidgetKind>() {
                Ok(kind) => items.push(kind),
                Err(_) => invalid.push(s),
            }
        }
        Ok(Self { items, invalid })
    }
}

/// Typed indexed container with one slot per [`WidgetKind`].
///
/// Provides exhaustive enum-keyed storage for per-widget values
/// without runtime hashing or `Option` lookup misses. Used by
/// `draw::layout::Layout` so that GPU widgets are stored under
/// their actual [`WidgetKind::Gpu(n)`] index — preventing the
/// dense-positional `Vec` shape that previously dropped GPU
/// identity when a sparse subset of GPU widgets was enabled.
///
/// The five base widgets each occupy a single field; GPU
/// widgets occupy a fixed-size `[T; MAX_GPUS]` array indexed
/// by the variant payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerWidget<T> {
    cpu: T,
    mem: T,
    net: T,
    process: T,
    disk: T,
    gpu: [T; MAX_GPUS],
}

impl<T: Default> Default for PerWidget<T> {
    fn default() -> Self {
        Self {
            cpu: T::default(),
            mem: T::default(),
            net: T::default(),
            process: T::default(),
            disk: T::default(),
            gpu: std::array::from_fn(|_| T::default()),
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
            WidgetKind::Gpu(n) => &self.gpu[n as usize],
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
            WidgetKind::Gpu(n) => &mut self.gpu[n as usize],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_round_trips_for_static_variants() {
        for variant in [
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
            WidgetKind::Disk,
        ] {
            let s = variant.to_string();
            assert_eq!(s.parse::<WidgetKind>().unwrap(), variant);
        }
    }

    #[test]
    fn display_round_trips_for_gpu_variants() {
        for n in 0..MAX_GPUS {
            let v = WidgetKind::Gpu(n as u8);
            let s = v.to_string();
            assert_eq!(s, format!("gpu{n}"));
            assert_eq!(s.parse::<WidgetKind>().unwrap(), v);
        }
    }

    #[test]
    fn from_str_rejects_unknown_static_name() {
        assert!("foo".parse::<WidgetKind>().is_err());
        assert!("Cpu".parse::<WidgetKind>().is_err()); // case-sensitive
        assert!("".parse::<WidgetKind>().is_err());
    }

    #[test]
    fn from_str_rejects_gpu_index_out_of_range() {
        let max = MAX_GPUS;
        assert!(format!("gpu{max}").parse::<WidgetKind>().is_err());
        assert!("gpu99".parse::<WidgetKind>().is_err());
    }

    #[test]
    fn from_str_rejects_gpu_with_no_digits() {
        assert!("gpu".parse::<WidgetKind>().is_err());
        assert!("gpux".parse::<WidgetKind>().is_err());
    }

    #[test]
    fn gpu_constructor_enforces_max() {
        assert!(WidgetKind::gpu(0).is_some());
        assert_eq!(
            WidgetKind::gpu(MAX_GPUS - 1),
            Some(WidgetKind::Gpu(MAX_GPUS as u8 - 1))
        );
        assert_eq!(WidgetKind::gpu(MAX_GPUS), None);
    }

    #[test]
    fn widget_list_serialise_emits_string_array() {
        let list = WidgetList::from_kinds([WidgetKind::Cpu, WidgetKind::Gpu(0)]);
        let toml = toml::Value::try_from(&list).unwrap();
        assert_eq!(
            toml,
            toml::Value::Array(vec![
                toml::Value::String("cpu".into()),
                toml::Value::String("gpu0".into()),
            ]),
        );
    }

    #[test]
    fn widget_list_deserialise_keeps_valid_drops_invalid() {
        let raw = toml::Value::Array(vec![
            toml::Value::String("cpu".into()),
            toml::Value::String("nope".into()),
            toml::Value::String("gpu1".into()),
        ]);
        let mut list: WidgetList = raw.try_into().unwrap();
        assert_eq!(list.as_slice(), &[WidgetKind::Cpu, WidgetKind::Gpu(1)]);
        assert_eq!(list.take_invalid(), vec!["nope".to_string()]);
        // Subsequent take returns empty.
        assert!(list.take_invalid().is_empty());
    }

    #[test]
    fn widget_list_deserialise_empty_array() {
        let raw = toml::Value::Array(vec![]);
        let list: WidgetList = raw.try_into().unwrap();
        assert!(list.as_slice().is_empty());
    }

    #[test]
    fn widget_list_remove_kind_returns_false_when_absent() {
        let mut list = WidgetList::from_kinds([WidgetKind::Cpu]);
        assert!(!list.remove_kind(WidgetKind::Mem));
        assert!(list.remove_kind(WidgetKind::Cpu));
        assert!(list.as_slice().is_empty());
    }

    #[test]
    fn per_widget_default_is_default_for_every_slot() {
        let p = PerWidget::<bool>::default();
        for kind in [
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
            WidgetKind::Disk,
        ] {
            assert!(!*p.get(kind));
        }
        for n in 0..MAX_GPUS {
            assert!(!*p.get(WidgetKind::Gpu(n as u8)));
        }
    }

    #[test]
    fn per_widget_base_slots_are_independent() {
        let mut p = PerWidget::<u32>::default();
        for (i, kind) in [
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
            WidgetKind::Disk,
        ]
        .into_iter()
        .enumerate()
        {
            *p.get_mut(kind) = i as u32 + 1;
        }
        assert_eq!(*p.get(WidgetKind::Cpu), 1);
        assert_eq!(*p.get(WidgetKind::Mem), 2);
        assert_eq!(*p.get(WidgetKind::Net), 3);
        assert_eq!(*p.get(WidgetKind::Proc), 4);
        assert_eq!(*p.get(WidgetKind::Disk), 5);
    }

    #[test]
    fn per_widget_gpu_slots_are_addressable_by_index() {
        let mut p = PerWidget::<u32>::default();
        for n in 0..MAX_GPUS {
            *p.get_mut(WidgetKind::Gpu(n as u8)) = (100 + n) as u32;
        }
        for n in 0..MAX_GPUS {
            assert_eq!(*p.get(WidgetKind::Gpu(n as u8)), (100 + n) as u32);
        }
        // Sparse writes preserve identity: writing only Gpu(2) leaves Gpu(0) and Gpu(1) untouched.
        let mut q = PerWidget::<u32>::default();
        *q.get_mut(WidgetKind::Gpu(2)) = 42;
        assert_eq!(*q.get(WidgetKind::Gpu(0)), 0);
        assert_eq!(*q.get(WidgetKind::Gpu(1)), 0);
        assert_eq!(*q.get(WidgetKind::Gpu(2)), 42);
    }

    #[test]
    fn per_widget_gpu_does_not_alias_base_slots() {
        let mut p = PerWidget::<u32>::default();
        *p.get_mut(WidgetKind::Cpu) = 1;
        *p.get_mut(WidgetKind::Gpu(0)) = 2;
        assert_eq!(*p.get(WidgetKind::Cpu), 1);
        assert_eq!(*p.get(WidgetKind::Gpu(0)), 2);
    }
}
