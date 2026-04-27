use crate::domain::cpu::{CpuInfo, get_cpu_series};
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::{Graph, GraphSymbol};
use crate::draw::meter::Meter;
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

use super::BoxArea;

/// Extracted settings for the CPU box, decoupled from Config.
pub struct CpuBoxSettings<'a> {
    pub graph_symbol: GraphSymbol,
    pub upper_source: &'a str,
    pub lower_source: &'a str,
    pub check_temp: bool,
    pub show_coretemp: bool,
    pub temp_scale: &'a str,
    pub update_ms: u64,
    pub current_preset: i64,
}

// Core panel column width thresholds (from btop's b_column_size algorithm).
// Each tier provides space for: label + mini-graph + percentage [+ temperature].
/// Widest tier: 21 chars + 12 per temp column.
const CORE_COL_WIDE: usize = 21;
const CORE_COL_WIDE_TEMP: usize = 12;
/// Medium tier: 15 chars + 6 per temp column.
const CORE_COL_MED: usize = 15;
const CORE_COL_MED_TEMP: usize = 6;
/// Minimal tier: 8 chars + 6 per temp column.
const CORE_COL_NARROW: usize = 8;
const CORE_COL_NARROW_TEMP: usize = 6;
/// Overhead rows in the core panel (CPU meter + load avg + border).
const CORE_PANEL_OVERHEAD: usize = 3;
/// Box border overhead (top + bottom).
const BOX_BORDER_ROWS: usize = 2;

