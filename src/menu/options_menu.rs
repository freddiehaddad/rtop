use crate::draw::box_drawing;
use crate::theme::Theme;
use crate::tools;

/// A single option entry in the options menu.
pub struct OptionEntry {
    pub key: String,
    pub display: String,
    pub kind: OptionKind,
}

pub enum OptionKind {
    Bool,
    Int,
    StringChoice(Vec<String>),
}

/// Draw the options menu centered on screen with a highlighted selection.
pub fn draw(
    term_width: usize,
    term_height: usize,
    entries: &[(String, String, bool)], // (key, display_value, is_bool)
    selected: usize,
    theme: &Theme,
) -> String {
    let w = 64.min(term_width.saturating_sub(4));
    let h = (entries.len() + 5).min(term_height.saturating_sub(2));
    let x = (term_width.saturating_sub(w)) / 2;
    let y = (term_height.saturating_sub(h)) / 2;

    let box_color = theme.c("div_line");
    let fg = theme.c("main_fg");
    let title = theme.c("title");
    let hi = theme.c("hi_fg");
    let sel_bg = theme.c("selected_bg");
    let sel_fg = theme.c("selected_fg");
    let inactive = theme.c("inactive_fg");

    let mut out = box_drawing::create_box(x, y, w, h, box_color, true, "options", "", 0, true);

    // Instructions
    out.push_str(&format!(
        "\x1b[{};{}H{}↑↓{}select  {}Enter{}toggle  {}Esc{}close",
        y + 1, x + 2, hi, fg, hi, fg, hi, fg
    ));

    // Option rows
    let visible_rows = h.saturating_sub(4);
    let scroll_offset = if selected >= visible_rows {
        selected - visible_rows + 1
    } else {
        0
    };

    for (i, (key, value, is_bool)) in entries
        .iter()
        .skip(scroll_offset)
        .take(visible_rows)
        .enumerate()
    {
        let abs_idx = scroll_offset + i;
        let row = y + 3 + i;
        let inner_w = w.saturating_sub(4);

        let val_display = if *is_bool {
            if value == "True" || value == "true" {
                format!("{}◉ {}{}", hi, value, fg)
            } else {
                format!("{}○ {}{}", inactive, value, fg)
            }
        } else {
            format!("{}{}{}", title, value, fg)
        };

        let key_display = tools::uresize(key, 26, false);

        if abs_idx == selected {
            // Highlighted row
            let bg_esc = sel_bg.replace("38;2", "48;2");
            out.push_str(&format!(
                "\x1b[{};{}H{}{} {:<26} {}{}\x1b[0m",
                row,
                x + 2,
                bg_esc,
                sel_fg,
                key_display,
                val_display,
                " ".repeat(inner_w.saturating_sub(28 + value.len()).min(20)),
            ));
        } else {
            out.push_str(&format!(
                "\x1b[{};{}H{} {:<26} {}",
                row,
                x + 2,
                fg,
                key_display,
                val_display,
            ));
        }
    }

    out.push_str("\x1b[0m");
    out
}
