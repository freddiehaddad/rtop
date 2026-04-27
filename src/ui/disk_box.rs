use crate::domain::disk::DiskData;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;
use crate::draw::meter::Meter;
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

use super::BoxArea;

/// Draw the disk box into an ANSI string.
///
/// Layout:
/// ╭─ disks ────────────────────╮
/// │ C: NTFS                    │
/// │  ■■■■■■■■■░░ 233G / 465G  │
/// │ D: NTFS                    │
/// │  ■■■░░░░░░░░ 1.2T / 3.6T  │
/// ╰────────────────────────────╯
pub fn draw(disks: &DiskData, area: &BoxArea, theme: &Theme) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.c(tc::DISK_BOX);
    let fg = theme.c(tc::MAIN_FG);
    let title_color = theme.c(tc::TITLE);
    let hi = theme.c(tc::HI_FG);
    let avail_grad = theme.g(tc::GRAD_AVAILABLE);
    let meter_bg = theme.c(tc::METER_BG);

    let inner_h = height.saturating_sub(2);
    let inner_w = width.saturating_sub(4);

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

    let mut row = 0;

    // Layout: " {label} {meter} {used/total} " — single row per disk
    // Value column: "274G/1.6T" = up to 10 chars
    let val_w = 10;

    for disk_name in &disks.disks_order {
        if row >= inner_h {
            break;
        }
        if let Some(disk) = disks.disks.get(disk_name) {
            let du = tools::floating_humanizer(disk.used, true, 0, false, false, false);
            let dt = tools::floating_humanizer(disk.total, true, 0, false, false, false);
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

            buf.mv(x + 2, y + 2 + row)
                .color(title_color)
                .text(&label)
                .text(disk_meter.render(disk.used_percent))
                .color(fg)
                .text(&tools::rjust(&value, val_w, false));
            row += 1;
        }
    }

    buf.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::disk::DiskInfo;
    use std::collections::HashMap;

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
        let mut disks = HashMap::new();
        disks.insert(
            "C:".into(),
            DiskInfo {
                name: "C:".into(),
                fstype: "NTFS".into(),
                total: 500 * GIB,
                used: 250 * GIB,
                used_percent: 50,
            },
        );
        disks.insert(
            "D:".into(),
            DiskInfo {
                name: "D:".into(),
                fstype: "NTFS".into(),
                total: 1000 * GIB,
                used: 300 * GIB,
                used_percent: 30,
            },
        );
        DiskData {
            disks,
            disks_order: vec!["C:".into(), "D:".into()],
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

    #[test]
    fn draw_contains_disks_title() {
        let output = draw(&make_disk_data(), &make_area(), &Theme::default());
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("disks"),
            "output should contain 'disks' title"
        );
    }

    #[test]
    fn draw_contains_drive_letters() {
        let output = draw(&make_disk_data(), &make_area(), &Theme::default());
        let plain = strip_ansi(&output);
        assert!(plain.contains("C:"), "output should contain 'C:'");
        assert!(plain.contains("D:"), "output should contain 'D:'");
    }

    #[test]
    fn draw_contains_filesystem_type() {
        let output = draw(&make_disk_data(), &make_area(), &Theme::default());
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("NTFS"),
            "output should contain filesystem type 'NTFS'"
        );
    }
}
