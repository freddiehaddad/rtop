use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::term;
use crate::theme::Theme;
use crate::theme_keys as tc;

/// Fixed characters consumed by the sort selector inset on the bottom border:
/// left_connector(1) + "← "(2) + " →"(2) + right_connector(1) + gap(1).
const SORT_INSET_OVERHEAD: usize = 7;

/// Render the top border with reverse, tree, sort selector, and (when
/// the proc list is paused) a `paused` state chip immediately after
/// the title.
pub(super) fn draw_top_border(
    x: usize,
    y: usize,
    width: usize,
    sort_by: crate::collect::process_display::ProcSort,
    tree_mode: bool,
    paused: bool,
    theme: &Theme,
) -> String {
    let border_color = theme.color(tc::PROC_WIDGET);
    let hi = theme.color(tc::HI_FG);
    let title_color = theme.color(tc::TITLE);
    let mut buf = AnsiBuffer::new();

    let sort_name = sort_by.as_str();
    let tree_star = if tree_mode { "*" } else { "" };

    // Build positions right-to-left from the right corner
    let mut pos = x + width - sort_name.len() - SORT_INSET_OVERHEAD;

    // Sort selector: ┐← sorting →┌
    let sort_text = format!("← {}{} {}→", title_color, sort_name, hi);
    let sort_inset = box_drawing::title_inset(&sort_text, border_color, hi, false);
    buf.mv(pos, y + 1).text(&sort_inset);

    // Tree button: ┐tree┌
    let tree_content = format!("tre{}{}", tree_star, "e");
    let tree_len = tree_content.len();
    if pos > x + 12 + tree_len {
        pos -= tree_len + 2;
        let tree_text = format!("tre{}{}e", tree_star, hi);
        let tree_inset = box_drawing::title_inset(&tree_text, border_color, title_color, false);
        buf.mv(pos, y + 1).text(&tree_inset);
    }

    // Reverse button: ┐reverse┌
    if pos > x + 12 {
        pos -= 9;
        let rev_inset = box_drawing::keybind_inset("reverse", border_color, hi, title_color, false);
        buf.mv(pos, y + 1).text(&rev_inset);
    }

    // Pause state chip: ┐paused┌, immediately after the title chip.
    // The proc widget's title chip has the form ┐⁴proc┌ rendered
    // starting at x + 3 by create_box; its visible width is
    // inset_width("⁴proc") = 7, so the pause chip's left connector
    // sits at x + 3 + 7 = x + 10. The chip is rendered only when
    // paused; when not paused the right-side chip positions stay
    // unchanged because they're computed from the right edge.
    if paused {
        let pause_inset = box_drawing::title_inset("paused", border_color, hi, false);
        let title_w = box_drawing::inset_width("\u{2074}proc"); // matches the title chip
        let pause_x = x + 3 + title_w;
        // Defensive: only render if there's room before the
        // right-side chips (which start at `pos`). Avoids garbled
        // borders on extremely narrow proc widgets.
        if pause_x + box_drawing::inset_width("paused") <= pos {
            buf.mv(pause_x, y + 1).text(&pause_inset);
        }
    }

    buf.finish()
}

/// Parameters for the proc bottom border rendering.
pub(super) struct BottomBorderParams<'a> {
    pub(super) x: usize,
    pub(super) bottom_y: usize,
    pub(super) width: usize,
    pub(super) filter: &'a str,
    pub(super) filtering: bool,
    pub(super) followed_pid: u32,
    pub(super) visible: usize,
    pub(super) total: usize,
    pub(super) armed_name: &'a str,
    pub(super) armed_force: bool,
    /// `true` when the cursor is on a dead row in the paused
    /// snapshot. Renders the `terminate` chip in the dead-row
    /// theme color as an affordance hint that the action is
    /// unavailable on this row.
    pub(super) terminate_disabled: bool,
}

