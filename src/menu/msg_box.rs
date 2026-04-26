use crate::draw::box_drawing;
use crate::tools;

/// Message box style.
pub enum MsgBoxStyle {
    Ok,
    YesNo,
}

/// Draw a modal message box centered on screen.
pub fn draw(
    term_width: usize,
    term_height: usize,
    title: &str,
    message: &str,
    style: MsgBoxStyle,
) -> String {
    let w = 50.min(term_width);
    let h = 8.min(term_height);
    let x = (term_width.saturating_sub(w)) / 2;
    let y = (term_height.saturating_sub(h)) / 2;

    let mut out = box_drawing::create_box(x, y, w, h, "", true, title, "", 0, true);

    let msg_trunc = tools::uresize(message, w.saturating_sub(4), false);
    out.push_str(&format!("\x1b[{};{}H{}", y + 3, x + 2, msg_trunc));

    let buttons = match style {
        MsgBoxStyle::Ok => "[OK]",
        MsgBoxStyle::YesNo => "[Y] Yes  [N] No",
    };
    let bx = x + (w.saturating_sub(tools::ulen(buttons, false))) / 2;
    out.push_str(&format!("\x1b[{};{}H{}", y + h - 2, bx, buttons));

    out
}
