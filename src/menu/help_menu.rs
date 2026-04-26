use crate::draw::box_drawing;
use crate::tools;

/// Draw the help menu centered on screen.
pub fn draw(term_width: usize, term_height: usize) -> String {
    let w = 60.min(term_width);
    let h = 40.min(term_height);
    let x = (term_width.saturating_sub(w)) / 2;
    let y = (term_height.saturating_sub(h)) / 2;

    let mut out = box_drawing::create_box(&box_drawing::BoxConfig {
        x, y, width: w, height: h, line_color: "", fill: true,
        title: "help", title2: "", num: 0, rounded: true,
    });

    let lines = [
        "─── Global ───────────────────────",
        "  q / Ctrl-C    Quit",
        "  m / Esc       Toggle main menu",
        "  h / ? / F1    Toggle help",
        "  o / F2        Toggle options",
        "  1-4           Toggle box (cpu/mem/net/proc)",
        "  d             Toggle disk info",
        "  +/-           Adjust update speed",
        "",
        "─── CPU ──────────────────────────",
        "  c             Toggle per-core CPU",
        "",
        "─── NET ──────────────────────────",
        "  n             Next network interface",
        "  b             Previous network interface",
        "  a             Toggle net auto-scale",
        "  y             Toggle net sync upload/download",
        "",
        "─── PROC ─────────────────────────",
        "  Up/Down       Select process",
        "  PgUp/PgDn     Page through processes",
        "  Home/End      Jump to first/last",
        "  Left/Right    Cycle sort column",
        "  r             Reverse sort order",
        "  f / /         Toggle kernel filter",
        "  e             Toggle tree view",
        "  i             Toggle IO mode",
        "  t             Terminate process",
        "  Enter         Show process details",
    ];

    for (i, line) in lines.iter().take(h.saturating_sub(3)).enumerate() {
        let trunc = tools::uresize(line, w.saturating_sub(4), false);
        out.push_str(&format!("\x1b[{};{}H{}", y + 2 + i, x + 2, trunc));
    }

    out
}
