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
    let hi = theme.c("hi_fg");
    let div_color = theme.c("div_line");
    let used_grad = theme.g("used");
    let free_grad = theme.g("free");
    let cached_grad = theme.g("cached");
    let avail_grad = theme.g("available");

    let has_disks = !mem.disks.is_empty();
    let inner_h = height.saturating_sub(2);

    // btop: mem_width = ceil((width-3)/2), rounded to even; disks_width = width - mem_width - 2
    let (mem_w, disk_w, divider_col) = if has_disks && width > 50 {
        let mw = ((width as f64 - 3.0) / 2.0).ceil() as usize;
        let mw = mw + (mw % 2); // round up to even
        let dw = width - mw - 2;
        (mw, dw, x + mw)
    } else {
        (width.saturating_sub(1), 0, 0)
    };

    // One full-width box with "mem" title (btop line 2453)
    let mut out =
        box_drawing::create_box(x, y, width, height, box_color, true, "mem", "", 2, rounded);

    // "disks" title inset on the top border (btop line 2454)
    // Placed at divider+2 using title_left + "d" highlighted + "isks" + title_right
    if disk_w > 0 {
        let disks_title_x = divider_col + 3;
        out.push_str(&format!(
            "\x1b[{};{}H{}{}{}{}{}{}{}",
            y + 1, disks_title_x,
            box_color,
            box_drawing::title_syms::TITLE_LEFT,
            hi, "d",
            title_color, "isks",
            box_color,
        ));
        out.push_str(box_drawing::title_syms::TITLE_RIGHT);

        // Divider: div_up at top, div_down at bottom, v_line in between (btop line 2458-2460)
        out.push_str(&format!(
            "\x1b[{};{}H{}{}",
            y + 1, divider_col + 1, box_color, symbols::DIV_UP
        ));
        out.push_str(&format!(
            "\x1b[{};{}H{}{}",
            y + height, divider_col + 1, box_color, symbols::DIV_DOWN
        ));
        out.push_str(div_color);
        for row_i in 1..height.saturating_sub(1) {
            out.push_str(&format!(
                "\x1b[{};{}H{}",
                y + 1 + row_i, divider_col + 1, symbols::V_LINE
            ));
        }
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
    let used_pct = (used * 100).checked_div(total_bytes).unwrap_or(0) as i32;
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
    let avail_pct = (avail * 100).checked_div(total_bytes).unwrap_or(0) as i32;
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
        let cached_pct = (cached * 100).checked_div(total_bytes).unwrap_or(0) as i32;
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
    let free_pct = (free * 100).checked_div(total_bytes).unwrap_or(0) as i32;
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

fn gradient_color(gradient: &[String], pct: i64) -> &str {
    if gradient.is_empty() {
        return "";
    }
    &gradient[pct.clamp(0, 100) as usize]
}

