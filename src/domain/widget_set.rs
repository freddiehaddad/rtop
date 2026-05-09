//! `WidgetSet` — a set of [`WidgetKind`]s with inline storage.
//!
//! Used wherever the engine, app state, or config needs to answer
//! "is this widget in the set?" for a small known-bounded universe
//! of widgets. Examples:
//!
//! * The runtime view filter (which widgets the user has chosen to
//!   hide) lives in `AppState` as a `WidgetSet`.
//! * The layout engine's per-frame visibility input is a
//!   `WidgetSet` composed by the app layer from hardware absence
//!   (GPUs without a backing device) and the user's view filter.
//! * The persisted form of the user's view filter (Config's
//!   `hidden_widgets`) so hide gestures survive restart.
//!
//! `HashSet<WidgetKind>` would heap-allocate for at most 13
//! entries (5 base widgets + `MAX_GPUS` GPU slots). `WidgetSet`
//! reuses the existing [`PerWidget<bool>`] container — fixed-size
//! inline storage, no allocation, identical algorithmic behaviour.
//! The API mirrors [`std::collections::HashSet`] for the operations
//! we use.
//!
//! TOML serialisation is the natural form: a sorted list of widget
//! names. Empty sets serialise as an empty array, which round-trips
//! cleanly.

use serde::de::{self, Deserializer, Visitor};
use serde::ser::{SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::domain::widget_kind::{PerWidget, WidgetKind};

/// A set of [`WidgetKind`]s with inline `O(1)` membership.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WidgetSet {
    members: PerWidget<bool>,
}

