use crate::domain::process::ProcInfo;
use crate::draw::box_drawing;
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
) -> String {
    let mut out = box_drawing::create_box(x, y, width, height, "", false, "proc", "", 0, rounded);

    // Header row
    let header = format!("{:<7} {:<20} {:>6} {:>6}", "PID", "Program", "Cpu%", "Mem%");
    let header_trunc = tools::uresize(&header, width.saturating_sub(4), false);
    out.push_str(&format!("\x1b[{};{}H{}", y + 1, x + 2, header_trunc));

    // Process rows
    let max_rows = height.saturating_sub(3);
    for (i, proc) in procs.iter().skip(start).take(max_rows).enumerate() {
        let row = y + 2 + i;
        let mem_pct = if proc.mem > 0 {
            format!("{:.1}", proc.cpu_p)
        } else {
            "0.0".into()
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
            out.push_str(&format!(
                "\x1b[{};{}H\x1b[7m{}\x1b[0m",
                row,
                x + 2,
                line_trunc
            ));
        } else {
            out.push_str(&format!("\x1b[{};{}H{}", row, x + 2, line_trunc));
        }
    }

    out
}
