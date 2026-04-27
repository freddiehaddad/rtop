use crate::domain::process::ProcInfo;
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::buffer::AnsiBuffer;
use crate::theme::Theme;
use crate::theme_keys as tc;
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
    let box_color = theme.c(tc::PROC_BOX);
    let fg = theme.c(tc::MAIN_FG);
    let title_color = theme.c(tc::TITLE);
    let hi = theme.c(tc::HI_FG);
    let sel_bg_esc = theme.bg(tc::SELECTED_BG);
    let sel_fg = theme.c(tc::SELECTED_FG);
    let tree_fg = theme.c(tc::PROC_TREE_FG);
    let proc_grad = theme.g(tc::GRAD_PROCESS);

    let mut buf = AnsiBuffer::new();
    buf.text(&box_drawing::create_box(&box_drawing::BoxConfig {
        x,
        y,
        width,
        height,
        line_color: box_color,
        fill: true,
        title: "proc",
        title2: "",
        num: 4,
        rounded,
        hi_color: hi,
        title_color,
    }));

    let inner_w = width.saturating_sub(4);
    if inner_w == 0 || height < 3 {
        return buf.finish();
    }

    // Detailed view panel
    let detail_rows = if detailed_pid > 0 {
        8_usize.min(height.saturating_sub(6))
    } else {
        0
    };
    if detailed_pid > 0 && detail_rows > 0 {
        if let Some(proc) = procs.iter().find(|p| p.pid == detailed_pid) {
            buf.text(&draw_detail_panel(proc, x, y, width, detail_rows, theme));
        }
    }

    // Column widths — add Command column when wide enough
    let pid_w = COL_PID;
    let cpu_w = COL_CPU;
    let mem_w = COL_MEM;
    let has_cmd_col = inner_w > CMD_COL_THRESHOLD;
    let (name_w, cmd_w) = if has_cmd_col {
        let prog = if tree_mode {
            // Tree prefixes need more room
            if inner_w > WIDE_PROG_THRESHOLD {
                PROG_WIDE + 8
            } else {
                PROG_NARROW + 8
            }
        } else if inner_w > WIDE_PROG_THRESHOLD {
            PROG_WIDE
        } else {
            PROG_NARROW
        };
        let cmd = inner_w.saturating_sub(pid_w + prog + cpu_w + mem_w + COL_SPACING);
        (prog, cmd)
    } else {
        let prog = if tree_mode {
            inner_w
                .saturating_sub(pid_w + cpu_w + mem_w + COL_SPACING_NO_CMD)
                .max(PROG_NARROW + 8)
        } else {
            inner_w.saturating_sub(pid_w + cpu_w + mem_w + COL_SPACING_NO_CMD)
        };
        (prog, 0)
    };

    // Header row with column titles and sort indicator
    let arrow = if sort_reversed { "▼" } else { "▲" };
    let sort_lower = sort_by.to_lowercase();
    let is_sort = |col: &str| -> bool {
        match col {
            "pid" => sort_lower == "pid",
            "name" => sort_lower == "name",
            "command" => sort_lower == "command",
            "cpu" => sort_lower.starts_with("cpu"),
            "mem" | "memory" => sort_lower == "memory",
            _ => false,
        }
    };
    let pid_label = if is_sort("pid") {
        format!("PID{arrow}")
    } else {
        "PID".into()
    };
    let name_label = if is_sort("name") {
        format!("Program{arrow}")
    } else {
        "Program".into()
    };
    let cpu_label = if is_sort("cpu") {
        format!("Cpu%{arrow}")
    } else {
        "Cpu%".into()
    };
    let mem_label = if is_sort("mem") {
        format!("Mem%{arrow}")
    } else {
        "Mem%".into()
    };

    // Build header with per-column coloring
    let header_row_y = y + 2 + detail_rows;
    let mut col_x = x + 2;

    // PID column
    let pid_str = format!("{:<pid_w$}", pid_label, pid_w = pid_w);
    let pid_color = if is_sort("pid") { hi } else { title_color };
    buf.mv(col_x, header_row_y).color(pid_color).text(&pid_str);
    col_x += pid_w + 1;

    // Program column
    let name_str = format!("{:<name_w$}", name_label, name_w = name_w);
    let name_color = if is_sort("name") { hi } else { title_color };
    buf.mv(col_x, header_row_y)
        .color(name_color)
        .text(&name_str);
    col_x += name_w + 1;

    // Command column (when terminal is wide enough)
    if has_cmd_col && cmd_w > 0 {
        let cmd_label = if is_sort("command") {
            format!("Command Line{arrow}")
        } else {
            "Command Line".into()
        };
        let cmd_str = format!("{:<cmd_w$}", cmd_label, cmd_w = cmd_w);
        let cmd_color = if is_sort("command") { hi } else { title_color };
        buf.mv(col_x, header_row_y).color(cmd_color).text(&cmd_str);
        col_x += cmd_w + 1;
    }

    // Cpu% column
    let cpu_str = format!("{:>cpu_w$}", cpu_label, cpu_w = cpu_w);
    let cpu_color = if is_sort("cpu") { hi } else { title_color };
    buf.mv(col_x, header_row_y).color(cpu_color).text(&cpu_str);
    col_x += cpu_w + 1;

    // Mem% column
    let mem_str = format!("{:>mem_w$}", mem_label, mem_w = mem_w);
    let mem_color = if is_sort("mem") { hi } else { title_color };
    buf.mv(col_x, header_row_y)
        .color(mem_color)
        .text(&mem_str)
        .reset();

    // Divider line under header
    let div_y = y + 3 + detail_rows;
    buf.mv(x + 1, div_y)
        .color(box_color)
        .text(symbols::DIV_LEFT)
        .color(box_color)
        .text(&symbols::H_LINE.repeat(width.saturating_sub(2)))
        .color(box_color)
        .text(symbols::DIV_RIGHT);

    // If we have a detail panel, draw a divider between detail and header
    if detail_rows > 0 {
        let detail_div_y = y + 1 + detail_rows;
        buf.mv(x + 1, detail_div_y)
            .color(box_color)
            .text(symbols::DIV_LEFT)
            .color(box_color)
            .text(&symbols::H_LINE.repeat(width.saturating_sub(2)))
            .color(box_color)
            .text(symbols::DIV_RIGHT);
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

        // Tree prefix rendered separately in tree_fg color
        let (tree_prefix, bare_name) = if tree_mode && !proc.prefix.is_empty() {
            (proc.prefix.as_str(), proc.name.as_str())
        } else {
            ("", proc.name.as_str())
        };
        let prefix_w = tools::ulen(tree_prefix, false);
        let name_avail = name_w.saturating_sub(prefix_w);
        let display_name = tools::uresize(bare_name, name_avail, false);

        // Build the line without the name column (we render it separately for tree coloring)
        let pid_str = format!("{:<pid_w$}", proc.pid, pid_w = pid_w);
        let cpu_str = format!("{:>cpu_w$.1}", proc.cpu_p, cpu_w = cpu_w);
        let mem_str_fmt = format!("{:>mem_w$}", mem_str, mem_w = mem_w);

        let cmd_display = if has_cmd_col && cmd_w > 0 {
            let raw = if proc.cmd.len() > proc.name.len() {
                proc.cmd[proc.name.len()..].trim()
            } else if proc.cmd != proc.name {
                &proc.cmd
            } else {
                ""
            };
            tools::uresize(raw, cmd_w, false)
        } else {
            String::new()
        };

        let name_padded = tools::ljust(&display_name, name_avail, false);

        if i + start == selected {
            let bg_esc = &sel_bg_esc;
            buf.mv(x + 2, row).text(bg_esc).color(sel_fg);
            buf.text(&pid_str).text(" ");
            if !tree_prefix.is_empty() {
                buf.text(tree_prefix);
            }
            buf.text(&name_padded);
            if prefix_w + name_avail < name_w {
                buf.text(&" ".repeat(name_w - prefix_w - name_avail));
            }
            buf.text(" ");
            if has_cmd_col && cmd_w > 0 {
                buf.text(&format!("{:<cmd_w$}", cmd_display, cmd_w = cmd_w));
                buf.text(" ");
            }
            buf.text(&cpu_str).text(" ").text(&mem_str_fmt);
            buf.text("\x1b[49m").reset();
        } else {
            buf.mv(x + 2, row).color(proc_color);
            buf.text(&pid_str).text(" ");
            if !tree_prefix.is_empty() {
                buf.color(tree_fg).text(tree_prefix).color(proc_color);
            }
            buf.text(&name_padded);
            if prefix_w + name_avail < name_w {
                buf.text(&" ".repeat(name_w - prefix_w - name_avail));
            }
            buf.text(" ");
            if has_cmd_col && cmd_w > 0 {
                buf.text(&format!("{:<cmd_w$}", cmd_display, cmd_w = cmd_w));
                buf.text(" ");
            }
            buf.text(&cpu_str).text(" ").text(&mem_str_fmt);
        }
    }

    // TOP border: reverse, tree, sort
    buf.text(&draw_top_border(x, y, width, sort_by, tree_mode, theme));

    // BOTTOM border
    let visible = procs.len().min(max_rows);
    buf.text(&draw_bottom_border(
        &BottomBorderParams {
            x,
            bottom_y: y + height,
            width,
            filter,
            filtering,
            visible,
            total: procs.len(),
        },
        theme,
    ));

    buf.finish()
}

