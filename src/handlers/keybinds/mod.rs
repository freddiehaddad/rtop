//! Declarative keybind table.
//!
//! The single source of truth for every keystroke the application
//! responds to. `BINDINGS` and `PREHOOKS` are walked by [`dispatch`]
//! on every key event; the table itself drives both the runtime
//! input dispatch and the help-menu generator.
//!
//! ## How input flows
//!
//! ```text
//! Key  ─► PREHOOKS ─► BINDINGS ─► fallback_for(state) ─► state mutation
//! ```
//!
//! 1. Each [`Prehook`] runs in order. A prehook may mutate state
//!    (e.g. cancel follow-mode on a navigation key) and either
//!    *consume* the key (short-circuit dispatch) or *continue* into
//!    the binding scan.
//! 2. The dispatcher walks [`BINDINGS`] in declaration order and
//!    invokes the first binding whose [`Binding::matches`] returns
//!    `true` for the current `(key, overlay-kind, vim_keys)`.
//! 3. If no binding matched and the current state has a fallback
//!    (registered in [`fallback_for`]), the fallback consumes the
//!    key — used by the text-input states ([`OverlayKind::Filter`]
//!    and [`OverlayKind::OptionsEdit`]) to append typed characters
//!    to their buffers.
//!
//! ## Adding a binding
//!
//! Add a `Binding { ... }` literal to [`BINDINGS`]. Set `help` to
//! `Some(HelpEntry { ... })` if it should appear in the help menu;
//! the help layout groups by `category`, in first-seen order.
//!
//! Binding actions take `&mut InputContext` and the matched `&Key`
//! (so a binding with multiple triggers — e.g. the per-digit widget
//! toggle — can branch on which digit fired). Actions return `()`
//! and signal application exit by setting `*ctx.quit = true`.

use crate::handlers::InputContext;
use crate::input::Key;
use crate::overlay::OverlayKind;

/// How a single key trigger participates in matching.
///
/// `VimOnly` triggers only fire when the user has enabled vim-style
/// navigation in the options menu; the dispatcher reads this flag
/// from `ctx.config.ui.vim_keys` once per key event.
#[derive(Debug, Clone, Copy)]
pub(crate) enum KeySpec {
    Always(Key),
    VimOnly(Key),
}

/// Help-menu metadata for a binding. `None` on bindings that
/// shouldn't appear in the help menu (overlay-internal Tab cycles,
/// Esc/menu-close, inline-editor command keys, …).
#[derive(Debug, Clone, Copy)]
pub(crate) struct HelpEntry {
    /// Section name in the help menu (e.g. `"Global"`, `"Process"`).
    pub category: &'static str,
    /// Display string for the key column. Hand-curated rather than
    /// auto-formatted from `keys` so multi-trigger bindings can be
    /// grouped ergonomically (`"p / Shift+P"`, `"1-5"`,
    /// `"q / Ctrl+C"`) and vim aliases can be omitted from help.
    pub keys: &'static str,
    /// User-facing description. Wrapping is the help-menu's job;
    /// keep this to a single short sentence fragment.
    pub description: &'static str,
}

/// Action signature for a binding. Receives the full input context
/// and the matched key (so a binding triggered by multiple keys can
/// branch on which one fired — e.g. extracting a digit from
/// `Key::Char(c @ '1'..='9')` for widget-toggle).
///
/// Actions mutate state and may set `*ctx.quit = true` to signal
/// application exit. They never return terminal output — the central
/// render path repaints based on the dirty flags they set.
pub(crate) type ActionFn = fn(&mut InputContext, &Key);

/// One keybind. Multiple `keys`, multiple `states` — a single entry
/// covers every (key, state) pair that should run the same action.
pub(crate) struct Binding {
    pub keys: &'static [KeySpec],
    pub states: &'static [OverlayKind],
    pub help: Option<HelpEntry>,
    pub action: ActionFn,
}

impl Binding {
    /// Whether this binding should fire for the given key, overlay
    /// kind, and vim-mode flag.
    pub fn matches(&self, key: &Key, state: OverlayKind, vim: bool) -> bool {
        if !self.states.contains(&state) {
            return false;
        }
        self.keys.iter().any(|spec| match *spec {
            KeySpec::Always(k) => k == *key,
            KeySpec::VimOnly(k) => vim && k == *key,
        })
    }
}

/// Pre-dispatch hook outcome. `Consume` short-circuits dispatch
/// (the key is considered handled); `Continue` lets the binding
/// scan proceed (used for state-mutating side effects like
/// `cancel_follow_on_nav`, where the key should also drive the
/// matched navigation binding).
pub(crate) enum PrehookOutcome {
    Continue,
    Consume,
}

pub(crate) type PrehookFn = fn(&Key, &mut InputContext) -> PrehookOutcome;

/// One pre-dispatch hook. State scoping is explicit so the hook
/// table is auditable in one place.
pub(crate) struct Prehook {
    pub states: &'static [OverlayKind],
    pub hook: PrehookFn,
}

impl Prehook {
    fn matches(&self, state: OverlayKind) -> bool {
        self.states.contains(&state)
    }
}

/// Catch-all handler for a state that intentionally consumes any
/// key not bound in [`BINDINGS`] — used by the text-input states
/// to append typed characters to their buffers.
pub(crate) type FallbackFn = fn(&Key, &mut InputContext);

/// Registered fallback for `state`, or `None` if the state has no
/// catch-all (most states don't).
fn fallback_for(state: OverlayKind) -> Option<FallbackFn> {
    match state {
        OverlayKind::Filter => Some(crate::overlay::filter::fallback_typed_char),
        OverlayKind::OptionsEdit => Some(crate::overlay::options::edit::fallback_typed_char),
        _ => None,
    }
}

/// Dispatch a single key event through the prehooks, the binding
/// table, and any state-specific fallback.
pub(crate) fn dispatch(key: &Key, ctx: &mut InputContext) {
    let state = ctx.overlay.active().kind();
    let vim = ctx.config.ui.vim_keys;

    for prehook in PREHOOKS {
        if !prehook.matches(state) {
            continue;
        }
        match (prehook.hook)(key, ctx) {
            PrehookOutcome::Consume => return,
            PrehookOutcome::Continue => {}
        }
    }

    for binding in BINDINGS {
        if binding.matches(key, state, vim) {
            (binding.action)(ctx, key);
            return;
        }
    }

    if let Some(fallback) = fallback_for(state) {
        fallback(key, ctx);
    }
}

mod prehooks;
mod table;

#[cfg(test)]
mod tests;

pub(crate) use table::{BINDINGS, PREHOOKS};
