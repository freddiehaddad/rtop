use crate::draw::box_drawing;
use crate::tools;

/// Draw the help menu centered on screen.
pub fn draw(term_width: usize, term_height: usize) -> String {
    let w = 60.min(term_width);
    let h = 20.min(term_height);
    let x = (term_width.saturating_sub(w)) / 2;
    let y = (term_height.saturating_sub(h)) / 2;

    let mut out = box_drawing::create_box(x, y, w, h, "", true, "help", "", 0, true);

    let lines = [
        "Key Bindings:",
        "",
        "  q / Ctrl-C    Quit",
        "  m / Esc       Toggle main menu",
        "  h / F1        Toggle help",
        "  o             Toggle options",
        "  Up/Down       Select process",
        "  Enter         Show process details",
        "  f             Filter processes",
        "  t             Toggle tree view",
        "  r             Reverse sort",
        "  +/-           Cycle sort column",
        "  n             Cycle network interface",
        "  b             Cycle graph symbols",
        "  p             Cycle presets",
    ];

    for (i, line) in lines.iter().take(h.saturating_sub(3)).enumerate() {
        let trunc = tools::uresize(line, w.saturating_sub(4), false);
        out.push_str(&format!("\x1b[{};{}H{}", y + 2 + i, x + 2, trunc));
    }

    out
}
