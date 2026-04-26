use crate::draw::box_drawing;
use crate::tools;

/// Draw the options menu centered on screen.
pub fn draw(term_width: usize, term_height: usize, entries: &[(&str, &str)]) -> String {
    let w = 60.min(term_width);
    let h = 24.min(term_height);
    let x = (term_width.saturating_sub(w)) / 2;
    let y = (term_height.saturating_sub(h)) / 2;

    let mut out = box_drawing::create_box(x, y, w, h, "", true, "options", "", 0, true);

    for (i, (key, value)) in entries.iter().take(h.saturating_sub(3)).enumerate() {
        let line = format!("  {:<24} {}", key, value);
        let trunc = tools::uresize(&line, w.saturating_sub(4), false);
        out.push_str(&format!("\x1b[{};{}H{}", y + 2 + i, x + 2, trunc));
    }

    out
}
