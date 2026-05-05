use crate::collect::CollectStatus;
use crate::domain::disk::DiskInfo;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::{Graph, GraphMode};
use crate::draw::meter::Meter;
use crate::theme::{Theme, gradient_color};
use crate::theme_keys as tc;
use crate::tools;

use super::BoxArea;

// --- Layout constants ---

/// Right-aligned value column width for normal mode ("274G/1.6T" = up to 10 chars).
const USAGE_VAL_W: usize = 10;
/// Right-aligned value column for IO separate read/write rows
/// (1 space + max 7 chars, e.g. "1023B/s").
const IO_VAL_W: usize = 8;
/// Right-aligned value column for IO combined rows
/// (1 space + max "R1023B/s W1023B/s" = 18 chars).
const IO_COMBINED_VAL_W: usize = 19;
/// Minimum graph/meter width.
const MIN_METER_W: usize = 5;
/// Minimum graph width in IO mode.
const MIN_IO_GRAPH_W: usize = 3;

/// Extracted settings for the disk box, decoupled from Config.
pub struct DiskBoxSettings {
    pub graph_symbol: GraphMode,
    pub base_10: bool,
    pub show_io_stat: bool,
    pub io_mode: bool,
    pub disk_io_mode: bool,
    pub io_graph_combined: bool,
}

/// Draw the disk box into an ANSI string.
///
/// `disks` is the post-filter slice of disks the caller wants rendered,
/// in display order. Filtering (via `DisksFilter`) and the resulting
/// height sizing happen at the call site so the renderer stays a pure
/// function of (data, settings, theme).
///
/// Layout:
/// ╭─ disks ────────────────────╮
/// │ C: NTFS ■■■■■■■░░ 233G/465G│
/// │ R 42M/s ⣀⣤⣶ W 8M/s ⣀ B 12% │
/// │ D: NTFS ■■■░░░░░ 1.2T/3.6T│
/// │ R 0B/s  ⣀⣀⣀ W 0B/s  ⣀ B 0% │
/// ╰────────────────────────────╯
pub fn draw(
    disks: &[&DiskInfo],
    area: &BoxArea,
    theme: &Theme,
    settings: &DiskBoxSettings,
    status: &CollectStatus,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.color(tc::DISK_BOX);
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
        line_color: box_color,
        fill: true,
        title: "disks",
        title2: "",
        num: super::DISK_KEY,
        rounded,
        hi_color: hi,
        title_color,
    }));

    super::draw_status_inset(&mut buf, status, "disks", x, y, box_color, title_color);

    let mut row = 0;

    let io_view = settings.io_mode || settings.disk_io_mode;

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
                // Separate read and write graph rows
                let label_r = format!("{} R ", disk.name);
                let label_r_len = tools::ulen(&label_r, false);
                let graph_w = inner_w
                    .saturating_sub(label_r_len + IO_VAL_W)
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
                    .text(&label_r)
                    .text(&rg_row)
                    .color(gradient_color(read_grad, read_pct))
                    .text(&tools::rjust(&speed_r, IO_VAL_W, false));
                row += 1;

                if row >= inner_h {
                    continue;
                }

                // Write row
                let label_w = format!("{} W ", disk.name);
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
                    .text(&label_w)
                    .text(&wg_row)
                    .color(gradient_color(write_grad, write_pct))
                    .text(&tools::rjust(&speed_w, IO_VAL_W, false));
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
    settings: &'a DiskBoxSettings,
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
    let read_label = format!("R {read_speed}");
    let write_label = format!("W {write_speed}");
    let busy = disk.busy_percent.clamp(0, 100);
    let busy_w: usize = 6; // "B" + 5-char right-justified value
    let busy_x = x + width.saturating_sub(busy_w);
    let left_w = width.saturating_sub(busy_w);

    let read_w = tools::ulen(&read_label, false);
    let write_w = tools::ulen(&write_label, false);
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
        .text(&format!(" {read_speed}"));
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
        .text(&format!(" {write_speed}"));

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

    fn make_area() -> BoxArea {
        BoxArea {
            x: 1,
            y: 1,
            width: 40,
            height: 12,
            rounded: true,
        }
    }

    fn settings() -> DiskBoxSettings {
        DiskBoxSettings {
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
            &settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("disks"),
            "output should contain 'disks' title"
        );
    }

    #[test]
    fn draw_contains_drive_letters() {
        let data = make_disk_data();
        let output = draw(
            &all_disks(&data),
            &make_area(),
            &Theme::default(),
            &settings(),
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
            &settings(),
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
            &settings(),
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
        let area = BoxArea {
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
            &settings(),
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
            &settings(),
            &CollectStatus::Ok,
        );
        let read_grad = theme.gradient(tc::GRAD_DISK_READ);
        let write_grad = theme.gradient(tc::GRAD_DISK_WRITE);
        let busy_grad = theme.gradient(tc::GRAD_DISK_BUSY);

        // read_history is tiny ints; current = 42 MB/s dwarfs them; so
        // visible_graph_max = current → pct = 100. Expected: " 42M/s".
        let expected_r = format!("{}{}", read_grad[100], " 42M/s");
        assert!(
            output.contains(&expected_r),
            "perf-row R speed should be GRAD_DISK_READ[100] adjacent to ' 42M/s'"
        );

        // Same logic for W — current 8 MB/s dominates the history window.
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
    fn io_combined_value_uses_read_gradient() {
        // The combined IO row renders one graph (using read_grad) and one
        // value cell. The value takes read_grad at the combined pct
        // (against the same max the graph row uses).
        let theme = Theme::default();
        let area = BoxArea {
            x: 1,
            y: 1,
            width: 60,
            height: 8,
            rounded: true,
        };
        let s = DiskBoxSettings {
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
            &settings(),
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
        let filter = DisksFilter::parse("C:");
        let visible = filter.apply(&data.disks);
        assert_eq!(visible.len(), 1);

        let output = draw(
            &visible,
            &make_area(),
            &Theme::default(),
            &settings(),
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
        let filter = DisksFilter::parse("!C:");
        let visible = filter.apply(&data.disks);
        assert_eq!(visible.len(), 1);

        let output = draw(
            &visible,
            &make_area(),
            &Theme::default(),
            &settings(),
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
