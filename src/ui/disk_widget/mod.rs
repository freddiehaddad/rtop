use crate::collect::CollectStatus;
use crate::domain::disk::DiskInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::GraphMode;
use crate::theme::Theme;
use crate::theme_keys as tc;

use super::WidgetArea;

mod io_view;
mod sizing;
mod usage_view;

pub use sizing::preferred_height;

use io_view::IoRowParams;
use usage_view::{PerfRowParams, UsageRowParams};

/// Per-frame view passed to [`draw`].
///
/// `graph_symbol` is pre-resolved from `DiskConfig::graph_symbol_disk +
/// UiConfig::graph_symbol` (per-widget override falls back to the
/// global default). `io_mode` lives in `RuntimeView` (it's a runtime
/// toggle); the rest mirror `DiskConfig` / `UiConfig` fields.
pub struct DiskFrame {
    pub graph_symbol: GraphMode,
    pub base_10: bool,
    pub show_io_stat: bool,
    pub io_mode: bool,
    pub disk_io_mode: bool,
    pub io_graph_combined: bool,
}

/// Draw the disk widget into an ANSI string.
///
/// `disks` is the post-filter slice of disks the caller wants rendered,
/// in display order. Filtering (via `DiskFilter`) and the resulting
/// height sizing happen at the call site so the renderer stays a pure
/// function of (data, settings, theme).
///
/// Layout (default, `show_io_stat=true`, `io_mode=false`):
/// ╭─┐⁵disk┌──────────────────────────────────────────────────────────╮
/// │ C: NTFS ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■ 286G/1.6T │
/// │ R 0.0B/s ⣀⣀⣸⣇⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ W  52K/s ⣀⣀⣸⣇⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ B   0% │
/// │ S: NTFS ■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■■ 83G/1024G │
/// │ R 0.0B/s ⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ W 0.0B/s ⣀⣀⣀⣸⣇⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣠⣄ B   0% │
/// ╰─┘io└─────────────────────────────────────────────────────────────╯
///
/// IO mode (`io_mode=true`, `io_graph_combined=false`): each disk
/// becomes two rows, one per direction. The R/W letter sits on the
/// right next to the speed value (matching the perf-row column
/// order); the graph fills the variable space between drive label
/// and letter. The bottom-border `io` inset gains a trailing `*`
/// to signal the toggle is active.
/// ╭─┐⁵disk┌──────────────────────────────────────────────────────────╮
/// │ C: ⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ R 0.0B/s │
/// │ C: ⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣸⣇⣀⣀⣀⣀⣀ W 0.0B/s │
/// ╰─┘io*└────────────────────────────────────────────────────────────╯
pub fn draw(
    disks: &[&DiskInfo],
    area: &WidgetArea,
    theme: &Theme,
    settings: &DiskFrame,
    status: &CollectStatus,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let border_color = theme.color(tc::DISK_WIDGET);
    let title_color = theme.color(tc::TITLE);
    let hi = theme.color(tc::HI_FG);
    let avail_grad = theme.gradient(tc::GRAD_AVAILABLE);
    let read_grad = theme.gradient(tc::GRAD_DISK_READ);
    let write_grad = theme.gradient(tc::GRAD_DISK_WRITE);
    let busy_grad = theme.gradient(tc::GRAD_DISK_BUSY);
    let meter_bg = theme.color(tc::METER_BG);

    let inner_h = height.saturating_sub(2);
    let inner_w = width.saturating_sub(4);
    let content_x = x + 3;

    let mut buf = AnsiBuffer::new();
    buf.text(&box_drawing::create_box(&box_drawing::BoxConfig {
        x,
        y,
        width,
        height,
        line_color: border_color,
        fill: true,
        title: "disk",
        title2: "",
        num: super::DISK_KEY,
        rounded,
        hi_color: hi,
        title_color,
    }));

    super::draw_status_inset(&mut buf, status, "disk", x, y, border_color, title_color);

    let io_view = settings.io_mode || settings.disk_io_mode;

    // Bottom-border keybind hint: `io` (with `i` highlighted) when
    // showing usage, `io*` when the IO view is active. Mirrors the
    // proc-widget `tree` / `tre*e` convention for binary toggles.
    let star = if io_view { "*" } else { "" };
    let io_text = format!("{hi}i{title_color}o{star}");
    let io_inset = box_drawing::title_inset(&io_text, border_color, title_color, true);
    buf.mv(x + 3, y + height).text(&io_inset);

    let mut row = 0;

    for disk in disks {
        if row >= inner_h {
            break;
        }

        if io_view {
            let io_params = IoRowParams {
                content_x,
                row_y: y + 2 + row,
                inner_w,
                inner_h_remaining: inner_h - row,
                theme,
                settings,
                read_grad,
                write_grad,
            };
            row += if settings.io_graph_combined {
                io_view::draw_combined_row(&mut buf, disk, &io_params)
            } else {
                io_view::draw_separate_rows(&mut buf, disk, &io_params)
            };
        } else {
            let usage_params = UsageRowParams {
                content_x,
                row_y: y + 2 + row,
                inner_w,
                theme,
                settings,
                avail_grad,
                meter_bg,
            };
            row += usage_view::draw_usage_row(&mut buf, disk, &usage_params);

            if settings.show_io_stat && row < inner_h {
                let perf_params = PerfRowParams {
                    content_x,
                    row_y: y + 2 + row,
                    inner_w,
                    theme,
                    settings,
                    read_grad,
                    write_grad,
                    busy_grad,
                };
                row += usage_view::draw_perf_row(&mut buf, disk, &perf_params);
            }
        }
    }

    buf.finish()
}

