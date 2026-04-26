use crate::domain::memory::MemInfo;
use crate::draw::box_drawing;
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

    let mut out = box_drawing::create_box(x, y, width, height, box_color, false, "mem", "", 0, rounded);

    // Show used / total with color
    let used = *mem.stats.get("used").unwrap_or(&0);
    let total = used + mem.stats.get("available").unwrap_or(&0);
    let used_pct = if total > 0 { (used * 100 / total) as usize } else { 0 };
    let used_str = tools::floating_humanizer(used, true, 0, false, false, false);
    let total_str = tools::floating_humanizer(total, true, 0, false, false, false);
    let pct_color = if !used_grad.is_empty() { &used_grad[used_pct.min(100)] } else { fg };
    out.push_str(&format!(
        "\x1b[{};{}H{}Used: {}{} {} / {}",
        y + 2, x + 2, fg, pct_color, used_str, fg, total_str
    ));

    // Show swap if space permits
    if height > 4 {
        let swap_used = *mem.stats.get("swap_used").unwrap_or(&0);
        let swap_total = *mem.stats.get("swap_total").unwrap_or(&0);
        if swap_total > 0 {
            let su = tools::floating_humanizer(swap_used, true, 0, false, false, false);
            let st = tools::floating_humanizer(swap_total, true, 0, false, false, false);
            let swap_pct = (swap_used * 100 / swap_total.max(1)) as usize;
            let swap_color = if !used_grad.is_empty() { &used_grad[swap_pct.min(100)] } else { fg };
            out.push_str(&format!(
                "\x1b[{};{}H{}Swap: {}{} {} / {}",
                y + 3, x + 2, fg, swap_color, su, fg, st
            ));
        }
    }

    // Show disks if space permits
    if height > 6 {
        let avail_grad = theme.g("available");
        for (i, disk_name) in mem.disks_order.iter().take(height.saturating_sub(6)).enumerate() {
            if let Some(disk) = mem.disks.get(disk_name) {
                let du = tools::floating_humanizer(disk.used, true, 0, false, false, false);
                let dt = tools::floating_humanizer(disk.total, true, 0, false, false, false);
                let dpct = disk.used_percent as usize;
                let disk_color = if !avail_grad.is_empty() { &avail_grad[dpct.min(100)] } else { fg };
                let line = format!("{}: {}{} {}/{} {}{}%", disk.name, disk_color, du, fg, dt, disk_color, disk.used_percent);
                let line_trunc = tools::uresize(&line, width.saturating_sub(4) + 30, false);
                out.push_str(&format!("\x1b[{};{}H{}", y + 5 + i, x + 2, line_trunc));
            }
        }
    }

    out.push_str("\x1b[0m");
    out
}

