use crate::collect::CollectStatus;
use crate::domain::disk::DiskInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::{Graph, GraphMode};
use crate::draw::meter::Meter;
use crate::theme::{Theme, gradient_color};
use crate::theme_keys as tc;
use crate::tools;

use super::WidgetArea;

// --- Layout constants ---

/// Right-aligned value column width for normal mode ("274G/1.6T" = up to 10 chars).
const USAGE_VAL_W: usize = 10;
/// Right-aligned value column for IO combined rows
/// (1 space + max "R1023B/s W1023B/s" = 18 chars).
const IO_COMBINED_VAL_W: usize = 19;
/// Right-aligned width for a shortened per-second speed value in
/// the IO display modes (perf-row mode and IO separate-row mode).
/// `floating_humanizer(_, shorten=true, _, _, true, _)` returns at
/// most 6 characters (e.g. `0.0B/s`, `999K/s`); the `+1` guarantees
/// at least one leading space between the `R`/`W` letter and the
/// value, mirroring the `B` column's `max_pct_width + 1` rjust
/// target. Without this fixed width the graphs visually shift when
/// the speed value's character count changes.
const IO_SPEED_W: usize = 7;
/// Minimum graph/meter width.
const MIN_METER_W: usize = 5;
/// Minimum graph width in IO mode.
const MIN_IO_GRAPH_W: usize = 3;

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

/// Preferred intrinsic height for the disk widget given the
/// snapshot hints, in rows (including borders).
///
/// Each disk reserves [`hints.disk_rows_per_unit`] rows (1 for
/// capacity-only or combined-IO; 2 for capacity-with-inline-IO or
/// split-graph IO view). Plus 2 border rows, with a floor of
/// [`crate::draw::layout::MIN_DISK_HEIGHT`].
pub fn preferred_height(hints: &crate::draw::layout::LayoutHints) -> usize {
    let content_rows = hints.disk_count * hints.disk_rows_per_unit as usize;
    (content_rows + 2).max(crate::draw::layout::MIN_DISK_HEIGHT)
}

