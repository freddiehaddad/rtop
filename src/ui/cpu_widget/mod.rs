use crate::collect::CollectStatus;
use crate::domain::config_enums::{CpuGraphSource, TempScale};
use crate::domain::cpu::CpuInfo;
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::{Graph, GraphMode};
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

use super::WidgetArea;

/// Per-frame view passed to [`draw`].
///
/// Bundles `CpuConfig` / `UiConfig` field reads with per-frame
/// computed values (current CPU snapshot fields). Constructed
/// inline at the call site — there's no `build_settings` helper
/// because every entry maps to either a fixed lookup (e.g.,
/// `config.cpu.cpu_invert_lower`) or a per-frame computation.
///
/// Chrome that previously lived on the CPU widget's bottom
/// border (menu/preset/update_interval/uptime/clock) is owned by
/// the borderless statusbar widget; this struct intentionally no
/// longer carries `update_ms`, `preset_name`, `filter_active`,
/// `show_uptime`, or `clock_format`.
pub struct CpuFrame<'a> {
    pub graph_symbol: GraphMode,
    pub upper_source: CpuGraphSource,
    pub lower_source: CpuGraphSource,
    pub check_temp: bool,
    pub show_coretemp: bool,
    pub temp_scale: TempScale,
    pub single_graph: bool,
    /// When `true`, scale each main graph's y-axis to the largest
    /// value in its visible window (matches the net widget). When
    /// `false` (default), scale to a fixed 0-100 absolute range —
    /// the height of the bar then directly maps to the CPU%.
    pub auto_scale: bool,
    pub invert_lower: bool,
    pub show_cpu_freq: bool,
    pub cpu_name: &'a str,
    pub custom_cpu_name: &'a str,
    pub show_cpu_watts: bool,
    pub cpu_watts: Option<f64>,
    pub cpu_max_watts: Option<f64>,
}

mod sizing;

use sizing::{CPU_STRUCTURAL_OVERHEAD, graph_max, stats_row_count};
pub use sizing::{CoreGridLayout, min_width, preferred_height};

