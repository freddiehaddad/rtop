use crate::domain::cpu::CpuInfo;
use crate::draw::box_drawing;
use crate::tools;

/// Draw the CPU box into an ANSI string. Returns escape-code output.
pub fn draw(
    _buf: &mut crate::cell_buffer::CellBuffer,
    cpu: &CpuInfo,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    rounded: bool,
) -> String {
    let mut out = box_drawing::create_box(x, y, width, height, "", false, "cpu", "", 0, rounded);

    // CPU name and total usage
    if let Some(total) = cpu.cpu_percent.get("total") {
        if let Some(&pct) = total.back() {
            let info = format!(" {} {}%", cpu.cpu_name, pct);
            let info_truncated = tools::uresize(&info, width.saturating_sub(4), false);
            out.push_str(&format!("\x1b[{};{}H{}", y + 2, x + 2, info_truncated));
        }
    }

    // Frequency
    if !cpu.cpu_hz.is_empty() && height > 3 {
        let hz = tools::uresize(&cpu.cpu_hz, width.saturating_sub(4), false);
        out.push_str(&format!("\x1b[{};{}H{}", y + 3, x + 2, hz));
    }

    // Uptime at bottom
    if height > 4 {
        let uptime = tools::sec_to_dhms(cpu.uptime_seconds, false, true);
        out.push_str(&format!(
            "\x1b[{};{}HUp {}",
            y + height - 1,
            x + 2,
            uptime
        ));
    }

    out
}
