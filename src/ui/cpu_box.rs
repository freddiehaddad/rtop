use crate::collect::CollectStatus;
use crate::domain::config_enums::{CpuGraphSource, TempScale};
use crate::domain::cpu::{CpuInfo, get_cpu_series};
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::{Graph, GraphMode};
use crate::draw::meter::Meter;
use crate::theme::{Theme, gradient_color};
use crate::theme_keys as tc;
use crate::tools;

use super::BoxArea;

/// Extracted settings for the CPU box, decoupled from Config.
pub struct CpuBoxSettings<'a> {
    pub graph_symbol: GraphMode,
    pub upper_source: CpuGraphSource,
    pub lower_source: CpuGraphSource,
    pub check_temp: bool,
    pub show_coretemp: bool,
    pub temp_scale: TempScale,
    pub single_graph: bool,
    pub update_ms: u64,
    pub current_preset: i64,
    pub invert_lower: bool,
    pub show_cpu_freq: bool,
    pub show_uptime: bool,
    pub cpu_name: &'a str,
    pub custom_cpu_name: &'a str,
    pub show_cpu_watts: bool,
    pub cpu_watts: Option<f64>,
    pub cpu_max_watts: Option<f64>,
    pub clock_format: &'a str,
}

/// Label width for stats meter rows (matches GPU box).
const STATS_LABEL_W: usize = 6;
/// Right-aligned value column width for stats meter rows (matches GPU box).
const STATS_VAL_W: usize = 10;
/// Preferred number of core rows per column.
const CORES_PER_COL: usize = 8;
/// Width of the percentage field per core (" 42%" with leading space).
const CORE_PCT_W: usize = 5;
/// Width of the temperature field per core (" 100°C" with leading space), or 0 when hidden.
const CORE_TEMP_W: usize = 6;
/// Minimum width for the mini-graph per core.
const CORE_GRAPH_MIN: usize = 3;
/// Inter-column gap (1 space between columns).
const CORE_COL_GAP: usize = 1;

/// Count how many stats rows will be rendered for a given data state.
pub fn stats_row_count(has_temp: bool, has_watts: bool) -> usize {
    let mut n = 2; // CPU + Load (always)
    if has_temp {
        n += 1;
    }
    if has_watts {
        n += 1;
    }
    n
}

/// Grid layout for the per-core display area.
///
/// Computes rows, columns, and per-column widths from core count and
/// available panel width. Uses `core_grid_shape()` as the single source
/// of truth for grid dimensions.
pub struct CoreGridLayout {
    /// Number of rows per column.
    pub rows: usize,
    /// Number of columns.
    pub cols: usize,
    /// Character width per column (label + graph + pct + temp + gap).
    /// The gap on the last column serves as right-side padding.
    pub col_w: usize,
    /// Character width of the core label ("C0" = 2, "C31" = 3, "127" = 3).
    pub label_w: usize,
    /// Character width of the mini-graph (flex element).
    pub graph_w: usize,
    /// Whether per-core temperature is shown.
    pub show_temp: bool,
}

/// Core grid shape: (rows_per_column, columns) for a given core count.
///
/// Shared by the layout engine (for height sizing) and the core panel
/// renderer (for drawing). This is the single source of truth.
pub(crate) fn core_grid_shape(core_count: usize) -> (usize, usize) {
    match core_count {
        0 => (0, 0),
        1..=8 => (core_count, 1),
        9..=12 => (2, core_count.div_ceil(2)),
        _ => (CORES_PER_COL, core_count.div_ceil(CORES_PER_COL)),
    }
}

impl CoreGridLayout {
    /// Compute the grid layout from core count and available width.
    ///
    /// `panel_inner_w` is the usable content width (already excludes
    /// the divider and 1-space padding on each side).
    pub fn new(core_count: usize, panel_inner_w: usize, show_coretemp: bool) -> Self {
        let (rows, cols) = core_grid_shape(core_count);
        let label_w = Self::label_width(core_count);
        let temp_w: usize = if show_coretemp { CORE_TEMP_W } else { 0 };

        // Column width: label + graph + pct + temp. Gaps between columns
        // are rendered during drawing, not baked into col_w.
        let total_gaps = cols.saturating_sub(1) * CORE_COL_GAP;
        let available_for_cells = panel_inner_w.saturating_sub(total_gaps);
        let col_w = available_for_cells / cols.max(1);
        let fixed_per_col = label_w + CORE_PCT_W + temp_w;
        let graph_w = col_w.saturating_sub(fixed_per_col).max(CORE_GRAPH_MIN);

        Self {
            rows,
            cols,
            col_w,
            label_w,
            graph_w,
            show_temp: show_coretemp,
        }
    }

