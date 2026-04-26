use crate::domain::memory::MemInfo;
use crate::draw::box_drawing;
use crate::tools;

/// Draw the memory box into an ANSI string.
pub fn draw(
    mem: &MemInfo,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    rounded: bool,
) -> String {
    let mut out = box_drawing::create_box(x, y, width, height, "", false, "mem", "", 0, rounded);

    // Show used / total
    let used = *mem.stats.get("used").unwrap_or(&0);
    let total = used + mem.stats.get("available").unwrap_or(&0);
    let used_str = tools::floating_humanizer(used, true, 0, false, false, false);
    let total_str = tools::floating_humanizer(total, true, 0, false, false, false);
    out.push_str(&format!(
        "\x1b[{};{}HUsed: {} / {}",
        y + 2,
        x + 2,
        used_str,
        total_str
    ));

    // Show swap if space permits
    if height > 4 {
        let swap_used = *mem.stats.get("swap_used").unwrap_or(&0);
        let swap_total = *mem.stats.get("swap_total").unwrap_or(&0);
        if swap_total > 0 {
            let su = tools::floating_humanizer(swap_used, true, 0, false, false, false);
            let st = tools::floating_humanizer(swap_total, true, 0, false, false, false);
            out.push_str(&format!(
                "\x1b[{};{}HSwap: {} / {}",
                y + 3,
                x + 2,
                su,
                st
            ));
        }
    }

    // Show disks if space permits
    if height > 6 {
        for (i, disk_name) in mem.disks_order.iter().take(height.saturating_sub(6)).enumerate() {
            if let Some(disk) = mem.disks.get(disk_name) {
                let du = tools::floating_humanizer(disk.used, true, 0, false, false, false);
                let dt = tools::floating_humanizer(disk.total, true, 0, false, false, false);
                let line = format!("{}: {} / {} ({}%)", disk.name, du, dt, disk.used_percent);
                let line_trunc = tools::uresize(&line, width.saturating_sub(4), false);
                out.push_str(&format!("\x1b[{};{}H{}", y + 5 + i, x + 2, line_trunc));
            }
        }
    }

    out
}
