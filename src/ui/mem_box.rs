use crate::domain::memory::MemInfo;
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::meter::Meter;
use crate::theme::Theme;
use crate::tools;

/// Draw the memory box into an ANSI string matching btop's layout.
///
/// Layout with disks:
/// ╭─ mem ──────────────────────╮╭─ disks ─────────────────╮
/// │ Used  ■■■■■■■■■■■░░░ 5.2G ││ C: NTFS                 │
/// │ Avail ■■■■░░░░░░░░░░ 3.1G ││  ■■■■■■■■■░ 233G / 465G │
/// │ Cache ■■■░░░░░░░░░░░ 1.8G ││ D: NTFS                 │
/// │ Free  ■■░░░░░░░░░░░░ 0.9G ││  ■■■░░░░░░░ 1.2T / 3.6T │
/// │                            ││                          │
/// │ Swap  ■■░░░░░░░░░░░░ 1.0G ││                          │
/// │   1.0G / 8.0G              ││                          │
/// ╰────────────────────────────╯╰──────────────────────────╯
pub fn draw(
    mem: &MemInfo,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    rounded: bool,
    theme: &Theme,
) -> String {
    let box_color = theme.c("mem_box");
    let fg = theme.c("main_fg");
    let title_color = theme.c("title");
    let div_color = theme.c("div_line");
    let used_grad = theme.g("used");
    let free_grad = theme.g("free");
    let cached_grad = theme.g("cached");
    let avail_grad = theme.g("available");

    let has_disks = !mem.disks.is_empty();
    let inner_h = height.saturating_sub(2);

    // Split width between mem section and disks section
    let (mem_w, disk_w) = if has_disks && width > 50 {
        // btop: mem_width = ceil((width-3)/2), rounded to even; disks_width = width - mem_width - 2
        let mw = ((width.saturating_sub(3) + 1) / 2 + 1) & !1; // ceil + round to even
        let dw = width.saturating_sub(mw + 2);
        (mw, dw)
    } else {
        (width.saturating_sub(1), 0)
    };
    let divider_col = if disk_w > 0 { x + mem_w } else { 0 };

    // Draw one full-width box with "mem" title
    let mut out =
        box_drawing::create_box(x, y, width, height, box_color, false, "mem", "", 0, rounded);

    // Draw disk divider and title inside the single box
    if disk_w > 0 {
        // Top junction: ┬
        out.push_str(&format!(
            "\x1b[{};{}H{}{}",
            y + 1,
            divider_col + 1,
            box_color,
            symbols::DIV_UP
        ));
        // Bottom junction: ┴
        out.push_str(&format!(
            "\x1b[{};{}H{}{}",
            y + height,
            divider_col + 1,
            box_color,
            symbols::DIV_DOWN
        ));
        // Vertical divider lines between top and bottom
        for row_i in 1..height.saturating_sub(1) {
            out.push_str(&format!(
                "\x1b[{};{}H{}{}",
                y + 1 + row_i,
                divider_col + 1,
                div_color,
                symbols::V_LINE
            ));
        }
        // "disks" title on top border after divider
        out.push_str(&format!(
            "\x1b[{};{}H{}{} {} {}{}",
            y + 1,
            divider_col + 2,
            box_color,
            symbols::H_LINE,
            "disks",
            symbols::H_LINE.repeat(disk_w.saturating_sub(9)),
            "",
        ));
    }

    let total_bytes = mem.stats.get("used").unwrap_or(&0)
        + mem.stats.get("available").unwrap_or(&0);
    let meter_area = mem_w.saturating_sub(4); // 2 border + 2 padding
    let label_w = 6; // "Used  ", "Avail ", etc.
    let meter_w = meter_area.saturating_sub(label_w + 6).max(5); // 6 for value display
    let meter_bg = theme.c("meter_bg");
    let used_meter = Meter::new(meter_w, used_grad, meter_bg);
    let avail_meter = Meter::new(meter_w, avail_grad, meter_bg);
    let cached_meter = Meter::new(meter_w, cached_grad, meter_bg);
    let free_meter = Meter::new(meter_w, free_grad, meter_bg);
    let mut row = 0;

    // Used
    let used = *mem.stats.get("used").unwrap_or(&0);
    let used_pct = if total_bytes > 0 {
        (used * 100 / total_bytes) as i32
    } else {
        0
    };
    let used_color = gradient_color(used_grad, used_pct as i64);
    let used_str = tools::floating_humanizer(used, true, 0, false, false, false);
    if row < inner_h {
        out.push_str(&format!(
            "\x1b[{};{}H{}Used  {}  {}{}",
            y + 2 + row,
            x + 2,
            title_color,
            used_meter.render(used_pct),
            used_color,
            used_str
        ));
        row += 1;
    }

    // Available
    let avail = *mem.stats.get("available").unwrap_or(&0);
    let avail_pct = if total_bytes > 0 {
        (avail * 100 / total_bytes) as i32
    } else {
        0
    };
    let avail_color = gradient_color(avail_grad, avail_pct as i64);
    let avail_str = tools::floating_humanizer(avail, true, 0, false, false, false);
    if row < inner_h {
        out.push_str(&format!(
            "\x1b[{};{}H{}Avail {}  {}{}",
            y + 2 + row,
            x + 2,
            title_color,
            avail_meter.render(avail_pct),
            avail_color,
            avail_str
        ));
        row += 1;
    }

    // Cached
    let cached = *mem.stats.get("cached").unwrap_or(&0);
    if cached > 0 && row < inner_h {
        let cached_pct = if total_bytes > 0 {
            (cached * 100 / total_bytes) as i32
        } else {
            0
        };
        let cache_color = gradient_color(cached_grad, cached_pct as i64);
        let cached_str = tools::floating_humanizer(cached, true, 0, false, false, false);
        out.push_str(&format!(
            "\x1b[{};{}H{}Cache {}  {}{}",
            y + 2 + row,
            x + 2,
            title_color,
            cached_meter.render(cached_pct),
            cache_color,
            cached_str
        ));
        row += 1;
    }

    // Free
    let free = *mem.stats.get("free").unwrap_or(&0);
    let free_pct = if total_bytes > 0 {
        (free * 100 / total_bytes) as i32
    } else {
        0
    };
    let free_color = gradient_color(free_grad, free_pct as i64);
    let free_str = tools::floating_humanizer(free, true, 0, false, false, false);
    if row < inner_h {
        out.push_str(&format!(
            "\x1b[{};{}H{}Free  {}  {}{}",
            y + 2 + row,
            x + 2,
            title_color,
            free_meter.render(free_pct),
            free_color,
            free_str
        ));
        row += 1;
    }

    // Blank line before swap
    if row < inner_h {
        row += 1;
    }

    // Swap
    let swap_used = *mem.stats.get("swap_used").unwrap_or(&0);
    let swap_total = *mem.stats.get("swap_total").unwrap_or(&0);
    if swap_total > 0 && row < inner_h {
        let swap_pct = (swap_used * 100 / swap_total.max(1)) as i32;
        let swap_str = tools::floating_humanizer(swap_used, true, 0, false, false, false);
        out.push_str(&format!(
            "\x1b[{};{}H{}Swap  {}  {}{}",
            y + 2 + row,
            x + 2,
            title_color,
            used_meter.render(swap_pct),
            fg,
            swap_str
        ));
        row += 1;

        // Swap total line
        if row < inner_h {
            let su = tools::floating_humanizer(swap_used, true, 0, false, false, false);
            let st = tools::floating_humanizer(swap_total, true, 0, false, false, false);
            out.push_str(&format!(
                "\x1b[{};{}H{}  {} / {}",
                y + 2 + row,
                x + 2,
                fg,
                su,
                st
            ));
        }
    }

    // Disks section (right panel, after the divider)
    if disk_w > 0 {
        let disk_x = divider_col + 1; // column after the vertical divider
        let disk_inner_w = disk_w.saturating_sub(1); // -1 for the right border
        let disk_meter_w = disk_inner_w.saturating_sub(16).max(5);
        let disk_meter = Meter::new(disk_meter_w, avail_grad, meter_bg);
        let mut drow = 0;

        for disk_name in &mem.disks_order {
            if drow + 1 >= inner_h {
                break;
            }
            if let Some(disk) = mem.disks.get(disk_name) {
                let disk_color = gradient_color(avail_grad, disk.used_percent as i64);
                let du = tools::floating_humanizer(disk.used, true, 0, false, false, false);
                let dt = tools::floating_humanizer(disk.total, true, 0, false, false, false);

                // Row 1: "C: NTFS"
                let fstype_label = if disk.fstype.is_empty() {
                    String::new()
                } else {
                    format!(" {}", disk.fstype)
                };
                out.push_str(&format!(
                    "\x1b[{};{}H{}{}{}{}",
                    y + 2 + drow,
                    disk_x + 1,
                    title_color,
                    tools::uresize(&disk.name, 4, false),
                    fg,
                    fstype_label,
                ));
                drow += 1;

                if drow >= inner_h {
                    break;
                }

                // Row 2: " ■■■■■■■■░ 233G / 465G"
                let usage_label = format!("{} / {}", du, dt);
                out.push_str(&format!(
                    "\x1b[{};{}H {} {}{}",
                    y + 2 + drow,
                    disk_x + 1,
                    disk_meter.render(disk.used_percent),
                    fg,
                    usage_label,
                ));
                drow += 1;
            }
        }
    }

    out.push_str("\x1b[0m");
    out
}

fn gradient_color<'a>(gradient: &'a [String], pct: i64) -> &'a str {
    if gradient.is_empty() {
        return "";
    }
    &gradient[pct.clamp(0, 100) as usize]
}