    /// Label width based on core count.
    fn label_width(core_count: usize) -> usize {
        if core_count >= 10 {
            4 // "C01 " or "001 " (100+), includes trailing space
        } else {
            3 // "C0 ", includes trailing space
        }
    }

    /// Format the label for a given core index.
    pub fn format_label(&self, index: usize) -> String {
        if self.label_w >= 4 {
            if index >= 100 {
                format!("{:03} ", index)
            } else {
                format!("C{:02} ", index)
            }
        } else {
            format!("C{} ", index)
        }
    }

    /// X position for a core in the given column, relative to content_x.
    pub fn col_offset(&self, col: usize) -> usize {
        col * (self.col_w + CORE_COL_GAP)
    }

    /// Exact width the grid occupies (including last column's trailing gap).
    pub fn grid_width(&self) -> usize {
        if self.cols == 0 {
            return 0;
        }
        self.cols * self.col_w + self.cols.saturating_sub(1) * CORE_COL_GAP
    }
}

/// Draw the CPU box into an ANSI string.
///
/// Layout:
/// ╭─┐¹cpu┌────────────────────────┬──────────────────┐Intel(R) Core(TM) i9-14900KF┌─╮
/// │                       user 2% │ CPU   ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■        4% │
/// │                               │ Temp  ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■      40°C │
/// │                               │ Watts ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■  31W/400W │
/// │                               │ Load  ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■      0.06 │
/// │                               │          1m: 0.06  5m: 0.06  15m: 0.06          │
/// │                               ├─┐Cores┌──────────────────────────────┐4.77 GHz┌─┤
/// │⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀│ C00 ⣀⣀⣀⣀⣀⣀⣀⣀   3%  32°C C08 ⣀⣀⣀⣀⣀⣀⣀⣀  10%  37°C │
/// │⠉⠉⠉⠙⠋⠉⠉⠙⠋⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉⠉│ C01 ⣀⣀⣀⣀⣀⣀⣀⣀   1%  32°C C09 ⣀⣀⣀⣀⣀⣀⣀⣀   0%  37°C │
/// │                               │ C02 ⣀⣀⣀⣀⣀⣀⣀⣀   1%  34°C C10 ⣀⣀⣀⣀⣀⣀⣀⣀   0%  37°C │
/// │                               │ C03 ⣀⣀⣀⣀⣀⣀⣀⣀   0%  34°C C11 ⣀⣀⣀⣀⣀⣀⣀⣀  12%  37°C │
/// │                               │ C04 ⣀⣀⣀⣀⣀⣀⣀⣀   2%  33°C C12 ⣀⣀⣀⣀⣀⣀⣀⣀   1%  35°C │
/// │                               │ C05 ⣀⣀⣀⣀⣀⣀⣀⣀   0%  35°C C13 ⣀⣀⣀⣀⣀⣀⣀⣀   0%  35°C │
/// │                               │ C06 ⣀⣀⣀⣀⣀⣀⣀⣀   1%  32°C C14 ⣀⣀⣀⣀⣀⣀⣀⣀   0%  35°C │
/// │                     system 2% │ C07 ⣀⣀⣀⣀⣀⣀⣀⣀   0%  33°C C15 ⣀⣀⣀⣀⣀⣀⣀⣀   5%  35°C │
/// ╰─┘menu└┘preset *0└┘─ 2000ms +└─┴─────────────────────────┘up 13d21:05└┘18:01:00└─╯
pub fn draw(
    cpu: &CpuInfo,
    area: &BoxArea,
    theme: &Theme,
    settings: &CpuBoxSettings,
    status: &CollectStatus,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.color(tc::CPU_BOX);
    let hi = theme.color(tc::HI_FG);
    let title_color = theme.color(tc::TITLE);
    let cpu_upper_gradient = theme.gradient(tc::GRAD_CPU_UPPER);
    let cpu_lower_gradient = theme.gradient(tc::GRAD_CPU_LOWER);
    let graph_text_color = theme.color(tc::GRAPH_TEXT);
    let graph_sym = settings.graph_symbol;
    let upper_key = match settings.upper_source {
        CpuGraphSource::User => "user",
        CpuGraphSource::System => "system",
        CpuGraphSource::Auto | CpuGraphSource::Total => "total",
    };
    let lower_key = match settings.lower_source {
        CpuGraphSource::User => "user",
        CpuGraphSource::System => "system",
        CpuGraphSource::Auto | CpuGraphSource::Total => "total",
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
        num: super::CPU_KEY,
        rounded,
        hi_color: hi,
        title_color,
    }));

    super::draw_status_inset(&mut buf, status, "cpu", x, y, box_color, title_color);

    let core_count = cpu.core_percent.len();
    let inner_h = height.saturating_sub(2);

    if inner_h == 0 || width < 6 {
        return buf.finish();
    }

    // --- Determine data availability for stats rows ---
    let has_temp = settings.check_temp && !cpu.temp.is_empty();
    let show_coretemp_flag = has_temp && settings.show_coretemp;
    let has_watts = settings.show_cpu_watts && settings.cpu_watts.is_some();
    let stats_rows = stats_row_count(has_temp, has_watts);
    // +2 for the load detail row + section_divider between stats and cores
    let panel_content_overhead = stats_rows + 2;

    // --- Core panel sizing via CoreGridLayout ---
    let grid = CoreGridLayout::new(core_count, width / 2, show_coretemp_flag);
    let b_width = if core_count == 0 || width <= 20 {
        0
    } else {
        // divider(1) + left_pad(1) + grid_content + right_pad(1)
        grid.grid_width() + 3
    };
    let b_height = if b_width == 0 {
        0
    } else {
        grid.rows + panel_content_overhead
    };
    let b_x = if b_width == 0 {
        x
    } else {
        x + width.saturating_sub(b_width + 1)
    };
    let b_y = if b_height == 0 { y } else { y + 1 };

    let graph_width = if b_width > 0 {
        width.saturating_sub(b_width + 2)
    } else {
        width.saturating_sub(2)
    };

    // --- Net-style graphs (no horizontal divider) ---
    let has_lower = !settings.single_graph && inner_h >= 2;
    let upper_h = if has_lower { inner_h / 2 } else { inner_h };
    let lower_h = if has_lower { inner_h - upper_h } else { 0 };

    // Draw the vertical divider for core panel
    if b_width > 0 {
        for row_i in 1..height.saturating_sub(1) {
            buf.mv(b_x + 1, y + 1 + row_i)
                .color(box_color)
                .text(symbols::V_LINE);
        }
        buf.mv(b_x + 1, y + 1)
            .color(box_color)
            .text(symbols::DIV_UP);
        buf.mv(b_x + 1, y + height)
            .color(box_color)
            .text(symbols::DIV_DOWN);

        // CPU name inset on the core panel top border (right-aligned)
        {
            let name_display = if settings.custom_cpu_name.is_empty() {
                settings.cpu_name
            } else {
                settings.custom_cpu_name
            };
            if !name_display.is_empty() {
                let max_name_w = b_width.saturating_sub(2);
                let name_trunc = tools::uresize(name_display, max_name_w, false);
                if !name_trunc.is_empty() {
                    let inset =
                        box_drawing::title_inset(&name_trunc, box_color, title_color, false);
                    let inset_x = box_drawing::right_inset_x(
                        b_x + 1,
                        b_width,
                        box_drawing::inset_width(&name_trunc),
                    );
                    buf.mv(inset_x, y + 1).text(&inset);
                }
            }
        }
    }

    // Upper graph (normal orientation)
    if upper_h > 0
        && graph_width > 0
        && let Some(data) = get_cpu_series(&cpu.cpu_percent, upper_key)
    {
        let mut graph = Graph::new(graph_width, upper_h, graph_sym, false, 100, 0);
        let rows = graph.render_rows(data, cpu_upper_gradient);
        for (i, row) in rows.iter().enumerate() {
            buf.mv(x + 2, y + 2 + i).text(row);
        }
    }

    // Upper graph overlay label
    if upper_h > 0 && graph_width > 0 {
        let upper_pct = get_cpu_series(&cpu.cpu_percent, upper_key)
            .and_then(|d| d.back().copied())
            .unwrap_or(0);
        let label = format!("{} {}%", upper_key, upper_pct);
        let label_vis = tools::ulen(&label, false);
        let lx = x + 1 + graph_width.saturating_sub(label_vis);
        let upper_color = if !cpu_upper_gradient.is_empty() {
            &cpu_upper_gradient[upper_pct.clamp(0, 100) as usize]
        } else {
            graph_text_color
        };
        buf.mv(lx, y + 2).color(upper_color).text(&label);
    }

    // Lower graph (inverted orientation)
    if lower_h > 0
        && graph_width > 0
        && let Some(data) = get_cpu_series(&cpu.cpu_percent, lower_key)
    {
        let lower_start_y = y + 2 + upper_h;
        let mut graph = Graph::new(
            graph_width,
            lower_h,
            graph_sym,
            settings.invert_lower,
            100,
            0,
        );
        let rows = graph.render_rows(data, cpu_lower_gradient);
        for (i, row) in rows.iter().enumerate() {
            buf.mv(x + 2, lower_start_y + i).text(row);
        }

        // Lower graph overlay label
        let lower_pct = data.back().copied().unwrap_or(0);
        let label = format!("{} {}%", lower_key, lower_pct);
        let label_vis = tools::ulen(&label, false);
        let lx = x + 1 + graph_width.saturating_sub(label_vis);
        let label_y = lower_start_y + lower_h - 1;
        let lower_color = if !cpu_lower_gradient.is_empty() {
            &cpu_lower_gradient[lower_pct.clamp(0, 100) as usize]
        } else {
            graph_text_color
        };
        buf.mv(lx, label_y).color(lower_color).text(&label);
    }

    // --- Core panel ---
    if b_width > 0 && b_height > 0 {
        let panel = CorePanelArea {
            x: b_x,
            y: b_y,
            width: b_width,
            height: b_height,
        };
        buf.text(&draw_core_panel(
            cpu,
            &panel,
            &grid,
            theme,
            &CorePanelParams {
                has_temp,
                temp_scale: settings.temp_scale,
                graph_sym,
                has_watts,
                cpu_watts: settings.cpu_watts,
                cpu_max_watts: settings.cpu_max_watts,
                stats_rows,
                show_freq: settings.show_cpu_freq,
            },
        ));
    }

    // Uptime and clock as bottom-right border insets
    {
        let bottom_y = y + height;
        let mut insets = String::new();
        let mut total_vis = 0;

        if settings.show_uptime {
            let uptime = tools::sec_to_dhms(cpu.uptime_seconds, false, true);
            if !uptime.is_empty() {
                let up_text = format!("up {}", uptime);
                let vis = box_drawing::inset_width(&up_text);
                insets.push_str(&box_drawing::title_inset(
                    &up_text,
                    box_color,
                    title_color,
                    true,
                ));
                total_vis += vis;
            }
        }

        let clock_str = tools::format_clock(settings.clock_format);
        if !clock_str.is_empty() {
            let vis = box_drawing::inset_width(&clock_str);
            insets.push_str(&box_drawing::title_inset(
                &clock_str,
                box_color,
                title_color,
                true,
            ));
            total_vis += vis;
        }

        if total_vis > 0 {
            let inset_x = box_drawing::right_inset_x(x, width, total_vis);
            buf.mv(inset_x, bottom_y).text(&insets);
        }
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
}

