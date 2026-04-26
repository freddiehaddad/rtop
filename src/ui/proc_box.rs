use crate::domain::process::ProcInfo;
use crate::draw::box_drawing;
use crate::theme::Theme;
use crate::tools;

/// Draw the process list box into an ANSI string.
pub fn draw(
    procs: &[ProcInfo],
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    rounded: bool,
    start: usize,
    selected: usize,
    theme: &Theme,
) -> String {
    let box_color = theme.c("proc_box");
    let fg = theme.c("main_fg");
    let title_color = theme.c("title");
    let hi = theme.c("hi_fg");
    let sel_bg = theme.c("selected_bg");
    let sel_fg = theme.c("selected_fg");
    let inactive = theme.c("inactive_fg");
    let proc_grad = theme.g("process");

    let mut out = box_drawing::create_box(x, y, width, height, box_color, false, "proc", "", 0, rounded);

    // Header row
    let header = format!("{:<7} {:<20} {:>6} {:>6}", "PID", "Program", "Cpu%", "Mem%");
    let header_trunc = tools::uresize(&header, width.saturating_sub(4), false);
    out.push_str(&format!("\x1b[{};{}H{}{}\x1b[0m", y + 1, x + 2, title_color, header_trunc));

    // Process rows
    let max_rows = height.saturating_sub(3);
    for (i, proc) in procs.iter().skip(start).take(max_rows).enumerate() {
        let row = y + 2 + i;
        let mem_pct = if proc.mem > 0 {
            format!("{:.1}", proc.cpu_p)
        } else {
            "0.0".into()
        };

        // Color process by CPU usage
        let cpu_pct = proc.cpu_p.clamp(0.0, 100.0) as usize;
        let proc_color = if !proc_grad.is_empty() {
            &proc_grad[cpu_pct.min(100)]
        } else {
            fg
        };

        let line = format!(
            "{:<7} {:<20} {:>6.1} {:>6}",
            proc.pid,
            tools::uresize(&proc.name, 20, false),
            proc.cpu_p,
            mem_pct
        );
        let line_trunc = tools::uresize(&line, width.saturating_sub(4), false);

        if i + start == selected {
            // Selected row: use selected_bg/fg colors
            out.push_str(&format!(
                "\x1b[{};{}H{}{}{}\x1b[0m",
                row, x + 2,
                sel_bg.replace("38;2", "48;2"), // Convert fg escape to bg
                sel_fg,
                line_trunc
            ));
        } else {
            out.push_str(&format!(
                "\x1b[{};{}H{}{}",
                row, x + 2, proc_color, line_trunc
            ));
        }
    }

    // Process count at bottom
    let count_str = format!("{}{}/{}", fg, procs.len().min(max_rows), procs.len());
    out.push_str(&format!(
        "\x1b[{};{}H{}",
        y + height - 1,
        x + width.saturating_sub(count_str.len() + 15), // rough right-align
        count_str
    ));

    out.push_str("\x1b[0m");
    out
}

