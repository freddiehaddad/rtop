use crate::domain::network::NetInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::{Graph, GraphSymbol};
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

use super::BoxArea;

/// Extracted settings for the network box, decoupled from Config.
pub struct NetBoxSettings {
    pub auto_scale: bool,
    pub sync_scale: bool,
    pub max_download: i64,
    pub max_upload: i64,
    pub graph_symbol: GraphSymbol,
}

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
    settings: &NetBoxSettings,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.c(tc::NET_BOX);
    let fg = theme.c(tc::MAIN_FG);
    let title_color = theme.c(tc::TITLE);
    let dl_grad = theme.g(tc::GRAD_DOWNLOAD);
    let ul_grad = theme.g(tc::GRAD_UPLOAD);
    let hi = theme.c(tc::HI_FG);

    let mut buf = AnsiBuffer::new();
    buf.text(&box_drawing::create_box(&box_drawing::BoxConfig {
        x,
        y,
        width,
        height,
        line_color: box_color,
        fill: true,
        title: "net",
        title2: "",
        num: 3,
        rounded,
        hi_color: hi,
        title_color,
    }));

    let graph_width = width.saturating_sub(2);
    let inner_h = height.saturating_sub(2);

    if inner_h == 0 || graph_width == 0 {
        return buf.finish();
    }

    let graph_sym = settings.graph_symbol;
    let net_auto = settings.auto_scale;
    let net_sync = settings.sync_scale;

    // Compute graph max values from the visible window only.
    let visible = graph_width;
    let dl_max_raw = if net_auto {
        let bw = &net.bandwidth.download;
        let start = bw.len().saturating_sub(visible);
        bw.iter().skip(start).copied().max().unwrap_or(1).max(1)
    } else {
        (settings.max_download * 1024).max(1)
    };
    let ul_max_raw = if net_auto {
        let bw = &net.bandwidth.upload;
        let start = bw.len().saturating_sub(visible);
        bw.iter().skip(start).copied().max().unwrap_or(1).max(1)
    } else {
        (settings.max_upload * 1024).max(1)
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
    {
        let dl_bw = &net.bandwidth.download;
        if dl_rows > 0 {
            let mut graph = Graph::new(graph_width, dl_rows, graph_sym, false, true, 0, 0);
            graph.max_value = dl_max;
            graph.create(dl_bw);
            let rows = graph.render_rows_colored(dl_bw, dl_grad);
            for (i, row) in rows.iter().enumerate() {
                buf.mv(x + 2, y + 2 + i).text(row);
            }
        }
    }

    // Download speed label overlaid at top-right: "▼ 1.2M/s"
    {
        let dl = &net.stat.download;
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
        buf.mv(lx, y + 2).color(dl_color).text(&label);
    }

    // Upload graph (inverted orientation, bottom half)
    {
        let ul_bw = &net.bandwidth.upload;
        if ul_rows > 0 {
            let ul_start_y = y + 2 + dl_rows;
            let mut graph = Graph::new(graph_width, ul_rows, graph_sym, true, true, 0, 0);
            graph.max_value = ul_max;
            graph.create(ul_bw);
            let rows = graph.render_rows_colored(ul_bw, ul_grad);
            for (i, row) in rows.iter().enumerate() {
                buf.mv(x + 2, ul_start_y + i).text(row);
            }
        }
    }

    // Upload speed label overlaid at bottom-right: "▲ 0.5M/s"
    {
        let ul = &net.stat.upload;
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
        buf.mv(lx, label_y).color(ul_color).text(&label);
    }

    // Interface selector and buttons on TOP border
    let iface_display = tools::uresize(iface, 15, false);

    // Build right-to-left on top border
    let mut top_x = x + width - 1;

    // Interface selector: ┐←b Ethernet n→┌
    let iface_text = format!("←b {}{} {}n→", title_color, iface_display, hi);
    let iface_inset = box_drawing::title_inset(&iface_text, box_color, hi, false);
    let iface_vis_len = 6 + iface_display.len();
    top_x = top_x.saturating_sub(iface_vis_len + 2);
    buf.mv(top_x, y + 1).text(&iface_inset);

    // zero button: ┐zero┌
    let zero_inset = box_drawing::keybind_inset("zero", box_color, hi, title_color, false);
    top_x = top_x.saturating_sub(6);
    if top_x > x + 10 {
        buf.mv(top_x, y + 1).text(&zero_inset);
    }

    // auto button: ┐auto┌
    let auto_inset = box_drawing::keybind_inset("auto", box_color, hi, title_color, false);
    top_x = top_x.saturating_sub(6);
    if top_x > x + 10 {
        buf.mv(top_x, y + 1).text(&auto_inset);
    }

    // sync button: ┐sync┌
    let sync_text = format!("s{}y{}nc", hi, title_color);
    let sync_inset = box_drawing::title_inset(&sync_text, box_color, title_color, false);
    top_x = top_x.saturating_sub(6);
    if top_x > x + 10 {
        buf.mv(top_x, y + 1).text(&sync_inset);
    }

    buf.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::network::{NetBandwidth, NetStat, NetStatPair};
    use std::collections::VecDeque;

    fn strip_ansi(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut in_escape = false;
        for ch in s.chars() {
            if in_escape {
                if ch.is_ascii_alphabetic() || ch == 'm' {
                    in_escape = false;
                }
                continue;
            }
            if ch == '\x1b' {
                in_escape = true;
                continue;
            }
            result.push(ch);
        }
        result
    }

    fn make_net_info() -> NetInfo {
        NetInfo {
            bandwidth: NetBandwidth {
                download: VecDeque::from([1024, 2048, 4096]),
                upload: VecDeque::from([512, 1024, 2048]),
            },
            stat: NetStatPair {
                download: NetStat {
                    speed: 4096,
                    top: 8192,
                    ..NetStat::default()
                },
                upload: NetStat {
                    speed: 2048,
                    top: 4096,
                    ..NetStat::default()
                },
            },
            ipv4: "192.168.1.100".into(),
            ipv6: String::new(),
            connected: true,
        }
    }

    fn make_area() -> BoxArea {
        BoxArea {
            x: 1,
            y: 1,
            width: 60,
            height: 14,
            rounded: true,
        }
    }

    fn make_settings() -> NetBoxSettings {
        NetBoxSettings {
            auto_scale: true,
            sync_scale: false,
            max_download: 100,
            max_upload: 100,
            graph_symbol: GraphSymbol::Braille,
        }
    }

    #[test]
    fn draw_contains_net_title() {
        let output = draw(
            &make_net_info(),
            "Ethernet",
            &make_area(),
            &Theme::default(),
            &make_settings(),
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("net"), "output should contain 'net' title");
    }

    #[test]
    fn draw_contains_interface_name() {
        let output = draw(
            &make_net_info(),
            "Ethernet",
            &make_area(),
            &Theme::default(),
            &make_settings(),
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("Ethernet"),
            "output should contain interface name 'Ethernet'"
        );
    }

    #[test]
    fn draw_contains_direction_indicators() {
        let output = draw(
            &make_net_info(),
            "Ethernet",
            &make_area(),
            &Theme::default(),
            &make_settings(),
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains('▼'),
            "output should contain download indicator '▼'"
        );
        assert!(
            plain.contains('▲'),
            "output should contain upload indicator '▲'"
        );
    }
}
