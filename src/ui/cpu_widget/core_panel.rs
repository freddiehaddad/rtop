//! Per-core grid panel rendering for the CPU widget.
//!
//! Owns the layout + render logic for the right-hand panel inside
//! the CPU widget: the stats meter rows (CPU / Temp / Watts /
//! Load), the load-average detail row, the section divider, and
//! the per-core mini-graphs grouped in columns.

use crate::domain::config_enums::TempScale;
use crate::domain::cpu::CpuInfo;
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::{Graph, GraphMode};
use crate::draw::meter::Meter;
use crate::theme::{Theme, gradient_color};
use crate::theme_keys as tc;
use crate::tools;

use super::sizing::{CORE_GRAPH_MIN, CoreGridLayout, STATS_LABEL_W, STATS_VAL_W};

/// Geometry of the per-core panel within the CPU widget.
pub(super) struct CorePanelArea {
    pub(super) x: usize,
    pub(super) y: usize,
    pub(super) width: usize,
    pub(super) height: usize,
}

/// Data needed by the core panel renderer, beyond the CpuInfo and theme.
pub(super) struct CorePanelParams {
    pub(super) has_temp: bool,
    pub(super) temp_scale: TempScale,
    pub(super) graph_sym: GraphMode,
    pub(super) has_watts: bool,
    pub(super) cpu_watts: Option<f64>,
    pub(super) cpu_max_watts: Option<f64>,
    pub(super) stats_rows: usize,
    pub(super) show_freq: bool,
}

