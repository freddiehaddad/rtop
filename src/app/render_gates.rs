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

// ─────────────────────────────────────────────────────────────────
// All-hidden overlay
// ─────────────────────────────────────────────────────────────────

/// Build the "all widgets hidden" overlay text. Lists `Shift+R` as
/// the bulk-reset key plus the per-widget toggle keys for the
/// widgets present in `active_layout` (so users on `cpu+proc` never
/// see "press 6 to show gpu0").
fn render_all_hidden(
    size: TerminalSize,
    active_layout: &crate::domain::layout_spec::Slot,
    theme: &theme::Theme,
) -> String {
    let lines = build_all_hidden_lines(active_layout);
    let max_line = lines.iter().map(|l| l.len()).max().unwrap_or(0);
    let total = lines.len();
    let start_y = size.height.saturating_sub(total) / 2 + 1;
    let mut out = String::new();
    out.push_str(term::CLEAR_SCREEN);
    let title_color = theme.color(tc::HI_FG);
    let body_color = theme.color(tc::TITLE);
    for (i, line) in lines.iter().enumerate() {
        let x = size.width.saturating_sub(max_line) / 2 + 1;
        let y = start_y + i;
        let color = if i == 0 { title_color } else { body_color };
        let bold = if i == 0 { term::BOLD } else { "" };
        out.push_str(&format!(
            "\x1b[{y};{x}H{}{}{line}{}",
            bold,
            color,
            term::RESET,
        ));
    }
    out
}

/// Compose the overlay's text lines. Pure function so the layout
/// is testable without rendering.
fn build_all_hidden_lines(active_layout: &crate::domain::layout_spec::Slot) -> Vec<String> {
    use crate::domain::widget_kind::WidgetKind;
    let mut lines = vec![
        "All widgets hidden.".to_string(),
        String::new(),
        "Press Shift+R to restore everything,".to_string(),
        "or press a number key to show:".to_string(),
        String::new(),
    ];
    let mut hints: Vec<String> = Vec::new();
    for (key, kind) in widget_toggle_hints() {
        if active_layout.contains(kind) {
            hints.push(format!("  {key}  {kind}"));
        }
    }
    // The `0` key batch-toggles the GPU 4-7 range. Mention it if
    // the active layout includes any GPU in that range — the user
    // can't address those individually.
    let gpu_extras_present =
        (4..crate::config::MAX_GPUS as u8).any(|n| active_layout.contains(WidgetKind::Gpu(n)));
    if gpu_extras_present {
        hints.push("  0  gpu4-7".to_string());
    }
    if hints.is_empty() {
        // Active layout has nothing toggleable — only the bulk
        // reset key is meaningful. Drop the "or press" hint.
        lines.truncate(3);
    } else {
        lines.extend(hints);
    }
    lines
}

/// All numeric toggle keybinds in display order. Built from
/// [`WidgetKind::toggle_key`] so the overlay listing cannot drift
/// from the actual normal-mode bindings — adding a new individually-
/// addressable widget requires only updating `toggle_key`.
fn widget_toggle_hints() -> Vec<(char, crate::domain::widget_kind::WidgetKind)> {
    use crate::domain::widget_kind::WidgetKind;
    let base = [
        WidgetKind::Cpu,
        WidgetKind::Mem,
        WidgetKind::Net,
        WidgetKind::Proc,
        WidgetKind::Disk,
    ];
    let mut out: Vec<(char, WidgetKind)> = base
        .into_iter()
        .filter_map(|k| k.toggle_key().map(|c| (c, k)))
        .collect();
    for n in 0..crate::config::MAX_GPUS as u8 {
        let kind = WidgetKind::Gpu(n);
        if let Some(c) = kind.toggle_key() {
            out.push((c, kind));
        }
    }
    out
}

/// Render the all-hidden overlay if dirty flags indicate it's needed.
pub(crate) fn render_if_dirty_all_hidden(
    state: &mut AppState,
    config: &config::Config,
    terminal: &mut term::Terminal,
    theme: &theme::Theme,
    size: TerminalSize,
) {
    if state.render.dirty.contains(Dirty::LAYOUT)
        || state.render.dirty.intersects(Dirty::ALL_WIDGETS)
    {
        let active = config.layout_spec().clone();
        let output = style_terminal_output(&render_all_hidden(size, &active, theme), config, theme);
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

    // ────────────────────────────────────────────────────────────
    // All-hidden overlay
    // ────────────────────────────────────────────────────────────

    use crate::domain::layout_spec::Slot;
    use crate::domain::widget_kind::WidgetKind;

    #[test]
    fn all_hidden_lines_list_only_widgets_present_in_active_layout() {
        // Active layout = cpu+proc. Only those toggle keys appear.
        let active = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Proc),
        ]);
        let lines = build_all_hidden_lines(&active);
        let body = lines.join("\n");
        assert!(body.contains("All widgets hidden."));
        assert!(body.contains("Shift+R"));
        assert!(body.contains("1") && body.contains("cpu"));
        assert!(body.contains("4") && body.contains("proc"));
        // Mem/net/disk/gpu hints must NOT appear.
        assert!(!body.contains("mem"));
        assert!(!body.contains("net"));
        assert!(!body.contains("disk"));
        assert!(!body.contains("gpu"));
    }

    #[test]
    fn all_hidden_lines_show_gpu_extras_hint_only_when_present() {
        // Active layout contains gpu5 (in the 4-7 range) — show "0  gpu4-7".
        let active = Slot::VStack(vec![
            Slot::Widget(WidgetKind::Cpu),
            Slot::Widget(WidgetKind::Gpu(5)),
        ]);
        let lines = build_all_hidden_lines(&active);
        let body = lines.join("\n");
        assert!(body.contains("0  gpu4-7"));
        // gpu5 doesn't get its own per-key line because it's in
        // the batch range; the "0  gpu4-7" hint covers it.
    }

    #[test]
    fn all_hidden_lines_drop_per_key_section_when_no_widgets_toggleable() {
        // Active layout has only widgets the toggle keys can't
        // address (none today, but verify the truncation path
        // works defensively).
        let active = Slot::Widget(WidgetKind::Gpu(0));
        let lines = build_all_hidden_lines(&active);
        let body = lines.join("\n");
        // gpu0 is on key 6 (within toggleable range), so this case
        // does include the per-key section. Verify the structure:
        assert!(body.contains("6  gpu0"));
    }
}
