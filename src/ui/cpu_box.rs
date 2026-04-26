use crate::domain::cpu::CpuInfo;
use crate::draw::box_drawing;
use crate::theme::Theme;
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
    theme: &Theme,
) -> String {
    let box_color = theme.c("cpu_box");
    let title_color = theme.c("title");
    let fg = theme.c("main_fg");
    let hi = theme.c("hi_fg");

    let mut out = box_drawing::create_box(x, y, width, height, box_color, false, "cpu", "", 0, rounded);

    // CPU name and total usage with gradient color
    if let Some(total) = cpu.cpu_percent.get("total") {
        if let Some(&pct) = total.back() {
            let cpu_gradient = theme.g("cpu");
            let pct_color = if !cpu_gradient.is_empty() {
                &cpu_gradient[pct.clamp(0, 100) as usize]
            } else {
                fg
            };
            let info = format!(
                " {}{} {}{}{}%{}",
                fg, cpu.cpu_name, pct_color, pct, fg, "\x1b[0m"
            );
            let info_truncated = tools::uresize(&info, width.saturating_sub(4) + 40, false); // extra for escapes
            out.push_str(&format!("\x1b[{};{}H{}", y + 2, x + 2, info_truncated));
        }
    }

    // Frequency
    if !cpu.cpu_hz.is_empty() && height > 3 {
        let hz = tools::uresize(&cpu.cpu_hz, width.saturating_sub(4), false);
        out.push_str(&format!("\x1b[{};{}H{}{}", y + 3, x + 2, fg, hz));
    }

    // Per-core mini display
    if height > 5 {
        let cols = 2.min(width.saturating_sub(4) / 20);
        if cols > 0 {
            let cpu_gradient = theme.g("cpu");
            for (i, core_data) in cpu.core_percent.iter().enumerate() {
                let col = i % cols;
                let row = i / cols;
                let cy = y + 4 + row;
                if cy >= y + height - 2 {
                    break;
                }
                let cx = x + 2 + col * (width.saturating_sub(4)) / cols;
                let pct = core_data.back().copied().unwrap_or(0);
                let pct_color = if !cpu_gradient.is_empty() {
                    &cpu_gradient[pct.clamp(0, 100) as usize]
                } else {
                    fg
                };
                out.push_str(&format!(
                    "\x1b[{};{}H{}{:<2} {}{}%{}",
                    cy, cx, fg, i, pct_color, pct, "\x1b[0m"
                ));
            }
        }
    }

    // Uptime at bottom
    if height > 4 {
        let uptime = tools::sec_to_dhms(cpu.uptime_seconds, false, true);
        out.push_str(&format!(
            "\x1b[{};{}H{}Up {}",
            y + height - 1,
            x + 2,
            fg,
            uptime
        ));
    }

    out.push_str("\x1b[0m");
    out
}
