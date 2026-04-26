use crate::domain::network::NetInfo;
use crate::draw::box_drawing;
use crate::draw::graph::{Graph, GraphSymbol};
use crate::theme::Theme;
use crate::tools;

/// Draw the network box into an ANSI string.
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

    let mut out = box_drawing::create_box(x, y, width, height, box_color, false, "net", "", 0, rounded);

    let graph_width = width.saturating_sub(2);
    let graph_rows = height.saturating_sub(4) / 2;

    // Download graph
    if let Some(dl_bw) = net.bandwidth.get("download") {
        if graph_rows > 0 {
            let mut graph = Graph::new(graph_width, 1, GraphSymbol::Braille, false, false, 0, 0);
            // Auto-scale: use max value in data
            let max_val = dl_bw.iter().copied().max().unwrap_or(1).max(1);
            graph.max_value = max_val;
            let graph_str = graph.render_row_colored(dl_bw, dl_grad);
            out.push_str(&format!("\x1b[{};{}H{}", y + 2, x + 1, graph_str));
        }
    }

    // Download speed label
    if let Some(dl) = net.stat.get("download") {
        let speed = tools::floating_humanizer(dl.speed, true, 0, false, true, false);
        let dl_color = if !dl_grad.is_empty() {
            let idx = if dl.top > 0 { (dl.speed * 100 / dl.top.max(1)) as usize } else { 0 };
            &dl_grad[idx.min(100)]
        } else { fg };
        let label = format!("{}▼ {}", dl_color, speed);
        let lx = x + width.saturating_sub(label.len().saturating_sub(10) + 2);
        out.push_str(&format!("\x1b[{};{}H{}", y + 2, x + 2, label));
    }

    // Upload graph (inverted)
    if let Some(ul_bw) = net.bandwidth.get("upload") {
        if graph_rows > 0 {
            let graph_y = y + 2 + graph_rows.min(1);
            let mut graph = Graph::new(graph_width, 1, GraphSymbol::Braille, true, false, 0, 0);
            let max_val = ul_bw.iter().copied().max().unwrap_or(1).max(1);
            graph.max_value = max_val;
            let graph_str = graph.render_row_colored(ul_bw, ul_grad);
            out.push_str(&format!("\x1b[{};{}H{}", graph_y, x + 1, graph_str));
        }
    }

    // Upload speed label
    if let Some(ul) = net.stat.get("upload") {
        let speed = tools::floating_humanizer(ul.speed, true, 0, false, true, false);
        let ul_color = if !ul_grad.is_empty() {
            let idx = if ul.top > 0 { (ul.speed * 100 / ul.top.max(1)) as usize } else { 0 };
            &ul_grad[idx.min(100)]
        } else { fg };
        out.push_str(&format!("\x1b[{};{}H{}▲ {}", y + 3, x + 2, ul_color, speed));
    }

    // IP address
    if height > 5 && !net.ipv4.is_empty() {
        out.push_str(&format!("\x1b[{};{}H{}IPv4: {}", y + height - 2, x + 2, fg, net.ipv4));
    }

    // Interface name at bottom
    let iface_line = format!("{}< {}{}{} >", fg, hi, iface, fg);
    out.push_str(&format!("\x1b[{};{}H{}", y + height - 1, x + 2, iface_line));

    out.push_str("\x1b[0m");
    out
}

