use crate::domain::process::ProcessAction;
use crate::draw::box_drawing;

/// Draw the process action menu (End Task, Terminate, Suspend, Resume).
#[allow(dead_code)] // UI component — will be connected to input handler
pub fn draw(term_width: usize, term_height: usize, selected: usize) -> String {
    let actions = [
        ProcessAction::EndTask,
        ProcessAction::Terminate,
        ProcessAction::Suspend,
        ProcessAction::Resume,
    ];

    let w = 30.min(term_width);
    let h = (actions.len() + 4).min(term_height);
    let x = (term_width.saturating_sub(w)) / 2;
    let y = (term_height.saturating_sub(h)) / 2;

    let mut out =
        box_drawing::create_box(x, y, w, h, "", true, "signal", "", 0, true);

    for (i, action) in actions.iter().enumerate() {
        let prefix = if i == selected { "> " } else { "  " };
        let line = format!("{}{}", prefix, action);
        out.push_str(&format!("\x1b[{};{}H{}", y + 2 + i, x + 2, line));
    }

    out
}
