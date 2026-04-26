use crate::domain::process::ProcInfo;
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::box_drawing::title_syms;
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
    draw_with_sort(procs, x, y, width, height, rounded, start, selected, theme, "", false, false, 0)
}

/// Draw the process list box with sort indicator on the active column.
pub fn draw_with_sort(
    procs: &[ProcInfo],
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    rounded: bool,
    start: usize,
    selected: usize,
    theme: &Theme,
    sort_by: &str,
    sort_reversed: bool,
    tree_mode: bool,
    detailed_pid: u32,
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
        x, y, width, height, box_color, true, "proc", "", 4, rounded,
    );

    let inner_w = width.saturating_sub(4);
    if inner_w == 0 || height < 3 {
        out.push_str("\x1b[0m");
        return out;
    }

    // Detailed view panel
    let detail_rows = if detailed_pid > 0 { 8_usize.min(height.saturating_sub(6)) } else { 0 };
    if detailed_pid > 0 && detail_rows > 0 {
        if let Some(proc) = procs.iter().find(|p| p.pid == detailed_pid) {
            out.push_str(&draw_detail_panel(proc, x, y, width, detail_rows, theme));
        }
    }

    // Column widths proportional to available space
    let pid_w = 7;
    let cpu_w = 7;
    let mem_w = 7;
    let name_w = inner_w.saturating_sub(pid_w + cpu_w + mem_w + 3); // 3 for spacing

    // Header row with column titles and sort indicator
    let arrow = if sort_reversed { "▼" } else { "▲" };
    let sort_lower = sort_by.to_lowercase();
    let is_sort = |col: &str| -> bool {
        match col {
            "pid" => sort_lower == "pid",
            "name" | "command" => sort_lower == "name" || sort_lower == "command",
            "cpu" => sort_lower.starts_with("cpu"),
            "mem" | "memory" => sort_lower == "memory",
            _ => false,
        }
    };
    let pid_label = if is_sort("pid") { format!("PID{arrow}") } else { "PID".into() };
    let name_label = if is_sort("name") { format!("Program{arrow}") } else { "Program".into() };
    let cpu_label = if is_sort("cpu") { format!("Cpu%{arrow}") } else { "Cpu%".into() };
    let mem_label = if is_sort("mem") { format!("Mem%{arrow}") } else { "Mem%".into() };

    // Build header with per-column coloring
    let header_row_y = y + 2 + detail_rows;
    let mut col_x = x + 2;

    // PID column
    let pid_str = format!("{:<pid_w$}", pid_label, pid_w = pid_w);
    let pid_color = if is_sort("pid") { hi } else { title_color };
    out.push_str(&format!("\x1b[{};{}H{}{}", header_row_y, col_x, pid_color, pid_str));
    col_x += pid_w + 1;

    // Program column
    let name_str = format!("{:<name_w$}", name_label, name_w = name_w);
    let name_color = if is_sort("name") { hi } else { title_color };
    out.push_str(&format!("\x1b[{};{}H{}{}", header_row_y, col_x, name_color, name_str));
    col_x += name_w + 1;

    // Cpu% column
    let cpu_str = format!("{:>cpu_w$}", cpu_label, cpu_w = cpu_w);
    let cpu_color = if is_sort("cpu") { hi } else { title_color };
    out.push_str(&format!("\x1b[{};{}H{}{}", header_row_y, col_x, cpu_color, cpu_str));
    col_x += cpu_w + 1;

    // Mem% column
    let mem_str = format!("{:>mem_w$}", mem_label, mem_w = mem_w);
    let mem_color = if is_sort("mem") { hi } else { title_color };
    out.push_str(&format!("\x1b[{};{}H{}{}\x1b[0m", header_row_y, col_x, mem_color, mem_str));

    // Divider line under header
    let div_y = y + 3 + detail_rows;
    out.push_str(&format!(
        "\x1b[{};{}H{}{}{}{}{}{}",
        div_y,
        x + 1,
        box_color, symbols::DIV_LEFT,
        inactive,
        symbols::H_LINE.repeat(width.saturating_sub(2)),
        box_color, symbols::DIV_RIGHT
    ));

    // If we have a detail panel, draw a divider between detail and header
    if detail_rows > 0 {
        let detail_div_y = y + 1 + detail_rows;
        out.push_str(&format!(
            "\x1b[{};{}H{}{}{}{}{}{}",
            detail_div_y,
            x + 1,
            box_color, symbols::DIV_LEFT,
            inactive,
            symbols::H_LINE.repeat(width.saturating_sub(2)),
            box_color, symbols::DIV_RIGHT
        ));
    }

    // Process rows
    let header_overhead = 3 + detail_rows; // border + header + divider + detail
    let max_rows = height.saturating_sub(header_overhead + 2); // -2 for top/bottom border
    for (i, proc) in procs.iter().skip(start).take(max_rows).enumerate() {
        let row = y + 4 + detail_rows + i;

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

        // Apply tree prefix to name if in tree mode
        let display_name = if tree_mode && !proc.prefix.is_empty() {
            format!("{}{}", proc.prefix, proc.name)
        } else {
            proc.name.clone()
        };

        let line = format!(
            "{:<pid_w$} {:<name_w$} {:>cpu_w$.1} {:>mem_w$}",
            proc.pid,
            tools::uresize(&display_name, name_w, false),
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

    // Bottom border keybind hints
    let bottom_y = y + height;

    // Left side: filter, tree toggle, sort column
    let sort_name = if sort_by.is_empty() { "cpu lazy" } else { sort_by };
    let tree_star = if tree_mode { "*" } else { "" };
    let left_hints = format!(
        "{}{}f{}ilter {}{}{}e{}{}tree{} {}{}←{}{}{}{}→{}",
        title_syms::TITLE_LEFT_DOWN,
        hi, fg,
        title_syms::TITLE_RIGHT_DOWN,
        title_syms::TITLE_LEFT_DOWN,
        hi, fg, tree_star,
        title_syms::TITLE_RIGHT_DOWN,
        title_syms::TITLE_LEFT_DOWN,
        hi, fg, sort_name, hi, fg,
        title_syms::TITLE_RIGHT_DOWN,
    );
    out.push_str(&format!(
        "\x1b[{};{}H{}{}",
        bottom_y, x + 2,
        box_color,
        left_hints
    ));

    // Right side: process count
    let visible = procs.len().min(max_rows);
    let count_str = format!("{}/{}", visible, procs.len());
    let count_x = x + width.saturating_sub(count_str.len() + 3);
    out.push_str(&format!(
        "\x1b[{};{}H{} {} ",
        bottom_y,
        count_x,
        fg,
        count_str
    ));

    out.push_str("\x1b[0m");
    out
}

/// Draw the detailed process info panel at the top of the proc box.
fn draw_detail_panel(
    proc: &ProcInfo,
    x: usize,
    y: usize,
    width: usize,
    rows: usize,
    theme: &Theme,
) -> String {
    let fg = theme.c("main_fg");
    let title_color = theme.c("title");
    let hi = theme.c("hi_fg");
    let inner_w = width.saturating_sub(4);

    let mut out = String::new();

    // Row 0: Title showing PID and name
    let detail_title = format!(" {} [{}] ", proc.name, proc.pid);
    let title_x = x + 2;
    if rows > 0 {
        out.push_str(&format!(
            "\x1b[{};{}H{}{}",
            y + 2, title_x,
            hi,
            tools::uresize(&detail_title, inner_w, false)
        ));
    }

    // Row 1: Command
    if rows > 1 {
        let cmd_line = format!("{}Cmd: {}{}", title_color, fg, tools::uresize(&proc.cmd, inner_w.saturating_sub(5), false));
        out.push_str(&format!("\x1b[{};{}H{}", y + 3, title_x, cmd_line));
    }

    // Row 2: User and status
    if rows > 2 {
        let info = format!(
            "{}User: {}{:<12} {}Status: {}{}",
            title_color, fg, tools::uresize(&proc.user, 12, false),
            title_color, fg, proc.state
        );
        out.push_str(&format!("\x1b[{};{}H{}", y + 4, title_x, tools::uresize(&info, inner_w, false)));
    }

    // Row 3: Threads, PPID
    if rows > 3 {
        let info = format!(
            "{}Threads: {}{:<6} {}Parent: {}{}",
            title_color, fg, proc.threads,
            title_color, fg, proc.ppid
        );
        out.push_str(&format!("\x1b[{};{}H{}", y + 5, title_x, tools::uresize(&info, inner_w, false)));
    }

    // Row 4: CPU and Memory
    if rows > 4 {
        let mem_str = tools::floating_humanizer(proc.mem, false, 0, false, false, false);
        let info = format!(
            "{}Cpu: {}{:.1}%    {}Mem: {}{}",
            title_color, fg, proc.cpu_p,
            title_color, fg, mem_str
        );
        out.push_str(&format!("\x1b[{};{}H{}", y + 6, title_x, tools::uresize(&info, inner_w, false)));
    }

    // Row 5: IO
    if rows > 5 {
        let io_r = tools::floating_humanizer(proc.io_read, true, 0, false, false, false);
        let io_w = tools::floating_humanizer(proc.io_write, true, 0, false, false, false);
        let info = format!(
            "{}IO Read: {}{:<8} {}IO Write: {}{}",
            title_color, fg, io_r,
            title_color, fg, io_w
        );
        out.push_str(&format!("\x1b[{};{}H{}", y + 7, title_x, tools::uresize(&info, inner_w, false)));
    }

    // Row 6: Priority
    if rows > 6 {
        let info = format!("{}Priority: {}{}", title_color, fg, proc.priority);
        out.push_str(&format!("\x1b[{};{}H{}", y + 8, title_x, tools::uresize(&info, inner_w, false)));
    }

    out
}