/// Draw the disk widget into an ANSI string.
///
/// `disks` is the post-filter slice of disks the caller wants rendered,
/// in display order. Filtering (via `DisksFilter`) and the resulting
/// height sizing happen at the call site so the renderer stays a pure
/// function of (data, settings, theme).
///
/// Layout (default, `show_io_stat=true`, `io_mode=false`):
/// ╭─┐⁵disks┌─────────────────────────────────────────────────────────╮
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
/// ╭─┐⁵disks┌─────────────────────────────────────────────────────────╮
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
    let fg = theme.color(tc::MAIN_FG);
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
        title: "disks",
        title2: "",
        num: super::DISK_KEY,
        rounded,
        hi_color: hi,
        title_color,
    }));

    super::draw_status_inset(&mut buf, status, "disks", x, y, border_color, title_color);

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
            // I/O mode: show throughput graphs instead of usage meters
            // Fixed value column: 1 space + max speed width (6 chars for shortened speeds)

            if settings.io_graph_combined {
                // Combined value: "R9.9M/s W9.9M/s" = up to 16 chars + 1 space

                // Combined read+write in a single graph row
                let label = format!("{} IO ", disk.name);
                let label_len = tools::ulen(&label, false);
                let graph_w = inner_w
                    .saturating_sub(label_len + IO_COMBINED_VAL_W)
                    .max(MIN_IO_GRAPH_W);

                let speed_r = tools::floating_humanizer(
                    disk.read_bytes_per_sec,
                    true,
                    0,
                    false,
                    true,
                    settings.base_10,
                );
                let speed_w = tools::floating_humanizer(
                    disk.write_bytes_per_sec,
                    true,
                    0,
                    false,
                    true,
                    settings.base_10,
                );
                let value = format!("R{} W{}", speed_r, speed_w);

                // Merge read+write histories by summing
                let combined: std::collections::VecDeque<i64> = {
                    let rlen = disk.read_history.len();
                    let wlen = disk.write_history.len();
                    let max_len = rlen.max(wlen);
                    let mut combined = std::collections::VecDeque::with_capacity(max_len);
                    for i in 0..max_len {
                        let r = if i < rlen { disk.read_history[i] } else { 0 };
                        let w = if i < wlen { disk.write_history[i] } else { 0 };
                        combined.push_back(r + w);
                    }
                    combined
                };

                let max = visible_graph_max(
                    &combined,
                    graph_w,
                    disk.read_bytes_per_sec + disk.write_bytes_per_sec,
                );
                let mut graph = Graph::new(graph_w, 1, settings.graph_symbol, false, max, 0);
                let graph_row = graph.render_row(&combined, read_grad);

                let combined_speed = disk.read_bytes_per_sec + disk.write_bytes_per_sec;
                let combined_pct =
                    ((combined_speed as i64).saturating_mul(100) / max).min(100) as i32;
                buf.mv(content_x, y + 2 + row)
                    .color(fg)
                    .text(&label)
                    .text(&graph_row)
                    .color(gradient_color(read_grad, combined_pct))
                    .text(&tools::rjust(&value, IO_COMBINED_VAL_W, false));
                row += 1;
            } else {
                // Separate read and write graph rows. The R/W
                // letter sits on the right next to the speed value
                // (matching the perf-row column order); the graph
                // fills the variable space between drive label and
                // ` R`/` W` letter. The 2-char gap (one trailing
                // space after the drive label, one leading space
                // before the letter) provides visual separation.
                let label = format!("{} ", disk.name);
                let label_len = tools::ulen(&label, false);
                // Right column: " R" or " W" (2 chars) + rjust(speed, IO_SPEED_W).
                let value_col_w = 2 + IO_SPEED_W;
                let graph_w = inner_w
                    .saturating_sub(label_len + value_col_w)
                    .max(MIN_IO_GRAPH_W);

                // Read row
                let speed_r = tools::floating_humanizer(
                    disk.read_bytes_per_sec,
                    true,
                    0,
                    false,
                    true,
                    settings.base_10,
                );

                let read_max =
                    visible_graph_max(&disk.read_history, graph_w, disk.read_bytes_per_sec);
                let mut rg = Graph::new(graph_w, 1, settings.graph_symbol, false, read_max, 0);
                let rg_row = rg.render_row(&disk.read_history, read_grad);

                let read_pct = ((disk.read_bytes_per_sec as i64).saturating_mul(100) / read_max)
                    .min(100) as i32;
                buf.mv(content_x, y + 2 + row)
                    .color(fg)
                    .text(&label)
                    .text(&rg_row)
                    .color(fg)
                    .text(" R")
                    .color(gradient_color(read_grad, read_pct))
                    .text(&tools::rjust(&speed_r, IO_SPEED_W, false));
                row += 1;

                if row >= inner_h {
                    continue;
                }

                // Write row
                let speed_w = tools::floating_humanizer(
                    disk.write_bytes_per_sec,
                    true,
                    0,
                    false,
                    true,
                    settings.base_10,
                );

                let write_max =
                    visible_graph_max(&disk.write_history, graph_w, disk.write_bytes_per_sec);
                let mut wg = Graph::new(graph_w, 1, settings.graph_symbol, false, write_max, 0);
                let wg_row = wg.render_row(&disk.write_history, write_grad);

                let write_pct = ((disk.write_bytes_per_sec as i64).saturating_mul(100) / write_max)
                    .min(100) as i32;
                buf.mv(content_x, y + 2 + row)
                    .color(fg)
                    .text(&label)
                    .text(&wg_row)
                    .color(fg)
                    .text(" W")
                    .color(gradient_color(write_grad, write_pct))
                    .text(&tools::rjust(&speed_w, IO_SPEED_W, false));
                row += 1;
            }
        } else {
            // Normal mode: usage meter
            let du = tools::floating_humanizer(disk.used, true, 0, false, false, settings.base_10);
            let dt = tools::floating_humanizer(disk.total, true, 0, false, false, settings.base_10);
            let value = format!("{}/{}", du, dt);

            // Label: "C: NTFS " — drive + fstype
            let label = if disk.fstype.is_empty() {
                format!("{} ", disk.name)
            } else {
                format!("{} {} ", disk.name, disk.fstype)
            };
            let label_len = label.len();
            let meter_w = inner_w
                .saturating_sub(label_len + USAGE_VAL_W)
                .max(MIN_METER_W);
            let disk_meter = Meter::new(meter_w, avail_grad, meter_bg);

            buf.mv(content_x, y + 2 + row)
                .color(fg)
                .text(&label)
                .text(disk_meter.render(disk.used_percent))
                .color(gradient_color(avail_grad, disk.used_percent))
                .text(&tools::rjust(&value, USAGE_VAL_W, false));
            row += 1;

            if settings.show_io_stat && row < inner_h {
                let params = PerfRowParams {
                    content_x,
                    row_y: y + 2 + row,
                    inner_w,
                    theme,
                    settings,
                    read_grad,
                    write_grad,
                    busy_grad,
                };
                draw_perf_row(&mut buf, disk, &params);
                row += 1;
            }
        }
    }

    buf.finish()
}

struct PerfRowParams<'a> {
    content_x: usize,
    row_y: usize,
    inner_w: usize,
    theme: &'a Theme,
    settings: &'a DiskFrame,
    read_grad: &'a [String],
    write_grad: &'a [String],
    busy_grad: &'a [String],
}

