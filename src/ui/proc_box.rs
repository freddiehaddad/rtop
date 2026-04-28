use crate::collect::CollectStatus;
use crate::domain::process::{PriorityClass, ProcDisplayEntry, ProcInfo};
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
const DETAIL_LABEL_W: usize = 9;
const DETAIL_COL_GAP: usize = 2;
const DETAIL_TWO_COL_MIN_WIDTH: usize = 48;

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
    entries: &[ProcDisplayEntry],
    area: &BoxArea,
    view: &ProcView,
    theme: &Theme,
    status: &CollectStatus,
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

    super::draw_status_inset(&mut buf, status, "proc", x, y, box_color, title_color);

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
        if let Some(proc) = entries
            .iter()
            .filter_map(|entry| procs.get(entry.proc_index))
            .find(|proc| proc.pid == detailed_pid)
        {
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
    for (i, entry) in entries.iter().skip(start).take(max_rows).enumerate() {
        let Some(proc) = procs.get(entry.proc_index) else {
            continue;
        };
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
        let (tree_prefix, bare_name) = if tree_mode && !entry.prefix.is_empty() {
            (entry.prefix.as_str(), proc.name.as_str())
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
    let visible = entries.len().min(max_rows);
    buf.text(&draw_bottom_border(
        &BottomBorderParams {
            x,
            bottom_y: y + height,
            width,
            filter,
            filtering,
            visible,
            total: entries.len(),
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
    let count_x = box_drawing::right_inset_x(p.x, p.width, box_drawing::inset_width(&count_str));
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
    let proc_grad = theme.g(tc::GRAD_PROCESS);
    let inner_w = width.saturating_sub(4);
    let content_rows = rows.saturating_sub(1);
    let detail_x = x + 3;

    let mut buf = AnsiBuffer::new();
    if inner_w == 0 || content_rows == 0 {
        return buf.finish();
    }

    draw_detail_header(
        &mut buf,
        proc,
        detail_x,
        y + 2,
        inner_w,
        DetailColors {
            label: title_color,
            value: fg,
            emphasis: hi,
        },
    );

    if content_rows > 1 {
        let cmd = if proc.cmd.is_empty() {
            proc.name.clone()
        } else {
            proc.cmd.clone()
        };
        let cmd_field = DetailField {
            label: "Cmd",
            value: cmd,
            color: fg,
        };
        buf.mv(detail_x, y + 3);
        draw_detail_field(&mut buf, &cmd_field, inner_w, title_color);
    }

    let fields = detail_fields(proc, fg, hi, proc_grad);
    let grid_rows = content_rows.saturating_sub(2);
    if inner_w >= DETAIL_TWO_COL_MIN_WIDTH {
        for (row, pair) in fields.chunks(2).take(grid_rows).enumerate() {
            let left = &pair[0];
            let right = pair.get(1);
            draw_detail_pair(
                &mut buf,
                left,
                right,
                detail_x,
                y + 4 + row,
                inner_w,
                title_color,
            );
        }
    } else {
        for (row, field_index) in NARROW_DETAIL_FIELD_ORDER
            .iter()
            .copied()
            .take(grid_rows)
            .enumerate()
        {
            if let Some(field) = fields.get(field_index) {
                buf.mv(detail_x, y + 4 + row);
                draw_detail_field(&mut buf, field, inner_w, title_color);
            }
        }
    }

    buf.finish()
}

struct DetailColors<'a> {
    label: &'a str,
    value: &'a str,
    emphasis: &'a str,
}

struct DetailField<'a> {
    label: &'static str,
    value: String,
    color: &'a str,
}

const NARROW_DETAIL_FIELD_ORDER: [usize; 9] = [0, 1, 4, 5, 2, 3, 8, 6, 7];

fn draw_detail_header(
    buf: &mut AnsiBuffer,
    proc: &ProcInfo,
    x: usize,
    y: usize,
    width: usize,
    colors: DetailColors<'_>,
) {
    let pid_value = proc.pid.to_string();
    let pid_label = "PID ";
    let pid_w = tools::ulen(pid_label, false) + tools::ulen(&pid_value, false);
    let pid_text = format!("{pid_label}{pid_value}");

    buf.mv(x, y);
    if width <= pid_w {
        buf.color(colors.label)
            .text(&tools::uresize(&pid_text, width, false));
        return;
    }

    let name_w = width.saturating_sub(pid_w + 1);
    let name = tools::uresize(&proc.name, name_w, false);
    let gap = width
        .saturating_sub(tools::ulen(&name, false) + pid_w)
        .max(1);

    buf.color(colors.emphasis)
        .text(&name)
        .text(&" ".repeat(gap))
        .color(colors.label)
        .text(pid_label)
        .color(colors.value)
        .text(&pid_value);
}

fn detail_fields<'a>(
    proc: &ProcInfo,
    fg: &'a str,
    hi: &'a str,
    proc_grad: &'a [String],
) -> Vec<DetailField<'a>> {
    let cpu_pct = proc.cpu_p.clamp(0.0, 100.0) as usize;
    let cpu_color = proc_grad.get(cpu_pct).map(String::as_str).unwrap_or(fg);
    let priority_color = if proc.priority >= PriorityClass::High {
        hi
    } else {
        fg
    };

    vec![
        DetailField {
            label: "User",
            value: detail_value_or_dash(&proc.user),
            color: fg,
        },
        DetailField {
            label: "Status",
            value: proc.state.to_string(),
            color: fg,
        },
        DetailField {
            label: "Parent",
            value: proc.ppid.to_string(),
            color: fg,
        },
        DetailField {
            label: "Threads",
            value: proc.threads.to_string(),
            color: fg,
        },
        DetailField {
            label: "CPU",
            value: format!("{:.1}%", proc.cpu_p),
            color: cpu_color,
        },
        DetailField {
            label: "Memory",
            value: tools::floating_humanizer(proc.mem, true, 0, false, false, false),
            color: fg,
        },
        DetailField {
            label: "IO Read",
            value: tools::floating_humanizer(proc.io_read, true, 0, false, false, false),
            color: fg,
        },
        DetailField {
            label: "IO Write",
            value: tools::floating_humanizer(proc.io_write, true, 0, false, false, false),
            color: fg,
        },
        DetailField {
            label: "Priority",
            value: proc.priority.to_string(),
            color: priority_color,
        },
    ]
}

fn detail_value_or_dash(value: &str) -> String {
    if value.is_empty() {
        "-".into()
    } else {
        value.into()
    }
}

fn draw_detail_pair(
    buf: &mut AnsiBuffer,
    left: &DetailField<'_>,
    right: Option<&DetailField<'_>>,
    x: usize,
    y: usize,
    width: usize,
    label_color: &str,
) {
    buf.mv(x, y);
    let Some(right) = right else {
        draw_detail_field(buf, left, width, label_color);
        return;
    };

    let left_w = width.saturating_sub(DETAIL_COL_GAP) / 2;
    let right_w = width.saturating_sub(left_w + DETAIL_COL_GAP);
    draw_detail_field(buf, left, left_w, label_color);
    buf.text(&" ".repeat(DETAIL_COL_GAP));
    draw_detail_field(buf, right, right_w, label_color);
}

fn draw_detail_field(
    buf: &mut AnsiBuffer,
    field: &DetailField<'_>,
    width: usize,
    label_color: &str,
) {
    if width == 0 {
        return;
    }

    let label_w = DETAIL_LABEL_W.min(width);
    buf.color(label_color)
        .text(&detail_ljust(field.label, label_w));

    let value_w = width.saturating_sub(label_w);
    if value_w > 0 {
        buf.color(field.color)
            .text(&detail_ljust(&field.value, value_w));
    }
}

fn detail_ljust(value: &str, width: usize) -> String {
    let truncated = tools::uresize(value, width, false);
    let padding = width.saturating_sub(tools::ulen(&truncated, false));
    format!("{truncated}{}", " ".repeat(padding))
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
            },
        ]
    }

    fn make_entries() -> Vec<ProcDisplayEntry> {
        (0..make_procs().len())
            .map(ProcDisplayEntry::flat)
            .collect()
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
        let output = draw_with_sort(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &make_view(),
            &Theme::default(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("proc"), "output should contain 'proc' title");
    }

    #[test]
    fn draw_contains_process_names() {
        let output = draw_with_sort(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &make_view(),
            &Theme::default(),
            &CollectStatus::Ok,
        );
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
        let output = draw_with_sort(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &make_view(),
            &Theme::default(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains('▲') || plain.contains('▼'),
            "output should contain a sort direction indicator (▲ or ▼)"
        );
    }

    #[test]
    fn detail_panel_uses_aligned_labels() {
        let mut view = make_view();
        view.detailed_pid = 100;
        let output = draw_with_sort(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &view,
            &Theme::default(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);

        assert!(plain.contains("Cmd      alpha.exe --flag"));
        assert!(plain.contains("User     SYSTEM"));
        assert!(plain.contains("Status   Running"));
        assert!(plain.contains("Parent   1"));
        assert!(plain.contains("Threads  4"));
    }

    #[test]
    fn detail_panel_right_aligns_pid_header() {
        let procs = make_procs();
        let output = draw_detail_panel(&procs[0], 1, 1, 80, 8, &Theme::default());
        let plain = strip_ansi(&output);
        let expected_gap = 76 - "alpha.exe".len() - "PID 100".len();
        let expected = format!("alpha.exe{}PID 100", " ".repeat(expected_gap));

        assert!(
            plain.contains(&expected),
            "header should preserve the right-aligned PID"
        );
    }

    #[test]
    fn detail_panel_truncates_command_before_coloring() {
        let mut procs = make_procs();
        procs[0].cmd = format!("alpha.exe {} tail-marker", "x".repeat(80));

        let output = draw_detail_panel(&procs[0], 1, 1, 36, 8, &Theme::default());
        let plain = strip_ansi(&output);

        assert!(plain.contains("Cmd      alpha.exe"));
        assert!(
            !plain.contains("tail-marker"),
            "long command text should be truncated to the detail width"
        );
    }

    #[test]
    fn detail_panel_narrow_mode_keeps_high_priority_fields() {
        let procs = make_procs();
        let output = draw_detail_panel(&procs[2], 1, 1, 42, 8, &Theme::default());
        let plain = strip_ansi(&output);

        assert!(plain.contains("User     Admin"));
        assert!(plain.contains("Status   Running"));
        assert!(plain.contains("CPU      25.0%"));
        assert!(plain.contains("Memory   200M"));
    }
}