/// Data needed by the core panel renderer, beyond the CpuInfo and theme.
struct CorePanelParams {
    has_temp: bool,
    temp_scale: TempScale,
    graph_sym: GraphMode,
    has_watts: bool,
    cpu_watts: Option<f64>,
    cpu_max_watts: Option<f64>,
    stats_rows: usize,
    show_freq: bool,
}

/// Render the core panel: stats meters, section divider, per-core mini-graphs.
fn draw_core_panel(
    cpu: &CpuInfo,
    panel: &CorePanelArea,
    grid: &CoreGridLayout,
    theme: &Theme,
    params: &CorePanelParams,
) -> String {
    let fg = theme.color(tc::MAIN_FG);
    let title_color = theme.color(tc::TITLE);
    let box_color = theme.color(tc::CPU_BOX);
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
            .color(title_color)
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
            .color(title_color)
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
            s.push_str(title_color);
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
            s.push_str(title_color);
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
            .color(title_color)
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
            box_color,
            symbols::DIV_LEFT,
            symbols::H_LINE.repeat(left_dashes),
            box_drawing::title_syms::TITLE_LEFT,
            title_color,
            section,
            box_color,
            box_drawing::title_syms::TITLE_RIGHT,
            symbols::H_LINE.repeat(mid_dashes),
        );
        if !hz_text.is_empty() {
            divider.push_str(&box_drawing::title_inset(
                &hz_text,
                box_color,
                title_color,
                false,
            ));
            divider.push_str(box_color);
            divider.push_str(symbols::H_LINE);
        }
        divider.push_str(box_color);
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

