use crate::domain::cpu::CpuInfo;
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::box_drawing::title_syms;
use crate::draw::graph::{Graph, GraphSymbol};
use crate::draw::meter::Meter;
use crate::theme::Theme;
use crate::tools;

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
    _buf: &mut crate::cell_buffer::CellBuffer,
    cpu: &CpuInfo,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    rounded: bool,
    theme: &Theme,
) -> String {
    let box_color = theme.c("cpu_box");
    let fg = theme.c("main_fg");
    let hi = theme.c("hi_fg");
    let title_color = theme.c("title");
    let div_color = theme.c("div_line");
    let cpu_gradient = theme.g("cpu");
    let graph_text_color = theme.c("graph_text");

    let mut out = box_drawing::create_box(x, y, width, height, box_color, true, "cpu", "", 1, rounded);

    // Determine core panel width on the right side
    // Core panel shows: " C## ⣿⣷⣤⣠⣀ ###% " = ~20 chars minimum
    let core_count = cpu.core_percent.len();
    let core_panel_w = if core_count > 0 && width > 40 {
        20_usize.max(width / 4).min(width / 3)
    } else {
        0
    };
    let graph_width = width.saturating_sub(2 + core_panel_w);
    let inner_h = height.saturating_sub(2); // rows between top and bottom borders

    if inner_h == 0 || graph_width == 0 {
        out.push_str("\x1b[0m");
        return out;
    }

    // Split inner area: upper graph, divider, lower graph
    // If height is small, just do upper graph
    let has_lower = inner_h >= 4;
    let divider_row = if has_lower { inner_h / 2 } else { inner_h };
    let upper_h = divider_row;
    let lower_h = if has_lower { inner_h - divider_row - 1 } else { 0 };

    // Draw the vertical divider for core panel
    if core_panel_w > 0 {
        let div_x = x + width - core_panel_w - 1;
        for row_i in 1..height.saturating_sub(1) {
            out.push_str(&format!(
                "\x1b[{};{}H{}{}",
                y + 1 + row_i, div_x + 1, div_color, symbols::V_LINE
            ));
        }
        // T-junction at top border
        out.push_str(&format!(
            "\x1b[{};{}H{}{}",
            y + 1, div_x + 1, box_color, symbols::DIV_UP
        ));
        // Bottom junction
        out.push_str(&format!(
            "\x1b[{};{}H{}{}",
            y + height, div_x + 1, box_color, symbols::DIV_DOWN
        ));

        // CPU frequency title inset on the top border of the core panel
        // btop: ──── 5.80GHz ──┤
        if !cpu.cpu_hz.is_empty() {
            let hz_str = &cpu.cpu_hz;
            let hz_title = format!(
                " {} ",
                hz_str
            );
            let hz_vis_len = hz_title.len();
            let avail = core_panel_w.saturating_sub(1); // -1 for the DIV_UP we already placed
            if hz_vis_len + 2 <= avail {
                let dashes = avail.saturating_sub(hz_vis_len + 1); // -1 for DIV_RIGHT
                let hz_x = div_x + 2; // start after the DIV_UP
                out.push_str(&format!(
                    "\x1b[{};{}H{}{}{}{}{}{}{}",
                    y + 1, hz_x,
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
    if upper_h > 0 {
        if let Some(total) = cpu.cpu_percent.get("total") {
            let mut graph = Graph::new(graph_width, upper_h, GraphSymbol::Braille, false, true, 100, 0);
            graph.create(total);
            let rows = graph.render_rows_colored(total, cpu_gradient);
            for (i, row) in rows.iter().enumerate() {
                out.push_str(&format!("\x1b[{};{}H{}", y + 2 + i, x + 2, row));
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
            "\x1b[{};{}H{}{}{}{}{}{}",
            div_y, x + 1,
            box_color, symbols::DIV_LEFT,
            div_color,
            symbols::H_LINE.repeat(left_dashes),
            mid_label,
            symbols::H_LINE.repeat(right_dashes),
        ));
        // Right junction if core panel present
        if core_panel_w > 0 {
            let div_x = x + width - core_panel_w - 1;
            out.push_str(&format!(
                "\x1b[{};{}H{}{}",
                div_y, div_x + 1, box_color, symbols::DIV_RIGHT
            ));
        }
    }

    // Lower graph (inverted orientation)
    if lower_h > 0 {
        if let Some(total) = cpu.cpu_percent.get("total") {
            let lower_start_y = y + 2 + divider_row + 1;
            let mut graph = Graph::new(graph_width, lower_h, GraphSymbol::Braille, true, true, 100, 0);
            graph.create(total);
            let rows = graph.render_rows_colored(total, cpu_gradient);
            for (i, row) in rows.iter().enumerate() {
                out.push_str(&format!("\x1b[{};{}H{}", lower_start_y + i, x + 2, row));
            }
        }
    }

    // Core panel on the right
    if core_panel_w > 0 {
        let panel_x = x + width - core_panel_w;
        let panel_inner_w = core_panel_w.saturating_sub(1); // -1 for right border
        let right_border_x = x + width; // position of the right border │

        // Row 1: CPU total meter "CPU ■■■■░░ ##%" followed by │
        // btop: "CPU " + meter + rjust(pct,4) + "%" + div_line + v_line
        if let Some(total) = cpu.cpu_percent.get("total") {
            if let Some(&pct) = total.back() {
                let label_len = 4; // "CPU "
                // meter width: panel_inner_w - "CPU " - rjust(pct,4) - "%" = panel_inner_w - 4 - 5
                let meter_w = panel_inner_w.saturating_sub(label_len + 5).max(3);
                let meter_bg = theme.c("meter_bg");
                let meter = Meter::new(meter_w, cpu_gradient, meter_bg);
                let pct_color = if !cpu_gradient.is_empty() {
                    &cpu_gradient[pct.clamp(0, 100) as usize]
                } else {
                    fg
                };
                out.push_str(&format!(
                    "\x1b[{};{}H{}CPU {}{}{}{}{}{}",
                    y + 2, panel_x + 1,
                    title_color,
                    pct_color, meter.render(pct as i32),
                    pct_color, tools::rjust(&pct.to_string(), 4, false),
                    fg, "%",
                ));
                // Place │ at the right border
                out.push_str(&format!(
                    "\x1b[{};{}H{}{}",
                    y + 2, right_border_x, box_color, symbols::V_LINE
                ));
            }
        }

        // Per-core rows with mini graphs
        let core_start = y + 3;
        let core_area = inner_h.saturating_sub(2);
        let mini_graph_w = 5_usize;

        for (i, core_data) in cpu.core_percent.iter().enumerate() {
            if i >= core_area {
                break;
            }
            let cy = core_start + i;
            let pct = core_data.back().copied().unwrap_or(0);
            let pct_color = if !cpu_gradient.is_empty() {
                &cpu_gradient[pct.clamp(0, 100) as usize]
            } else {
                fg
            };

            // Core label: "C##" (2-3 chars)
            let core_label = format!("C{}", i);

            // Mini braille graph (5 chars wide, 1 row)
            let mut mini_graph = Graph::new(mini_graph_w, 1, GraphSymbol::Braille, false, false, 100, 0);
            let mini_str = mini_graph.render_row_colored(core_data, cpu_gradient);

            // btop: C# + graph + rjust(pct,4) + "%" + div_line + v_line
            out.push_str(&format!(
                "\x1b[{};{}H{}{:<3}{}{}{}{}",
                cy, panel_x + 1,
                fg, core_label,
                mini_str,
                pct_color,
                tools::rjust(&pct.to_string(), 4, false),
                "%",
            ));
            // Place │ at the right border
            out.push_str(&format!(
                "\x1b[{};{}H{}{}",
                cy, right_border_x, box_color, symbols::V_LINE
            ));
        }
    }

    // Uptime overlaid on lower-left of graph area
    // btop: placed at row 1 (just below top border) for normal orientation
    let uptime = tools::sec_to_dhms(cpu.uptime_seconds, false, true);
    let up_str = format!("up {}", uptime);
    let up_y = y + 2; // row just below top border, like btop
    if !uptime.is_empty() {
        out.push_str(&format!(
            "\x1b[{};{}H{}{}",
            up_y, x + 2, graph_text_color, up_str
        ));
    }

    // Bottom border keybind hints: menu, preset, -/+ update timer
    let bottom_y = y + height;
    let hints = format!(
        "{}{}{}m{}enu{}{} {}{}{}p{}reset{}{} {}{}{}─{}{}+{}{}",
        box_color, title_syms::TITLE_LEFT_DOWN,
        hi, fg, box_color, title_syms::TITLE_RIGHT_DOWN,
        box_color, title_syms::TITLE_LEFT_DOWN,
        hi, fg, box_color, title_syms::TITLE_RIGHT_DOWN,
        box_color, title_syms::TITLE_LEFT_DOWN,
        hi, fg, hi, box_color, title_syms::TITLE_RIGHT_DOWN,
    );
    out.push_str(&format!(
        "\x1b[{};{}H{}",
        bottom_y, x + 3,
        hints
    ));

    out.push_str("\x1b[0m");
    out
}