// ---------------------------------------------------------------------------
// Widget impl
// ---------------------------------------------------------------------------

/// Disk widget renderer. Unit struct — the widget has no per-
/// instance state.
pub struct DiskWidget;

impl super::Widget for DiskWidget {
    fn kinds(&self) -> &'static [crate::domain::widget_kind::WidgetKind] {
        const KINDS: &[crate::domain::widget_kind::WidgetKind] =
            &[crate::domain::widget_kind::WidgetKind::Disk];
        KINDS
    }

    fn preferred_height(&self, hints: &crate::draw::layout::LayoutHints) -> usize {
        preferred_height(hints)
    }

    fn min_width(&self, _hints: &crate::draw::layout::LayoutHints) -> usize {
        crate::draw::layout::MIN_MEM_WIDTH
    }

    fn min_height(&self, hints: &crate::draw::layout::LayoutHints) -> usize {
        preferred_height(hints)
    }

    fn render(&self, params: &crate::app::RenderParams<'_>, output: &mut String) {
        let Some(disk_dim) = params
            .layout
            .dims_for(crate::domain::widget_kind::WidgetKind::Disk)
        else {
            return;
        };
        let Some(disk) = params.disk else {
            return;
        };
        let area = super::WidgetArea::from_dim(disk_dim, params.rounded);
        let frame = DiskFrame {
            graph_symbol: crate::draw::graph::GraphMode::from_config(
                params.config.disk.graph_symbol_disk,
                params.config.ui.graph_symbol,
            ),
            base_10: params.config.ui.base_10_sizes,
            show_io_stat: params.config.disk.show_io_stat,
            io_mode: params.view.io_mode,
            disk_io_mode: params.config.disk.disk_io_mode,
            io_graph_combined: params.config.disk.io_graph_combined,
        };
        let filter = crate::domain::disk::DiskFilter::parse(&params.config.disk.disk_filter);
        let visible = filter.apply(&disk.info.disks);
        output.push_str(&draw(&visible, &area, params.theme, &frame, &disk.status));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::disk::{DiskData, DiskInfo};
    use crate::draw::graph::GraphMode;
    use crate::draw::layout::{LayoutHints, MIN_DISK_HEIGHT};

    #[test]
    fn preferred_height_uses_two_rows_per_disk_when_io_stat_shown() {
        let hints = LayoutHints {
            disk_count: 3,
            disk_rows_per_unit: 2,
            ..Default::default()
        };
        // 3 disks * 2 rows + 2 borders = 8.
        assert_eq!(preferred_height(&hints), 8);
    }

    #[test]
    fn preferred_height_uses_one_row_per_disk_when_io_stat_hidden() {
        let hints = LayoutHints {
            disk_count: 3,
            disk_rows_per_unit: 1,
            ..Default::default()
        };
        // 3 disks * 1 row + 2 borders = 5, floored at MIN_DISK_HEIGHT.
        assert_eq!(preferred_height(&hints), 5.max(MIN_DISK_HEIGHT));
    }

    #[test]
    fn preferred_height_floors_at_min_disk_height() {
        let hints = LayoutHints {
            disk_count: 0,
            disk_rows_per_unit: 2,
            ..Default::default()
        };
        assert_eq!(preferred_height(&hints), MIN_DISK_HEIGHT);
    }

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

    const GIB: u64 = 1024 * 1024 * 1024;

    fn make_disk_data() -> DiskData {
        DiskData {
            disks: vec![
                DiskInfo {
                    name: "C:".into(),
                    fstype: "NTFS".into(),
                    total: 500 * GIB,
                    used: 250 * GIB,
                    used_percent: 50,
                    read_bytes_per_sec: 42 * 1024 * 1024,
                    write_bytes_per_sec: 8 * 1024 * 1024,
                    read_top: 100 * 1024 * 1024,
                    write_top: 40 * 1024 * 1024,
                    busy_percent: 12,
                    read_history: [0, 10, 42, 21, 8].into_iter().collect(),
                    write_history: [0, 4, 8, 2, 1].into_iter().collect(),
                },
                DiskInfo {
                    name: "D:".into(),
                    fstype: "NTFS".into(),
                    total: 1000 * GIB,
                    used: 300 * GIB,
                    used_percent: 30,
                    read_bytes_per_sec: 1024 * 1024,
                    write_bytes_per_sec: 0,
                    read_top: 10 * 1024 * 1024,
                    write_top: 1,
                    busy_percent: 0,
                    read_history: [0, 1, 0, 1, 0].into_iter().collect(),
                    write_history: [0, 0, 0, 0, 0].into_iter().collect(),
                },
            ],
        }
    }

    fn make_area() -> WidgetArea {
        WidgetArea {
            x: 1,
            y: 1,
            width: 40,
            height: 12,
            rounded: true,
        }
    }

    fn frame() -> DiskFrame {
        DiskFrame {
            graph_symbol: GraphMode::Braille,
            base_10: false,
            show_io_stat: true,
            io_mode: false,
            disk_io_mode: false,
            io_graph_combined: false,
        }
    }

    /// Borrow every disk in the test fixture into the `&[&DiskInfo]`
    /// shape that `draw` consumes after filtering. Production callers
    /// build the same shape via `DiskFilter::apply`; tests want the
    /// unfiltered set.
    fn all_disks(data: &DiskData) -> Vec<&DiskInfo> {
        data.disks.iter().collect()
    }

    #[test]
    fn draw_contains_disk_title() {
        let data = make_disk_data();
        let output = draw(
            &all_disks(&data),
            &make_area(),
            &Theme::default(),
            &frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("disk"), "output should contain 'disk' title");
    }

    #[test]
    fn io_inset_inactive_renders_io_with_keybind_colour() {
        // Stateless `io` inset: `i` highlighted in HI, `o` in TITLE,
        // no trailing `*`. Mirrors the proc:tree convention for a
        // binary toggle that is currently OFF.
        let data = make_disk_data();
        let theme = Theme::default();
        let output = draw(
            &all_disks(&data),
            &make_area(),
            &theme,
            &frame(),
            &CollectStatus::Ok,
        );
        let title = theme.color(tc::TITLE);
        let hi = theme.color(tc::HI_FG);

        // `i` is preceded by HI (the keybind colour).
        assert!(
            output.contains(&format!("{hi}i")),
            "keybind 'i' should render in HI colour"
        );
        // `o` is preceded by TITLE (the label colour).
        assert!(
            output.contains(&format!("{title}o")),
            "label 'o' should render in TITLE colour"
        );
        // No `*` marker when the IO view is inactive.
        let plain = strip_ansi(&output);
        assert!(
            !plain.contains("io*"),
            "no '*' marker should appear when io_view is inactive"
        );
    }

    #[test]
    fn io_inset_active_appends_star_marker() {
        // When io_mode is on, the inset becomes `io*`. The `*` is
        // in TITLE colour (it follows the embedded title-colour
        // switch after `i`), and the trailing label is `o*`.
        let data = make_disk_data();
        let theme = Theme::default();
        let mut s = frame();
        s.io_mode = true;
        let output = draw(
            &all_disks(&data),
            &make_area(),
            &theme,
            &s,
            &CollectStatus::Ok,
        );
        let title = theme.color(tc::TITLE);
        let hi = theme.color(tc::HI_FG);

        assert!(
            output.contains(&format!("{hi}i")),
            "keybind 'i' should render in HI colour"
        );
        assert!(
            output.contains(&format!("{title}o*")),
            "label 'o*' should render in TITLE colour with the star marker"
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("io*"),
            "visible text should contain 'io*' when io_view is active"
        );
    }

    #[test]
    fn io_inset_marks_active_when_disk_io_mode_persistent_flag_set() {
        // The `*` marker tracks the live IO view, which is
        // (io_mode || disk_io_mode). With only disk_io_mode set,
        // the marker should still appear so the user can see the
        // view is active.
        let data = make_disk_data();
        let theme = Theme::default();
        let mut s = frame();
        s.disk_io_mode = true;
        let output = draw(
            &all_disks(&data),
            &make_area(),
            &theme,
            &s,
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("io*"),
            "io marker should appear when disk_io_mode is set, even with io_mode=false"
        );
    }

    #[test]
    fn draw_contains_drive_letters() {
        let data = make_disk_data();
        let output = draw(
            &all_disks(&data),
            &make_area(),
            &Theme::default(),
            &frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("C:"), "output should contain 'C:'");
        assert!(plain.contains("D:"), "output should contain 'D:'");
    }

    #[test]
    fn draw_contains_filesystem_type() {
        let data = make_disk_data();
        let output = draw(
            &all_disks(&data),
            &make_area(),
            &Theme::default(),
            &frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("NTFS"),
            "output should contain filesystem type 'NTFS'"
        );
    }

    #[test]
    fn draw_contains_disk_perf_labels() {
        let data = make_disk_data();
        let output = draw(
            &all_disks(&data),
            &make_area(),
            &Theme::default(),
            &frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("R "), "output should contain read label");
        assert!(plain.contains("W "), "output should contain write label");
        assert!(
            plain.contains("B") && plain.contains("12%"),
            "output should contain busy label"
        );
    }

    #[test]
    fn normal_mode_usage_value_uses_avail_gradient() {
        // Defends Option A: the usage value cell takes the meter's gradient
        // (avail_grad) at used_percent. Pre-fix it was MAIN_FG while the
        // meter rendered in colour.
        let theme = Theme::default();
        let area = WidgetArea {
            x: 1,
            y: 1,
            width: 40,
            height: 12,
            rounded: true,
        };
        let data = make_disk_data();
        let output = draw(
            &all_disks(&data),
            &area,
            &theme,
            &frame(),
            &CollectStatus::Ok,
        );
        let avail_grad = theme.gradient(tc::GRAD_AVAILABLE);
        // C: used_percent = 50 → "250G/500G" rjust(10) = " 250G/500G"
        let expected_c = format!("{}{}", avail_grad[50], " 250G/500G");
        assert!(
            output.contains(&expected_c),
            "C: usage value should be GRAD_AVAILABLE[50] adjacent to ' 250G/500G'"
        );
        // D: used_percent = 30 → "300G/1000G" (10 chars, no leading space)
        let expected_d = format!("{}{}", avail_grad[30], "300G/1000G");
        assert!(
            output.contains(&expected_d),
            "D: usage value should be GRAD_AVAILABLE[30] adjacent to '300G/1000G'"
        );
    }

    #[test]
    fn perf_row_speeds_and_busy_use_their_gradients() {
        // Defends Option A: R/W speeds are coloured by their gradients at
        // pct of the row's graph max (visible_graph_max), not lifetime
        // peaks. Busy was already gradient. Pre-fix only busy was coloured.
        let theme = Theme::default();
        let data = make_disk_data();
        let output = draw(
            &all_disks(&data),
            &make_area(),
            &theme,
            &frame(),
            &CollectStatus::Ok,
        );
        let read_grad = theme.gradient(tc::GRAD_DISK_READ);
        let write_grad = theme.gradient(tc::GRAD_DISK_WRITE);
        let busy_grad = theme.gradient(tc::GRAD_DISK_BUSY);

        // read_history is tiny ints; current = 42 MB/s dwarfs them; so
        // visible_graph_max = current → pct = 100. Expected: "  42M/s"
        // ("42M/s" is 5 chars, rjusted to IO_SPEED_W = 7 yields
        // 2 leading spaces).
        let expected_r = format!("{}{}", read_grad[100], "  42M/s");
        assert!(
            output.contains(&expected_r),
            "perf-row R speed should be GRAD_DISK_READ[100] adjacent to '  42M/s'"
        );

        // Same logic for W — current 8 MB/s dominates the history window.
        // "8.0M/s" is 6 chars, rjusted to 7 yields 1 leading space.
        let expected_w = format!("{}{}", write_grad[100], " 8.0M/s");
        assert!(
            output.contains(&expected_w),
            "perf-row W speed should be GRAD_DISK_WRITE[100] adjacent to ' 8.0M/s'"
        );

        // Busy: 12 % → format!("{:>5}", "12%") = "  12%".
        let expected_b = format!("{}{}", busy_grad[12], "  12%");
        assert!(
            output.contains(&expected_b),
            "busy value should be GRAD_DISK_BUSY[12] adjacent to '  12%'"
        );
    }

    #[test]
    fn perf_row_graph_position_constant_across_speed_widths() {
        // Regression: pre-fix, the R/W graphs would shift by one column
        // when the speed-string width changed (e.g. "30K/s" 5 chars vs
        // "0.0B/s" 6 chars). After the rjust-to-IO_SPEED_W fix the
        // R, W, and B letters land at the same column on every row.
        let theme = Theme::default();
        let data = DiskData {
            disks: vec![
                // Row 1: speeds that would render as 5-char raw strings
                // ("30K/s", "8.0M/s") — wait, "8.0M/s" is 6 chars. Let
                // us pick widths deliberately:
                //   read = 30 KiB/s -> "30K/s" (5 chars)
                //   write = 0       -> "0.0B/s" (6 chars)
                DiskInfo {
                    name: "X:".into(),
                    fstype: "NTFS".into(),
                    total: 100 * GIB,
                    used: 50 * GIB,
                    used_percent: 50,
                    read_bytes_per_sec: 30 * 1024,
                    write_bytes_per_sec: 0,
                    read_top: 100 * 1024,
                    write_top: 1,
                    busy_percent: 0,
                    read_history: [0, 30, 0, 30, 0].into_iter().collect(),
                    write_history: [0; 5].into_iter().collect(),
                },
                // Row 2: both speeds are 6-char raw strings.
                DiskInfo {
                    name: "Y:".into(),
                    fstype: "NTFS".into(),
                    total: 100 * GIB,
                    used: 25 * GIB,
                    used_percent: 25,
                    read_bytes_per_sec: 0,
                    write_bytes_per_sec: 0,
                    read_top: 1,
                    write_top: 1,
                    busy_percent: 0,
                    read_history: [0; 5].into_iter().collect(),
                    write_history: [0; 5].into_iter().collect(),
                },
            ],
        };
        let output = draw(
            &all_disks(&data),
            &make_area(),
            &theme,
            &frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);

        // strip_ansi drops the term::mv positioning codes so the
        // rendered chunks concatenate; we cannot rely on `lines()`.
        // Instead, find all R/W/B labels in the flat output and
        // assert the R→W and W→B distances are identical across the
        // two perf rows. Those distances depend only on the layout
        // constants (`fixed_left`, `read_graph_w`, `write_graph_w`),
        // not on the speed values — so the regression of "W graph
        // shifts when speed width changes" would manifest as
        // mismatched distances. The "R "/"W "/"B " patterns are
        // unique to the perf-row labels for the test fixture's drive
        // names ("X:", "Y:") which contain no R/W/B characters.
        let r_positions: Vec<usize> = plain.match_indices("R ").map(|(i, _)| i).collect();
        let w_positions: Vec<usize> = plain.match_indices("W ").map(|(i, _)| i).collect();
        let b_positions: Vec<usize> = plain.match_indices("B ").map(|(i, _)| i).collect();
        assert_eq!(
            r_positions.len(),
            2,
            "expected 2 'R ' labels (one per disk), got {}: {:?}",
            r_positions.len(),
            r_positions
        );
        assert_eq!(
            w_positions.len(),
            2,
            "expected 2 'W ' labels, got {}: {:?}",
            w_positions.len(),
            w_positions
        );
        assert_eq!(
            b_positions.len(),
            2,
            "expected 2 'B ' labels, got {}: {:?}",
            b_positions.len(),
            b_positions
        );
        assert_eq!(
            w_positions[0] - r_positions[0],
            w_positions[1] - r_positions[1],
            "W column must be at the same offset from R on every row \
             (regression: pre-fix this shifted by one when the speed \
             string changed width)"
        );
        assert_eq!(
            b_positions[0] - w_positions[0],
            b_positions[1] - w_positions[1],
            "B column must be at the same offset from W on every row"
        );
    }

    #[test]
    fn io_separate_rows_position_constant_across_speed_widths() {
        // Regression: in IO separate-row mode (toggled with `i`),
        // the R/W letter and speed value live on the right edge of
        // each row. They must stay at fixed columns regardless of
        // the speed-string width — the same property the perf-row
        // mode delivers via the IO_SPEED_W rjust.
        let theme = Theme::default();
        let area = WidgetArea {
            x: 1,
            y: 1,
            width: 60,
            height: 12,
            rounded: true,
        };
        let s = DiskFrame {
            graph_symbol: GraphMode::Braille,
            base_10: false,
            show_io_stat: false,
            io_mode: true,
            disk_io_mode: false,
            io_graph_combined: false,
        };
        let data = DiskData {
            disks: vec![
                // Read = 30 KiB/s -> "30K/s" (5 chars) ; write = 0 -> "0.0B/s" (6 chars)
                DiskInfo {
                    name: "X:".into(),
                    fstype: "NTFS".into(),
                    total: 100 * GIB,
                    used: 50 * GIB,
                    used_percent: 50,
                    read_bytes_per_sec: 30 * 1024,
                    write_bytes_per_sec: 0,
                    read_top: 100 * 1024,
                    write_top: 1,
                    busy_percent: 0,
                    read_history: [0, 30, 0, 30, 0].into_iter().collect(),
                    write_history: [0; 5].into_iter().collect(),
                },
                // Read = 0 -> "0.0B/s" (6 chars) ; write = 200 -> "200B/s" (6 chars)
                DiskInfo {
                    name: "Y:".into(),
                    fstype: "NTFS".into(),
                    total: 100 * GIB,
                    used: 25 * GIB,
                    used_percent: 25,
                    read_bytes_per_sec: 0,
                    write_bytes_per_sec: 200,
                    read_top: 1,
                    write_top: 1,
                    busy_percent: 0,
                    read_history: [0; 5].into_iter().collect(),
                    write_history: [0, 0, 0, 0, 200].into_iter().collect(),
                },
            ],
        };
        let output = draw(&all_disks(&data), &area, &theme, &s, &CollectStatus::Ok);
        let plain = strip_ansi(&output);

        // Each disk renders two rows in IO separate mode: one " R "
        // and one " W " (note the leading space — the layout is
        // `label graph " R" rjust(speed)`). With two disks we expect
        // 2 of each.
        let r_positions: Vec<usize> = plain.match_indices(" R ").map(|(i, _)| i).collect();
        let w_positions: Vec<usize> = plain.match_indices(" W ").map(|(i, _)| i).collect();
        assert_eq!(
            r_positions.len(),
            2,
            "expected 2 ' R ' labels (one per disk), got {}: {:?}",
            r_positions.len(),
            r_positions
        );
        assert_eq!(
            w_positions.len(),
            2,
            "expected 2 ' W ' labels (one per disk), got {}: {:?}",
            w_positions.len(),
            w_positions
        );

        // The R-to-W and consecutive-row-to-row distances depend on
        // the row width (constant) and rjust-padded speed columns
        // (also constant via IO_SPEED_W), so they must be identical
        // across disks. A regression in the rjust target would
        // manifest as mismatched distances.
        assert_eq!(
            w_positions[0] - r_positions[0],
            w_positions[1] - r_positions[1],
            "W must be at the same offset from R on every disk \
             (regression: the row width or speed-column width drifted)"
        );
    }

    #[test]
    fn io_combined_value_uses_read_gradient() {
        // The combined IO row renders one graph (using read_grad) and one
        // value cell. The value takes read_grad at the combined pct
        // (against the same max the graph row uses).
        let theme = Theme::default();
        let area = WidgetArea {
            x: 1,
            y: 1,
            width: 60,
            height: 8,
            rounded: true,
        };
        let s = DiskFrame {
            graph_symbol: GraphMode::Braille,
            base_10: false,
            show_io_stat: false,
            io_mode: true,
            disk_io_mode: false,
            io_graph_combined: true,
        };
        let data = make_disk_data();
        let output = draw(&all_disks(&data), &area, &theme, &s, &CollectStatus::Ok);
        let read_grad = theme.gradient(tc::GRAD_DISK_READ);
        // C: r = 42 MB/s, w = 8 MB/s → combined 50 MB/s, max from current
        // ≥ history window → pct = 100. Combined value text: "R42M/s W8.0M/s".
        let expected = format!("{}{}", read_grad[100], "R42M/s W8.0M/s");
        // combined value is rjust to IO_COMBINED_VAL_W = 19 → 5 leading spaces.
        let expected_padded = format!("{}     {}", read_grad[100], "R42M/s W8.0M/s");
        assert!(
            output.contains(&expected) || output.contains(&expected_padded),
            "combined IO value should be GRAD_DISK_READ[100] adjacent to 'R42M/s W8.0M/s' (with leading rjust padding)"
        );
    }

    #[test]
    fn body_labels_use_main_fg() {
        // Body label rule: drive labels (C: NTFS) and perf row labels (R, W,
        // B) render in MAIN_FG. Pre-shift these were TITLE.
        let theme = Theme::default();
        let data = make_disk_data();
        let output = draw(
            &all_disks(&data),
            &make_area(),
            &theme,
            &frame(),
            &CollectStatus::Ok,
        );
        let fg = theme.color(tc::MAIN_FG);
        assert!(
            output.contains(&format!("{fg}C: NTFS ")),
            "drive label 'C: NTFS' should be preceded by MAIN_FG"
        );
        // Perf row R/W/B labels — each rendered with .color(fg).text("R") etc.
        assert!(
            output.contains(&format!("{fg}R")),
            "perf row 'R' label should be preceded by MAIN_FG"
        );
        assert!(
            output.contains(&format!("{fg}W")),
            "perf row 'W' label should be preceded by MAIN_FG"
        );
        assert!(
            output.contains(&format!("{fg}B")),
            "perf row 'B' label should be preceded by MAIN_FG"
        );
    }

    #[test]
    fn draw_with_disk_filter_renders_only_matching_drives() {
        // End-to-end: the same code path the app uses — parse the user's
        // disk_filter, apply it to the live disk list, hand the borrowed
        // result to draw. The renderer must show only the matching drives
        // and no ghost rows for filtered-out drives.
        use crate::domain::disk::DiskFilter;

        let data = make_disk_data();
        let filter = DiskFilter::parse(&["C:".to_string()]);
        let visible = filter.apply(&data.disks);
        assert_eq!(visible.len(), 1);

        let output = draw(
            &visible,
            &make_area(),
            &Theme::default(),
            &frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("C:"), "C: row should be rendered");
        assert!(
            !plain.contains("D: NTFS"),
            "D: row must not be rendered when filter excludes it"
        );
    }

    #[test]
    fn draw_with_exclude_filter_hides_listed_drives() {
        use crate::domain::disk::DiskFilter;

        let data = make_disk_data();
        let filter = DiskFilter::parse(&["!C:".to_string()]);
        let visible = filter.apply(&data.disks);
        assert_eq!(visible.len(), 1);

        let output = draw(
            &visible,
            &make_area(),
            &Theme::default(),
            &frame(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            !plain.contains("C: NTFS"),
            "C: row must not be rendered when excluded"
        );
        assert!(plain.contains("D:"), "D: row should be rendered");
    }
}
