//! Bottom-border insets for the CPU widget.
//!
//! Owns the bottom border keybind hints (menu / preset cycler /
//! update rate). The uptime + clock insets remain in `mod.rs`
//! because they're only conditionally drawn from the main draw
//! orchestrator.

use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::buffer::AnsiBuffer;
use crate::theme::Theme;
use crate::theme_keys as tc;

/// Render the bottom border keybind hints (menu, preset cycler, update rate).
///
/// The preset hint is laid out as `← P NAME p →`: arrows + spaces
/// in the label colour (TITLE), only the keybind letters `P` and
/// `p` in the keybind colour (HI). Mirrors the colour + spacing
/// rule established for the net-widget iface inset.
pub(super) fn draw_bottom_hints(
    x: usize,
    bottom_y: usize,
    update_ms: u64,
    preset_name: &str,
    filter_active: bool,
    theme: &Theme,
) -> String {
    let border_color = theme.color(tc::CPU_WIDGET);
    let title_color = theme.color(tc::TITLE);
    let hi = theme.color(tc::HI_FG);

    let menu_inset = box_drawing::keybind_inset("menu", border_color, hi, title_color, true);
    // `*` suffix on the preset name signals an active runtime view
    // filter — at least one widget is hidden via the toggle keys.
    // Cleared by `Shift+R` (and on Ctrl+R config reload). The
    // suffix is one byte so it has no visible-width impact on the
    // surrounding layout maths.
    let suffix = if filter_active { "*" } else { "" };
    let preset_text = format!(
        "{} {}P{} {}{} {}p{} {}",
        symbols::LEFT_ARROW,
        hi,
        title_color,
        preset_name,
        suffix,
        hi,
        title_color,
        symbols::RIGHT_ARROW,
    );
    let preset_inset = box_drawing::title_inset(&preset_text, border_color, title_color, true);
    let rate_label = format!("{update_ms}ms");
    let rate_text = format!("─ {}{} {}+", title_color, rate_label, hi);
    let rate_inset = box_drawing::title_inset(&rate_text, border_color, hi, true);
    let hints = format!("{menu_inset}{preset_inset}{rate_inset}");

    let mut buf = AnsiBuffer::new();
    buf.mv(x + 3, bottom_y).text(&hints);
    buf.finish()
}