impl WidgetSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` if no widgets are in the set.
    pub fn is_empty(&self) -> bool {
        WidgetKind::all().all(|k| !*self.members.get(k))
    }

    /// `true` if `kind` is in the set.
    pub fn contains(&self, kind: WidgetKind) -> bool {
        *self.members.get(kind)
    }

    /// Add `kind` to the set. Returns whether the set changed.
    pub fn insert(&mut self, kind: WidgetKind) -> bool {
        let slot = self.members.get_mut(kind);
        let was_present = *slot;
        *slot = true;
        !was_present
    }

    /// Toggle `kind`'s membership. Returns whether `kind` is in the
    /// set after the toggle.
    pub fn toggle(&mut self, kind: WidgetKind) -> bool {
        let slot = self.members.get_mut(kind);
        *slot = !*slot;
        *slot
    }

    /// Remove all members from the set.
    pub fn clear(&mut self) {
        self.members = PerWidget::default();
    }

    /// Iterate over the [`WidgetKind`]s in the set, in canonical
    /// order (cpu, mem, net, proc, disk, gpu0..gpuN, statusbar) —
    /// the order [`WidgetKind::all`] produces.
    pub fn iter(&self) -> impl Iterator<Item = WidgetKind> + '_ {
        WidgetKind::all().filter(|k| self.contains(*k))
    }

    /// Add every member of `other` to this set.
    pub fn extend_from(&mut self, other: &WidgetSet) {
        for kind in other.iter() {
            self.insert(kind);
        }
    }
}

impl Serialize for WidgetSet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Canonical-order list of widget names — the same order
        // `iter()` produces, which is stable across runs.
        let mut seq = serializer.serialize_seq(None)?;
        for kind in self.iter() {
            // `WidgetKind`'s own Serialize impl emits the canonical
            // name string ("cpu", "gpu0", ...).
            seq.serialize_element(&kind)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for WidgetSet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct WidgetSetVisitor;

        impl<'de> Visitor<'de> for WidgetSetVisitor {
            type Value = WidgetSet;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a list of widget names")
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<WidgetSet, A::Error> {
                let mut set = WidgetSet::new();
                // Drop unknown entries silently so a hand-edited
                // TOML naming an unknown widget (typo, removed
                // widget kind in a future version) doesn't kill
                // the rest of the layer's settings. Same defensive
                // policy as `WidgetKind`'s `FromStr` callers.
                while let Some(name) = seq.next_element::<String>()? {
                    if let Ok(kind) = name.parse::<WidgetKind>() {
                        set.insert(kind);
                    }
                }
                Ok(set)
            }
        }

        deserializer.deserialize_seq(WidgetSetVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MAX_GPUS;

    #[test]
    fn new_set_is_empty() {
        let set = WidgetSet::new();
        assert!(set.is_empty());
        for kind in [
            WidgetKind::Cpu,
            WidgetKind::Mem,
            WidgetKind::Net,
            WidgetKind::Proc,
            WidgetKind::Disk,
        ] {
            assert!(!set.contains(kind));
        }
        for n in 0..MAX_GPUS as u8 {
            assert!(!set.contains(WidgetKind::Gpu(n)));
        }
    }

    #[test]
    fn insert_marks_present_and_reports_change() {
        let mut set = WidgetSet::new();
        assert!(set.insert(WidgetKind::Cpu));
        assert!(set.contains(WidgetKind::Cpu));
        assert!(!set.is_empty());
        // Re-insert reports unchanged.
        assert!(!set.insert(WidgetKind::Cpu));
    }

    #[test]
    fn toggle_flips_and_returns_new_state() {
        let mut set = WidgetSet::new();
        assert!(set.toggle(WidgetKind::Net));
        assert!(set.contains(WidgetKind::Net));
        assert!(!set.toggle(WidgetKind::Net));
        assert!(!set.contains(WidgetKind::Net));
    }

    #[test]
    fn clear_drops_every_member() {
        let mut set = WidgetSet::new();
        set.insert(WidgetKind::Cpu);
        set.insert(WidgetKind::Gpu(3));
        assert!(!set.is_empty());
        set.clear();
        assert!(set.is_empty());
        assert!(!set.contains(WidgetKind::Cpu));
        assert!(!set.contains(WidgetKind::Gpu(3)));
    }

    #[test]
    fn iter_yields_members_in_canonical_order() {
        let mut set = WidgetSet::new();
        // Insert in non-canonical order to verify iter sorts.
        set.insert(WidgetKind::Gpu(2));
        set.insert(WidgetKind::Cpu);
        set.insert(WidgetKind::Disk);
        set.insert(WidgetKind::Gpu(0));
        let kinds: Vec<_> = set.iter().collect();
        assert_eq!(
            kinds,
            vec![
                WidgetKind::Cpu,
                WidgetKind::Disk,
                WidgetKind::Gpu(0),
                WidgetKind::Gpu(2),
            ]
        );
    }

    #[test]
    fn iter_on_empty_set_yields_nothing() {
        let set = WidgetSet::new();
        assert_eq!(set.iter().count(), 0);
    }

    #[test]
    fn gpu_indices_are_independent() {
        let mut set = WidgetSet::new();
        set.insert(WidgetKind::Gpu(1));
        assert!(set.contains(WidgetKind::Gpu(1)));
        assert!(!set.contains(WidgetKind::Gpu(0)));
        assert!(!set.contains(WidgetKind::Gpu(2)));
    }

    #[test]
    fn extend_from_unions_members() {
        let mut a = WidgetSet::new();
        a.insert(WidgetKind::Cpu);
        a.insert(WidgetKind::Mem);

        let mut b = WidgetSet::new();
        b.insert(WidgetKind::Mem);
        b.insert(WidgetKind::Net);

        a.extend_from(&b);
        let kinds: Vec<_> = a.iter().collect();
        assert_eq!(
            kinds,
            vec![WidgetKind::Cpu, WidgetKind::Mem, WidgetKind::Net]
        );
    }

    #[test]
    fn equality_ignores_insertion_order() {
        let mut a = WidgetSet::new();
        a.insert(WidgetKind::Cpu);
        a.insert(WidgetKind::Net);
        let mut b = WidgetSet::new();
        b.insert(WidgetKind::Net);
        b.insert(WidgetKind::Cpu);
        assert_eq!(a, b);
    }

    // ────────────────────────────────────────────────────────────
    // Serde
    // ────────────────────────────────────────────────────────────

    #[test]
    fn serialise_emits_canonical_order_string_array() {
        let mut set = WidgetSet::new();
        set.insert(WidgetKind::Net);
        set.insert(WidgetKind::Cpu);
        set.insert(WidgetKind::Gpu(2));
        let toml = toml::Value::try_from(set).unwrap();
        assert_eq!(
            toml,
            toml::Value::Array(vec![
                toml::Value::String("cpu".into()),
                toml::Value::String("net".into()),
                toml::Value::String("gpu2".into()),
            ]),
        );
    }

    #[test]
    fn serialise_empty_set_emits_empty_array() {
        let set = WidgetSet::new();
        let toml = toml::Value::try_from(set).unwrap();
        assert_eq!(toml, toml::Value::Array(vec![]));
    }

    #[test]
    fn deserialise_keeps_known_widget_names_drops_unknown() {
        let raw = toml::Value::Array(vec![
            toml::Value::String("cpu".into()),
            toml::Value::String("nope".into()),
            toml::Value::String("gpu1".into()),
        ]);
        let set: WidgetSet = raw.try_into().unwrap();
        let kinds: Vec<_> = set.iter().collect();
        assert_eq!(kinds, vec![WidgetKind::Cpu, WidgetKind::Gpu(1)]);
    }

    #[test]
    fn deserialise_empty_array_is_empty_set() {
        let raw = toml::Value::Array(vec![]);
        let set: WidgetSet = raw.try_into().unwrap();
        assert!(set.is_empty());
    }

    #[test]
    fn round_trip_preserves_membership() {
        let mut original = WidgetSet::new();
        original.insert(WidgetKind::Cpu);
        original.insert(WidgetKind::Disk);
        original.insert(WidgetKind::Gpu(0));
        original.insert(WidgetKind::Gpu(7));
        let toml = toml::Value::try_from(original).unwrap();
        let loaded: WidgetSet = toml.try_into().unwrap();
        assert_eq!(loaded, original);
    }
}
