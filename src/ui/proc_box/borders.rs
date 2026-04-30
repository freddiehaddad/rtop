use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::term;
use crate::theme::Theme;
use crate::theme_keys as tc;

/// Fixed characters consumed by the sort selector inset on the bottom border:
/// left_connector(1) + "← "(2) + " →"(2) + right_connector(1) + gap(1).
const SORT_INSET_OVERHEAD: usize = 7;

/// Render the top border with reverse, tree, and sort selector labels.
pub(super) fn draw_top_border(
    x: usize,
    y: usize,
    width: usize,
    sort_by: &str,
    tree_mode: bool,
    theme: &Theme,
) -> String {
    let box_color = theme.color(tc::PROC_BOX);
    let hi = theme.color(tc::HI_FG);
    let title_color = theme.color(tc::TITLE);
    let mut buf = AnsiBuffer::new();

    let sort_name = if sort_by.is_empty() {
        "cpu lazy"
    } else {
        sort_by
    };
    let tree_star = if tree_mode { "*" } else { "" };

    // Build positions right-to-left from the right corner
    let mut pos = x + width - sort_name.len() - SORT_INSET_OVERHEAD;

    // Sort selector: ┐← sorting →┌
    let sort_text = format!("← {}{} {}→", title_color, sort_name, hi);
    let sort_inset = box_drawing::title_inset(&sort_text, box_color, hi, false);
    buf.mv(pos, y + 1).text(&sort_inset);

    // Tree button: ┐tree┌
    let tree_content = format!("tre{}{}", tree_star, "e");
    let tree_len = tree_content.len();
    if pos > x + 12 + tree_len {
        pos -= tree_len + 2;
        let tree_text = format!("tre{}{}e", tree_star, hi);
        let tree_inset = box_drawing::title_inset(&tree_text, box_color, title_color, false);
        buf.mv(pos, y + 1).text(&tree_inset);
    }

    // Reverse button: ┐reverse┌
    if pos > x + 12 {
        pos -= 9;
        let rev_inset = box_drawing::keybind_inset("reverse", box_color, hi, title_color, false);
        buf.mv(pos, y + 1).text(&rev_inset);
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
}

/// Render the bottom border with select, info, terminate, and filter labels.
pub(super) fn draw_bottom_border(p: &BottomBorderParams, theme: &Theme) -> String {
    let box_color = theme.color(tc::PROC_BOX);
    let fg = theme.color(tc::MAIN_FG);
    let hi = theme.color(tc::HI_FG);
    let title_color = theme.color(tc::TITLE);
    let mut buf = AnsiBuffer::new();

    let select_text = format!("↑{} select {}↓", title_color, hi);
    let select_inset = box_drawing::title_inset(&select_text, box_color, hi, true);
    let info_text = format!("info {}↵", hi);
    let info_inset = box_drawing::title_inset(&info_text, box_color, title_color, true);
    let term_inset = box_drawing::keybind_inset("terminate", box_color, hi, title_color, true);
    let bottom_hints = format!("{}{}{}", select_inset, info_inset, term_inset);
    buf.mv(p.x + 3, p.bottom_y).text(&bottom_hints);

    // Filter label
    let cursor = if p.filtering {
        format!("{} {}", term::UNDERLINE, term::UNDERLINE_OFF)
    } else {
        String::new()
    };
    let filter_label = if !p.filter.is_empty() || p.filtering {
        let filter_text = format!("filter: {}{}{}", fg, p.filter, cursor);
        box_drawing::keybind_inset(&filter_text, box_color, hi, title_color, true)
    } else {
        box_drawing::keybind_inset("filter", box_color, hi, title_color, true)
    };
    buf.text(&filter_label);

    // Following label
    if p.followed_pid > 0 {
        let follow_bg = theme.color(tc::PROC_FOLLOW_BG);
        let follow_text = format!("{}following", follow_bg);
        let follow_inset = box_drawing::title_inset(&follow_text, box_color, title_color, true);
        buf.text(&follow_inset);
    }

    // Right side: process count with border inset chars
    let count_str = format!("{}/{}", p.visible, p.total);
    let count_x = box_drawing::right_inset_x(p.x, p.width, box_drawing::inset_width(&count_str));
    buf.mv(count_x, p.bottom_y)
        .text(&box_drawing::title_inset(&count_str, box_color, fg, true));

    buf.finish()
}