/// Render the top border with reverse, tree, and sort selector labels.
fn draw_top_border(
    x: usize,
    y: usize,
    width: usize,
    sort_by: &str,
    tree_mode: bool,
    theme: &Theme,
) -> String {
    let box_color = theme.c(tc::PROC_BOX);
    let hi = theme.c(tc::HI_FG);
    let title_color = theme.c(tc::TITLE);
    let mut buf = AnsiBuffer::new();

    let sort_name = if sort_by.is_empty() {
        "cpu lazy"
    } else {
        sort_by
    };
    let tree_star = if tree_mode { "*" } else { "" };

    // Build positions right-to-left from the right corner
    let mut pos = x + width - sort_name.len() - 7;

    // Sort selector: ┐← sorting →┌
    let sort_text = format!("← {}{} {}→", title_color, sort_name, hi);
    let sort_inset = box_drawing::title_inset(&sort_text, box_color, hi, false);
    buf.mv(pos, y + 1).text(&sort_inset);

    // Tree button: ┐tree┌
    let tree_content = format!("tre{}{}", tree_star, "e");
    let tree_len = tree_content.len();
    if pos > x + 12 + tree_len {
        pos -= tree_len + 2;
        let tree_text = format!("tre{}{}e", tree_star, hi);
        let tree_inset = box_drawing::title_inset(&tree_text, box_color, title_color, false);
        buf.mv(pos, y + 1).text(&tree_inset);
    }

    // Reverse button: ┐reverse┌
    if pos > x + 12 {
        pos -= 9;
        let rev_inset = box_drawing::keybind_inset("reverse", box_color, hi, title_color, false);
        buf.mv(pos, y + 1).text(&rev_inset);
    }

    buf.finish()
}

