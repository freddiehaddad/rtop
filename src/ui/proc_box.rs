use crate::domain::process::ProcInfo;
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::theme::Theme;
use crate::tools;

/// Draw the process list box into an ANSI string matching btop's layout.
///
/// Layout:
/// ╭─ proc ───────────────────────────────────╮
/// │ PID    Program              Cpu%    Mem%  │
/// │ 1234   chrome.exe           12.3   1.2G  │
/// │ 5678   code.exe              8.1   0.9G  │
/// │ ...                                      │
/// ╰──────────────────────── 25/350 ──────────╯
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

    let mut out = box_drawing::create_box(
        x, y, width, height, box_color, false, "proc", "", 0, rounded,
    );

    let inner_w = width.saturating_sub(4);
    if inner_w == 0 || height < 3 {
        out.push_str("\x1b[0m");
        return out;
    }

    // Column widths proportional to available space
    let pid_w = 7;
    let cpu_w = 7;
    let mem_w = 7;
    let name_w = inner_w.saturating_sub(pid_w + cpu_w + mem_w + 3); // 3 for spacing

    // Header row with column titles
    let header = format!(
        "{:<pid_w$} {:<name_w$} {:>cpu_w$} {:>mem_w$}",
        "PID",
        "Program",
        "Cpu%",
        "Mem%",
        pid_w = pid_w,
        name_w = name_w,
        cpu_w = cpu_w,
        mem_w = mem_w
    );
    let header_trunc = tools::uresize(&header, inner_w, false);
    out.push_str(&format!(
        "\x1b[{};{}H{}{}\x1b[0m",
        y + 2,
        x + 2,
        title_color,
        header_trunc
    ));

    // Divider line under header
    out.push_str(&format!(
        "\x1b[{};{}H{}{}{}{}",
        y + 3,
        x + 1,
        box_color, symbols::DIV_LEFT,
        inactive,
        symbols::H_LINE.repeat(width.saturating_sub(4))
    ));
    out.push_str(&format!(
        "{}{}",
        box_color, symbols::DIV_RIGHT
    ));

    // Process rows
    let max_rows = height.saturating_sub(5); // -2 border, -1 header, -1 divider, -1 footer
    for (i, proc) in procs.iter().skip(start).take(max_rows).enumerate() {
        let row = y + 4 + i;

        // Format memory value
        let mem_str = if proc.mem > 0 {
            tools::floating_humanizer(proc.mem, true, 0, false, false, false)
        } else {
            "0B".into()
        };

        // Color process by CPU usage
        let cpu_pct = proc.cpu_p.clamp(0.0, 100.0) as usize;
        let proc_color = if !proc_grad.is_empty() {
            &proc_grad[cpu_pct.min(100)]
        } else {
            fg
        };

        let line = format!(
            "{:<pid_w$} {:<name_w$} {:>cpu_w$.1} {:>mem_w$}",
            proc.pid,
            tools::uresize(&proc.name, name_w, false),
            proc.cpu_p,
            mem_str,
            pid_w = pid_w,
            name_w = name_w,
            cpu_w = cpu_w,
            mem_w = mem_w
        );
        let line_trunc = tools::uresize(&line, inner_w, false);

        if i + start == selected {
            // Selected row: highlight with selected colors
            let bg_esc = sel_bg.replace("38;2", "48;2");
            out.push_str(&format!(
                "\x1b[{};{}H{}{}{}{}\x1b[0m",
                row,
                x + 2,
                bg_esc,
                sel_fg,
                tools::ljust(&line_trunc, inner_w, false),
                "\x1b[49m"
            ));
        } else {
            out.push_str(&format!(
                "\x1b[{};{}H{}{}",
                row,
                x + 2,
                proc_color,
                line_trunc
            ));
        }
    }

    // Footer: process count right-aligned on bottom border
    let visible = procs.len().min(max_rows);
    let count_str = format!("{}/{}", visible, procs.len());
    let count_x = x + width.saturating_sub(count_str.len() + 3);
    out.push_str(&format!(
        "\x1b[{};{}H{} {} ",
        y + height,
        count_x,
        fg,
        count_str
    ));

    out.push_str("\x1b[0m");
    out
}
