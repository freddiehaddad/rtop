use crate::domain::process::PriorityClass;
use crate::draw::box_drawing;

/// Draw the priority class selection menu.
pub fn draw(term_width: usize, term_height: usize, selected: usize) -> String {
    let classes = [
        PriorityClass::Idle,
        PriorityClass::BelowNormal,
        PriorityClass::Normal,
        PriorityClass::AboveNormal,
        PriorityClass::High,
        PriorityClass::Realtime,
    ];

    let w = 30.min(term_width);
    let h = (classes.len() + 4).min(term_height);
    let x = (term_width.saturating_sub(w)) / 2;
    let y = (term_height.saturating_sub(h)) / 2;

    let mut out =
        box_drawing::create_box(x, y, w, h, "", true, "priority", "", 0, true);

    for (i, cls) in classes.iter().enumerate() {
        let prefix = if i == selected { "> " } else { "  " };
        let line = format!("{}{}", prefix, cls);
        out.push_str(&format!("\x1b[{};{}H{}", y + 2 + i, x + 2, line));
    }

    out
}
