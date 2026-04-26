use crate::domain::network::NetInfo;
use crate::draw::box_drawing;
use crate::draw::box_drawing::title_syms;
use crate::draw::graph::{Graph, GraphSymbol};
use crate::term;
use crate::theme::Theme;
use crate::tools;

use super::BoxArea;

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
    area: &BoxArea,
    theme: &Theme,
    config: &crate::config::Config,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.c("net_box");
    let fg = theme.c("main_fg");
    let title_color = theme.c("title");
    let dl_grad = theme.g("download");
    let ul_grad = theme.g("upload");
    let hi = theme.c("hi_fg");

    let mut out = box_drawing::create_box(&box_drawing::BoxConfig {
        x, y, width, height, line_color: box_color, fill: true,
        title: "net", title2: "", num: 3, rounded,
        hi_color: hi, title_color,
    });

    let graph_width = width.saturating_sub(2);
    let inner_h = height.saturating_sub(2);

    if inner_h == 0 || graph_width == 0 {
        out.push_str("\x1b[0m");
        return out;
    }

    let graph_sym = GraphSymbol::from_config(config.get_string("graph_symbol_net"), config.get_string("graph_symbol"));
    let net_auto = config.get_bool("net_auto");
    let net_sync = config.get_bool("net_sync");

    // Compute graph max values
    let dl_max_raw = if net_auto {
        net.bandwidth.get("download").map(|bw| bw.iter().copied().max().unwrap_or(1).max(1)).unwrap_or(1)
    } else {
        (config.get_int("net_download") * 1024).max(1)
    };
    let ul_max_raw = if net_auto {
        net.bandwidth.get("upload").map(|bw| bw.iter().copied().max().unwrap_or(1).max(1)).unwrap_or(1)
    } else {
        (config.get_int("net_upload") * 1024).max(1)
    };
    let (dl_max, ul_max) = if net_sync {
        let m = dl_max_raw.max(ul_max_raw);
        (m, m)
    } else {
        (dl_max_raw, ul_max_raw)
    };

    // Split inner area between download (top half) and upload (bottom half)
    let dl_rows = inner_h / 2;
    let ul_rows = inner_h - dl_rows;

    // Download graph (normal orientation, top half)
    if let Some(dl_bw) = net.bandwidth.get("download") {
        if dl_rows > 0 {
            let mut graph = Graph::new(graph_width, dl_rows, graph_sym, false, true, 0, 0);
            graph.max_value = dl_max;
            graph.create(dl_bw);
            let rows = graph.render_rows_colored(dl_bw, dl_grad);
            for (i, row) in rows.iter().enumerate() {
                out.push_str(&format!("{}{}", term::mv(x + 2, y + 2 + i), row));
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
        out.push_str(&format!("{}{}{}", term::mv(lx, y + 2), dl_color, label));
    }

    // Upload graph (inverted orientation, bottom half)
    if let Some(ul_bw) = net.bandwidth.get("upload") {
        if ul_rows > 0 {
            let ul_start_y = y + 2 + dl_rows;
            let mut graph =
                Graph::new(graph_width, ul_rows, graph_sym, true, true, 0, 0);
            graph.max_value = ul_max;
            graph.create(ul_bw);
            let rows = graph.render_rows_colored(ul_bw, ul_grad);
            for (i, row) in rows.iter().enumerate() {
                out.push_str(&format!("{}{}", term::mv(x + 2, ul_start_y + i), row));
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
        let label_y = y + height - 1;
        out.push_str(&format!("{}{}{}", term::mv(lx, label_y), ul_color, label));
    }

    // Interface selector and buttons on TOP border (btop lines 1504-1519)
    // All on the top border, right-aligned: ┐sync┌ ┐auto┌ ┐zero┌ ┐←b Ethernet n→┌
    let iface_display = tools::uresize(iface, 15, false);

    // Build right-to-left on top border
    let mut top_x = x + width - 1; // start from right corner

    // Interface selector: ┐←b Ethernet n→┌
    let iface_inset = format!(
        "{}{}{}←b {}{} {}n→{}{}",
        box_color, title_syms::TITLE_LEFT,
        hi, title_color, iface_display, hi,
        box_color, title_syms::TITLE_RIGHT,
    );
    // visible chars: "←b " + name + " n→" = 3 + name + 3 = 6 + name, plus 2 inset chars
    let iface_vis_len = 6 + iface_display.len();
    top_x = top_x.saturating_sub(iface_vis_len + 2);
    out.push_str(&format!("{}{}", term::mv(top_x, y + 1), iface_inset));

    // zero button: ┐zero┌
    let zero_inset = format!(
        "{}{}{}z{}ero{}{}",
        box_color, title_syms::TITLE_LEFT,
        hi, title_color, box_color, title_syms::TITLE_RIGHT,
    );
    top_x = top_x.saturating_sub(6);
    if top_x > x + 10 {
        out.push_str(&format!("{}{}", term::mv(top_x, y + 1), zero_inset));
    }

    // auto button: ┐auto┌
    let auto_inset = format!(
        "{}{}{}a{}uto{}{}",
        box_color, title_syms::TITLE_LEFT,
        hi, title_color, box_color, title_syms::TITLE_RIGHT,
    );
    top_x = top_x.saturating_sub(6);
    if top_x > x + 10 {
        out.push_str(&format!("{}{}", term::mv(top_x, y + 1), auto_inset));
    }

    // sync button: ┐sync┌
    let sync_inset = format!(
        "{}{}{}s{}y{}nc{}{}",
        box_color, title_syms::TITLE_LEFT,
        title_color, hi, title_color, box_color, title_syms::TITLE_RIGHT,
    );
    top_x = top_x.saturating_sub(6);
    if top_x > x + 10 {
        out.push_str(&format!("{}{}", term::mv(top_x, y + 1), sync_inset));
    }

    out.push_str("\x1b[0m");
    out
}
