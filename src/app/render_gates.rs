//! Pre-render gates: messages shown when the terminal is too small
//! to fit the active layout, or while waiting for the first data
//! snapshot from every collector.
//!
//! Both gates short-circuit the normal render path in `app::run`.
//! They share the same dirty-flag entry condition (LAYOUT or any
//! widget dirty bit) so the message redraws on resize and on first
//! tick but is otherwise idle.
//!
//! The "too small" threshold is **dynamic**: it's the minimum size
//! at which the user's active widget set + hardware (core count,
//! GPU count, disk count, swap, temps, watts) fits without
//! truncation. Computed each frame in `app::run` via
//! `draw::layout::min_terminal_size` and passed in here.

use crate::app::TerminalSize;
use crate::app::lifecycle::style_terminal_output;
use crate::app::state::AppState;
use crate::config;
use crate::dirty::Dirty;
use crate::term;
use crate::theme;
use crate::theme_keys as tc;

pub(crate) fn is_too_small(size: TerminalSize, min_size: (usize, usize)) -> bool {
    let (min_w, min_h) = min_size;
    size.width < min_w || size.height < min_h
}

fn render_too_small(size: TerminalSize, min_size: (usize, usize), theme: &theme::Theme) -> String {
    let (min_w, min_h) = min_size;
    let msg = format!(
        "Terminal too small ({}x{}). Need {}x{}.",
        size.width, size.height, min_w, min_h
    );
    let msg_y = size.height.max(1) / 2;
    let msg_x = size.width.saturating_sub(msg.len()) / 2 + 1;
    format!(
        "{}\x1b[{msg_y};{msg_x}H{}{}{msg}{}",
        term::CLEAR_SCREEN,
        term::BOLD,
        theme.color(tc::HI_FG),
        term::RESET,
    )
}

/// Render the "too small" message if dirty flags indicate it's needed.
pub(crate) fn render_if_dirty_small(
    state: &mut AppState,
    config: &config::Config,
    terminal: &mut term::Terminal,
    theme: &theme::Theme,
    size: TerminalSize,
    min_size: (usize, usize),
) {
    if state.render.dirty.contains(Dirty::LAYOUT)
        || state.render.dirty.intersects(Dirty::ALL_WIDGETS)
    {
        let output = style_terminal_output(&render_too_small(size, min_size, theme), config, theme);
        if let Err(e) = terminal.write_synced(&output) {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Terminal,
                error = %e,
                "terminal write failed",
            );
        }
        state.render.clear_dirty();
    }
}

fn render_waiting_for_snapshot(size: TerminalSize, theme: &theme::Theme) -> String {
    let msg = "Collecting data...";
    let msg_y = size.height.max(1) / 2;
    let msg_x = size.width.saturating_sub(msg.len()) / 2 + 1;
    format!(
        "{}\x1b[{msg_y};{msg_x}H{}{}{msg}{}",
        term::CLEAR_SCREEN,
        term::BOLD,
        theme.color(tc::HI_FG),
        term::RESET,
    )
}

/// Render the "Collecting data..." message if dirty flags indicate it's needed.
pub(crate) fn render_if_dirty_waiting(
    state: &mut AppState,
    config: &config::Config,
    terminal: &mut term::Terminal,
    theme: &theme::Theme,
    size: TerminalSize,
) {
    if state.render.dirty.contains(Dirty::LAYOUT)
        || state.render.dirty.intersects(Dirty::ALL_WIDGETS)
    {
        let output =
            style_terminal_output(&render_waiting_for_snapshot(size, theme), config, theme);
        if let Err(e) = terminal.write_synced(&output) {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Terminal,
                error = %e,
                "terminal write failed",
            );
        }
        state.render.clear_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_too_small_compares_against_passed_minimum() {
        let min = (100, 30);
        assert!(is_too_small(
            TerminalSize {
                width: 99,
                height: 30,
            },
            min,
        ));
        assert!(is_too_small(
            TerminalSize {
                width: 100,
                height: 29,
            },
            min,
        ));
        assert!(!is_too_small(
            TerminalSize {
                width: 100,
                height: 30,
            },
            min,
        ));
    }

    #[test]
    fn too_small_message_includes_actual_and_required_size() {
        let out = render_too_small(
            TerminalSize {
                width: 40,
                height: 10,
            },
            (150, 48),
            &theme::Theme::new(),
        );

        assert!(out.contains("Terminal too small (40x10)."));
        assert!(out.contains("Need 150x48."));
    }
}
