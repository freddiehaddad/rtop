use crate::domain::cpu::CpuInfo;
use crate::draw::box_drawing;
use crate::draw::graph::{Graph, GraphSymbol};
use crate::draw::meter::Meter;
use crate::theme::Theme;
use crate::tools;

/// Draw the CPU box into an ANSI string. Returns escape-code output.
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
    let cpu_gradient = theme.g("cpu");

    let mut out = box_drawing::create_box(x, y, width, height, box_color, false, "cpu", "", 0, rounded);

    // Row 1: CPU name and total usage
    if let Some(total) = cpu.cpu_percent.get("total") {
        if let Some(&pct) = total.back() {
            let pct_color = if !cpu_gradient.is_empty() { &cpu_gradient[pct.clamp(0, 100) as usize] } else { fg };
            out.push_str(&format!(
                "\x1b[{};{}H {}{} {}{}{}%",
                y + 2, x + 1, fg, cpu.cpu_name, pct_color, pct, fg
            ));
        }
    }

    // Rows 2-3: CPU usage graph (braille)
    let graph_width = width.saturating_sub(2);
    if height > 5 {
        if let Some(total) = cpu.cpu_percent.get("total") {
            let mut graph = Graph::new(graph_width, 1, GraphSymbol::Braille, false, false, 100, 0);
            let graph_str = graph.render_row_colored(total, cpu_gradient);
            out.push_str(&format!("\x1b[{};{}H{}", y + 3, x + 1, graph_str));

            if height > 6 {
                let mut graph_low = Graph::new(graph_width, 1, GraphSymbol::Braille, true, false, 100, 0);
                let graph_low_str = graph_low.render_row_colored(total, cpu_gradient);
                out.push_str(&format!("\x1b[{};{}H{}", y + 4, x + 1, graph_low_str));
            }
        }
    }

    // Per-core display with mini meters
    let core_start_y = if height > 7 { y + 5 } else { y + 3 };
    let core_area_h = (y + height - 2).saturating_sub(core_start_y);
    if core_area_h > 0 && !cpu.core_percent.is_empty() {
        let cols = if width > 80 { 4 } else if width > 50 { 2 } else { 1 };
        let col_w = (width.saturating_sub(4)) / cols;
        let meter_w = col_w.saturating_sub(8).min(15);

        for (i, core_data) in cpu.core_percent.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            if row >= core_area_h { break; }
            let cy = core_start_y + row;
            let cx = x + 2 + col * col_w;
            let pct = core_data.back().copied().unwrap_or(0);
            let pct_color = if !cpu_gradient.is_empty() { &cpu_gradient[pct.clamp(0, 100) as usize] } else { fg };

            let meter = Meter::new(meter_w);
            let meter_str = meter.render(pct as i32);
            out.push_str(&format!(
                "\x1b[{};{}H{}{:<2}{}{} {}{}%{}",
                cy, cx, fg, i, pct_color, meter_str, pct_color, pct, fg
            ));
        }
    }

    // Bottom: Frequency + Uptime
    if height > 4 {
        let bottom_y = y + height - 1;
        if !cpu.cpu_hz.is_empty() {
            out.push_str(&format!("\x1b[{};{}H{}{}", bottom_y, x + 2, fg, cpu.cpu_hz));
        }
        let uptime = tools::sec_to_dhms(cpu.uptime_seconds, false, true);
        let up_str = format!("Up {}", uptime);
        let up_x = x + width.saturating_sub(up_str.len() + 2);
        out.push_str(&format!("\x1b[{};{}H{}{}", bottom_y, up_x, fg, up_str));
    }

    out.push_str("\x1b[0m");
    out
}
