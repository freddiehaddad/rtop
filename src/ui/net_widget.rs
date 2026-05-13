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
/// `auto_scale` / `sync_scale` come from `RuntimeView`.
/// `graph_symbol` is pre-resolved from
/// `NetConfig::graph_symbol_net` + `UiConfig::graph_symbol`. The
/// currently-selected adapter is passed to [`draw`] as the first
/// argument (`&NetInfo`); it carries both the stable identifier
/// (`stable_id`) and the description used for the chip text, so
/// the frame doesn't carry an adapter handle.
pub struct NetFrame {
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
/// ╭─┐³net┌────────────────────────────────────────┐← < Ethernet (1 Gbps) > →┌─╮
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
/// ╰─┘sync└┘auto└────────────────────────────────────────────────┘↓ 12G ↑ 3G└─╯
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
        let label_vis = tools::ulen(&label);
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
        let label_vis = tools::ulen(&label);
        let lx = x + width.saturating_sub(label_vis + 1);
        let label_y = y + height - 1;
        buf.mv(lx, label_y).color(bot_color).text(&label);
    }

    // Top-right border (combined scope chip):
    //   ← < <description> [ (<link>) ] > →
    //
    // The chip text is the adapter's driver description (e.g.
    // "Realtek PCIe GbE Family Controller"). The persistent
    // identifier is the adapter GUID (`NetInfo::stable_id`),
    // resolved upstream by the dispatch path; the description is
    // queried at render time from the looked-up `NetInfo` and
    // displayed verbatim.
    //
    // link_speed merges into the same chip rather than rendering
    // as a separate inset because both pieces are properties of
    // the currently-selected interface — the cycler answers
    // "which adapter?" and the link speed answers "running at what?".
    //
    // Truncation is dynamic: when the available zone is too narrow,
    // the description truncates with `…`; if that still doesn't fit,
    // the link is dropped from the chip; if even the link-less chip
    // won't fit, the chip is omitted entirely.
    {
        let display_name_full = net.description.as_str();
        let link_str = if net.link_speed > 0 {
            format_link_speed(net.link_speed)
        } else {
            String::new()
        };

        // Available zone width: from one column past the title chip's
        // right edge, to one column before the widget's right border.
        // `(zone_right + 1).saturating_sub(zone_left)` reports 0
        // columns (rather than a phantom 1) when the title chip
        // would already overflow the right edge.
        let title_chip_end = x + 3 + box_drawing::inset_width("\u{00B3}net");
        let zone_left = title_chip_end + 1;
        let zone_right = x + width - 2;
        let zone_width = (zone_right + 1).saturating_sub(zone_left);

        // Fixed visible width (excluding the variable name section)
        // for each chip variant, including the two outer connectors
        // (┐ ... ┌) and the inner spacing around chevrons:
        //   With link:    ┐← < <NAME> (<LINK>) > →┌  =  13 + name + link
        //   Without link: ┐← < <NAME> > →┌            =  10 + name
        let with_link_overhead = 13 + tools::ulen(&link_str);
        let without_link_overhead = 10;
        let with_link_max_name = zone_width.saturating_sub(with_link_overhead);
        let without_link_max_name = zone_width.saturating_sub(without_link_overhead);
        let name_full_len = tools::ulen(display_name_full);

        let (display_name, link_in_chip) =
            if !link_str.is_empty() && with_link_max_name >= name_full_len {
                (display_name_full.to_string(), true)
            } else if !link_str.is_empty() && with_link_max_name >= 2 {
                let truncated = format!(
                    "{}…",
                    tools::uresize(display_name_full, with_link_max_name - 1)
                );
                (truncated, true)
            } else if without_link_max_name >= name_full_len {
                (display_name_full.to_string(), false)
            } else if without_link_max_name >= 2 {
                let truncated = format!(
                    "{}…",
                    tools::uresize(display_name_full, without_link_max_name - 1)
                );
                (truncated, false)
            } else {
                (String::new(), false)
            };

        if !display_name.is_empty() {
            let chip_text = if link_in_chip {
                format!("← {hi}<{title_color} {display_name} ({link_str}) {hi}>{title_color} →")
            } else {
                format!("← {hi}<{title_color} {display_name} {hi}>{title_color} →")
            };
            let plain = if link_in_chip {
                format!("← < {display_name} ({link_str}) > →")
            } else {
                format!("← < {display_name} > →")
            };
            let chip_x = box_drawing::right_inset_x(x, width, box_drawing::inset_width(&plain));
            let chip_inset = box_drawing::title_inset(&chip_text, border_color, title_color, false);
            buf.mv(chip_x, y + 1).text(&chip_inset);
        }
    }

    // Bottom border: sync and auto on the left side; cumulative
    // totals on the right side. The adapter chip lives on the
    // top-right border (see the chip block above). sync/auto append
    // `*` when active (mirrors disk's `io*` convention for binary
    // toggles).
    let bottom_y = y + height;

    let sync_marker = if net_sync { "*" } else { "" };
    let auto_marker = if net_auto { "*" } else { "" };
    let sync_text = format!("{hi}s{title_color}ync{sync_marker}");
    let auto_text = format!("{hi}a{title_color}uto{auto_marker}");
    let sync_inset = box_drawing::title_inset(&sync_text, border_color, title_color, true);
    let auto_inset = box_drawing::title_inset(&auto_text, border_color, title_color, true);

    let mut bx = x + 3;
    buf.mv(bx, bottom_y).text(&sync_inset);
    bx += box_drawing::inset_width(&format!("sync{sync_marker}"));
    buf.mv(bx, bottom_y).text(&auto_inset);
    bx += box_drawing::inset_width(&format!("auto{auto_marker}"));

    // Bottom-right: cumulative totals since rtop started. Format
    // `↓ 12.4G ↑ 3.1G`. Skipped when the bottom border doesn't have
    // room without overlapping the left-side sync/auto insets.
    let dl_total =
        tools::floating_humanizer(net.stat.download.total, true, 0, false, false, base_10);
    let ul_total = tools::floating_humanizer(net.stat.upload.total, true, 0, false, false, base_10);
    let totals_text = format!("↓ {dl_total} ↑ {ul_total}");
    let totals_vis = box_drawing::inset_width(&totals_text);
    let totals_x = box_drawing::right_inset_x(x, width, totals_vis);
    // Need at least one column of border between the auto inset's
    // right edge and the totals inset's left edge so they read as
    // separate elements rather than a continuous string.
    if totals_x > bx {
        let totals_inset = box_drawing::title_inset(&totals_text, border_color, title_color, true);
        buf.mv(totals_x, bottom_y).text(&totals_inset);
    }

    buf.finish()
}

