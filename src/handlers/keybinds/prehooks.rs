//! Pre-dispatch hooks. Run before the binding scan; can mutate
//! state and either short-circuit dispatch ([`PrehookOutcome::Consume`])
//! or fall through to the binding scan
//! ([`PrehookOutcome::Continue`]).
//!
//! Both hooks are scoped to [`MenuState::None`] (the only state
//! where their target state — `armed_terminate`, `followed_pid` —
//! can possibly be set). The state filter is enforced declaratively
//! in [`super::PREHOOKS`].

use crate::handlers::InputContext;
use crate::input::Key;

use super::PrehookOutcome;

/// Disarm a pending terminate confirmation (`t`/`T` first-press
/// arms; any other key disarms). Always consumes the disarming
/// key — the user shouldn't accidentally trigger another binding
/// while disarming.
pub(super) fn armed_terminate_disarm(key: &Key, ctx: &mut InputContext) -> PrehookOutcome {
    if ctx.process.armed_terminate.is_some() && !matches!(key, Key::Char('t' | 'T')) {
        ctx.process.armed_terminate = None;
        ctx.render.dirty.mark_proc_widget();
        return PrehookOutcome::Consume;
    }
    PrehookOutcome::Continue
}

/// Cancel "follow process" mode on any navigation key. The key
/// itself must still drive the matched navigation binding, so this
/// hook always returns `Continue` after the side effect.
pub(super) fn cancel_follow_on_nav(key: &Key, ctx: &mut InputContext) -> PrehookOutcome {
    let is_nav = matches!(
        key,
        Key::Up
            | Key::Down
            | Key::PageUp
            | Key::PageDown
            | Key::Home
            | Key::End
            | Key::Char('j' | 'k')
            | Key::CtrlB
            | Key::CtrlF
            | Key::CtrlD
            | Key::CtrlU
    );
    if is_nav && ctx.process.followed_pid > 0 {
        ctx.process.followed_pid = 0;
    }
    PrehookOutcome::Continue
}
