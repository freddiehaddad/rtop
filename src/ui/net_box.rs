use crate::domain::network::NetInfo;
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::box_drawing::title_syms;
use crate::draw::graph::{Graph, GraphSymbol};
use crate::theme::Theme;
use crate::tools;

/// Draw the network box into an ANSI string matching btop's layout.
///
/// Layout:
/// ╭─ net ─────────────────────────────────╮
/// │⣿⣷⣤⣠⣀⡀⢠⣿⣷⣤⣠⣀⡀⢠⣿⣷⣤⣠⣀⡀    ▼ 1.2M/s │
/// │⣿⣷⣤⣠⣀⡀⢠⣿⣷⣤⣠⣀⡀⢠⣿⣷⣤⣠⣀⡀            │
/// │⡇⣇⣧⣷⣿⡇⣇⣧⣷⣿⡇⣇⣧⣷⣿⡇⣇⣧⣷⣿            │
/// │⡇⣇⣧⣷⣿⡇⣇⣧⣷⣿⡇⣇⣧⣷⣿⡇⣇⣧⣷⣿    ▲ 0.5M/s │
/// ╰── < Ethernet > ─── b ◀ ── n ▶ ───────╯
pub fn draw(
    net: &NetInfo,
    iface: &str,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    rounded: bool,
    theme: &Theme,
) -> String {
    let box_color = theme.c("net_box");
    let fg = theme.c("main_fg");
    let dl_grad = theme.g("download");
    let ul_grad = theme.g("upload");
    let hi = theme.c("hi_fg");
    let graph_text_color = theme.c("graph_text");

    let title = format!("{}net", fg);
    let mut out = box_drawing::create_box(x, y, width, height, box_color, true, "net", "", 3, rounded);

    let graph_width = width.saturating_sub(2);
    let inner_h = height.saturating_sub(2);

    if inner_h == 0 || graph_width == 0 {
        out.push_str("\x1b[0m");
        return out;
    }

    // Split inner area between download (top half) and upload (bottom half)
    let dl_rows = inner_h / 2;
    let ul_rows = inner_h - dl_rows;

    // Download graph (normal orientation, top half)
    if let Some(dl_bw) = net.bandwidth.get("download") {
        if dl_rows > 0 {
            let mut graph = Graph::new(graph_width, dl_rows, GraphSymbol::Braille, false, true, 0, 0);
            let max_val = dl_bw.iter().copied().max().unwrap_or(1).max(1);
            graph.max_value = max_val;
            graph.create(dl_bw);
            let rows = graph.render_rows_colored(dl_bw, dl_grad);
            for (i, row) in rows.iter().enumerate() {
                out.push_str(&format!("\x1b[{};{}H{}", y + 2 + i, x + 2, row));
            }
        }
    }

    // Download speed label overlaid at top-right: "▼ 1.2M/s"
    if let Some(dl) = net.stat.get("download") {
        let speed = tools::floating_humanizer(dl.speed, true, 0, false, true, false);
        let dl_color = if !dl_grad.is_empty() {
            let idx = if dl.top > 0 {
                (dl.speed * 100 / dl.top.max(1)) as usize
            } else {
                0
            };
            &dl_grad[idx.min(100)]
        } else {
            fg
        };
        let label = format!("▼ {}", speed);
        let lx = x + width.saturating_sub(label.len() + 2);
        out.push_str(&format!("\x1b[{};{}H{}{}", y + 2, lx, dl_color, label));
    }

    // Upload graph (inverted orientation, bottom half)
    if let Some(ul_bw) = net.bandwidth.get("upload") {
        if ul_rows > 0 {
            let ul_start_y = y + 2 + dl_rows;
            let mut graph =
                Graph::new(graph_width, ul_rows, GraphSymbol::Braille, true, true, 0, 0);
            let max_val = ul_bw.iter().copied().max().unwrap_or(1).max(1);
            graph.max_value = max_val;
            graph.create(ul_bw);
            let rows = graph.render_rows_colored(ul_bw, ul_grad);
            for (i, row) in rows.iter().enumerate() {
                out.push_str(&format!("\x1b[{};{}H{}", ul_start_y + i, x + 2, row));
            }
        }
    }

    // Upload speed label overlaid at bottom-right: "▲ 0.5M/s"
    if let Some(ul) = net.stat.get("upload") {
        let speed = tools::floating_humanizer(ul.speed, true, 0, false, true, false);
        let ul_color = if !ul_grad.is_empty() {
            let idx = if ul.top > 0 {
                (ul.speed * 100 / ul.top.max(1)) as usize
            } else {
                0
            };
            &ul_grad[idx.min(100)]
        } else {
            fg
        };
        let label = format!("▲ {}", speed);
        let lx = x + width.saturating_sub(label.len() + 2);
        let label_y = y + height - 2;
        out.push_str(&format!("\x1b[{};{}H{}{}", label_y, lx, ul_color, label));
    }

    // Interface name and keybind hints at bottom border
    let iface_label = format!("{}< {}{}{} >", fg, hi, iface, fg);
    let nav_hints = format!(
        " {}{}{}b{} ◀{}{} {}{}{}n{} ▶{}{}",
        box_color, title_syms::TITLE_LEFT_DOWN,
        hi, fg, box_color, title_syms::TITLE_RIGHT_DOWN,
        box_color, title_syms::TITLE_LEFT_DOWN,
        hi, fg, box_color, title_syms::TITLE_RIGHT_DOWN,
    );
    out.push_str(&format!(
        "\x1b[{};{}H {}{}{}",
        y + height, x + 2, iface_label, box_color, nav_hints
    ));

    out.push_str("\x1b[0m");
    out
}