/// Render the bottom border with select, info, terminate, and filter labels.
pub(super) fn draw_bottom_border(p: &BottomBorderParams, theme: &Theme) -> String {
    let border_color = theme.color(tc::PROC_WIDGET);
    let fg = theme.color(tc::MAIN_FG);
    let hi = theme.color(tc::HI_FG);
    let title_color = theme.color(tc::TITLE);
    let mut buf = AnsiBuffer::new();

    if !p.armed_name.is_empty() {
        let (action, confirm_key) = if p.armed_force {
            ("KILL", "T")
        } else {
            ("terminate", "t")
        };
        let prompt = format!("press {} to {} {}", confirm_key, action, p.armed_name);
        let prompt_inset = box_drawing::title_inset(&prompt, border_color, title_color, true);
        buf.mv(p.x + 3, p.bottom_y).text(&prompt_inset);
    } else {
        let select_text = format!("↑{} select {}↓", title_color, hi);
        let select_inset = box_drawing::title_inset(&select_text, border_color, hi, true);
        let info_text = format!("info {}↵", hi);
        let info_inset = box_drawing::title_inset(&info_text, border_color, title_color, true);
        // Dim the `terminate` chip when the cursor is on a dead row
        // in the paused snapshot — affordance hint that the action
        // is unavailable on this row. Both the keybind highlight
        // (`t`) and the body text (`erminate`) shift to
        // `dead_proc_fg`; the chip's `┐`/`┌` connectors stay in
        // the regular border color (they're structural, not
        // semantic). See §6.7 of plan.md.
        let (term_hi, term_text) = if p.terminate_disabled {
            let dead = theme.color(tc::DEAD_PROC_FG);
            (dead, dead)
        } else {
            (hi, title_color)
        };
        let term_inset =
            box_drawing::keybind_inset("terminate", border_color, term_hi, term_text, true);
        let bottom_hints = format!("{}{}{}", select_inset, info_inset, term_inset);
        buf.mv(p.x + 3, p.bottom_y).text(&bottom_hints);

        // Filter label (hidden when armed to make room for confirmation prompt)
        let cursor = if p.filtering {
            format!("{} {}", term::UNDERLINE, term::UNDERLINE_OFF)
        } else {
            String::new()
        };
        let filter_label = if !p.filter.is_empty() || p.filtering {
            let filter_text = format!("filter: {}{}{}", fg, p.filter, cursor);
            box_drawing::keybind_inset(&filter_text, border_color, hi, title_color, true)
        } else {
            box_drawing::keybind_inset("filter", border_color, hi, title_color, true)
        };
        buf.text(&filter_label);
    }

    // Following label (hidden when armed — prompt replaces entire line)
    if p.armed_name.is_empty() && p.followed_pid > 0 {
        // Render "following" as a chip whose colors match the followed row
        // in the list (FOLLOWED_BG background, FOLLOWED_FG foreground), so
        // the eye links the inset and the row. The trailing reset is
        // mandatory: buf.mv() (used by subsequent inset placement) only
        // repositions the cursor and does not clear SGR state, so without
        // a reset the chip's bg would bleed into the count "N/M" inset.
        let follow_bg = theme.background(tc::FOLLOWED_BG);
        let follow_fg = theme.color(tc::FOLLOWED_FG);
        let follow_text = format!("{follow_bg}{follow_fg}following{}", term::RESET);
        let follow_inset = box_drawing::title_inset(&follow_text, border_color, title_color, true);
        buf.text(&follow_inset);
    }

    // Right side: process count with border inset chars
    let count_str = format!("{}/{}", p.visible, p.total);
    let count_x = box_drawing::right_inset_x(p.x, p.width, box_drawing::inset_width(&count_str));
    buf.mv(count_x, p.bottom_y).text(&box_drawing::title_inset(
        &count_str,
        border_color,
        title_color,
        true,
    ));

    buf.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collect::process_display::ProcSort;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut in_esc = false;
        for ch in s.chars() {
            if in_esc {
                if ch.is_ascii_alphabetic() || ch == 'm' {
                    in_esc = false;
                }
                continue;
            }
            if ch == '\x1b' {
                in_esc = true;
                continue;
            }
            out.push(ch);
        }
        out
    }

    #[test]
    fn paused_top_border_contains_paused_chip() {
        let theme = Theme::default();
        let out = draw_top_border(0, 0, 80, ProcSort::Cpu, false, true, &theme);
        let plain = strip_ansi(&out);
        assert!(
            plain.contains("paused"),
            "paused chip should be present when paused=true: {plain}"
        );
    }

    #[test]
    fn live_top_border_omits_paused_chip() {
        let theme = Theme::default();
        let out = draw_top_border(0, 0, 80, ProcSort::Cpu, false, false, &theme);
        let plain = strip_ansi(&out);
        assert!(
            !plain.contains("paused"),
            "paused chip must not appear when paused=false: {plain}"
        );
    }

    #[test]
    fn paused_chip_uses_title_inset_connectors() {
        // The chip is rendered via box_drawing::title_inset, which
        // produces ┐text┌. Asserting these connector chars wrap the
        // word `paused` ensures the chip uses the standard helper
        // and not raw ANSI / hand-rolled text.
        let theme = Theme::default();
        let out = draw_top_border(0, 0, 80, ProcSort::Cpu, false, true, &theme);
        let plain = strip_ansi(&out);
        // title_inset on a top border emits TITLE_LEFT before and
        // TITLE_RIGHT after the content. We only need to confirm
        // both connectors exist on either side of "paused".
        let idx = plain.find("paused").expect("paused chip must be present");
        let before = &plain[..idx];
        let after = &plain[idx + "paused".len()..];
        assert!(
            before.contains(crate::draw::box_drawing::title_syms::TITLE_LEFT),
            "left connector must precede the chip text"
        );
        assert!(
            after.starts_with(crate::draw::box_drawing::title_syms::TITLE_RIGHT),
            "right connector must immediately follow the chip text"
        );
    }

    fn bottom_params(terminate_disabled: bool) -> BottomBorderParams<'static> {
        BottomBorderParams {
            x: 0,
            bottom_y: 10,
            width: 80,
            filter: "",
            filtering: false,
            followed_pid: 0,
            visible: 5,
            total: 5,
            armed_name: "",
            armed_force: false,
            terminate_disabled,
        }
    }

    #[test]
    fn terminate_chip_dimmed_when_selected_pid_dead() {
        let theme = Theme::default();
        let out = draw_bottom_border(&bottom_params(true), &theme);
        let dead_fg = theme.color(tc::DEAD_PROC_FG);
        // The first character of the chip text (`t` in `terminate`)
        // is rendered in the chip's `hi` color. When disabled, both
        // the hi and the body color are dead_proc_fg, so the chip
        // text is preceded by dead_proc_fg + `t` + dead_proc_fg +
        // `erminate`. We assert the body-color shift to be sure.
        assert!(
            out.contains(&format!("{dead_fg}erminate")),
            "disabled terminate chip body should be in dead_proc_fg"
        );
    }

    #[test]
    fn terminate_chip_normal_color_when_live() {
        let theme = Theme::default();
        let out = draw_bottom_border(&bottom_params(false), &theme);
        let title_color = theme.color(tc::TITLE);
        assert!(
            out.contains(&format!("{title_color}erminate")),
            "live terminate chip body should be in TITLE color"
        );
    }
}
