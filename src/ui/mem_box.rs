use crate::domain::memory::MemInfo;
use crate::draw::box_drawing;
use crate::draw::meter::Meter;
use crate::theme::Theme;
use crate::tools;

/// Draw the memory box into an ANSI string.
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
    let used_grad = theme.g("used");
    let free_grad = theme.g("free");
    let cached_grad = theme.g("cached");
    let avail_grad = theme.g("available");

    let mut out = box_drawing::create_box(x, y, width, height, box_color, false, "mem", "", 0, rounded);

    let meter_w = width.saturating_sub(22).min(30).max(5);
    let used = *mem.stats.get("used").unwrap_or(&0);
    let total = used + mem.stats.get("available").unwrap_or(&0);
    let cached = *mem.stats.get("cached").unwrap_or(&0);
    let free = *mem.stats.get("free").unwrap_or(&0);

    // Row 1: Used
    let used_pct = if total > 0 { (used * 100 / total) as i32 } else { 0 };
    let meter = Meter::new(meter_w);
    let used_color = gradient_color(used_grad, used_pct as i64);
    let used_str = tools::floating_humanizer(used, true, 0, false, false, false);
    out.push_str(&format!(
        "\x1b[{};{}H{}Used  {}{} {}{}{}",
        y + 2, x + 2, fg, used_color, meter.render(used_pct), used_color, used_pct, "%"
    ));

    // Row 2: Available
    if height > 3 {
        let avail = *mem.stats.get("available").unwrap_or(&0);
        let avail_pct = if total > 0 { (avail * 100 / total) as i32 } else { 0 };
        let avail_color = gradient_color(avail_grad, avail_pct as i64);
        out.push_str(&format!(
            "\x1b[{};{}H{}Avail {}{} {}{}{}",
            y + 3, x + 2, fg, avail_color, meter.render(avail_pct), avail_color, avail_pct, "%"
        ));
    }

    // Row 3: Cached
    if height > 4 && cached > 0 {
        let cached_pct = if total > 0 { (cached * 100 / total) as i32 } else { 0 };
        let cache_color = gradient_color(cached_grad, cached_pct as i64);
        out.push_str(&format!(
            "\x1b[{};{}H{}Cache {}{} {}{}{}",
            y + 4, x + 2, fg, cache_color, meter.render(cached_pct), cache_color, cached_pct, "%"
        ));
    }

    // Row 4: Free
    if height > 5 {
        let free_pct = if total > 0 { (free * 100 / total) as i32 } else { 0 };
        let free_color = gradient_color(free_grad, free_pct as i64);
        out.push_str(&format!(
            "\x1b[{};{}H{}Free  {}{} {}{}{}",
            y + 5, x + 2, fg, free_color, meter.render(free_pct), free_color, free_pct, "%"
        ));
    }

    // Row 5: Swap
    if height > 6 {
        let swap_used = *mem.stats.get("swap_used").unwrap_or(&0);
        let swap_total = *mem.stats.get("swap_total").unwrap_or(&0);
        if swap_total > 0 {
            let swap_pct = (swap_used * 100 / swap_total.max(1)) as i32;
            let swap_color = gradient_color(used_grad, swap_pct as i64);
            let su = tools::floating_humanizer(swap_used, true, 0, false, false, false);
            let st = tools::floating_humanizer(swap_total, true, 0, false, false, false);
            out.push_str(&format!(
                "\x1b[{};{}H{}Swap  {}{} {}{}/{}\x1b[0m",
                y + 6, x + 2, fg, swap_color, meter.render(swap_pct), fg, su, st
            ));
        }
    }

    // Disks
    if height > 8 {
        let disk_start = y + 8;
        for (i, disk_name) in mem.disks_order.iter().take(height.saturating_sub(9)).enumerate() {
            if let Some(disk) = mem.disks.get(disk_name) {
                let dpct = disk.used_percent;
                let disk_color = gradient_color(avail_grad, dpct as i64);
                let du = tools::floating_humanizer(disk.used, true, 0, false, false, false);
                let dt = tools::floating_humanizer(disk.total, true, 0, false, false, false);
                out.push_str(&format!(
                    "\x1b[{};{}H{}{:<4}{}{} {}{}/{}",
                    disk_start + i, x + 2, fg,
                    tools::uresize(&disk.name, 4, false),
                    disk_color, meter.render(dpct), fg, du, dt
                ));
            }
        }
    }

    // Total memory at bottom
    if height > 3 {
        let total_str = tools::floating_humanizer(total, false, 0, false, false, false);
        out.push_str(&format!(
            "\x1b[{};{}H{}Total: {}",
            y + height - 1, x + 2, fg, total_str
        ));
    }

    out.push_str("\x1b[0m");
    out
}

fn gradient_color<'a>(gradient: &'a [String], pct: i64) -> &'a str {
    if gradient.is_empty() { return ""; }
    &gradient[pct.clamp(0, 100) as usize]
}