/// Draw the CPU box into an ANSI string matching btop's layout.
///
/// Layout:
/// ╭──── cpu ──── ... ──╮
/// │ <upper graph>      │CPU ■■■■░░ 42%│
/// │ <upper graph>      │              │
/// │──── ▲▼ ────────────│ C0 ⣿⣷⣤ 42% │
/// │ <lower graph inv>  │ C1 ⣿⣷⣤ 38% │
/// │ <lower graph inv>  │ C2 ⣿⣷⣤ 55% │
/// │up 3d12:45          │ C3 ⣿⣷⣤ 22% │
/// ╰────────────────────┴──────────────╯
pub fn draw(cpu: &CpuInfo, area: &BoxArea, theme: &Theme, settings: &CpuBoxSettings) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.c(tc::CPU_BOX);
    let hi = theme.c(tc::HI_FG);
    let title_color = theme.c(tc::TITLE);
    let cpu_gradient = theme.g(tc::GRAD_CPU);
    let graph_text_color = theme.c(tc::GRAPH_TEXT);
    let graph_sym = settings.graph_symbol;
    let upper_key = match settings.upper_source {
        "user" => "user",
        "system" => "system",
        _ => "total",
    };
    let lower_key = match settings.lower_source {
        "user" => "user",
        "system" => "system",
        _ => "total",
    };

    let mut buf = AnsiBuffer::new();
    buf.text(&box_drawing::create_box(&box_drawing::BoxConfig {
        x,
        y,
        width,
        height,
        line_color: box_color,
        fill: true,
        title: "cpu",
        title2: "",
        num: 1,
        rounded,
        hi_color: hi,
        title_color,
    }));

    let core_count = cpu.core_percent.len();
    let inner_h = height.saturating_sub(2);

    if inner_h == 0 || width < 6 {
        return buf.finish();
    }

    // --- btop core panel sizing (calcSizes from btop_draw.cpp:2297-2327) ---
    let has_temp = settings.check_temp && !cpu.temp.is_empty();
    let show_coretemp_flag = has_temp && settings.show_coretemp;
    let show_temp: usize = if show_coretemp_flag { 1 } else { 0 };

    // b_columns = max(1, ceil(coreCount / (height - 5)))
    let b_columns = if core_count == 0 || height <= 5 {
        1usize
    } else {
        1usize.max(core_count.div_ceil(height - 5))
    };

    // Determine column size and b_width
    let wide_col = CORE_COL_WIDE + CORE_COL_WIDE_TEMP * show_temp;
    let med_col = CORE_COL_MED + CORE_COL_MED_TEMP * show_temp;
    let narrow_col = CORE_COL_NARROW + CORE_COL_NARROW_TEMP * show_temp;
    let max_panel = width - width / 3;

    let (_, b_width) = if core_count == 0 || width <= 20 {
        (0usize, 0usize)
    } else if b_columns * wide_col < max_panel {
        let w = 29usize.max(wide_col * b_columns - (b_columns - 1));
        (2, w)
    } else if b_columns * med_col < max_panel {
        let w = med_col * b_columns - (b_columns - 1);
        (1, w)
    } else {
        let w = narrow_col * b_columns + 1;
        (0, w)
    };

    // b_height: enough for cores + CPU meter + load avg + border
    let b_height = if b_width == 0 {
        0
    } else {
        let rows_for_cores = core_count.div_ceil(b_columns);
        (height - BOX_BORDER_ROWS).min(rows_for_cores + CORE_PANEL_OVERHEAD)
    };

    // b_x = x + width - b_width - 1
    let b_x = if b_width == 0 {
        x
    } else {
        x + width - b_width - 1
    };
    // b_y = y + ceil((height-2)/2) - ceil(b_height/2) + 1
    let b_y = if b_height == 0 {
        y
    } else {
        let half_inner = (height - 2).div_ceil(2);
        let half_panel = b_height.div_ceil(2);
        y + half_inner.saturating_sub(half_panel) + 1
    };

    let graph_width = if b_width > 0 {
        width.saturating_sub(b_width + 2)
    } else {
        width.saturating_sub(2)
    };

    // Split inner area: upper graph, divider, lower graph
    let has_lower = inner_h >= 4;
    let divider_row = if has_lower { inner_h / 2 } else { inner_h };
    let upper_h = divider_row;
    let lower_h = if has_lower {
        inner_h - divider_row - 1
    } else {
        0
    };

    // Draw the vertical divider for core panel
    if b_width > 0 {
        for row_i in 1..height.saturating_sub(1) {
            buf.mv(b_x + 1, y + 1 + row_i)
                .color(box_color)
                .text(symbols::V_LINE);
        }
        // T-junction at top border
        buf.mv(b_x + 1, y + 1)
            .color(box_color)
            .text(symbols::DIV_UP);
        // Bottom junction
        buf.mv(b_x + 1, y + height)
            .color(box_color)
            .text(symbols::DIV_DOWN);

        // CPU frequency title inset on the top border
        if !cpu.cpu_hz.is_empty() {
            let hz_str = &cpu.cpu_hz;
            let hz_title = format!(" {} ", hz_str);
            let hz_vis_len = hz_title.len();
            let avail = b_width.saturating_sub(1);
            if hz_vis_len + 2 <= avail {
                let dashes = avail.saturating_sub(hz_vis_len + 1);
                let hz_x = b_x + 2;
                buf.mv(hz_x, y + 1)
                    .color(box_color)
                    .text(&symbols::H_LINE.repeat(dashes))
                    .text(&box_drawing::title_inset(
                        hz_str,
                        box_color,
                        title_color,
                        false,
                    ));
            }
        }
    }

    // Upper graph (normal orientation)
    if upper_h > 0 && graph_width > 0 {
        if let Some(data) = get_cpu_series(&cpu.cpu_percent, upper_key) {
            let mut graph = Graph::new(graph_width, upper_h, graph_sym, false, true, 100, 0);
            graph.create(data);
            let rows = graph.render_rows_colored(data, cpu_gradient);
            for (i, row) in rows.iter().enumerate() {
                buf.mv(x + 2, y + 2 + i).text(row);
            }
        }
    }

    // Divider line: ├──┐▲user┌──┐system▼┌──────────┤
    if has_lower {
        let div_y = y + 2 + divider_row;
        let upper_label = format!("▲{}", upper_key);
        let lower_label = format!("{}▼", lower_key);
        let upper_inset = box_drawing::title_inset(&upper_label, box_color, title_color, false);
        let lower_inset = box_drawing::title_inset(&lower_label, box_color, title_color, false);
        // Visible: ├ + left_dashes + ┐▲user┌ + ──  + ┐system▼┌ + right_dashes + ┤ (if core panel)
        let upper_vis = upper_label.len() + 2; // inset chars
        let lower_vis = lower_label.len() + 2;
        let mid_dashes = 2;
        let fixed = upper_vis + mid_dashes + lower_vis;
        let left_dashes = (graph_width.saturating_sub(fixed)) / 2;
        let right_dashes = graph_width.saturating_sub(fixed + left_dashes);
        buf.mv(x + 1, div_y)
            .color(box_color)
            .text(symbols::DIV_LEFT)
            .text(&symbols::H_LINE.repeat(left_dashes))
            .text(&upper_inset)
            .color(box_color)
            .text(&symbols::H_LINE.repeat(mid_dashes))
            .text(&lower_inset)
            .color(box_color)
            .text(&symbols::H_LINE.repeat(right_dashes));
        if b_width > 0 {
            buf.mv(b_x + 1, div_y)
                .color(box_color)
                .text(symbols::DIV_RIGHT);
        }
    }

    // Lower graph (inverted orientation)
    if lower_h > 0 && graph_width > 0 {
        if let Some(data) = get_cpu_series(&cpu.cpu_percent, lower_key) {
            let lower_start_y = y + 2 + divider_row + 1;
            let mut graph = Graph::new(graph_width, lower_h, graph_sym, true, true, 100, 0);
            graph.create(data);
            let rows = graph.render_rows_colored(data, cpu_gradient);
            for (i, row) in rows.iter().enumerate() {
                buf.mv(x + 2, lower_start_y + i).text(row);
            }
        }
    }

    // --- Core panel ---
    if b_width > 0 && b_height > 0 {
        let panel = CorePanelArea {
            x: b_x,
            y: b_y,
            width: b_width,
            height: b_height,
            columns: b_columns,
        };
        let temp_scale = settings.temp_scale;
        buf.text(&draw_core_panel(
            cpu,
            &panel,
            has_temp,
            show_coretemp_flag,
            temp_scale,
            graph_sym,
            theme,
        ));
    }

    // Uptime overlaid on lower-left of graph area
    let uptime = tools::sec_to_dhms(cpu.uptime_seconds, false, true);
    let up_str = format!("up {}", uptime);
    let up_y = y + 2;
    if !uptime.is_empty() {
        buf.mv(x + 2, up_y).color(graph_text_color).text(&up_str);
    }

    // Bottom border keybind hints
    buf.text(&draw_bottom_hints(
        x,
        y + height,
        settings.update_ms,
        settings.current_preset,
        theme,
    ));

    buf.finish()
}

