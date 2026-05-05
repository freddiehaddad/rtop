//! Typed identifier for the named layout boxes (cpu, mem, net,
//! proc, disk, gpuN). Replaces the prior `Vec<String>` /
//! `&[&'static str]` representation throughout layout, preset,
//! and toggle code so that:
//!
//! - The set of valid boxes is checked by the type system rather
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

/// One layout box. The non-GPU variants are fixed; `Gpu(n)` is a
/// per-device box where `n` is in `0..MAX_GPUS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoxKind {
    Cpu,
    Mem,
    Net,
    Proc,
    Disk,
    Gpu(u8),
}

impl BoxKind {
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

impl Display for BoxKind {
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
/// [`BoxKind`]. Carries the offending input so that callers
/// (notably `BoxList`'s deserialiser) can surface a useful warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseBoxKindError(pub String);

impl Display for ParseBoxKindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid box name '{}'", self.0)
    }
}

impl std::error::Error for ParseBoxKindError {}

impl FromStr for BoxKind {
    type Err = ParseBoxKindError;

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
                .ok_or_else(|| ParseBoxKindError(other.to_string())),
        }
    }
}

impl Serialize for BoxKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for BoxKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BoxKindVisitor;

        impl Visitor<'_> for BoxKindVisitor {
            type Value = BoxKind;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "a box name (\"cpu\", \"mem\", \"net\", \"proc\", \"disk\", or \"gpuN\" with 0 <= N < {MAX_GPUS})"
                )
            }

            fn visit_str<E: de::Error>(self, s: &str) -> Result<BoxKind, E> {
                s.parse().map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_str(BoxKindVisitor)
    }
}

/// A list of [`BoxKind`]s with deserialise-time error capture.
///
/// `Vec<BoxKind>` would reject the entire load on a single bad
/// string, which is too strict — we want to drop the bad entries,
/// keep the good ones, and surface a warning. `BoxList` reads the
/// raw TOML as `Vec<String>`, parses each entry, and stores
/// failures separately so `Config::validate` can warn the user
/// without losing the rest of the layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoxList {
    items: Vec<BoxKind>,
    /// String entries from the last deserialise pass that failed
    /// to parse as a `BoxKind`. Cleared by `take_invalid` after
    /// `Config::validate` reports them.
    invalid: Vec<String>,
}

impl BoxList {
    pub fn new(items: Vec<BoxKind>) -> Self {
        Self {
            items,
            invalid: Vec::new(),
        }
    }

    pub fn from_kinds<I: IntoIterator<Item = BoxKind>>(iter: I) -> Self {
        Self::new(iter.into_iter().collect())
    }

    pub fn as_slice(&self) -> &[BoxKind] {
        &self.items
    }

    pub fn push(&mut self, kind: BoxKind) {
        self.items.push(kind);
    }

    pub fn remove_kind(&mut self, kind: BoxKind) -> bool {
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

impl Serialize for BoxList {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // TOML output is the canonical string form; invalid
        // entries are dropped so the saved file matches what the
        // runtime actually used.
        self.items.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BoxList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Vec::<String>::deserialize(deserializer)?;
        let mut items = Vec::with_capacity(raw.len());
        let mut invalid = Vec::new();
        for s in raw {
            match s.parse::<BoxKind>() {
                Ok(kind) => items.push(kind),
                Err(_) => invalid.push(s),
            }
        }
        Ok(Self { items, invalid })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_round_trips_for_static_variants() {
        for variant in [
            BoxKind::Cpu,
            BoxKind::Mem,
            BoxKind::Net,
            BoxKind::Proc,
            BoxKind::Disk,
        ] {
            let s = variant.to_string();
            assert_eq!(s.parse::<BoxKind>().unwrap(), variant);
        }
    }

    #[test]
    fn display_round_trips_for_gpu_variants() {
        for n in 0..MAX_GPUS {
            let v = BoxKind::Gpu(n as u8);
            let s = v.to_string();
            assert_eq!(s, format!("gpu{n}"));
            assert_eq!(s.parse::<BoxKind>().unwrap(), v);
        }
    }

    #[test]
    fn from_str_rejects_unknown_static_name() {
        assert!("foo".parse::<BoxKind>().is_err());
        assert!("Cpu".parse::<BoxKind>().is_err()); // case-sensitive
        assert!("".parse::<BoxKind>().is_err());
    }

    #[test]
    fn from_str_rejects_gpu_index_out_of_range() {
        let max = MAX_GPUS;
        assert!(format!("gpu{max}").parse::<BoxKind>().is_err());
        assert!("gpu99".parse::<BoxKind>().is_err());
    }

    #[test]
    fn from_str_rejects_gpu_with_no_digits() {
        assert!("gpu".parse::<BoxKind>().is_err());
        assert!("gpux".parse::<BoxKind>().is_err());
    }

    #[test]
    fn gpu_constructor_enforces_max() {
        assert!(BoxKind::gpu(0).is_some());
        assert_eq!(
            BoxKind::gpu(MAX_GPUS - 1),
            Some(BoxKind::Gpu(MAX_GPUS as u8 - 1))
        );
        assert_eq!(BoxKind::gpu(MAX_GPUS), None);
    }

    #[test]
    fn box_list_serialise_emits_string_array() {
        let list = BoxList::from_kinds([BoxKind::Cpu, BoxKind::Gpu(0)]);
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
    fn box_list_deserialise_keeps_valid_drops_invalid() {
        let raw = toml::Value::Array(vec![
            toml::Value::String("cpu".into()),
            toml::Value::String("nope".into()),
            toml::Value::String("gpu1".into()),
        ]);
        let mut list: BoxList = raw.try_into().unwrap();
        assert_eq!(list.as_slice(), &[BoxKind::Cpu, BoxKind::Gpu(1)]);
        assert_eq!(list.take_invalid(), vec!["nope".to_string()]);
        // Subsequent take returns empty.
        assert!(list.take_invalid().is_empty());
    }

    #[test]
    fn box_list_deserialise_empty_array() {
        let raw = toml::Value::Array(vec![]);
        let list: BoxList = raw.try_into().unwrap();
        assert!(list.as_slice().is_empty());
    }

    #[test]
    fn box_list_remove_kind_returns_false_when_absent() {
        let mut list = BoxList::from_kinds([BoxKind::Cpu]);
        assert!(!list.remove_kind(BoxKind::Mem));
        assert!(list.remove_kind(BoxKind::Cpu));
        assert!(list.as_slice().is_empty());
    }
}