/// Draw the CPU widget into an ANSI string.
pub fn draw(
    cpu: &CpuInfo,
    area: &WidgetArea,
    theme: &Theme,
    settings: &CpuFrame,
    status: &CollectStatus,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let border_color = theme.color(tc::CPU_WIDGET);
    let hi = theme.color(tc::HI_FG);
    let title_color = theme.color(tc::TITLE);
    let cpu_upper_gradient = theme.gradient(tc::GRAD_CPU_UPPER);
    let cpu_lower_gradient = theme.gradient(tc::GRAD_CPU_LOWER);
    let graph_text_color = theme.color(tc::GRAPH_TEXT);
    let graph_sym = settings.graph_symbol;
    let upper_label = settings.upper_source.display_label();
    let lower_label = settings.lower_source.display_label();

    let mut buf = AnsiBuffer::new();
    buf.text(&box_drawing::create_box(&box_drawing::BoxConfig {
        x,
        y,
        width,
        height,
        line_color: border_color,
        fill: true,
        title: "cpu",
        title2: "",
        num: super::CPU_KEY,
        rounded,
        hi_color: hi,
        title_color,
    }));

    super::draw_status_inset(&mut buf, status, "cpu", x, y, border_color, title_color);

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
    //
    // Half-and-half budget: the core panel gets up to `inner_usable
    // / 2` so the main graph keeps roughly the other half. Within
    // that budget `CoreGridLayout::new` picks the highest tier
    // (Comfortable / Compact / Minimal) whose column width fits,
    // and the per-core mini-graph (Comfortable only) expands into
    // any leftover budget.
    //
    // If even the Minimal tier doesn't fit in half the inner width,
    // retry with the full inner width — the core panel takes
    // everything and the main graph collapses to zero. The global
    // `min_terminal_size` gate triggers "Terminal too small"
    // before this fallback fails outright.
    let inner_usable = width.saturating_sub(CPU_STRUCTURAL_OVERHEAD);
    let half_budget = inner_usable / 2;
    let mut grid = CoreGridLayout::new(core_count, half_budget, show_coretemp_flag);
    if grid.cols == 0 && core_count > 0 {
        grid = CoreGridLayout::new(core_count, inner_usable, show_coretemp_flag);
    }

    let b_width = if grid.cols == 0 {
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
                .color(border_color)
                .text(symbols::V_LINE);
        }
        buf.mv(b_x + 1, y + 1)
            .color(border_color)
            .text(symbols::DIV_UP);
        buf.mv(b_x + 1, y + height)
            .color(border_color)
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
                        box_drawing::title_inset(&name_trunc, border_color, title_color, false);
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
    if upper_h > 0 && graph_width > 0 {
        let data = cpu.cpu_percent.series(settings.upper_source);
        let max = graph_max(data, graph_width, settings.auto_scale);
        let mut graph = Graph::new(graph_width, upper_h, graph_sym, false, max, 0);
        let rows = graph.render_rows(data, cpu_upper_gradient);
        for (i, row) in rows.iter().enumerate() {
            buf.mv(x + 2, y + 2 + i).text(row);
        }
    }

    // Upper graph overlay label
    if upper_h > 0 && graph_width > 0 {
        let upper_pct = cpu
            .cpu_percent
            .series(settings.upper_source)
            .back()
            .copied()
            .unwrap_or(0);
        let label = format!("{} {}%", upper_label, upper_pct);
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
    if lower_h > 0 && graph_width > 0 {
        let data = cpu.cpu_percent.series(settings.lower_source);
        let lower_start_y = y + 2 + upper_h;
        let max = graph_max(data, graph_width, settings.auto_scale);
        let mut graph = Graph::new(
            graph_width,
            lower_h,
            graph_sym,
            settings.invert_lower,
            max,
            0,
        );
        let rows = graph.render_rows(data, cpu_lower_gradient);
        for (i, row) in rows.iter().enumerate() {
            buf.mv(x + 2, lower_start_y + i).text(row);
        }

        // Lower graph overlay label
        let lower_pct = data.back().copied().unwrap_or(0);
        let label = format!("{} {}%", lower_label, lower_pct);
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

    buf.finish()
}

mod core_panel;

use core_panel::{CorePanelArea, CorePanelParams, draw_core_panel};

// ---------------------------------------------------------------------------
// Widget impl
// ---------------------------------------------------------------------------

/// CPU widget renderer. Unit struct — the widget has no per-
/// instance state.
pub struct CpuWidget;

impl super::Widget for CpuWidget {
    fn kinds(&self) -> &'static [crate::domain::widget_kind::WidgetKind] {
        const KINDS: &[crate::domain::widget_kind::WidgetKind] =
            &[crate::domain::widget_kind::WidgetKind::Cpu];
        KINDS
    }

    fn preferred_height(&self, hints: &crate::draw::layout::LayoutHints) -> usize {
        // The container-relative `term_height/3` clamp lives on the
        // layout engine because it depends on terminal dimensions
        // (not in `LayoutHints`); we apply only the per-widget
        // intrinsic floor here.
        preferred_height(hints).max(crate::draw::layout::MIN_CPU_HEIGHT)
    }

    fn min_width(&self, hints: &crate::draw::layout::LayoutHints) -> usize {
        min_width(hints)
    }

    fn min_height(&self, hints: &crate::draw::layout::LayoutHints) -> usize {
        preferred_height(hints).max(crate::draw::layout::MIN_CPU_HEIGHT)
    }

    fn render(&self, params: &crate::app::RenderParams<'_>, output: &mut String) {
        let Some(cpu_dim) = params
            .layout
            .dims_for(crate::domain::widget_kind::WidgetKind::Cpu)
        else {
            return;
        };
        let Some(cpu) = params.cpu else {
            return;
        };
        let area = super::WidgetArea::from_dim(cpu_dim, params.rounded);
        let frame = CpuFrame {
            graph_symbol: crate::draw::graph::GraphMode::from_config(
                params.config.cpu.graph_symbol_cpu,
                params.config.ui.graph_symbol,
            ),
            upper_source: params.config.cpu.cpu_graph_upper,
            lower_source: params.config.cpu.cpu_graph_lower,
            check_temp: params.config.cpu.check_temp,
            show_coretemp: params.config.cpu.show_coretemp,
            temp_scale: params.config.cpu.temp_scale,
            single_graph: params.config.cpu.cpu_single_graph,
            auto_scale: params.config.cpu.cpu_auto_scale,
            invert_lower: params.config.cpu.cpu_invert_lower,
            show_cpu_freq: params.config.cpu.show_cpu_freq,
            cpu_name: &cpu.info.cpu_name,
            custom_cpu_name: &params.config.cpu.custom_cpu_name,
            show_cpu_watts: params.config.cpu.show_cpu_watts,
            cpu_watts: cpu.info.cpu_watts,
            cpu_max_watts: cpu.info.cpu_max_watts,
        };
        output.push_str(&draw(&cpu.info, &area, params.theme, &frame, &cpu.status));
    }
}

#[cfg(test)]
mod tests {
    use super::sizing::{CORE_COL_GAP, CORE_GRAPH_MIN, CORE_PCT_W, CORE_TEMP_W};
    use super::*;
    use crate::draw::layout::LayoutHints;
    use std::collections::VecDeque;

    #[test]
    fn core_grid_layout_picks_comfortable_when_budget_is_ample() {
        // 4 cores, 1 column, large budget — Comfortable wins, graph_w expands.
        let g = CoreGridLayout::new(4, 80, true);
        assert_eq!(g.cols, 1);
        assert!(g.graph_w >= CORE_GRAPH_MIN);
        assert!(g.show_temp);
        assert_eq!(g.col_w, g.label_w + g.graph_w + CORE_PCT_W + CORE_TEMP_W);
    }

    #[test]
    fn core_grid_layout_drops_graph_at_compact_tier() {
        // 32 cores → 4 columns. Budget exactly fits Compact:
        //   per-col = label(3) + pct(5) + temp(6) = 14
        //   4 * 14 + 3 gaps = 59.
        let g = CoreGridLayout::new(32, 59, true);
        assert_eq!(g.cols, 4);
        assert_eq!(g.graph_w, 0, "Compact tier drops the per-core graph");
        assert!(g.show_temp, "Compact tier keeps the per-core temperature");
        assert_eq!(g.col_w, 3 + CORE_PCT_W + CORE_TEMP_W);
    }

    #[test]
    fn core_grid_layout_drops_temp_at_minimal_tier() {
        // 32 cores → 4 columns. Budget exactly fits Minimal:
        //   per-col = label(3) + pct(5) = 8
        //   4 * 8 + 3 gaps = 35.
        let g = CoreGridLayout::new(32, 35, true);
        assert_eq!(g.cols, 4);
        assert_eq!(g.graph_w, 0);
        assert!(!g.show_temp, "Minimal tier drops the per-core temperature");
        assert_eq!(g.col_w, 3 + CORE_PCT_W);
    }

    #[test]
    fn core_grid_layout_returns_empty_when_below_minimal() {
        // Budget can't fit even Minimal — grid signals "no panel".
        let g = CoreGridLayout::new(32, 20, true);
        assert_eq!(g.cols, 0);
        assert_eq!(g.rows, 0);
    }

    #[test]
    fn core_grid_layout_col_w_matches_actual_render_width() {
        // Regression: col_w must equal the actual rendered cell width
        // so the renderer's column-offset arithmetic doesn't overflow.
        for &cores in &[1usize, 4, 8, 12, 16, 32, 64, 128] {
            for &budget in &[10usize, 30, 50, 80, 120, 200] {
                for &temp in &[false, true] {
                    let g = CoreGridLayout::new(cores, budget, temp);
                    if g.cols == 0 {
                        continue;
                    }
                    let temp_w = if g.show_temp { CORE_TEMP_W } else { 0 };
                    assert_eq!(
                        g.col_w,
                        g.label_w + g.graph_w + CORE_PCT_W + temp_w,
                        "col_w mismatch for cores={cores}, budget={budget}, temp={temp}",
                    );
                    // Actual grid width must fit within the budget.
                    assert!(
                        g.grid_width() <= budget,
                        "grid_width {} > budget {} for cores={cores}, temp={temp}",
                        g.grid_width(),
                        budget,
                    );
                }
            }
        }
    }

    #[test]
    fn min_width_grows_with_core_count() {
        let mk = |cores| LayoutHints {
            core_count: cores,
            ..Default::default()
        };
        assert!(min_width(&mk(4)) < min_width(&mk(32)));
        assert!(min_width(&mk(32)) < min_width(&mk(128)));
    }

    #[test]
    fn min_width_for_32_cores_fits_minimal_tier_exactly() {
        // 32 cores → 4 columns of (label=3, pct=5, gap=1) +
        // structural overhead 5 = 4*8 + 3 + 5 = 40.
        let hints = LayoutHints {
            core_count: 32,
            ..Default::default()
        };
        assert_eq!(min_width(&hints), 40);
    }

    #[test]
    fn format_label_omits_c_prefix_and_zero_pads_to_max_index_width() {
        // 32 cores → max index 31 → 2 digits → label_w = 3.
        let g = CoreGridLayout::new(32, 80, false);
        assert_eq!(g.label_w, 3);
        assert_eq!(g.format_label(0), "00 ");
        assert_eq!(g.format_label(7), "07 ");
        assert_eq!(g.format_label(31), "31 ");
    }

    #[test]
    fn format_label_widths_match_decade_tiers() {
        // 1-9 cores → 1 digit → label_w = 2.
        let g = CoreGridLayout::new(8, 80, false);
        assert_eq!(g.label_w, 2);
        assert_eq!(g.format_label(0), "0 ");
        assert_eq!(g.format_label(7), "7 ");

        // 10-99 cores → 2 digits → label_w = 3.
        let g = CoreGridLayout::new(64, 200, false);
        assert_eq!(g.label_w, 3);
        assert_eq!(g.format_label(63), "63 ");

        // 100+ cores → 3 digits → label_w = 4.
        let g = CoreGridLayout::new(128, 200, false);
        assert_eq!(g.label_w, 4);
        assert_eq!(g.format_label(0), "000 ");
        assert_eq!(g.format_label(127), "127 ");
    }
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

    fn make_area() -> WidgetArea {
        WidgetArea {
            x: 1,
            y: 1,
            width: 80,
            height: 20,
            rounded: true,
        }
    }

    fn make_frame() -> CpuFrame<'static> {
        CpuFrame {
            graph_symbol: GraphMode::Braille,
            upper_source: CpuGraphSource::User,
            lower_source: CpuGraphSource::System,
            check_temp: false,
            show_coretemp: false,
            temp_scale: TempScale::Celsius,
            single_graph: false,
            auto_scale: false,
            invert_lower: true,
            show_cpu_freq: true,
            cpu_name: "Test CPU",
            custom_cpu_name: "",
            show_cpu_watts: false,
            cpu_watts: None,
            cpu_max_watts: None,
        }
    }

    #[test]
    fn draw_contains_cpu_title() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("cpu"), "output should contain 'cpu' title");
    }

    /// The CPU widget's bottom border no longer carries any
    /// chrome insets — every value (menu, preset, update interval,
    /// uptime, clock) lives on the borderless statusbar widget
    /// instead. This test pins that contract so a future change
    /// can't accidentally re-introduce CPU-side chrome.
    #[test]
    fn draw_does_not_contain_relocated_chrome() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            !plain.contains("menu"),
            "CPU widget must not render the 'menu' inset",
        );
        assert!(
            !plain.contains("← P"),
            "CPU widget must not render the preset cycler",
        );
        assert!(
            !plain.contains("ms +"),
            "CPU widget must not render the update-rate inset",
        );
        assert!(
            !plain.contains("up "),
            "CPU widget must not render the uptime inset",
        );
    }

    #[test]
    fn draw_output_is_non_empty() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
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
            &make_frame(),
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
            &make_frame(),
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
    fn draw_contains_cores_divider() {
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
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
            &make_frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("0.84"), "should contain 1-min load: {plain}");
        assert!(plain.contains("0.38"), "should contain 5-min load: {plain}");
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
            &make_frame(),
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
        let mut settings = make_frame();
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
    fn stats_panel_labels_use_main_fg() {
        // Body label rule: CPU/Temp/Watts/Load meter labels render in
        // MAIN_FG, not TITLE. Pre-shift these were TITLE, which made them
        // look identical to widget title insets and section dividers; the
        // shift creates a clean two-tier visual hierarchy.
        let theme = Theme::default();
        let mut settings = make_frame();
        settings.show_cpu_watts = true;
        settings.cpu_watts = Some(42.5);
        settings.cpu_max_watts = Some(125.0);
        let output = draw(
            &make_cpu_info(),
            &make_area(),
            &theme,
            &settings,
            &CollectStatus::Ok,
        );
        let fg = theme.color(tc::MAIN_FG);
        for label in &["CPU   ", "Watts ", "Load  "] {
            assert!(
                output.contains(&format!("{fg}{label}")),
                "cpu stats label {label:?} should be preceded by MAIN_FG"
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

    // ----- graph_max / auto-scale --------------------------------------

    #[test]
    fn graph_max_off_returns_fixed_100() {
        let data: VecDeque<i64> = VecDeque::from([5, 10, 7]);
        assert_eq!(graph_max(&data, 80, false), 100);
        let empty: VecDeque<i64> = VecDeque::new();
        assert_eq!(graph_max(&empty, 80, false), 100);
    }

    #[test]
    fn graph_max_on_returns_visible_window_max() {
        let data: VecDeque<i64> = VecDeque::from([99, 88, 5, 10, 7]);
        // Width covers the last 3 values only; 99 and 88 fall outside.
        assert_eq!(graph_max(&data, 3, true), 10);
        // Wider window includes the older spike.
        assert_eq!(graph_max(&data, 80, true), 99);
    }

    #[test]
    fn graph_max_on_floors_at_one_for_empty_or_zero_data() {
        let empty: VecDeque<i64> = VecDeque::new();
        assert_eq!(graph_max(&empty, 80, true), 1);
        let zeros: VecDeque<i64> = VecDeque::from([0, 0, 0]);
        assert_eq!(graph_max(&zeros, 80, true), 1);
    }

    #[test]
    fn auto_scale_changes_rendered_graph_for_low_values() {
        // With fixed 0-100 scale, low values render as a mostly-empty
        // graph. With auto-scale, the same low values fill the box.
        // The two outputs must differ.
        let area = make_area();
        let theme = Theme::default();
        let mut cpu = CpuInfo::default();
        // Low-percentage data so the rescale is dramatic.
        let low: VecDeque<i64> = VecDeque::from([3, 5, 4, 6, 5, 4, 3, 5]);
        cpu.cpu_percent.user = low.clone();
        cpu.cpu_percent.system = low;

        let mut s_off = make_frame();
        s_off.auto_scale = false;
        let out_off = draw(&cpu, &area, &theme, &s_off, &CollectStatus::Ok);

        let mut s_on = make_frame();
        s_on.auto_scale = true;
        let out_on = draw(&cpu, &area, &theme, &s_on, &CollectStatus::Ok);

        assert_ne!(
            out_off, out_on,
            "auto_scale=true must produce different graph output \
             than auto_scale=false on a low-value data series"
        );
    }
}