/// Geometry of the per-core panel within the CPU box.
struct CorePanelArea {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    columns: usize,
}

/// Render the per-core panel (CPU meter, mini-graphs, percentages, temperatures, load avg).
fn draw_core_panel(
    cpu: &CpuInfo,
    panel: &CorePanelArea,
    has_temp: bool,
    show_coretemp: bool,
    temp_scale: &str,
    graph_sym: GraphSymbol,
    theme: &Theme,
) -> String {
    let fg = theme.c(tc::MAIN_FG);
    let title_color = theme.c(tc::TITLE);
    let box_color = theme.c(tc::CPU_BOX);
    let cpu_gradient = theme.g(tc::GRAD_CPU);
    let temp_gradient = theme.g(tc::GRAD_TEMP);
    let mut buf = AnsiBuffer::new();
    let core_count = cpu.core_percent.len();

    let panel_inner_w = panel.width;
    let meter_bg = theme.c(tc::METER_BG);

    // Row 0 of core panel: CPU meter line (btop line 842)
    {
        let total = &cpu.cpu_percent.total;
        if let Some(&pct) = total.back() {
            let pct_color = if !cpu_gradient.is_empty() {
                &cpu_gradient[pct.clamp(0, 100) as usize]
            } else {
                fg
            };
            let temp_suffix_len = if has_temp { 1 + 5 + 4 + 2 } else { 0 };
            let meter_w = panel_inner_w.saturating_sub(4 + 5 + temp_suffix_len).max(1);
            let meter = Meter::new(meter_w, cpu_gradient, meter_bg);
            buf.mv(panel.x + 2, panel.y + 1)
                .color(title_color)
                .text("CPU ")
                .color(pct_color)
                .text(meter.render(pct as i32))
                .color(pct_color)
                .text(&tools::rjust(&pct.to_string(), 4, false))
                .text("%");
            if has_temp {
                if let Some(pkg_data) = cpu.temp.first() {
                    let pkg_temp = pkg_data.back().copied().unwrap_or(0);
                    let mut tg = Graph::new(5, 1, graph_sym, false, false, 100, 0);
                    let tg_str = tg.render_row_colored(pkg_data, temp_gradient);
                    let t_color = if !temp_gradient.is_empty() {
                        &temp_gradient[(pkg_temp.clamp(0, 100)) as usize]
                    } else {
                        fg
                    };
                    let (conv_temp, temp_unit) = crate::tools::celsius_to(pkg_temp, temp_scale);
                    let temp_text = format!("{:>3}{}", conv_temp, temp_unit);
                    buf.text(" ").text(&tg_str).color(t_color).text(&temp_text);
                }
            }
        }
    }

    // Per-core rows with multi-column wrapping
    let col_w = if panel.columns > 0 {
        panel_inner_w
            .checked_div(panel.columns)
            .unwrap_or(panel_inner_w)
    } else {
        panel_inner_w
    };
    let mut cx: usize = 0;
    let mut cy: usize = 1;
    let mut cc: usize = 0;

    for (i, core_data) in cpu.core_percent.iter().enumerate() {
        let pct = core_data.back().copied().unwrap_or(0);
        let pct_color = if !cpu_gradient.is_empty() {
            &cpu_gradient[pct.clamp(0, 100) as usize]
        } else {
            fg
        };

        let row_y = panel.y + cy + 1;
        let row_x = panel.x + cx + 2;

        let label = if core_count >= 100 {
            format!("{:>3}", i)
        } else if core_count >= 10 {
            format!("C{:<2}", i)
        } else {
            format!("C{}", i)
        };
        let label_w = label.len();

        let sep_w: usize = if cc + 1 < panel.columns { 1 } else { 0 };
        let pct_w: usize = 4;
        let temp_w: usize = if has_temp && show_coretemp { 5 } else { 0 };
        let fixed_w = label_w + pct_w + temp_w + sep_w;
        let graph_w = col_w.saturating_sub(fixed_w);

        // Position and write label
        buf.mv(row_x, row_y).color(fg).text(&label);

        // Mini graph
        if graph_w >= 3 {
            let mut mini = Graph::new(graph_w, 1, graph_sym, false, false, 100, 0);
            let mini_str = mini.render_row_colored(core_data, cpu_gradient);
            buf.text(&mini_str);
        } else if graph_w > 0 {
            let skip = format!("\x1b[{}C", graph_w);
            buf.text(&skip);
        }

        // Percentage
        let pct_text = format!("{:>3}%", pct);
        buf.color(pct_color).text(&pct_text).color(fg);

        // Per-core temperature
        if has_temp && show_coretemp {
            let num_core_temps = cpu.temp.len().saturating_sub(1);
            let temp_idx = if num_core_temps > 0 {
                (i % num_core_temps) + 1
            } else {
                0
            };
            let core_temp = cpu
                .temp
                .get(temp_idx)
                .and_then(|dq| dq.back())
                .copied()
                .unwrap_or(0);
            let t_color = if !temp_gradient.is_empty() {
                &temp_gradient[(core_temp.clamp(0, 100)) as usize]
            } else {
                fg
            };
            let (conv_temp, temp_unit) = crate::tools::celsius_to(core_temp, temp_scale);
            let temp_text = format!("{:>3}{}", conv_temp, temp_unit);
            buf.color(t_color).text(&temp_text);
        }

        // Column separator
        if cc + 1 < panel.columns {
            buf.color(box_color).text(symbols::V_LINE);
        }

        cy += 1;
        let cores_per_col = core_count.div_ceil(panel.columns).max(1);
        if cy > cores_per_col && i != core_count - 1 {
            cc += 1;
            if cc >= panel.columns {
                break;
            }
            cy = 1;
            cx = col_w * cc;
        }
    }

    // Load average on bottom row of core panel
    let lavg_y = panel.y + panel.height;
    let lavg_str = format!(
        "Load avg: {:.2} {:.2} {:.2}",
        cpu.load_avg[0], cpu.load_avg[1], cpu.load_avg[2]
    );
    let lavg_vis_len = lavg_str.len();
    if lavg_vis_len <= panel_inner_w {
        let lavg_x = panel.x + 2 + (panel_inner_w.saturating_sub(lavg_vis_len)) / 2;
        buf.mv(lavg_x, lavg_y).color(fg).text(&lavg_str);
    }

    buf.finish()
}