/// Render the bottom border keybind hints (menu, preset with number, update rate).
fn draw_bottom_hints(
    x: usize,
    bottom_y: usize,
    update_ms: u64,
    current_preset: i64,
    theme: &Theme,
) -> String {
    let box_color = theme.color(tc::CPU_BOX);
    let title_color = theme.color(tc::TITLE);
    let hi = theme.color(tc::HI_FG);

    let preset_label = format!("preset *{}", current_preset);
    let rate_label = format!("{}ms", update_ms);
    let menu_inset = box_drawing::keybind_inset("menu", box_color, hi, title_color, true);
    let preset_inset = box_drawing::keybind_inset(&preset_label, box_color, hi, title_color, true);
    let rate_text = format!("─ {}{} {}+", title_color, rate_label, hi);
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
        let mut cpu = CpuInfo {
            cpu_name: "Test CPU".into(),
            cpu_hz: "3.50 GHz".into(),
            core_count: 4,
            uptime_seconds: 86400,
            load_avg: [0.84, 0.38, 0.40],
            ..CpuInfo::default()
        };
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
            graph_symbol: GraphMode::Braille,
            upper_source: CpuGraphSource::User,
            lower_source: CpuGraphSource::System,
            check_temp: false,
            show_coretemp: false,
            temp_scale: TempScale::Celsius,
            single_graph: false,
            update_ms: 2000,
            current_preset: 0,
            invert_lower: true,
            show_cpu_freq: true,
            show_uptime: true,
            cpu_name: "Test CPU",
            custom_cpu_name: "",
            show_cpu_watts: false,
            cpu_watts: None,
            cpu_max_watts: None,
            clock_format: "",
        }
    }

    #[test]
    fn draw_contains_cpu_title() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
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
            &CollectStatus::Ok,
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
            &CollectStatus::Ok,
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
            &CollectStatus::Ok,
        );
        assert!(!output.is_empty(), "draw output should not be empty");
    }

    #[test]
    fn draw_contains_stats_rows() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("CPU"), "should contain CPU stats row");
        assert!(plain.contains("Load"), "should contain Load stats row");
    }

    #[test]
    fn draw_contains_overlay_labels() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("user") && plain.contains('%'),
            "should contain upper graph overlay label"
        );
        assert!(
            plain.contains("system") && plain.contains('%'),
            "should contain lower graph overlay label"
        );
    }

    #[test]
    fn draw_contains_uptime_inset() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("up "), "should contain uptime border inset");
    }

    #[test]
    fn draw_contains_cores_divider() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("Cores"),
            "should contain Cores section divider"
        );
    }

    #[test]
    fn draw_contains_load_detail_row() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("0.84"), "should contain 1-min load: {plain}");
        assert!(plain.contains("0.38"), "should contain 5-min load: {plain}");
    }

    #[test]
    fn bottom_hints_use_title_for_label_text() {
        // Defends border-inset color consistency: pre-fix the CPU bottom
        // hints rendered "enu", "reset *0", and "2000ms" in MAIN_FG while
        // every other widget's border insets use TITLE for label/value text.
        // Hotkey letters (m, p, ─, +) stay HI_FG.
        let theme = Theme::default();
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &theme,
            &make_settings(),
            &CollectStatus::Ok,
        );
        let title = theme.color(tc::TITLE);
        assert!(
            output.contains(&format!("{}{}", title, "enu")),
            "menu inset 'enu' should be preceded by TITLE"
        );
        assert!(
            output.contains(&format!("{}{}", title, "reset *0")),
            "preset inset 'reset *0' should be preceded by TITLE"
        );
        assert!(
            output.contains(&format!("{}{}", title, "2000ms")),
            "rate inset '2000ms' should be preceded by TITLE"
        );
    }

    #[test]
    fn stats_panel_cpu_value_uses_cpu_upper_gradient() {
        // Defends Option A. Pre-fix the CPU/Temp/Watts/Load values in the
        // stats panel rendered MAIN_FG even though their meters used
        // GRAD_CPU_UPPER / GRAD_TEMP. The same widget's per-core grid
        // already coloured by gradient, so the widget disagreed with itself.
        let theme = Theme::default();
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &theme,
            &make_settings(),
            &CollectStatus::Ok,
        );
        let cpu_grad = theme.gradient(tc::GRAD_CPU_UPPER);
        // CPU total = 50 → CPU value cell is rjust("50%", 10) = "       50%"
        // immediately preceded by GRAD_CPU_UPPER[50].
        let expected = format!("{}{}", cpu_grad[50], "       50%");
        assert!(
            output.contains(&expected),
            "CPU stats value should be GRAD_CPU_UPPER[50] followed by '       50%'"
        );
        // Load = 0.84 → load_pct = 84.
        let expected_load = format!("{}{}", cpu_grad[84], "      0.84");
        assert!(
            output.contains(&expected_load),
            "Load stats value should be GRAD_CPU_UPPER[84] followed by '      0.84'"
        );
    }

    #[test]
    fn stats_panel_watts_without_max_stays_main_fg() {
        // Explicit exception: when cpu_max_watts is None there is no meter
        // (the meter slot is filled with blanks for alignment), so the value
        // text stays MAIN_FG, not coloured by any gradient. This guards the
        // exception against future refactors.
        let theme = Theme::default();
        let mut settings = make_settings();
        settings.show_cpu_watts = true;
        settings.cpu_watts = Some(42.5);
        settings.cpu_max_watts = None;
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &theme,
            &settings,
            &CollectStatus::Ok,
        );
        let fg = theme.color(tc::MAIN_FG);
        let cpu_grad = theme.gradient(tc::GRAD_CPU_UPPER);
        // Watts value '42.5 W' rjust'd to 10 = '    42.5 W'.
        let expected = format!("{}{}", fg, "    42.5 W");
        assert!(
            output.contains(&expected),
            "Watts-no-max value should be MAIN_FG followed by '    42.5 W'"
        );
        // And it must NOT be preceded by a CPU_UPPER gradient escape.
        for grad_escape in cpu_grad {
            assert!(
                !output.contains(&format!("{}{}", grad_escape, "    42.5 W")),
                "Watts-no-max value must not be coloured by GRAD_CPU_UPPER"
            );
        }
    }

    #[test]
    fn grid_layout_single_column_small() {
        let grid = CoreGridLayout::new(4, 20, false);
        assert_eq!(grid.rows, 4);
        assert_eq!(grid.cols, 1);
    }

    #[test]
    fn grid_layout_two_rows_midrange() {
        let grid = CoreGridLayout::new(12, 80, false);
        assert_eq!(grid.rows, 2);
        assert_eq!(grid.cols, 6);
    }

    #[test]
    fn grid_layout_eight_rows_32_cores() {
        let grid = CoreGridLayout::new(32, 80, false);
        assert_eq!(grid.rows, 8);
        assert_eq!(grid.cols, 4);
    }

    #[test]
    fn grid_layout_eight_rows_16_cores() {
        let grid = CoreGridLayout::new(16, 60, false);
        assert_eq!(grid.rows, 8);
        assert_eq!(grid.cols, 2);
    }

    #[test]
    fn grid_layout_col_offset_evenly_spaced() {
        let grid = CoreGridLayout::new(32, 80, false);
        let stride = grid.col_w + CORE_COL_GAP;
        assert_eq!(grid.col_offset(0), 0);
        assert_eq!(grid.col_offset(1), stride);
        assert_eq!(grid.col_offset(2), stride * 2);
    }
}
