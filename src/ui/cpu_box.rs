use crate::domain::cpu::CpuInfo;
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::box_drawing::title_syms;
use crate::draw::graph::{Graph, GraphSymbol};
use crate::draw::meter::Meter;
use crate::term;
use crate::theme::Theme;
use crate::tools;

use super::BoxArea;

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
pub fn draw(
    cpu: &CpuInfo,
    area: &BoxArea,
    theme: &Theme,
    update_ms: u64,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.c("cpu_box");
    let hi = theme.c("hi_fg");
    let title_color = theme.c("title");
    let div_color = theme.c("div_line");
    let cpu_gradient = theme.g("cpu");
    let graph_text_color = theme.c("graph_text");

    let mut out = box_drawing::create_box(&box_drawing::BoxConfig {
        x, y, width, height, line_color: box_color, fill: true,
        title: "cpu", title2: "", num: 1, rounded,
        hi_color: hi, title_color,
    });

    let core_count = cpu.core_percent.len();
    let inner_h = height.saturating_sub(2);

    if inner_h == 0 || width < 6 {
        out.push_str("\x1b[0m");
        return out;
    }

    // --- btop core panel sizing (calcSizes from btop_draw.cpp:2297-2327) ---
    let has_temp = !cpu.temp.is_empty();
    let show_temp: usize = if has_temp { 1 } else { 0 };

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
    let b_x = if b_width == 0 { x } else { x + width - b_width - 1 };
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
    let lower_h = if has_lower { inner_h - divider_row - 1 } else { 0 };

    // Draw the vertical divider for core panel
    if b_width > 0 {
        for row_i in 1..height.saturating_sub(1) {
            out.push_str(&format!(
                "{}{}{}",
                term::mv(b_x + 1, y + 1 + row_i), div_color, symbols::V_LINE
            ));
        }
        // T-junction at top border
        out.push_str(&format!(
            "{}{}{}",
            term::mv(b_x + 1, y + 1), box_color, symbols::DIV_UP
        ));
        // Bottom junction
        out.push_str(&format!(
            "{}{}{}",
            term::mv(b_x + 1, y + height), box_color, symbols::DIV_DOWN
        ));

        // CPU frequency title inset on the top border
        if !cpu.cpu_hz.is_empty() {
            let hz_str = &cpu.cpu_hz;
            let hz_title = format!(" {} ", hz_str);
            let hz_vis_len = hz_title.len();
            let avail = b_width.saturating_sub(1);
            if hz_vis_len + 2 <= avail {
                let dashes = avail.saturating_sub(hz_vis_len + 1);
                let hz_x = b_x + 2;
                out.push_str(&format!(
                    "{}{}{}{}{}{}{}{}",
                    term::mv(hz_x, y + 1),
                    box_color,
                    symbols::H_LINE.repeat(dashes),
                    title_syms::TITLE_LEFT,
                    title_color, hz_str,
                    box_color,
                    title_syms::TITLE_RIGHT,
                ));
            }
        }
    }

    // Upper graph (normal orientation)
    if upper_h > 0 && graph_width > 0 {
        if let Some(total) = cpu.cpu_percent.get("total") {
            let mut graph = Graph::new(graph_width, upper_h, GraphSymbol::Braille, false, true, 100, 0);
            graph.create(total);
            let rows = graph.render_rows_colored(total, cpu_gradient);
            for (i, row) in rows.iter().enumerate() {
                out.push_str(&format!("{}{}", term::mv(x + 2, y + 2 + i), row));
            }
        }
    }

    // Divider line with ▲▼
    if has_lower {
        let div_y = y + 2 + divider_row;
        let mid_label = format!(" {}▲▼{} ", hi, div_color);
        let label_vis_len = 4; // " ▲▼ "
        let left_dashes = (graph_width.saturating_sub(label_vis_len)) / 2;
        let right_dashes = graph_width.saturating_sub(label_vis_len + left_dashes);
        out.push_str(&format!(
            "{}{}{}{}{}{}{}",
            term::mv(x + 1, div_y),
            box_color, symbols::DIV_LEFT,
            div_color,
            symbols::H_LINE.repeat(left_dashes),
            mid_label,
            symbols::H_LINE.repeat(right_dashes),
        ));
        if b_width > 0 {
            out.push_str(&format!(
                "{}{}{}",
                term::mv(b_x + 1, div_y), box_color, symbols::DIV_RIGHT
            ));
        }
    }

    // Lower graph (inverted orientation)
    if lower_h > 0 && graph_width > 0 {
        if let Some(total) = cpu.cpu_percent.get("total") {
            let lower_start_y = y + 2 + divider_row + 1;
            let mut graph = Graph::new(graph_width, lower_h, GraphSymbol::Braille, true, true, 100, 0);
            graph.create(total);
            let rows = graph.render_rows_colored(total, cpu_gradient);
            for (i, row) in rows.iter().enumerate() {
                out.push_str(&format!("{}{}", term::mv(x + 2, lower_start_y + i), row));
            }
        }
    }

    // --- Core panel ---
    if b_width > 0 && b_height > 0 {
        let panel = CorePanelArea { x: b_x, y: b_y, width: b_width, height: b_height, columns: b_columns };
        out.push_str(&draw_core_panel(cpu, &panel, has_temp, theme));
    }

    // Uptime overlaid on lower-left of graph area
    let uptime = tools::sec_to_dhms(cpu.uptime_seconds, false, true);
    let up_str = format!("up {}", uptime);
    let up_y = y + 2;
    if !uptime.is_empty() {
        out.push_str(&format!(
            "{}{}{}",
            term::mv(x + 2, up_y), graph_text_color, up_str
        ));
    }

    // Bottom border keybind hints
    out.push_str(&draw_bottom_hints(x, y + height, update_ms, theme));

    out.push_str("\x1b[0m");
    out
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
    theme: &Theme,
) -> String {
    let fg = theme.c("main_fg");
    let title_color = theme.c("title");
    let div_color = theme.c("div_line");
    let cpu_gradient = theme.g("cpu");
    let temp_gradient = theme.g("temp");
    let mut out = String::new();
    let core_count = cpu.core_percent.len();

    let panel_inner_w = panel.width;
    let meter_bg = theme.c("meter_bg");

    // Row 0 of core panel: CPU meter line (btop line 842)
    // "CPU " + meter + " ###%" [+ " " + temp_graph(5) + " ###°C"]
    if let Some(total) = cpu.cpu_percent.get("total") {
        if let Some(&pct) = total.back() {
            let pct_color = if !cpu_gradient.is_empty() {
                &cpu_gradient[pct.clamp(0, 100) as usize]
            } else {
                fg
            };
            let temp_suffix_len = if has_temp { 1 + 5 + 4 + 2 } else { 0 }; // " " + graph(5) + " ##°C"
            let meter_w = panel_inner_w.saturating_sub(4 + 5 + temp_suffix_len).max(1);
            let meter = Meter::new(meter_w, cpu_gradient, meter_bg);
            out.push_str(&format!(
                "{}{}CPU {}{}{}{}{}",
                term::mv(panel.x + 2, panel.y + 1),
                title_color,
                pct_color, meter.render(pct as i32),
                pct_color, tools::rjust(&pct.to_string(), 4, false),
                "%",
            ));
            if has_temp {
                // Package temp graph + value on CPU meter row
                if let Some(pkg_data) = cpu.temp.first() {
                    let pkg_temp = pkg_data.back().copied().unwrap_or(0);
                    let mut tg = Graph::new(5, 1, GraphSymbol::Braille, false, false, 100, 0);
                    let tg_str = tg.render_row_colored(pkg_data, temp_gradient);
                    let t_color = if !temp_gradient.is_empty() {
                        &temp_gradient[(pkg_temp.clamp(0, 100)) as usize]
                    } else {
                        fg
                    };
                    out.push_str(&format!(
                        " {}{}{:>3}°C",
                        tg_str, t_color, pkg_temp
                    ));
                }
            }
        }
    }

    // Per-core rows with multi-column wrapping (btop lines 878-923)
    // Each core row must fit exactly in col_w visible characters.
    let col_w = if panel.columns > 0 { panel_inner_w.checked_div(panel.columns).unwrap_or(panel_inner_w) } else { panel_inner_w };
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

        // Build the core line with absolute positioning for each part.
        // Layout (fitting in col_w chars):
        //   "C##" (2-3 chars) + graph (variable) + " ##%" (4-5 chars) + " ##°C" (5 chars) + "│" (1 char if multi-col)
        let label = if core_count >= 100 { format!("{:>3}", i) }
            else if core_count >= 10 { format!("C{:<2}", i) }
            else { format!("C{}", i) };
        let label_w = label.len();

        let sep_w: usize = if cc + 1 < panel.columns { 1 } else { 0 }; // │ separator
        let pct_w: usize = 4; // " ##%"
        let temp_w: usize = if has_temp { 5 } else { 0 }; // " ##°C"
        let fixed_w = label_w + pct_w + temp_w + sep_w;
        let graph_w = col_w.saturating_sub(fixed_w);

        // Position and write label
        out.push_str(&format!("{}{}{}", term::mv(row_x, row_y), fg, label));

        // Mini graph
        if graph_w >= 3 {
            let mut mini = Graph::new(graph_w, 1, GraphSymbol::Braille, false, false, 100, 0);
            let mini_str = mini.render_row_colored(core_data, cpu_gradient);
            out.push_str(&mini_str);
        } else if graph_w > 0 {
            out.push_str(&format!("\x1b[{}C", graph_w)); // skip space
        }

        // Percentage — positioned absolutely to ensure alignment
        out.push_str(&format!("{}{:>3}{}%", pct_color, pct, fg));

        // Per-core temperature
        if has_temp {
            // cpu.temp: index 0 = package, 1+ = per physical core
            // If more logical cores than temp sensors (hyperthreading),
            // map back to the physical core's temperature.
            let num_core_temps = cpu.temp.len().saturating_sub(1); // exclude package
            let temp_idx = if num_core_temps > 0 {
                (i % num_core_temps) + 1
            } else {
                0
            };
            let core_temp = cpu.temp.get(temp_idx)
                .and_then(|dq| dq.back())
                .copied()
                .unwrap_or(0);
            let t_color = if !temp_gradient.is_empty() {
                &temp_gradient[(core_temp.clamp(0, 100)) as usize]
            } else {
                fg
            };
            out.push_str(&format!("{}{:>3}°C", t_color, core_temp));
        }

        // Column separator
        if cc + 1 < panel.columns {
            out.push_str(&format!("{}{}", div_color, symbols::V_LINE));
        }

        cy += 1;
        // btop line 920-923: wrap to next column when column is full
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

    // Load average on bottom row of core panel (btop lines 927-938)
    let lavg_y = panel.y + panel.height;
    let lavg_str = format!(
        "Load avg: {:.2} {:.2} {:.2}",
        cpu.load_avg[0], cpu.load_avg[1], cpu.load_avg[2]
    );
    let lavg_vis_len = lavg_str.len();
    if lavg_vis_len <= panel_inner_w {
        let lavg_x = panel.x + 2 + (panel_inner_w.saturating_sub(lavg_vis_len)) / 2;
        out.push_str(&format!(
            "{}{}{}",
            term::mv(lavg_x, lavg_y), fg, lavg_str
        ));
    }

    out
}

/// Render the bottom border keybind hints (menu, preset, update rate).
fn draw_bottom_hints(x: usize, bottom_y: usize, update_ms: u64, theme: &Theme) -> String {
    let box_color = theme.c("cpu_box");
    let fg = theme.c("main_fg");
    let hi = theme.c("hi_fg");

    let rate_label = format!("{}ms", update_ms);
    let hints = format!(
        "{}{}{}m{}enu{}{} {}{}{}p{}reset{}{} {}{}{}─ {}{} {}+{}{}",
        box_color, title_syms::TITLE_LEFT_DOWN,
        hi, fg, box_color, title_syms::TITLE_RIGHT_DOWN,
        box_color, title_syms::TITLE_LEFT_DOWN,
        hi, fg, box_color, title_syms::TITLE_RIGHT_DOWN,
        box_color, title_syms::TITLE_LEFT_DOWN,
        hi, fg, rate_label, hi, box_color, title_syms::TITLE_RIGHT_DOWN,
    );
    format!("{}{}", term::mv(x + 3, bottom_y), hints)
}