/// Parameters for the proc bottom border rendering.
struct BottomBorderParams<'a> {
    x: usize,
    bottom_y: usize,
    width: usize,
    filter: &'a str,
    filtering: bool,
    visible: usize,
    total: usize,
}

/// Render the bottom border with select, info, terminate, and filter labels.
fn draw_bottom_border(p: &BottomBorderParams, theme: &Theme) -> String {
    let box_color = theme.c(tc::PROC_BOX);
    let fg = theme.c(tc::MAIN_FG);
    let hi = theme.c(tc::HI_FG);
    let title_color = theme.c(tc::TITLE);
    let mut buf = AnsiBuffer::new();

    let select_text = format!("↑{} select {}↓", title_color, hi);
    let select_inset = box_drawing::title_inset(&select_text, box_color, hi, true);
    let info_text = format!("info {}↵", hi);
    let info_inset = box_drawing::title_inset(&info_text, box_color, title_color, true);
    let term_inset = box_drawing::keybind_inset("terminate", box_color, hi, title_color, true);
    let bottom_hints = format!("{}{}{}", select_inset, info_inset, term_inset);
    buf.mv(p.x + 3, p.bottom_y).text(&bottom_hints);

    // Filter label
    let cursor = if p.filtering { "\x1b[4m \x1b[24m" } else { "" };
    let filter_label = if !p.filter.is_empty() || p.filtering {
        let filter_text = format!("filter: {}{}{}", fg, p.filter, cursor);
        box_drawing::keybind_inset(&filter_text, box_color, hi, title_color, true)
    } else {
        box_drawing::keybind_inset("filter", box_color, hi, title_color, true)
    };
    buf.text(&filter_label);

    // Right side: process count with border inset chars
    let count_str = format!("{}/{}", p.visible, p.total);
    let count_x = p.x + p.width.saturating_sub(count_str.len() + 3);
    buf.mv(count_x, p.bottom_y)
        .text(&box_drawing::title_inset(&count_str, box_color, fg, true));

    buf.finish()
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
    let fg = theme.c(tc::MAIN_FG);
    let title_color = theme.c(tc::TITLE);
    let hi = theme.c(tc::HI_FG);
    let inner_w = width.saturating_sub(4);

    let mut buf = AnsiBuffer::new();

    // Row 0: Title showing PID and name
    let detail_title = format!(" {} [{}] ", proc.name, proc.pid);
    let title_x = x + 2;
    if rows > 0 {
        buf.mv(title_x, y + 2)
            .color(hi)
            .text(&tools::uresize(&detail_title, inner_w, false));
    }

    // Row 1: Command
    if rows > 1 {
        let cmd_line = format!(
            "{}Cmd: {}{}",
            title_color,
            fg,
            tools::uresize(&proc.cmd, inner_w.saturating_sub(5), false)
        );
        buf.mv(title_x, y + 3).text(&cmd_line);
    }

    // Row 2: User and status
    if rows > 2 {
        let info = format!(
            "{}User: {}{:<12} {}Status: {}{}",
            title_color,
            fg,
            tools::uresize(&proc.user, 12, false),
            title_color,
            fg,
            proc.state
        );
        buf.mv(title_x, y + 4)
            .text(&tools::uresize(&info, inner_w, false));
    }

    // Row 3: Threads, PPID
    if rows > 3 {
        let info = format!(
            "{}Threads: {}{:<6} {}Parent: {}{}",
            title_color, fg, proc.threads, title_color, fg, proc.ppid
        );
        buf.mv(title_x, y + 5)
            .text(&tools::uresize(&info, inner_w, false));
    }

    // Row 4: CPU and Memory
    if rows > 4 {
        let mem_str = tools::floating_humanizer(proc.mem, false, 0, false, false, false);
        let info = format!(
            "{}Cpu: {}{:.1}%    {}Mem: {}{}",
            title_color, fg, proc.cpu_p, title_color, fg, mem_str
        );
        buf.mv(title_x, y + 6)
            .text(&tools::uresize(&info, inner_w, false));
    }

    // Row 5: IO
    if rows > 5 {
        let io_r = tools::floating_humanizer(proc.io_read, true, 0, false, false, false);
        let io_w = tools::floating_humanizer(proc.io_write, true, 0, false, false, false);
        let info = format!(
            "{}IO Read: {}{:<8} {}IO Write: {}{}",
            title_color, fg, io_r, title_color, fg, io_w
        );
        buf.mv(title_x, y + 7)
            .text(&tools::uresize(&info, inner_w, false));
    }

    // Row 6: Priority
    if rows > 6 {
        let info = format!("{}Priority: {}{}", title_color, fg, proc.priority);
        buf.mv(title_x, y + 8)
            .text(&tools::uresize(&info, inner_w, false));
    }

    buf.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::process::{PriorityClass, ProcState};

    fn strip_ansi(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut in_escape = false;
        for ch in s.chars() {
            if in_escape {
                if ch.is_ascii_alphabetic() || ch == 'm' {
                    in_escape = false;
                }
                continue;
            }
            if ch == '\x1b' {
                in_escape = true;
                continue;
            }
            result.push(ch);
        }
        result
    }

    fn make_procs() -> Vec<ProcInfo> {
        vec![
            ProcInfo {
                pid: 100,
                name: "alpha.exe".into(),
                cmd: "alpha.exe --flag".into(),
                threads: 4,
                user: "SYSTEM".into(),
                mem: 1024 * 1024 * 100,
                cpu_p: 12.5,
                state: ProcState::Running,
                priority: PriorityClass::Normal,
                ppid: 1,
                cpu_time: 0,
                io_read: 0,
                io_write: 0,
                prefix: String::new(),
                depth: 0,
                tree_index: 0,
            },
            ProcInfo {
                pid: 200,
                name: "beta.exe".into(),
                cmd: "beta.exe".into(),
                threads: 2,
                user: "User".into(),
                mem: 1024 * 1024 * 50,
                cpu_p: 5.0,
                state: ProcState::Running,
                priority: PriorityClass::Normal,
                ppid: 1,
                cpu_time: 0,
                io_read: 0,
                io_write: 0,
                prefix: String::new(),
                depth: 0,
                tree_index: 1,
            },
            ProcInfo {
                pid: 300,
                name: "gamma.exe".into(),
                cmd: "gamma.exe --verbose".into(),
                threads: 8,
                user: "Admin".into(),
                mem: 1024 * 1024 * 200,
                cpu_p: 25.0,
                state: ProcState::Running,
                priority: PriorityClass::High,
                ppid: 1,
                cpu_time: 0,
                io_read: 0,
                io_write: 0,
                prefix: String::new(),
                depth: 0,
                tree_index: 2,
            },
        ]
    }

    fn make_area() -> BoxArea {
        BoxArea {
            x: 1,
            y: 1,
            width: 80,
            height: 20,
            rounded: true,
        }
    }

    fn make_view() -> ProcView<'static> {
        ProcView {
            start: 0,
            selected: 0,
            sort_by: "cpu lazy",
            sort_reversed: false,
            tree_mode: false,
            detailed_pid: 0,
            filter: "",
            filtering: false,
        }
    }

    #[test]
    fn draw_contains_proc_title() {
        let output = draw_with_sort(&make_procs(), &make_area(), &make_view(), &Theme::default());
        let plain = strip_ansi(&output);
        assert!(plain.contains("proc"), "output should contain 'proc' title");
    }

    #[test]
    fn draw_contains_process_names() {
        let output = draw_with_sort(&make_procs(), &make_area(), &make_view(), &Theme::default());
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("alpha.exe"),
            "output should contain 'alpha.exe'"
        );
        assert!(
            plain.contains("beta.exe"),
            "output should contain 'beta.exe'"
        );
        assert!(
            plain.contains("gamma.exe"),
            "output should contain 'gamma.exe'"
        );
    }

    #[test]
    fn draw_contains_sort_column_indicator() {
        let output = draw_with_sort(&make_procs(), &make_area(), &make_view(), &Theme::default());
        let plain = strip_ansi(&output);
        assert!(
            plain.contains('▲') || plain.contains('▼'),
            "output should contain a sort direction indicator (▲ or ▼)"
        );
    }
}
