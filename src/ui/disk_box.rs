use crate::collect::CollectStatus;
use crate::domain::disk::DiskData;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::{Graph, GraphSymbol};
use crate::draw::meter::Meter;
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

use super::BoxArea;

/// Extracted settings for the disk box, decoupled from Config.
pub struct DiskBoxSettings {
    pub graph_symbol: GraphSymbol,
    pub base_10: bool,
    pub show_io_stat: bool,
    pub io_mode: bool,
    pub disk_io_mode: bool,
    pub io_graph_combined: bool,
}

/// Draw the disk box into an ANSI string.
///
/// Layout:
/// ╭─ disks ────────────────────╮
/// │ C: NTFS ■■■■■■■░░ 233G/465G│
/// │ R 42M/s ⣀⣤⣶ W 8M/s ⣀ B 12% │
/// │ D: NTFS ■■■░░░░░ 1.2T/3.6T│
/// │ R 0B/s  ⣀⣀⣀ W 0B/s  ⣀ B 0% │
/// ╰────────────────────────────╯
pub fn draw(
    disks: &DiskData,
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
        num: 6,
        rounded,
        hi_color: hi,
        title_color,
    }));

    super::draw_status_inset(&mut buf, status, "disks", x, y, box_color, title_color);

    let mut row = 0;

    let io_view = settings.io_mode || settings.disk_io_mode;

    // Layout: " {label} {meter} {used/total} " — single row per disk
    // Value column: "274G/1.6T" = up to 10 chars
    let val_w = 10;

    for disk in &disks.disks {
        if row >= inner_h {
            break;
        }

        if io_view {
            // I/O mode: show throughput graphs instead of usage meters
            if settings.io_graph_combined {
                // Combined read+write in a single graph row
                let label = format!("{} IO ", disk.name);
                let label_len = tools::ulen(&label, false);
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
                let value_w = tools::ulen(&value, false);
                let graph_w = inner_w.saturating_sub(label_len + value_w + 1).max(3);

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
                let mut graph = Graph::new(graph_w, 1, settings.graph_symbol, false, true, max, 0);
                let graph_row = graph.render_row_colored(&combined, read_grad);

                buf.mv(content_x, y + 2 + row)
                    .color(title_color)
                    .text(&label)
                    .text(&graph_row)
                    .color(fg)
                    .text(" ")
                    .text(&value);
                row += 1;
            } else {
                // Separate read and write graph rows
                // Read row
                let label_r = format!("{} R ", disk.name);
                let label_r_len = tools::ulen(&label_r, false);
                let speed_r = tools::floating_humanizer(
                    disk.read_bytes_per_sec,
                    true,
                    0,
                    false,
                    true,
                    settings.base_10,
                );
                let speed_r_w = tools::ulen(&speed_r, false);
                let graph_rw = inner_w.saturating_sub(label_r_len + speed_r_w + 1).max(3);

                let read_max =
                    visible_graph_max(&disk.read_history, graph_rw, disk.read_bytes_per_sec);
                let mut rg =
                    Graph::new(graph_rw, 1, settings.graph_symbol, false, true, read_max, 0);
                let rg_row = rg.render_row_colored(&disk.read_history, read_grad);

                buf.mv(content_x, y + 2 + row)
                    .color(title_color)
                    .text(&label_r)
                    .text(&rg_row)
                    .color(fg)
                    .text(" ")
                    .text(&speed_r);
                row += 1;

                if row >= inner_h {
                    continue;
                }

                // Write row
                let label_w = format!("{} W ", disk.name);
                let label_w_len = tools::ulen(&label_w, false);
                let speed_w = tools::floating_humanizer(
                    disk.write_bytes_per_sec,
                    true,
                    0,
                    false,
                    true,
                    settings.base_10,
                );
                let speed_w_vis = tools::ulen(&speed_w, false);
                let graph_ww = inner_w.saturating_sub(label_w_len + speed_w_vis + 1).max(3);

                let write_max =
                    visible_graph_max(&disk.write_history, graph_ww, disk.write_bytes_per_sec);
                let mut wg = Graph::new(
                    graph_ww,
                    1,
                    settings.graph_symbol,
                    false,
                    true,
                    write_max,
                    0,
                );
                let wg_row = wg.render_row_colored(&disk.write_history, write_grad);

                buf.mv(content_x, y + 2 + row)
                    .color(title_color)
                    .text(&label_w)
                    .text(&wg_row)
                    .color(fg)
                    .text(" ")
                    .text(&speed_w);
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
            let meter_w = inner_w.saturating_sub(label_len + val_w).max(5);
            let disk_meter = Meter::new(meter_w, avail_grad, meter_bg);

            buf.mv(content_x, y + 2 + row)
                .color(title_color)
                .text(&label)
                .text(disk_meter.render(disk.used_percent))
                .color(fg)
                .text(&tools::rjust(&value, val_w, false));
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
    let title_color = params.theme.color(tc::TITLE);
    let read_speed =
        tools::floating_humanizer(disk.read_bytes_per_sec, true, 0, false, true, base_10);
    let write_speed =
        tools::floating_humanizer(disk.write_bytes_per_sec, true, 0, false, true, base_10);
    let read_label = format!("R {read_speed}");
    let write_label = format!("W {write_speed}");
    let busy = disk.busy_percent.clamp(0, 100);
    let busy_label = format!("B {busy}%");
    let busy_w = tools::ulen(&busy_label, false).min(width);
    let busy_x = x + width.saturating_sub(busy_w);
    let left_w = width.saturating_sub(busy_w + 1);

    let read_w = tools::ulen(&read_label, false);
    let write_w = tools::ulen(&write_label, false);
    let fixed_left = read_w + write_w + 4; // labels plus spaces around the two graphs
    let graph_total = left_w.saturating_sub(fixed_left);
    let read_graph_w = graph_total / 2;
    let write_graph_w = graph_total.saturating_sub(read_graph_w);

    let mut col = x;
    buf.mv(col, y)
        .color(title_color)
        .text("R")
        .color(fg)
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
            true,
            visible_graph_max(&disk.read_history, read_graph_w, disk.read_bytes_per_sec),
            0,
        );
        let graph_row = graph.render_row_colored(&disk.read_history, params.read_grad);
        buf.text(&graph_row);
        col += read_graph_w;
    }

    if col + write_w < busy_x {
        buf.text(" ");
        col += 1;
    }
    buf.color(title_color)
        .text("W")
        .color(fg)
        .text(&format!(" {write_speed}"));
    col += write_w;

    let available_write_graph_w = write_graph_w.min(busy_x.saturating_sub(col + 1));
    if available_write_graph_w > 0 {
        buf.text(" ");
        let mut graph = Graph::new(
            available_write_graph_w,
            1,
            params.settings.graph_symbol,
            false,
            true,
            visible_graph_max(
                &disk.write_history,
                available_write_graph_w,
                disk.write_bytes_per_sec,
            ),
            0,
        );
        let graph_row = graph.render_row_colored(&disk.write_history, params.write_grad);
        buf.text(&graph_row);
    }

    let busy_color = if !params.busy_grad.is_empty() {
        &params.busy_grad[busy as usize]
    } else {
        fg
    };
    buf.mv(busy_x, y)
        .color(title_color)
        .text("B")
        .color(busy_color)
        .text(&format!(" {busy}%"));
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
    use crate::domain::disk::DiskInfo;
    use crate::draw::graph::GraphSymbol;

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
            graph_symbol: GraphSymbol::Braille,
            base_10: false,
            show_io_stat: true,
            io_mode: false,
            disk_io_mode: false,
            io_graph_combined: false,
        }
    }

    #[test]
    fn draw_contains_disks_title() {
        let output = draw(
            &make_disk_data(),
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
        let output = draw(
            &make_disk_data(),
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
        let output = draw(
            &make_disk_data(),
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
        let output = draw(
            &make_disk_data(),
            &make_area(),
            &Theme::default(),
            &settings(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("R "), "output should contain read label");
        assert!(plain.contains("W "), "output should contain write label");
        assert!(plain.contains("B 12%"), "output should contain busy label");
    }
}