fn draw_perf_row(
    buf: &mut AnsiBuffer,
    disk: &crate::domain::disk::DiskInfo,
    params: &PerfRowParams,
) {
    let x = params.content_x;
    let y = params.row_y;
    let width = params.inner_w;
    if width == 0 {
        return;
    }

    let base_10 = params.settings.base_10;
    let fg = params.theme.color(tc::MAIN_FG);
    let read_speed =
        tools::floating_humanizer(disk.read_bytes_per_sec, true, 0, false, true, base_10);
    let write_speed =
        tools::floating_humanizer(disk.write_bytes_per_sec, true, 0, false, true, base_10);
    let busy = disk.busy_percent.clamp(0, 100);
    let busy_w: usize = 6; // "B" + 5-char right-justified value
    let busy_x = x + width.saturating_sub(busy_w);
    let left_w = width.saturating_sub(busy_w);

    // Constant column widths so the graph never shifts when the
    // speed-string width changes. Mirrors the `B` column pattern:
    // letter + rjust(value, max_value_width + 1) — the +1 inside
    // IO_SPEED_W guarantees at least one leading space.
    let read_w = 1 + IO_SPEED_W;
    let write_w = 1 + IO_SPEED_W;
    let fixed_left = read_w + write_w + 4; // labels plus spaces around the two graphs
    let graph_total = left_w.saturating_sub(fixed_left);
    let read_graph_w = graph_total / 2;
    let write_graph_w = graph_total.saturating_sub(read_graph_w);

    let read_graph_max =
        visible_graph_max(&disk.read_history, read_graph_w, disk.read_bytes_per_sec);
    let read_pct =
        ((disk.read_bytes_per_sec as i64).saturating_mul(100) / read_graph_max).min(100) as i32;
    let read_color = gradient_color(params.read_grad, read_pct);

    let mut col = x;
    buf.mv(col, y)
        .color(fg)
        .text("R")
        .color(read_color)
        .text(&tools::rjust(&read_speed, IO_SPEED_W, false));
    col += read_w;

    if read_graph_w > 0 {
        buf.text(" ");
        col += 1;
        let mut graph = Graph::new(
            read_graph_w,
            1,
            params.settings.graph_symbol,
            false,
            read_graph_max,
            0,
        );
        let graph_row = graph.render_row(&disk.read_history, params.read_grad);
        buf.text(&graph_row);
        col += read_graph_w;
    }

    if col + write_w < busy_x {
        buf.text(" ");
        col += 1;
    }

    let available_write_graph_w = write_graph_w.min(busy_x.saturating_sub(col + 1));
    let write_graph_max = if available_write_graph_w > 0 {
        visible_graph_max(
            &disk.write_history,
            available_write_graph_w,
            disk.write_bytes_per_sec,
        )
    } else {
        // No room for the graph — still need a sane denominator for value
        // coloring. Match the "lifetime peak when no window" idiom by using
        // the visible-window max over the full history.
        visible_graph_max(
            &disk.write_history,
            disk.write_history.len().max(1),
            disk.write_bytes_per_sec,
        )
    };
    let write_pct =
        ((disk.write_bytes_per_sec as i64).saturating_mul(100) / write_graph_max).min(100) as i32;
    let write_color = gradient_color(params.write_grad, write_pct);

    buf.color(fg)
        .text("W")
        .color(write_color)
        .text(&tools::rjust(&write_speed, IO_SPEED_W, false));

    if available_write_graph_w > 0 {
        buf.text(" ");
        let mut graph = Graph::new(
            available_write_graph_w,
            1,
            params.settings.graph_symbol,
            false,
            write_graph_max,
            0,
        );
        let graph_row = graph.render_row(&disk.write_history, params.write_grad);
        buf.text(&graph_row);
    }

    let busy_color = gradient_color(params.busy_grad, busy);
    buf.mv(busy_x, y)
        .color(fg)
        .text("B")
        .color(busy_color)
        .text(&format!("{:>5}", format!("{busy}%")));
}

fn visible_graph_max(history: &std::collections::VecDeque<i64>, width: usize, current: u64) -> i64 {
    history
        .iter()
        .rev()
        .take(width.max(1))
        .copied()
        .max()
        .unwrap_or(0)
        .max(current as i64)
        .max(1)
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
    /// build the same shape via `DisksFilter::apply`; tests want the
    /// unfiltered set.
    fn all_disks(data: &DiskData) -> Vec<&DiskInfo> {
        data.disks.iter().collect()
    }

    #[test]
    fn draw_contains_disks_title() {
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
            plain.contains("disks"),
            "output should contain 'disks' title"
        );
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
    fn draw_with_disks_filter_renders_only_matching_drives() {
        // End-to-end: the same code path the app uses — parse the user's
        // disks_filter, apply it to the live disk list, hand the borrowed
        // result to draw. The renderer must show only the matching drives
        // and no ghost rows for filtered-out drives.
        use crate::domain::disk::DisksFilter;

        let data = make_disk_data();
        let filter = DisksFilter::parse(&["C:".to_string()]);
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
        use crate::domain::disk::DisksFilter;

        let data = make_disk_data();
        let filter = DisksFilter::parse(&["!C:".to_string()]);
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