// ---------------------------------------------------------------------------
// Widget impl
// ---------------------------------------------------------------------------

/// Network widget renderer. Unit struct — the widget has no
/// per-instance state.
pub struct NetWidget;

impl super::Widget for NetWidget {
    fn kinds(&self) -> &'static [crate::domain::widget_kind::WidgetKind] {
        const KINDS: &[crate::domain::widget_kind::WidgetKind] =
            &[crate::domain::widget_kind::WidgetKind::Net];
        KINDS
    }

    fn preferred_height(&self, _: &crate::draw::layout::LayoutHints) -> usize {
        // Net is a Fill widget — preferred is its absolute minimum;
        // the container distributes slack from sibling Preferred
        // widgets.
        crate::draw::layout::MIN_NET_HEIGHT
    }

    fn min_width(&self, _: &crate::draw::layout::LayoutHints) -> usize {
        crate::draw::layout::MIN_NET_WIDTH
    }

    fn min_height(&self, _: &crate::draw::layout::LayoutHints) -> usize {
        crate::draw::layout::MIN_NET_HEIGHT
    }

    fn render(&self, params: &crate::app::RenderParams<'_>, output: &mut String) {
        let Some(net_dim) = params
            .layout
            .dims_for(crate::domain::widget_kind::WidgetKind::Net)
        else {
            return;
        };
        let Some(net) = params.net else {
            return;
        };
        let iface_id = params.selected_net_iface;
        let default_net = crate::domain::network::NetInfo::default();
        let net_info = net
            .nets
            .iter()
            .find(|n| n.stable_id == iface_id)
            .unwrap_or(&default_net);
        let area = super::WidgetArea::from_dim(net_dim, params.rounded);
        let frame = NetFrame {
            auto_scale: params.view.net_auto,
            sync_scale: params.view.net_sync,
            max_download: params.config.net.net_download,
            max_upload: params.config.net.net_upload,
            graph_symbol: crate::draw::graph::GraphMode::from_config(
                params.config.net.graph_symbol_net,
                params.config.ui.graph_symbol,
            ),
            swap_dl_ul: params.config.net.swap_upload_download,
            base_10: params.config.ui.base_10_sizes,
        };
        output.push_str(&draw(net_info, &area, params.theme, &frame, &net.status));
    }
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
            stable_id: "{12345678-1234-1234-1234-123456789012}".into(),
            description: "Ethernet".into(),
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

    fn make_frame() -> NetFrame {
        NetFrame {
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
        // keybind characters themselves in the keybind colour (HI_FG).
        // The format also has a space on each side of every arrow so
        // the keybind char and the arrow read as separate tokens.
        // Chip text: `← < Ethernet (1 Gbps) > →`. Keybinds are `<` / `>`.
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
        // `<` is preceded by HI (embedded switch).
        assert!(
            output.contains(&format!("{hi}<")),
            "keybind '<' should render in HI colour"
        );
        // The space + iface name + space region is in TITLE.
        assert!(
            output.contains(&format!("{title} Ethernet ")),
            "iface name and surrounding spaces should render in TITLE colour"
        );
        // `>` is preceded by HI.
        assert!(
            output.contains(&format!("{hi}>")),
            "keybind '>' should render in HI colour"
        );
        // ` →` (leading space + arrow) is in TITLE so the closing
        // border isn't HI-coloured and the arrow isn't fused with `>`.
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

    /// Build a NetInfo whose cumulative totals are exactly
    /// `dl_total_bytes` / `ul_total_bytes`.
    fn make_net_info_with_totals(dl_total_bytes: u64, ul_total_bytes: u64) -> NetInfo {
        let mut info = make_net_info();
        info.stat.download.total = dl_total_bytes;
        info.stat.upload.total = ul_total_bytes;
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
    fn totals_inset_skipped_when_bottom_border_lacks_room() {
        // A narrow widget where the left-side `sync` + `auto*` insets
        // leave no horizontal room on the bottom border for a
        // cumulative-totals inset must omit the totals rather than
        // overlap. The iface cycler moved to the top-right zone, so
        // the bottom-left is much shorter than before — this guard
        // now triggers only at very narrow widths.
        let mut area = make_area();
        area.width = 30;
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
        // widget. With width=30 it should be absent. (The in-graph
        // ▼ arrow uses a different glyph, so this check is unique
        // to the border inset.)
        assert!(
            !plain.contains("↓ "),
            "totals inset must be skipped when there is no room; got: {plain}"
        );
    }

    // ── Cycler chip placement ───────────────────────────────────────

    /// Returns the row of the most recent cursor-position escape
    /// (`\x1b[<row>;<col>H`) in `output` that appears before the
    /// first occurrence of `needle`. Returns `None` if `needle`
    /// is not found or no cursor move precedes it.
    fn cursor_row_for_substring(output: &str, needle: &str) -> Option<usize> {
        let chip_pos = output.find(needle)?;
        let prefix = &output[..chip_pos];
        let mut last_h_row: Option<usize> = None;
        let mut search = 0;
        while let Some(esc_rel) = prefix[search..].find("\x1b[") {
            let esc_abs = search + esc_rel + 2;
            let rest = &prefix[esc_abs..];
            let Some(end_rel) = rest.find(|c: char| c.is_ascii_alphabetic()) else {
                break;
            };
            if rest.as_bytes()[end_rel] == b'H' {
                let params = &rest[..end_rel];
                if let Some(semi) = params.find(';')
                    && let Ok(r) = params[..semi].parse::<usize>()
                {
                    last_h_row = Some(r);
                }
            }
            search = esc_abs + end_rel + 1;
        }
        last_h_row
    }

    #[test]
    fn iface_cycler_chip_uses_chevron_keybinds() {
        // The cycler chip text must contain `<` and `>` (the new
        // keybind characters) in HI colour, with the iface name
        // between them in TITLE colour.
        use crate::theme_keys as tc;
        let theme = Theme::default();
        let output = draw(
            &make_net_info(),
            &make_area(),
            &theme,
            &make_frame(),
            &CollectStatus::Ok,
        );
        let hi = theme.color(tc::HI_FG);
        assert!(
            output.contains(&format!("{hi}<")),
            "iface cycler must contain '<' in HI colour"
        );
        assert!(
            output.contains(&format!("{hi}>")),
            "iface cycler must contain '>' in HI colour"
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("← < Ethernet (1 Gbps) > →"),
            "iface cycler text must read '← < Ethernet (1 Gbps) > →': {plain}"
        );
    }

    #[test]
    fn iface_cycler_chip_renders_on_top_border_not_bottom() {
        // The iface cycler is a scope cycler (top-right zone), not
        // a display toggle. With `make_area()` (y=1, height=14) the
        // top border lives at row y+1 = 2 and the bottom border at
        // y+height = 15. The chip text "Ethernet" appears once in
        // the rendered output (inside the cycler chip) — the most
        // recent cursor-position escape preceding it must be a move
        // to row 2.
        let output = draw(
            &make_net_info(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &CollectStatus::Ok,
        );
        let row = cursor_row_for_substring(&output, "Ethernet")
            .expect("iface chip text must be preceded by a cursor-position escape");
        assert_eq!(
            row, 2,
            "iface cycler chip must be rendered on the top border (row 2), not row {row}"
        );
    }
}
