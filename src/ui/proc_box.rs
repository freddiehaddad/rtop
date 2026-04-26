use crate::domain::process::ProcInfo;
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::box_drawing::title_syms;
use crate::term;
use crate::theme::Theme;
use crate::tools;

use super::{BoxArea, ProcView};

// Process list column widths.
const COL_PID: usize = 7;
const COL_CPU: usize = 7;
const COL_MEM: usize = 7;
/// Minimum inner width to show the Command column.
const CMD_COL_THRESHOLD: usize = 55;
/// Inner width above which the Program column expands.
const WIDE_PROG_THRESHOLD: usize = 70;
const PROG_WIDE: usize = 16;
const PROG_NARROW: usize = 8;
/// Number of spacing characters between columns.
const COL_SPACING: usize = 4;
const COL_SPACING_NO_CMD: usize = 3;

/// Draw the process list box into an ANSI string matching btop's layout.
///
/// Layout:
/// ╭─ proc ───────────────────────────────────╮
/// │ PID    Program              Cpu%    Mem%  │
/// │ 1234   chrome.exe           12.3   1.2G  │
/// │ 5678   code.exe              8.1   0.9G  │
/// │ ...                                      │
/// ╰──────────────────────── 25/350 ──────────╯
/// Draw the process list box with sort indicator on the active column.
pub fn draw_with_sort(
    procs: &[ProcInfo],
    area: &BoxArea,
    view: &ProcView,
    theme: &Theme,
) -> String {
    let x = area.x;
    let y = area.y;
    let width = area.width;
    let height = area.height;
    let rounded = area.rounded;
    let start = view.start;
    let selected = view.selected;
    let sort_by = view.sort_by;
    let sort_reversed = view.sort_reversed;
    let tree_mode = view.tree_mode;
    let detailed_pid = view.detailed_pid;
    let filter = view.filter;
    let filtering = view.filtering;
    let box_color = theme.c("proc_box");
    let fg = theme.c("main_fg");
    let title_color = theme.c("title");
    let hi = theme.c("hi_fg");
    let sel_bg = theme.c("selected_bg");
    let sel_fg = theme.c("selected_fg");
    let inactive = theme.c("inactive_fg");
    let proc_grad = theme.g("process");

    let mut out = box_drawing::create_box(&box_drawing::BoxConfig {
        x, y, width, height, line_color: box_color, fill: true,
        title: "proc", title2: "", num: 4, rounded,
    });

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

    // Column widths — add Command column when wide enough
    let pid_w = COL_PID;
    let cpu_w = COL_CPU;
    let mem_w = COL_MEM;
    let has_cmd_col = inner_w > CMD_COL_THRESHOLD;
    let (name_w, cmd_w) = if has_cmd_col {
        let prog = if inner_w > WIDE_PROG_THRESHOLD { PROG_WIDE } else { PROG_NARROW };
        let cmd = inner_w.saturating_sub(pid_w + prog + cpu_w + mem_w + COL_SPACING);
        (prog, cmd)
    } else {
        (inner_w.saturating_sub(pid_w + cpu_w + mem_w + COL_SPACING_NO_CMD), 0)
    };

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
    out.push_str(&format!("{}{}{}", term::mv(col_x, header_row_y), pid_color, pid_str));
    col_x += pid_w + 1;

    // Program column
    let name_str = format!("{:<name_w$}", name_label, name_w = name_w);
    let name_color = if is_sort("name") { hi } else { title_color };
    out.push_str(&format!("{}{}{}", term::mv(col_x, header_row_y), name_color, name_str));
    col_x += name_w + 1;

    // Command column (when terminal is wide enough)
    if has_cmd_col && cmd_w > 0 {
        let cmd_label = if is_sort("command") { format!("Cmd{arrow}") } else { "Cmd".into() };
        let cmd_str = format!("{:<cmd_w$}", cmd_label, cmd_w = cmd_w);
        let cmd_color = if is_sort("command") { hi } else { title_color };
        out.push_str(&format!("{}{}{}", term::mv(col_x, header_row_y), cmd_color, cmd_str));
        col_x += cmd_w + 1;
    }

    // Cpu% column
    let cpu_str = format!("{:>cpu_w$}", cpu_label, cpu_w = cpu_w);
    let cpu_color = if is_sort("cpu") { hi } else { title_color };
    out.push_str(&format!("{}{}{}", term::mv(col_x, header_row_y), cpu_color, cpu_str));
    col_x += cpu_w + 1;

    // Mem% column
    let mem_str = format!("{:>mem_w$}", mem_label, mem_w = mem_w);
    let mem_color = if is_sort("mem") { hi } else { title_color };
    out.push_str(&format!("{}{}{}\x1b[0m", term::mv(col_x, header_row_y), mem_color, mem_str));

    // Divider line under header
    let div_y = y + 3 + detail_rows;
    out.push_str(&format!(
        "{}{}{}{}{}{}{}",
        term::mv(x + 1, div_y),
        box_color, symbols::DIV_LEFT,
        inactive,
        symbols::H_LINE.repeat(width.saturating_sub(2)),
        box_color, symbols::DIV_RIGHT
    ));

    // If we have a detail panel, draw a divider between detail and header
    if detail_rows > 0 {
        let detail_div_y = y + 1 + detail_rows;
        out.push_str(&format!(
            "{}{}{}{}{}{}{}",
            term::mv(x + 1, detail_div_y),
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

        let line = if has_cmd_col && cmd_w > 0 {
            // Extract just the args from cmd (remove the exe name/path)
            let cmd_display = if proc.cmd.len() > proc.name.len() {
                proc.cmd[proc.name.len()..].trim().to_string()
            } else if proc.cmd != proc.name {
                proc.cmd.clone()
            } else {
                String::new()
            };
            format!(
                "{:<pid_w$} {:<name_w$} {:<cmd_w$} {:>cpu_w$.1} {:>mem_w$}",
                proc.pid,
                tools::uresize(&display_name, name_w, false),
                tools::uresize(&cmd_display, cmd_w, false),
                proc.cpu_p,
                mem_str,
                pid_w = pid_w,
                name_w = name_w,
                cmd_w = cmd_w,
                cpu_w = cpu_w,
                mem_w = mem_w
            )
        } else {
            format!(
                "{:<pid_w$} {:<name_w$} {:>cpu_w$.1} {:>mem_w$}",
                proc.pid,
                tools::uresize(&display_name, name_w, false),
                proc.cpu_p,
                mem_str,
                pid_w = pid_w,
                name_w = name_w,
                cpu_w = cpu_w,
                mem_w = mem_w
            )
        };
        let line_trunc = tools::uresize(&line, inner_w, false);

        if i + start == selected {
            // Selected row: highlight with selected colors
            let bg_esc = sel_bg.replace("38;2", "48;2");
            out.push_str(&format!(
                "{}{}{}{}{}\x1b[0m",
                term::mv(x + 2, row),
                bg_esc,
                sel_fg,
                tools::ljust(&line_trunc, inner_w, false),
                "\x1b[49m"
            ));
        } else {
            out.push_str(&format!(
                "{}{}{}",
                term::mv(x + 2, row),
                proc_color,
                line_trunc
            ));
        }
    }

    // TOP border: reverse, tree, ← sorting → (btop lines 1882-1909)
    let sort_name = if sort_by.is_empty() { "cpu lazy" } else { sort_by };
    let tree_star = if tree_mode { "*" } else { "" };

    // Build positions right-to-left from the right corner
    // btop line 1884: sort_pos = x + width - sort_len - 8
    let mut pos = x + width - sort_name.len() - 7;

    // Sort selector: ┐← sorting →┌
    let sort_inset = format!(
        "{}{}{}← {}{} {}→{}{}",
        box_color, title_syms::TITLE_LEFT,
        hi, title_color, sort_name, hi,
        box_color, title_syms::TITLE_RIGHT,
    );
    out.push_str(&format!("{}{}", term::mv(pos, y + 1), sort_inset));

    // Tree button: ┐tree┌  (visible: "tree" = 4 + star, plus 2 inset chars)
    let tree_content = format!("tre{}{}", tree_star, "e");
    let tree_len = tree_content.len();
    if pos > x + 12 + tree_len {
        pos -= tree_len + 2;
        let tree_inset = format!(
            "{}{}{}tre{}{}e{}{}",
            box_color, title_syms::TITLE_LEFT,
            title_color, tree_star, hi, box_color, title_syms::TITLE_RIGHT,
        );
        out.push_str(&format!("{}{}", term::mv(pos, y + 1), tree_inset));
    }

    // Reverse button: ┐reverse┌  (visible: "reverse" = 7, plus 2 inset chars)
    if pos > x + 12 {
        pos -= 9; // 7 + 2
        let rev_inset = format!(
            "{}{}{}r{}everse{}{}",
            box_color, title_syms::TITLE_LEFT,
            hi, title_color, box_color, title_syms::TITLE_RIGHT,
        );
        out.push_str(&format!("{}{}", term::mv(pos, y + 1), rev_inset));
    }

    // BOTTOM border: ┘↑ select ↓┘ ┘info ↵┘ ┘terminate┘ ┘filter┘ (filter appended at end)
    let bottom_y = y + height;
    let bottom_hints = format!(
        "{}{}{}↑{} select {}↓{}{}{}{}{}info {}↵{}{}{}{}{}t{}erminate{}{}",
        box_color, title_syms::TITLE_LEFT_DOWN,
        hi, title_color, hi, box_color, title_syms::TITLE_RIGHT_DOWN,
        box_color, title_syms::TITLE_LEFT_DOWN,
        title_color, hi, box_color, title_syms::TITLE_RIGHT_DOWN,
        box_color, title_syms::TITLE_LEFT_DOWN,
        hi, title_color, box_color, title_syms::TITLE_RIGHT_DOWN,
    );
    out.push_str(&format!("{}{}", term::mv(x + 3, bottom_y), bottom_hints));

    // Filter label — appended after the other elements
    let cursor = if filtering { "\x1b[4m \x1b[24m" } else { "" };
    let filter_label = if !filter.is_empty() || filtering {
        format!(
            "{}{}{}f{}ilter: {}{}{}{}{}",
            box_color, title_syms::TITLE_LEFT_DOWN,
            hi, title_color,
            fg, filter, cursor,
            box_color, title_syms::TITLE_RIGHT_DOWN,
        )
    } else {
        format!(
            "{}{}{}f{}ilter{}{}",
            box_color, title_syms::TITLE_LEFT_DOWN,
            hi, title_color,
            box_color, title_syms::TITLE_RIGHT_DOWN,
        )
    };
    out.push_str(&filter_label);

    // Right side: process count with border inset chars
    let visible = procs.len().min(max_rows);
    let count_str = format!("{}/{}", visible, procs.len());
    let count_x = x + width.saturating_sub(count_str.len() + 3);
    out.push_str(&format!(
        "{}{}{}{}{}{}{}",
        term::mv(count_x, bottom_y),
        box_color, title_syms::TITLE_LEFT_DOWN,
        fg, count_str,
        box_color, title_syms::TITLE_RIGHT_DOWN,
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
            "{}{}{}",
            term::mv(title_x, y + 2),
            hi,
            tools::uresize(&detail_title, inner_w, false)
        ));
    }

    // Row 1: Command
    if rows > 1 {
        let cmd_line = format!("{}Cmd: {}{}", title_color, fg, tools::uresize(&proc.cmd, inner_w.saturating_sub(5), false));
        out.push_str(&format!("{}{}", term::mv(title_x, y + 3), cmd_line));
    }

    // Row 2: User and status
    if rows > 2 {
        let info = format!(
            "{}User: {}{:<12} {}Status: {}{}",
            title_color, fg, tools::uresize(&proc.user, 12, false),
            title_color, fg, proc.state
        );
        out.push_str(&format!("{}{}", term::mv(title_x, y + 4), tools::uresize(&info, inner_w, false)));
    }

    // Row 3: Threads, PPID
    if rows > 3 {
        let info = format!(
            "{}Threads: {}{:<6} {}Parent: {}{}",
            title_color, fg, proc.threads,
            title_color, fg, proc.ppid
        );
        out.push_str(&format!("{}{}", term::mv(title_x, y + 5), tools::uresize(&info, inner_w, false)));
    }

    // Row 4: CPU and Memory
    if rows > 4 {
        let mem_str = tools::floating_humanizer(proc.mem, false, 0, false, false, false);
        let info = format!(
            "{}Cpu: {}{:.1}%    {}Mem: {}{}",
            title_color, fg, proc.cpu_p,
            title_color, fg, mem_str
        );
        out.push_str(&format!("{}{}", term::mv(title_x, y + 6), tools::uresize(&info, inner_w, false)));
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
        out.push_str(&format!("{}{}", term::mv(title_x, y + 7), tools::uresize(&info, inner_w, false)));
    }

    // Row 6: Priority
    if rows > 6 {
        let info = format!("{}Priority: {}{}", title_color, fg, proc.priority);
        out.push_str(&format!("{}{}", term::mv(title_x, y + 8), tools::uresize(&info, inner_w, false)));
    }

    out
}