/// Render the bottom border keybind hints (menu, preset with number, update rate).
fn draw_bottom_hints(
    x: usize,
    bottom_y: usize,
    update_ms: u64,
    current_preset: i64,
    theme: &Theme,
) -> String {
    let box_color = theme.c(tc::CPU_BOX);
    let fg = theme.c(tc::MAIN_FG);
    let hi = theme.c(tc::HI_FG);

    let preset_label = format!("p{}reset *{}", fg, current_preset);
    let rate_label = format!("{}ms", update_ms);
    let menu_inset = box_drawing::keybind_inset("menu", box_color, hi, fg, true);
    let preset_inset = box_drawing::title_inset(&preset_label, box_color, hi, true);
    let rate_text = format!("─ {}{} {}+", fg, rate_label, hi);
    let rate_inset = box_drawing::title_inset(&rate_text, box_color, hi, true);
    let hints = format!("{}{}{}", menu_inset, preset_inset, rate_inset);

    let mut buf = AnsiBuffer::new();
    buf.mv(x + 3, bottom_y).text(&hints);
    buf.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Strip ANSI escape codes so we can assert on visible text.
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

    fn make_cpu_info() -> CpuInfo {
        let mut cpu = CpuInfo::default();
        cpu.cpu_name = "Test CPU".into();
        cpu.cpu_hz = "3.50 GHz".into();
        cpu.core_count = 4;
        cpu.uptime_seconds = 86400;
        cpu.cpu_percent.total = VecDeque::from([50]);
        cpu.cpu_percent.user = VecDeque::from([30]);
        cpu.cpu_percent.system = VecDeque::from([20]);
        cpu.cpu_percent.idle = VecDeque::from([50]);
        cpu.core_percent = vec![
            VecDeque::from([40]),
            VecDeque::from([60]),
            VecDeque::from([30]),
            VecDeque::from([80]),
        ];
        cpu
    }

    fn make_area() -> BoxArea {
        BoxArea {
            x: 1,
            y: 1,
            width: 80,
            height: 20,
            rounded: true,
        }
    }

    fn make_settings() -> CpuBoxSettings<'static> {
        CpuBoxSettings {
            graph_symbol: GraphSymbol::Braille,
            upper_source: "user",
            lower_source: "system",
            check_temp: false,
            show_coretemp: false,
            temp_scale: "celsius",
            update_ms: 2000,
            current_preset: 0,
        }
    }

    #[test]
    fn draw_contains_cpu_title() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("cpu"), "output should contain 'cpu' title");
    }

    #[test]
    fn draw_contains_preset_label() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("reset"),
            "output should contain preset label"
        );
        assert!(
            plain.contains("*0"),
            "output should contain preset number '*0'"
        );
    }

    #[test]
    fn draw_contains_update_rate() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("2000ms"),
            "output should contain update rate '2000ms'"
        );
    }

    #[test]
    fn draw_output_is_non_empty() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
        );
        assert!(!output.is_empty(), "draw output should not be empty");
    }
}
