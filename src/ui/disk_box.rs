use crate::domain::disk::DiskData;
use crate::draw::box_drawing;
use crate::draw::meter::Meter;
use crate::term;
use crate::theme::Theme;
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
pub fn draw(
    disks: &DiskData,
    area: &BoxArea,
    theme: &Theme,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let box_color = theme.c("mem_box"); // same color family as mem
    let fg = theme.c("main_fg");
    let title_color = theme.c("title");
    let avail_grad = theme.g("available");
    let meter_bg = theme.c("meter_bg");

    let inner_h = height.saturating_sub(2);
    let inner_w = width.saturating_sub(4);

    let mut out = box_drawing::create_box(&box_drawing::BoxConfig {
        x, y, width, height, line_color: box_color, fill: true,
        title: "disks", title2: "", num: 6, rounded,
    });

    let meter_w = inner_w.saturating_sub(16).max(5);
    let disk_meter = Meter::new(meter_w, avail_grad, meter_bg);
    let mut row = 0;

    for disk_name in &disks.disks_order {
        if row + 1 >= inner_h {
            break;
        }
        if let Some(disk) = disks.disks.get(disk_name) {
            let du = tools::floating_humanizer(disk.used, true, 0, false, false, false);
            let dt = tools::floating_humanizer(disk.total, true, 0, false, false, false);

            // Row 1: "C: NTFS"
            let fstype_label = if disk.fstype.is_empty() {
                String::new()
            } else {
                format!(" {}", disk.fstype)
            };
            out.push_str(&format!(
                "{}{}{}{}{}",
                term::mv(x + 2, y + 2 + row),
                title_color,
                tools::uresize(&disk.name, 4, false),
                fg,
                fstype_label,
            ));
            row += 1;

            if row >= inner_h {
                break;
            }

            // Row 2: " ■■■■■■■■░ 233G / 465G"
            let usage_label = format!("{} / {}", du, dt);
            out.push_str(&format!(
                "{} {} {}{}",
                term::mv(x + 2, y + 2 + row),
                disk_meter.render(disk.used_percent),
                fg,
                usage_label,
            ));
            row += 1;
        }
    }

    out.push_str("\x1b[0m");
    out
}
