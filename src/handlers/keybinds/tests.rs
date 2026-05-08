//! Validation tests for the keybind table.
//!
//! These tests guarantee structural properties that the per-state
//! `match` dispatch used to enforce by construction:
//!
//! * No two bindings claim the same `(state, key, vim-condition)`
//!   tuple — the dispatcher uses first-match semantics, so a
//!   duplicate would silently shadow.
//! * No binding in [`OverlayKind::Filter`] or [`OverlayKind::OptionsEdit`]
//!   is triggered by a printable `Key::Char(_)` or `Key::Space`
//!   trigger — those would silently steal text input from the
//!   per-state `fallback_typed_char`.
//! * Help-menu metadata is internally consistent.

use super::{BINDINGS, Binding, HelpEntry, KeySpec, PREHOOKS, Prehook};
use crate::input::Key;
use crate::overlay::OverlayKind;

const ALL_STATES: &[OverlayKind] = &[
    OverlayKind::None,
    OverlayKind::Main,
    OverlayKind::Help,
    OverlayKind::Options,
    OverlayKind::OptionsEdit,
    OverlayKind::Filter,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum VimCondition {
    Always,
    VimOnly,
}

fn key_specs(binding: &Binding) -> impl Iterator<Item = (Key, VimCondition)> + '_ {
    binding.keys.iter().map(|spec| match *spec {
        KeySpec::Always(k) => (k, VimCondition::Always),
        KeySpec::VimOnly(k) => (k, VimCondition::VimOnly),
    })
}

#[test]
fn no_duplicate_state_key_bindings() {
    use std::collections::HashMap;
    let mut seen: HashMap<(OverlayKind, Key, VimCondition), usize> = HashMap::new();
    for (idx, binding) in BINDINGS.iter().enumerate() {
        for state in binding.states {
            for (key, vim) in key_specs(binding) {
                let entry = (*state, key, vim);
                if let Some(prev) = seen.insert(entry, idx) {
                    panic!(
                        "duplicate binding for {:?} key={:?} vim={:?}: indices {} and {}",
                        state, key, vim, prev, idx
                    );
                }
            }
        }
    }
}

#[test]
fn text_states_have_no_printable_bindings() {
    for binding in BINDINGS {
        let touches_text_state = binding
            .states
            .iter()
            .any(|s| matches!(s, OverlayKind::Filter | OverlayKind::OptionsEdit));
        if !touches_text_state {
            continue;
        }
        for (key, _) in key_specs(binding) {
            let printable = matches!(key, Key::Char(_) | Key::Space);
            assert!(
                !printable,
                "binding for {:?} in a text-input state would steal typed input \
                 from the dispatcher's fallback_typed_char",
                key,
            );
        }
    }
}

#[test]
fn every_binding_targets_at_least_one_state() {
    for (idx, binding) in BINDINGS.iter().enumerate() {
        assert!(
            !binding.states.is_empty(),
            "binding #{} has empty `states` — it would never fire",
            idx,
        );
        for state in binding.states {
            assert!(
                ALL_STATES.contains(state),
                "binding #{} references unknown state {:?}",
                idx,
                state,
            );
        }
    }
}

#[test]
fn every_binding_has_at_least_one_trigger() {
    for (idx, binding) in BINDINGS.iter().enumerate() {
        assert!(
            !binding.keys.is_empty(),
            "binding #{} has empty `keys` — it would never fire",
            idx,
        );
    }
}

#[test]
fn help_entries_are_non_empty() {
    for (idx, binding) in BINDINGS.iter().enumerate() {
        let Some(HelpEntry {
            category,
            keys,
            description,
        }) = binding.help
        else {
            continue;
        };
        assert!(
            !category.is_empty(),
            "binding #{} has empty help.category",
            idx,
        );
        assert!(!keys.is_empty(), "binding #{} has empty help.keys", idx);
        assert!(
            !description.is_empty(),
            "binding #{} has empty help.description",
            idx,
        );
    }
}

#[test]
fn prehooks_target_at_least_one_state() {
    for (idx, prehook) in PREHOOKS.iter().enumerate() {
        let Prehook { states, .. } = prehook;
        assert!(
            !states.is_empty(),
            "prehook #{} has empty `states` — it would never fire",
            idx,
        );
    }
}

#[test]
fn matches_respects_vim_flag() {
    let binding = Binding {
        keys: &[KeySpec::VimOnly(Key::Char('j'))],
        states: &[OverlayKind::None],
        help: None,
        action: |_, _| {},
    };
    assert!(binding.matches(&Key::Char('j'), OverlayKind::None, true));
    assert!(!binding.matches(&Key::Char('j'), OverlayKind::None, false));
}

#[test]
fn matches_respects_state_filter() {
    let binding = Binding {
        keys: &[KeySpec::Always(Key::Char('q'))],
        states: &[OverlayKind::Help],
        help: None,
        action: |_, _| {},
    };
    assert!(binding.matches(&Key::Char('q'), OverlayKind::Help, false));
    assert!(!binding.matches(&Key::Char('q'), OverlayKind::None, false));
}