/// Render the core panel: stats meters, section divider, per-core mini-graphs.
pub(super) fn draw_core_panel(
    cpu: &CpuInfo,
    panel: &CorePanelArea,
    grid: &CoreGridLayout,
    theme: &Theme,
    params: &CorePanelParams,
) -> String {
    let fg = theme.color(tc::MAIN_FG);
    let title_color = theme.color(tc::TITLE);
    let border_color = theme.color(tc::CPU_WIDGET);
    let cpu_gradient = theme.gradient(tc::GRAD_CPU_UPPER);
    let temp_gradient = theme.gradient(tc::GRAD_TEMP);
    let mut buf = AnsiBuffer::new();

    // Content area: 1 space after divider, 1 space before right border
    let panel_inner_w = panel.width.saturating_sub(3);
    let meter_bg = theme.color(tc::METER_BG);
    let content_x = panel.x + 3;

    // GPU-style meter layout: label(6) + meter(flex) + value(10, rjust)
    let meter_w = panel_inner_w
        .saturating_sub(STATS_LABEL_W + STATS_VAL_W)
        .max(5);
    let cpu_meter = Meter::new(meter_w, cpu_gradient, meter_bg);

    let mut row = 0;

    // Row: CPU utilization meter (always shown)
    if let Some(&pct) = cpu.cpu_percent.total.back() {
        let pct = pct.clamp(0, 100) as i32;
        buf.mv(content_x, panel.y + 1 + row)
            .color(fg)
            .text("CPU   ")
            .text(cpu_meter.render(pct))
            .color(gradient_color(cpu_gradient, pct))
            .text(&tools::rjust(&format!("{}%", pct), STATS_VAL_W, true));
        row += 1;
    }

    // Row: Temperature meter (shown when LHM provides temps)
    if params.has_temp
        && let Some(pkg_data) = cpu.temp.first()
    {
        let pkg_temp = pkg_data.back().copied().unwrap_or(0);
        let temp_pct = pkg_temp.clamp(0, 100) as i32;
        let temp_meter = Meter::new(meter_w, temp_gradient, meter_bg);
        let (conv_temp, temp_unit) = crate::tools::celsius_to(pkg_temp, params.temp_scale);
        buf.mv(content_x, panel.y + 1 + row)
            .color(fg)
            .text("Temp  ")
            .text(temp_meter.render(temp_pct))
            .color(gradient_color(temp_gradient, temp_pct))
            .text(&tools::rjust(
                &format!("{}{}", conv_temp, temp_unit),
                STATS_VAL_W,
                true,
            ));
        row += 1;
    }

    // Row: Watts meter (shown when LHM provides power data)
    if params.has_watts
        && let Some(watts) = params.cpu_watts
    {
        let watts_str = if let Some(max_w) = params.cpu_max_watts {
            let pct = if max_w > 0.0 {
                (watts / max_w * 100.0).clamp(0.0, 100.0) as i32
            } else {
                0
            };
            let watts_meter = Meter::new(meter_w, cpu_gradient, meter_bg);
            let val = format!("{:.0}W/{:.0}W", watts, max_w);
            let mut s = String::new();
            s.push_str(fg);
            s.push_str("Watts ");
            s.push_str(watts_meter.render(pct));
            s.push_str(gradient_color(cpu_gradient, pct));
            s.push_str(&tools::rjust(&val, STATS_VAL_W, true));
            s
        } else {
            // No max — show value only, no meter bar; value stays fg per the
            // "gradient only when a meter exists" rule.
            let val = format!("{:.1} W", watts);
            let mut s = String::new();
            s.push_str(fg);
            s.push_str("Watts ");
            // Fill meter space with blanks for alignment
            s.push_str(&" ".repeat(meter_w));
            s.push_str(fg);
            s.push_str(&tools::rjust(&val, STATS_VAL_W, true));
            s
        };
        buf.mv(content_x, panel.y + 1 + row).text(&watts_str);
        row += 1;
    }

    // Row: Load meter (always shown)
    {
        let load1 = cpu.load_avg[0];
        // load_avg is a 0.0–1.0 fraction of total CPU capacity
        let load_pct = (load1 * 100.0).clamp(0.0, 100.0) as i32;
        let load_meter = Meter::new(meter_w, cpu_gradient, meter_bg);
        let load_val = format!("{:.2}", load1);
        buf.mv(content_x, panel.y + 1 + row)
            .color(fg)
            .text("Load  ")
            .text(load_meter.render(load_pct))
            .color(gradient_color(cpu_gradient, load_pct))
            .text(&tools::rjust(&load_val, STATS_VAL_W, true));
        row += 1;
    }

    // Load averages row (centered, below meter)
    {
        let lavg_text = if panel_inner_w >= 30 {
            format!(
                "1m: {:.2}  5m: {:.2}  15m: {:.2}",
                cpu.load_avg[0], cpu.load_avg[1], cpu.load_avg[2]
            )
        } else {
            format!(
                "{:.2}  {:.2}  {:.2}",
                cpu.load_avg[0], cpu.load_avg[1], cpu.load_avg[2]
            )
        };
        let lavg_vis = tools::ulen(&lavg_text, false);
        if lavg_vis <= panel_inner_w {
            let lavg_x = content_x + (panel_inner_w.saturating_sub(lavg_vis)) / 2;
            buf.mv(lavg_x, panel.y + 1 + row).color(fg).text(&lavg_text);
        }
        row += 1;
    }

    // Section divider: ├─┐Cores┌──────────┐5.05 GHz┌─┤
    let divider_y = panel.y + 1 + row;
    {
        let section = "Cores";
        let width = panel.width.saturating_sub(1);
        let left_vis = tools::ulen(section, false) + 2; // +2 for inset chars
        let left_dashes = 1;

        // Frequency inset on the right side of the divider
        let hz_text = if params.show_freq && !cpu.cpu_hz.is_empty() {
            cpu.cpu_hz.clone()
        } else {
            String::new()
        };
        let hz_vis = if hz_text.is_empty() {
            0
        } else {
            tools::ulen(&hz_text, false) + 2 // +2 for inset chars
        };

        let mid_dashes = width.saturating_sub(left_dashes + left_vis + hz_vis + 1);
        let mut divider = format!(
            "{}{}{}{}{}{}{}{}{}",
            border_color,
            symbols::DIV_LEFT,
            symbols::H_LINE.repeat(left_dashes),
            box_drawing::title_syms::TITLE_LEFT,
            title_color,
            section,
            border_color,
            box_drawing::title_syms::TITLE_RIGHT,
            symbols::H_LINE.repeat(mid_dashes),
        );
        if !hz_text.is_empty() {
            divider.push_str(&box_drawing::title_inset(
                &hz_text,
                border_color,
                title_color,
                false,
            ));
            divider.push_str(border_color);
            divider.push_str(symbols::H_LINE);
        }
        divider.push_str(border_color);
        divider.push_str(symbols::DIV_RIGHT);
        buf.mv(panel.x + 1, divider_y).text(&divider);
    }

    // Per-core rows using grid layout
    let core_start_y = panel.y + 1 + params.stats_rows + 2; // stats + load detail + divider

    for (i, core_data) in cpu.core_percent.iter().enumerate() {
        let col = i / grid.rows;
        let row = i % grid.rows;

        if col >= grid.cols {
            break;
        }

        let row_x = content_x + grid.col_offset(col);
        let row_y = core_start_y + row;
        if row_y > panel.y + panel.height {
            break;
        }

        let pct = core_data.back().copied().unwrap_or(0);
        let pct_color = gradient_color(cpu_gradient, pct.clamp(0, 100) as i32);

        let label = grid.format_label(i);
        buf.mv(row_x, row_y).color(fg).text(&label);

        if grid.graph_w >= CORE_GRAPH_MIN {
            let mut mini = Graph::new(grid.graph_w, 1, params.graph_sym, false, 100, 0);
            let mini_str = mini.render_row(core_data, cpu_gradient);
            buf.text(&mini_str);
        }

        buf.color(pct_color).text(&format!("{:>4}%", pct)).color(fg);

        if grid.show_temp {
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
            let t_color = gradient_color(temp_gradient, core_temp.clamp(0, 100) as i32);
            let (conv_temp, temp_unit) = crate::tools::celsius_to(core_temp, params.temp_scale);
            buf.color(t_color)
                .text(&format!("{:>4}{}", conv_temp, temp_unit));
        }
    }

    buf.finish()
}
