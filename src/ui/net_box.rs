use crate::collect::CollectStatus;
use crate::domain::network::NetInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::{Graph, GraphSymbol};
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

use super::BoxArea;

/// Format link speed in bits/sec to a human-readable string (e.g. "1 Gbps", "100 Mbps").
fn format_link_speed(bps: u64) -> String {
    if bps >= 1_000_000_000 {
        format!("{} Gbps", bps / 1_000_000_000)
    } else if bps >= 1_000_000 {
        format!("{} Mbps", bps / 1_000_000)
    } else if bps >= 1_000 {
        format!("{} Kbps", bps / 1_000)
    } else {
        format!("{bps} bps")
    }
}

/// Extracted settings for the network box, decoupled from Config.
pub struct NetBoxSettings<'a> {
    pub iface: &'a str,
    pub auto_scale: bool,
    pub sync_scale: bool,
    pub max_download: i64,
    pub max_upload: i64,
    pub graph_symbol: GraphSymbol,
    pub swap_dl_ul: bool,
    pub base_10: bool,
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
    area: &BoxArea,
    theme: &Theme,
    settings: &NetBoxSettings,
    status: &CollectStatus,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.color(tc::NET_BOX);
    let fg = theme.color(tc::MAIN_FG);
    let title_color = theme.color(tc::TITLE);
    let dl_grad = theme.gradient(tc::GRAD_DOWNLOAD);
    let ul_grad = theme.gradient(tc::GRAD_UPLOAD);
    let hi = theme.color(tc::HI_FG);

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
        num: super::NET_KEY,
        rounded,
        hi_color: hi,
        title_color,
    }));

    super::draw_status_inset(&mut buf, status, "net", x, y, box_color, title_color);

    let graph_width = width.saturating_sub(2);
    let inner_h = height.saturating_sub(2);

    if inner_h == 0 || graph_width == 0 {
        return buf.finish();
    }

    let graph_sym = settings.graph_symbol;
    let net_auto = settings.auto_scale;
    let net_sync = settings.sync_scale;
    let swap = settings.swap_dl_ul;
    let base_10 = settings.base_10;

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

    // When swap is true: upload on top (normal), download on bottom (inverted)
    // When swap is false: download on top (normal), upload on bottom (inverted)
    let top_bw = if swap {
        &net.bandwidth.upload
    } else {
        &net.bandwidth.download
    };
    let bot_bw = if swap {
        &net.bandwidth.download
    } else {
        &net.bandwidth.upload
    };
    let top_max = if swap { ul_max } else { dl_max };
    let bot_max = if swap { dl_max } else { ul_max };
    let top_grad = if swap { ul_grad } else { dl_grad };
    let bot_grad = if swap { dl_grad } else { ul_grad };
    let top_stat = if swap {
        &net.stat.upload
    } else {
        &net.stat.download
    };
    let bot_stat = if swap {
        &net.stat.download
    } else {
        &net.stat.upload
    };
    let top_arrow = if swap { "▲" } else { "▼" };
    let bot_arrow = if swap { "▼" } else { "▲" };

    // Split inner area between top half and bottom half
    let top_rows = inner_h / 2;
    let bot_rows = inner_h - top_rows;

    // Top graph (normal orientation)
    {
        if top_rows > 0 {
            let mut graph = Graph::new(graph_width, top_rows, graph_sym, false, top_max, 0);
            let rows = graph.render_rows(top_bw, top_grad);
            for (i, row) in rows.iter().enumerate() {
                buf.mv(x + 2, y + 2 + i).text(row);
            }
        }
    }

    // Top speed label overlaid at top-right
    {
        let speed = tools::floating_humanizer(top_stat.speed, true, 0, false, true, base_10);
        let top_color = if !top_grad.is_empty() {
            let idx = if top_stat.top > 0 {
                (top_stat.speed * 100 / top_stat.top.max(1)) as usize
            } else {
                0
            };
            &top_grad[idx.min(100)]
        } else {
            fg
        };
        let label = format!("{} {}", top_arrow, speed);
        let lx = x + width.saturating_sub(label.len() + 2);
        buf.mv(lx, y + 2).color(top_color).text(&label);
    }

    // Bottom graph (inverted orientation)
    {
        if bot_rows > 0 {
            let bot_start_y = y + 2 + top_rows;
            let mut graph = Graph::new(graph_width, bot_rows, graph_sym, true, bot_max, 0);
            let rows = graph.render_rows(bot_bw, bot_grad);
            for (i, row) in rows.iter().enumerate() {
                buf.mv(x + 2, bot_start_y + i).text(row);
            }
        }
    }

    // Bottom speed label overlaid at bottom-right
    {
        let speed = tools::floating_humanizer(bot_stat.speed, true, 0, false, true, base_10);
        let bot_color = if !bot_grad.is_empty() {
            let idx = if bot_stat.top > 0 {
                (bot_stat.speed * 100 / bot_stat.top.max(1)) as usize
            } else {
                0
            };
            &bot_grad[idx.min(100)]
        } else {
            fg
        };
        let label = format!("{} {}", bot_arrow, speed);
        let lx = x + width.saturating_sub(label.len() + 2);
        let label_y = y + height - 1;
        buf.mv(lx, label_y).color(bot_color).text(&label);
    }

    // Link speed inset on top right border
    if net.link_speed > 0 {
        let speed_str = format_link_speed(net.link_speed);
        let inset = box_drawing::title_inset(&speed_str, box_color, title_color, false);
        let inset_x = box_drawing::right_inset_x(x, width, box_drawing::inset_width(&speed_str));
        buf.mv(inset_x, y + 1).text(&inset);
    }

    // Bottom border: sync, auto, zero, interface selector
    let bottom_y = y + height;
    let iface_display = tools::uresize(settings.iface, 15, false);

    let sync_inset = box_drawing::keybind_inset("sync", box_color, hi, title_color, true);
    let auto_inset = box_drawing::keybind_inset("auto", box_color, hi, title_color, true);
    let zero_inset = box_drawing::keybind_inset("zero", box_color, hi, title_color, true);
    let iface_text = format!("←b {}{} {}n→", title_color, iface_display, hi);
    let iface_inset = box_drawing::title_inset(&iface_text, box_color, hi, true);

    let mut bx = x + 3;
    buf.mv(bx, bottom_y).text(&sync_inset);
    bx += box_drawing::inset_width("sync");
    buf.mv(bx, bottom_y).text(&auto_inset);
    bx += box_drawing::inset_width("auto");
    buf.mv(bx, bottom_y).text(&zero_inset);
    bx += box_drawing::inset_width("zero");
    buf.mv(bx, bottom_y).text(&iface_inset);

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
            name: "Ethernet".into(),
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
            link_speed: 1_000_000_000,
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

    fn make_settings() -> NetBoxSettings<'static> {
        NetBoxSettings {
            iface: "Ethernet",
            auto_scale: true,
            sync_scale: false,
            max_download: 100,
            max_upload: 100,
            graph_symbol: GraphSymbol::Braille,
            swap_dl_ul: false,
            base_10: false,
        }
    }

    #[test]
    fn draw_contains_net_title() {
        let output = draw(
            &make_net_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("net"), "output should contain 'net' title");
    }

    #[test]
    fn draw_contains_interface_name() {
        let output = draw(
            &make_net_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
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
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
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
