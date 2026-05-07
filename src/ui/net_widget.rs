use crate::collect::CollectStatus;
use crate::domain::network::NetInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::{Graph, GraphMode};
use crate::theme::{Theme, gradient_color};
use crate::theme_keys as tc;
use crate::tools;

use super::WidgetArea;

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

/// Per-frame view passed to [`draw`].
///
/// `iface` is the currently-selected interface name (per-frame,
/// from `NetworkViewState::selected_iface`). `auto_scale` /
/// `sync_scale` come from `RuntimeView`. `graph_symbol` is
/// pre-resolved from `NetConfig::graph_symbol_net +
/// UiConfig::graph_symbol`.
pub struct NetFrame<'a> {
    pub iface: &'a str,
    pub auto_scale: bool,
    pub sync_scale: bool,
    pub max_download: i64,
    pub max_upload: i64,
    pub graph_symbol: GraphMode,
    pub swap_dl_ul: bool,
    pub base_10: bool,
}

/// Draw the network widget into an ANSI string matching btop's layout.
///
/// Layout:
/// ╭─┐³net┌───────────────────────────────────────────────────────────┐1 Gbps┌─╮
/// │              ⢸⡇                        ⢸⡇                        0.0B/s ▼ │
/// │              ⢸⣿⣿⡇                      ⢸⡇                                 │
/// │              ⢸⣿⣿⣇⡀                     ⢸⡇                                 │
/// │             ⢸⣿⣿⣿⣿⡇                     ⢸⡇                                 │
/// │             ⢸⣿⣿⣿⣿⡇                    ⢠⣼⡇                                 │
/// │⣀⣀⣠⣄⣀⣀⣀⣀⣀⣀⣀⣀⣀⣸⣿⣿⣿⣿⣇⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣸⣿⣿⣿⣿⣇⣠⣄⣀⣀⣀⣀⣀⣀⣀⣀⣀⣸⣇⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀│
/// │⠉⠉⠉⠉⠉⠉⠉⠉⠉⠙⠋⠉⠉⢹⣿⣿⡏⠉⠙⠋⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠙⠹⠏⢹⣿⡏⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉│
/// │             ⢸⣿⣿⡇                      ⢸⣿⡇                                 │
/// │             ⢸⣿⣿⡇                      ⢸⣿⡇                                 │
/// │             ⢸⡏⠉⠁                      ⠈⢹⡇                                 │
/// │             ⢸⡇                         ⢸⡇                                 │
/// │                                        ⢸⡇                        0.0B/s ▲ │
/// ╰─┘sync└┘auto└┘zero└┘← b Ethernet n →└──────────────────────────────────────╯
pub fn draw(
    net: &NetInfo,
    area: &WidgetArea,
    theme: &Theme,
    settings: &NetFrame,
    status: &CollectStatus,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let border_color = theme.color(tc::NET_WIDGET);
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
        line_color: border_color,
        fill: true,
        title: "net",
        title2: "",
        num: super::NET_KEY,
        rounded,
        hi_color: hi,
        title_color,
    }));

    super::draw_status_inset(&mut buf, status, "net", x, y, border_color, title_color);

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
        let pct = if top_stat.top > 0 {
            ((top_stat.speed * 100) / top_stat.top.max(1)).min(100) as i32
        } else {
            0
        };
        let top_color = gradient_color(top_grad, pct);
        let label = format!("{} {}", speed, top_arrow);
        let label_vis = tools::ulen(&label, false);
        let lx = x + width.saturating_sub(label_vis + 1);
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
        let pct = if bot_stat.top > 0 {
            ((bot_stat.speed * 100) / bot_stat.top.max(1)).min(100) as i32
        } else {
            0
        };
        let bot_color = gradient_color(bot_grad, pct);
        let label = format!("{} {}", speed, bot_arrow);
        let label_vis = tools::ulen(&label, false);
        let lx = x + width.saturating_sub(label_vis + 1);
        let label_y = y + height - 1;
        buf.mv(lx, label_y).color(bot_color).text(&label);
    }

    // Link speed inset on top right border
    if net.link_speed > 0 {
        let speed_str = format_link_speed(net.link_speed);
        let inset = box_drawing::title_inset(&speed_str, border_color, title_color, false);
        let inset_x = box_drawing::right_inset_x(x, width, box_drawing::inset_width(&speed_str));
        buf.mv(inset_x, y + 1).text(&inset);
    }

    // Bottom border: sync, auto, zero, interface selector (left side);
    // cumulative totals (right side).
    // sync/auto append `*` when active (mirrors disk's `io*` and
    // proc's `tre*e` convention for binary toggles). zero is a
    // momentary action and never has a marker.
    let bottom_y = y + height;
    let iface_display = tools::uresize(settings.iface, 15, false);

    let sync_marker = if net_sync { "*" } else { "" };
    let auto_marker = if net_auto { "*" } else { "" };
    let sync_text = format!("{hi}s{title_color}ync{sync_marker}");
    let auto_text = format!("{hi}a{title_color}uto{auto_marker}");
    let sync_inset = box_drawing::title_inset(&sync_text, border_color, title_color, true);
    let auto_inset = box_drawing::title_inset(&auto_text, border_color, title_color, true);
    let zero_inset = box_drawing::keybind_inset("zero", border_color, hi, title_color, true);
    let iface_text = format!(
        "← {}b{} {} {}n{} →",
        hi, title_color, iface_display, hi, title_color,
    );
    let iface_inset = box_drawing::title_inset(&iface_text, border_color, title_color, true);

    let mut bx = x + 3;
    buf.mv(bx, bottom_y).text(&sync_inset);
    bx += box_drawing::inset_width(&format!("sync{sync_marker}"));
    buf.mv(bx, bottom_y).text(&auto_inset);
    bx += box_drawing::inset_width(&format!("auto{auto_marker}"));
    buf.mv(bx, bottom_y).text(&zero_inset);
    bx += box_drawing::inset_width("zero");
    buf.mv(bx, bottom_y).text(&iface_inset);
    bx += box_drawing::inset_width(&iface_text);

    // Bottom-right: cumulative totals since last reset (the data
    // that `z` operates on). Format `↓ 12.4G ↑ 3.1G`. Skipped when
    // the bottom border doesn't have room without overlapping the
    // left-side keybind/iface insets.
    let dl_total = tools::floating_humanizer(
        net.stat.download.displayed_total(),
        true,
        0,
        false,
        false,
        base_10,
    );
    let ul_total = tools::floating_humanizer(
        net.stat.upload.displayed_total(),
        true,
        0,
        false,
        false,
        base_10,
    );
    let totals_text = format!("↓ {dl_total} ↑ {ul_total}");
    let totals_vis = box_drawing::inset_width(&totals_text);
    let totals_x = box_drawing::right_inset_x(x, width, totals_vis);
    // Need at least one column of border between the iface inset's
    // right edge and the totals inset's left edge so they read as
    // separate elements rather than a continuous string.
    if totals_x > bx {
        let totals_inset = box_drawing::title_inset(&totals_text, border_color, title_color, true);
        buf.mv(totals_x, bottom_y).text(&totals_inset);
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

    fn make_area() -> WidgetArea {
        WidgetArea {
            x: 1,
            y: 1,
            width: 60,
            height: 14,
            rounded: true,
        }
    }

    fn make_frame() -> NetFrame<'static> {
        NetFrame {
            iface: "Ethernet",
            auto_scale: true,
            sync_scale: false,
            max_download: 100,
            max_upload: 100,
            graph_symbol: GraphMode::Braille,
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
            &make_frame(),
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
            &make_frame(),
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
            &make_frame(),
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

    #[test]
    fn iface_inset_colours_arrows_as_label_and_letters_as_keybind() {
        // Locks in the rule that arrow-style keybind insets render
        // arrows + spaces in the label colour (TITLE) and only the
        // keybind letters themselves in the keybind colour (HI_FG).
        // Pre-fix everything in `←b Ethernet n→` was rendered in HI_FG.
        // The format also has a space on each side of every arrow so
        // the keybind letter and the arrow read as separate tokens.
        use crate::theme_keys as tc;
        let theme = Theme::default();
        let output = draw(
            &make_net_info(),
            &make_area(),
            &theme,
            &make_frame(),
            &CollectStatus::Ok,
        );
        let title = theme.color(tc::TITLE);
        let hi = theme.color(tc::HI_FG);

        // `← ` (arrow + trailing space) is in TITLE; the title_inset
        // wrapper sets text_color = TITLE, so both inherit it.
        assert!(
            output.contains(&format!("{title}← ")),
            "left arrow + trailing space should render in TITLE colour"
        );
        // `b` is preceded by HI (embedded switch).
        assert!(
            output.contains(&format!("{hi}b")),
            "keybind 'b' should render in HI colour"
        );
        // The space + iface name + space region is in TITLE.
        assert!(
            output.contains(&format!("{title} Ethernet ")),
            "iface name and surrounding spaces should render in TITLE colour"
        );
        // `n` is preceded by HI.
        assert!(
            output.contains(&format!("{hi}n")),
            "keybind 'n' should render in HI colour"
        );
        // ` →` (leading space + arrow) is in TITLE so the closing
        // border isn't HI-coloured and the arrow isn't fused with `n`.
        assert!(
            output.contains(&format!("{title} →")),
            "leading space + right arrow should render in TITLE colour"
        );
    }

    #[test]
    fn sync_auto_insets_have_no_marker_when_inactive() {
        // Mirror of disk_widget's io_inset_inactive_has_no_star_marker:
        // when both auto and sync are off, neither inset should show a `*`.
        let mut s = make_frame();
        s.auto_scale = false;
        s.sync_scale = false;
        let output = draw(
            &make_net_info(),
            &make_area(),
            &Theme::default(),
            &s,
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            !plain.contains("sync*"),
            "no '*' marker should appear next to 'sync' when sync is inactive",
        );
        assert!(
            !plain.contains("auto*"),
            "no '*' marker should appear next to 'auto' when auto is inactive",
        );
    }

    #[test]
    fn sync_inset_appends_star_marker_when_active() {
        // Mirror of disk_widget's io_inset_active_appends_star_marker.
        let mut s = make_frame();
        s.sync_scale = true;
        let output = draw(
            &make_net_info(),
            &make_area(),
            &Theme::default(),
            &s,
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("sync*"),
            "visible text should contain 'sync*' when sync_scale is on",
        );
    }

    #[test]
    fn auto_inset_appends_star_marker_when_active() {
        let mut s = make_frame();
        s.auto_scale = true;
        let output = draw(
            &make_net_info(),
            &make_area(),
            &Theme::default(),
            &s,
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("auto*"),
            "visible text should contain 'auto*' when auto_scale is on",
        );
    }

    #[test]
    fn zero_inset_never_shows_marker() {
        // `z` is a momentary action (resets totals); it has no
        // toggle state, so the `zero` inset must never grow a `*`.
        for (auto, sync) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut s = make_frame();
            s.auto_scale = auto;
            s.sync_scale = sync;
            let output = draw(
                &make_net_info(),
                &make_area(),
                &Theme::default(),
                &s,
                &CollectStatus::Ok,
            );
            let plain = strip_ansi(&output);
            assert!(
                !plain.contains("zero*"),
                "zero inset must never show a '*' marker (auto={auto} sync={sync})",
            );
        }
    }

    /// Build a NetInfo whose displayed totals are exactly
    /// `dl_total_bytes` / `ul_total_bytes`. Uses `last` only
    /// (zero offset, zero rollover).
    fn make_net_info_with_totals(dl_total_bytes: u64, ul_total_bytes: u64) -> NetInfo {
        let mut info = make_net_info();
        info.stat.download.last = dl_total_bytes;
        info.stat.upload.last = ul_total_bytes;
        info.stat.download.offset = 0;
        info.stat.upload.offset = 0;
        info
    }

    #[test]
    fn totals_inset_renders_on_bottom_right_border() {
        let info = make_net_info_with_totals(12 * 1024 * 1024 * 1024, 3 * 1024 * 1024 * 1024);
        let output = draw(
            &info,
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        // Expected format: ↓ <dl> ↑ <ul>, in base-2 sizing.
        assert!(
            plain.contains("↓ 12.0G ↑ 3.00G")
                || plain.contains("↓ 12.0G ↑ 3.0G")
                || plain.contains("↓ 12G ↑ 3"),
            "totals inset should appear with directional arrows; got: {plain}"
        );
    }

    #[test]
    fn totals_inset_uses_displayed_total_after_reset() {
        // After `z`, offset == last + rollover, so displayed total
        // is 0. Confirm the rendered inset reflects that, not raw
        // `last`.
        let mut info = make_net_info_with_totals(5_000_000, 7_000_000);
        info.stat.download.offset = info.stat.download.last;
        info.stat.upload.offset = info.stat.upload.last;
        let output = draw(
            &info,
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("↓ 0.0B ↑ 0.0B") || plain.contains("↓ 0B ↑ 0B"),
            "displayed totals should be 0 after offset == last; got: {plain}"
        );
    }

    #[test]
    fn totals_inset_skipped_when_bottom_border_lacks_room() {
        // A narrow widget where the four left-side insets +
        // iface name leave no horizontal room on the bottom
        // border for a totals inset must omit the totals rather
        // than overlap.
        let mut area = make_area();
        area.width = 50;
        let info = make_net_info_with_totals(1_000_000, 2_000_000);
        let output = draw(
            &info,
            &area,
            &Theme::default(),
            &make_frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        // Down arrow + space + value would be present on a wider
        // widget. With width=50 it should be absent. (The in-graph
        // ▼ arrow uses a different glyph, so this check is unique
        // to the border inset.)
        assert!(
            !plain.contains("↓ "),
            "totals inset must be skipped when there is no room; got: {plain}"
        );
    }
}
