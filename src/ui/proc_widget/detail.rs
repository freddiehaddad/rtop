use super::ProcFrame;
use super::rows::{display_proc_cpu, format_proc_memory, mem_percent};
use super::{STATE_GRADIENT_EXITED, STATE_GRADIENT_RUNNING, STATE_GRADIENT_SUSPENDED};
use crate::domain::process::{PriorityClass, ProcInfo, ProcState};
use crate::draw::buffer::AnsiBuffer;
use crate::draw::meter::Meter;
use crate::theme::{Theme, gradient_color};
use crate::theme_keys as tc;
use crate::tools;

/// Label column width, sized to the longest label the panel uses
/// ("IO Write", "Priority", "CPU Time") plus a separating space.
const DETAIL_LABEL_W: usize = 9;
const DETAIL_COL_GAP: usize = 2;
/// Right-aligned value column for meter cells. Fits "2400.0%" from
/// per-core CPU mode and "1023.9M" from `floating_humanizer`, plus a
/// column of slack so the bar never touches the digits.
const DETAIL_VAL_W: usize = 8;
/// Floor for the meter bar, matching `.max(5)` in the other widgets.
const DETAIL_MIN_METER_W: usize = 5;
/// Inner width at or above which the field grid uses three columns.
const DETAIL_THREE_COL_MIN_W: usize = 110;
/// Inner width at or above which the field grid uses two columns.
const DETAIL_TWO_COL_MIN_W: usize = 48;
/// Cells in the field grid, laid out row-major across the columns.
const DETAIL_CELL_COUNT: usize = 9;

/// Grid columns available at this inner width.
fn detail_columns(inner_w: usize) -> usize {
    if inner_w >= DETAIL_THREE_COL_MIN_W {
        3
    } else if inner_w >= DETAIL_TWO_COL_MIN_W {
        2
    } else {
        1
    }
}

/// Rows the panel wants at this width: the header, the Args row, the
/// Path row where it fits, the field grid, and the row the layout
/// reserves for the divider beneath. The caller clamps this to the
/// space the widget can actually spare.
pub(super) fn detail_panel_rows(inner_w: usize) -> usize {
    let cols = detail_columns(inner_w);
    // Header + Args + divider, plus Path once there is room for two
    // columns; a single-column panel drops Path first because Args
    // carries more signal.
    let fixed = if cols >= 2 { 4 } else { 3 };
    fixed + DETAIL_CELL_COUNT.div_ceil(cols)
}

/// Per-column widths. The remainder lands in the last column so the
/// two meter columns stay exactly equal — an uneven bar is visible,
/// an uneven trailing column is not.
fn detail_col_widths(width: usize, cols: usize) -> ([usize; 3], usize) {
    let cols = cols.clamp(1, 3);
    let avail = width.saturating_sub(DETAIL_COL_GAP * (cols - 1));
    let base = avail / cols;
    let mut w = [0usize; 3];
    for slot in w.iter_mut().take(cols) {
        *slot = base;
    }
    w[cols - 1] += avail - base * cols;
    (w, cols)
}

/// Parameters for [`draw_detail_panel`].
pub(super) struct DetailPanelParams<'a> {
    pub proc: &'a ProcInfo,
    /// Name of the process owning `proc.ppid`, resolved by the caller
    /// against the same snapshot the list renders. `None` when the
    /// parent has exited or is outside the snapshot.
    pub parent_name: Option<&'a str>,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub rows: usize,
    pub settings: &'a ProcFrame,
    pub theme: &'a Theme,
    pub dead: bool,
}

/// Draw the detailed process info panel at the top of the proc widget.
pub(super) fn draw_detail_panel(params: &DetailPanelParams<'_>) -> String {
    let DetailPanelParams {
        proc,
        parent_name,
        x,
        y,
        width,
        rows,
        settings,
        theme,
        dead,
    } = *params;
    let colors = DetailColors::new(theme);
    let inner_w = width.saturating_sub(4);
    let content_rows = rows.saturating_sub(1);
    let detail_x = x + 3;

    let mut buf = AnsiBuffer::new();
    if inner_w == 0 || content_rows == 0 {
        return buf.finish();
    }

    let text_color = colors.value;
    let name_color = colors.emphasis;
    draw_detail_header(
        &mut buf,
        proc,
        detail_x,
        y + 2,
        inner_w,
        &colors,
        name_color,
    );

    let cols = detail_columns(inner_w);
    let mut row = 1;

    let raw = if proc.cmd.is_empty() {
        proc.name.as_str()
    } else {
        proc.cmd.as_str()
    };
    let (exe, args) = split_command(raw);
    let value_w = inner_w.saturating_sub(DETAIL_LABEL_W);

    if cols >= 2 && row < content_rows {
        let dir = parent_dir(exe);
        let value = if dir.is_empty() {
            "-".to_string()
        } else {
            mid_elide(dir, value_w)
        };
        buf.mv(detail_x, y + 2 + row);
        draw_detail_cell(
            &mut buf,
            &DetailCell {
                label: "Path",
                value,
                color: text_color,
                kind: CellKind::Text,
            },
            inner_w,
            &colors,
        );
        row += 1;
    }

    if row < content_rows {
        let value = if args.is_empty() {
            "-".to_string()
        } else {
            tail_elide(args, value_w)
        };
        buf.mv(detail_x, y + 2 + row);
        draw_detail_cell(
            &mut buf,
            &DetailCell {
                label: "Args",
                value,
                color: text_color,
                kind: CellKind::Text,
            },
            inner_w,
            &colors,
        );
        row += 1;
    }

    let cells = detail_cells(proc, parent_name, settings, &colors, dead);
    let (widths, cols) = detail_col_widths(inner_w, cols);
    for chunk in cells.chunks(cols) {
        if row >= content_rows {
            break;
        }
        buf.mv(detail_x, y + 2 + row);
        for (i, cell) in chunk.iter().enumerate() {
            if i > 0 {
                buf.text(&" ".repeat(DETAIL_COL_GAP));
            }
            draw_detail_cell(&mut buf, cell, widths[i], &colors);
        }
        row += 1;
    }

    buf.finish()
}

struct DetailColors<'a> {
    /// Label foreground. `DATA_LABEL_FG` sits between `main_fg` and
    /// `main_bg` in brightness, so a label reads as secondary to
    /// its value while staying comfortably legible.
    label: &'a str,
    value: &'a str,
    emphasis: &'a str,
    grad: &'a [String],
    meter_bg: &'a str,
}

impl<'a> DetailColors<'a> {
    fn new(theme: &'a Theme) -> Self {
        Self {
            label: theme.color(tc::DATA_LABEL_FG),
            value: theme.color(tc::MAIN_FG),
            emphasis: theme.color(tc::HI_FG),
            grad: theme.gradient(tc::GRAD_PROCESS),
            meter_bg: theme.color(tc::METER_BG),
        }
    }
}

/// How a cell renders its value.
enum CellKind {
    /// Bar filled to this percentage, clamped to `0..=100` by `Meter`.
    Meter(i32),
    Text,
}

struct DetailCell<'a> {
    label: &'static str,
    value: String,
    color: &'a str,
    kind: CellKind,
}

/// Split a Windows command line into the executable and the argument
/// tail. A quoted `argv[0]` is authoritative; otherwise the first
/// space ends the path, which is the best that can be done without
/// probing the filesystem.
fn split_command(cmd: &str) -> (&str, &str) {
    if let Some(rest) = cmd.strip_prefix('"') {
        return match rest.find('"') {
            Some(end) => (&rest[..end], rest[end + 1..].trim_start()),
            None => (rest, ""),
        };
    }
    match cmd.find(' ') {
        Some(i) => (&cmd[..i], cmd[i + 1..].trim_start()),
        None => (cmd, ""),
    }
}

/// Directory portion of a path, without the trailing separator.
fn parent_dir(path: &str) -> &str {
    match path.rfind(['\\', '/']) {
        Some(i) => &path[..i],
        None => "",
    }
}

/// Truncate in the middle. An install path carries more signal at its
/// root and its leaf than in the middle.
fn mid_elide(text: &str, width: usize) -> String {
    if tools::ulen(text) <= width {
        return text.to_string();
    }
    if width <= 1 {
        return tools::uresize(text, width);
    }
    let keep = width - 1;
    let head_w = keep * 2 / 3;
    let tail_w = keep - head_w;
    let head: String = text.chars().take(head_w).collect();
    let tail: String = {
        let mut chars: Vec<char> = text.chars().rev().take(tail_w).collect();
        chars.reverse();
        chars.into_iter().collect()
    };
    format!("{head}\u{2026}{tail}")
}

/// Truncate at the end. Arguments read left to right, so the head is
/// what matters.
fn tail_elide(text: &str, width: usize) -> String {
    if tools::ulen(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    format!("{}\u{2026}", tools::uresize(text, width - 1))
}

/// Format total CPU time (kernel + user), held in 100-nanosecond
/// intervals, as `H:MM:SS`. Hours are not wrapped, so a long-lived
/// service reads `147:02:11`.
fn format_cpu_time(intervals_100ns: u64) -> String {
    let secs = intervals_100ns / 10_000_000;
    format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

fn draw_detail_header(
    buf: &mut AnsiBuffer,
    proc: &ProcInfo,
    x: usize,
    y: usize,
    width: usize,
    colors: &DetailColors<'_>,
    name_color: &str,
) {
    let pid_value = proc.pid.to_string();
    let pid_label = "PID ";
    let pid_w = tools::ulen(pid_label) + tools::ulen(&pid_value);
    let pid_text = format!("{pid_label}{pid_value}");

    buf.mv(x, y);
    if width <= pid_w {
        buf.color(colors.label)
            .text(&tools::uresize(&pid_text, width));
        return;
    }

    let name_w = width.saturating_sub(pid_w + 1);
    let name = tools::uresize(&proc.name, name_w);
    let gap = width.saturating_sub(tools::ulen(&name) + pid_w).max(1);

    buf.color(name_color)
        .text(&name)
        .text(&" ".repeat(gap))
        .color(colors.label)
        .text(pid_label)
        .color(colors.value)
        .text(&pid_value);
}

/// Colour for the Status value, placing the state on the process
/// gradient as a severity scale.
///
/// Falls back to the surrounding text colour when `proc_colors` is
/// off, matching how the CPU and Memory cells behave, and for
/// `Unknown`, which carries no severity to place on the ramp.
fn status_color<'a>(
    dead: bool,
    state: ProcState,
    settings: &ProcFrame,
    colors: &DetailColors<'a>,
    fallback: &'a str,
) -> &'a str {
    if !settings.proc_colors || colors.grad.is_empty() {
        return fallback;
    }
    let position = if dead {
        STATE_GRADIENT_EXITED
    } else {
        match state {
            ProcState::Running => STATE_GRADIENT_RUNNING,
            ProcState::Suspended => STATE_GRADIENT_SUSPENDED,
            ProcState::Unknown => return fallback,
        }
    };
    gradient_color(colors.grad, position)
}

fn detail_cells<'a>(
    proc: &ProcInfo,
    parent_name: Option<&str>,
    settings: &ProcFrame,
    colors: &DetailColors<'a>,
    dead: bool,
) -> [DetailCell<'a>; DETAIL_CELL_COUNT] {
    let display_cpu = display_proc_cpu(proc.cpu_p, settings);
    let cpu_fill = display_cpu.round().clamp(0.0, 100.0) as i32;
    let mem_fill = mem_percent(proc.mem, settings.total_mem).round() as i32;
    let text_color = colors.value;

    let metric_color = |fill: i32| {
        if settings.proc_colors {
            gradient_color(colors.grad, fill)
        } else {
            colors.value
        }
    };

    // An exited process keeps its last-seen values; only the Status
    // cell reports that it is gone, so the rest of the panel reads
    // exactly as it did while the process was alive.
    let status = if dead {
        "\u{2717} Exited".to_string()
    } else {
        proc.state.to_string()
    };
    let priority_color = if proc.priority >= PriorityClass::High {
        colors.emphasis
    } else {
        colors.value
    };
    let parent = match parent_name {
        Some(name) => format!("{} ({})", name, proc.ppid),
        None => proc.ppid.to_string(),
    };
    let io = format!(
        "{} / {}",
        tools::floating_humanizer(proc.io_read, true, 0, false, false, settings.base_10),
        tools::floating_humanizer(proc.io_write, true, 0, false, false, settings.base_10),
    );

    [
        DetailCell {
            label: "CPU",
            value: format!("{display_cpu:.1}%"),
            color: metric_color(cpu_fill),
            kind: CellKind::Meter(cpu_fill),
        },
        DetailCell {
            label: "Memory",
            value: format_proc_memory(proc.mem, settings),
            color: metric_color(mem_fill),
            kind: CellKind::Meter(mem_fill),
        },
        DetailCell {
            label: "CPU Time",
            value: format_cpu_time(proc.cpu_time),
            color: text_color,
            kind: CellKind::Text,
        },
        DetailCell {
            label: "User",
            value: detail_value_or_dash(&proc.user),
            color: text_color,
            kind: CellKind::Text,
        },
        DetailCell {
            label: "Status",
            value: status,
            color: status_color(dead, proc.state, settings, colors, text_color),
            kind: CellKind::Text,
        },
        DetailCell {
            label: "Priority",
            value: proc.priority.to_string(),
            color: priority_color,
            kind: CellKind::Text,
        },
        DetailCell {
            label: "Parent",
            value: parent,
            color: text_color,
            kind: CellKind::Text,
        },
        DetailCell {
            label: "Threads",
            value: proc.threads.to_string(),
            color: text_color,
            kind: CellKind::Text,
        },
        DetailCell {
            label: "IO R/W",
            value: io,
            color: text_color,
            kind: CellKind::Text,
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

/// Render one cell. Meter cells use the layout shared by the cpu, gpu,
/// mem and disk widgets: label, then a bar absorbing all slack, then
/// the value right-aligned in a fixed column so digits never shift.
fn draw_detail_cell(
    buf: &mut AnsiBuffer,
    cell: &DetailCell<'_>,
    width: usize,
    colors: &DetailColors<'_>,
) {
    if width == 0 {
        return;
    }
    let label_w = DETAIL_LABEL_W.min(width);
    buf.color(colors.label)
        .text(&detail_ljust(cell.label, label_w));

    let rest = width - label_w;
    if rest == 0 {
        return;
    }

    match cell.kind {
        CellKind::Meter(pct) => {
            let meter_w = rest
                .saturating_sub(DETAIL_VAL_W)
                .max(DETAIL_MIN_METER_W)
                .min(rest);
            let meter = Meter::new(meter_w, colors.grad, colors.meter_bg);
            buf.text(meter.render(pct));
            let val_w = rest - meter_w;
            if val_w > 0 {
                buf.color(cell.color)
                    .text(&tools::rjust(&cell.value, val_w, true));
            }
        }
        CellKind::Text => {
            buf.color(cell.color).text(&detail_ljust(&cell.value, rest));
        }
    }
}

fn detail_ljust(value: &str, width: usize) -> String {
    let truncated = tools::uresize(value, width);
    let padding = width.saturating_sub(tools::ulen(&truncated));
    format!("{truncated}{}", " ".repeat(padding))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::process::{PriorityClass, ProcInfo, ProcState};

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

    fn make_frame() -> ProcFrame {
        ProcFrame {
            proc_per_core: true,
            core_count: 4,
            proc_mem_bytes: true,
            total_mem: 1024 * 1024 * 1024,
            proc_colors: true,
            base_10: false,
        }
    }

    fn make_panel<'a>(
        proc: &'a ProcInfo,
        width: usize,
        rows: usize,
        settings: &'a ProcFrame,
        theme: &'a Theme,
        dead: bool,
    ) -> DetailPanelParams<'a> {
        DetailPanelParams {
            proc,
            parent_name: None,
            x: 1,
            y: 1,
            width,
            rows,
            settings,
            theme,
            dead,
        }
    }

    #[test]
    fn detail_panel_right_aligns_pid_header() {
        let procs = make_procs();
        let theme = Theme::default();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&procs[0], 80, 8, &frame, &theme, false));
        let plain = strip_ansi(&output);
        let expected_gap = 76 - "alpha.exe".len() - "PID 100".len();
        let expected = format!("alpha.exe{}PID 100", " ".repeat(expected_gap));

        assert!(
            plain.contains(&expected),
            "header should preserve the right-aligned PID"
        );
    }

    #[test]
    fn detail_panel_splits_path_and_args() {
        let mut procs = make_procs();
        procs[0].cmd = "\"C:\\bin\\tools\\alpha.exe\" --flag tail-marker".into();

        let theme = Theme::default();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&procs[0], 80, 9, &frame, &theme, false));
        let plain = strip_ansi(&output);

        assert!(
            plain.contains("Path     C:\\bin\\tools"),
            "Path row shows the directory, not the executable: {plain}"
        );
        assert!(
            plain.contains("Args     --flag tail-marker"),
            "Args row shows the argument tail: {plain}"
        );
    }

    #[test]
    fn detail_panel_elides_long_args_before_coloring() {
        let mut procs = make_procs();
        procs[0].cmd = format!("alpha.exe {} tail-marker", "x".repeat(200));

        let theme = Theme::default();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&procs[0], 36, 9, &frame, &theme, false));
        let plain = strip_ansi(&output);

        assert!(plain.contains("Args     xxx"));
        assert!(
            !plain.contains("tail-marker"),
            "long argument text should be elided to the detail width"
        );
    }

    #[test]
    fn detail_panel_narrow_mode_keeps_high_priority_fields() {
        let procs = make_procs();
        let theme = Theme::default();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&procs[2], 60, 9, &frame, &theme, false));
        let plain = strip_ansi(&output);

        assert!(plain.contains("User     Admin"), "{plain}");
        assert!(plain.contains("Status   Running"), "{plain}");
        // CPU and Memory are meter cells, so the value trails the bar
        // rather than following the label directly.
        assert!(plain.contains("25.0%"), "{plain}");
        assert!(plain.contains("200M"), "{plain}");
    }

    #[test]
    fn detail_panel_omits_status_line_for_live_process() {
        let procs = make_procs();
        let theme = Theme::default();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&procs[0], 80, 8, &frame, &theme, false));
        let plain = strip_ansi(&output);
        assert!(
            !plain.contains("Process exited"),
            "live process must not include the exited status line: {plain}"
        );
    }

    #[test]
    fn detail_panel_includes_status_line_for_dead_process() {
        let procs = make_procs();
        let theme = Theme::default();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&procs[0], 80, 9, &frame, &theme, true));
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("Status   \u{2717} Exited"),
            "dead process detail panel must report Exited: {plain}"
        );
    }

    #[test]
    fn dead_panel_colors_only_the_status_value() {
        // An exited process keeps its last-seen values, so only the
        // Status cell shifts to the exited gradient colour; the rest
        // of the panel reads exactly as it did while alive.
        let theme = Theme::default();
        let procs = make_procs();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&procs[0], 120, 7, &frame, &theme, true));
        let exited = gradient_color(theme.gradient(tc::GRAD_PROCESS), STATE_GRADIENT_EXITED);

        assert!(
            output.contains(&format!("{exited}\u{2717} Exited")),
            "the Status value takes the exited gradient colour"
        );
        assert!(
            output.contains(&format!("{}SYSTEM", theme.color(tc::MAIN_FG))),
            "other values stay in MAIN_FG when the process has exited"
        );
        assert!(
            output.contains(&format!("{}User", theme.color(tc::DATA_LABEL_FG))),
            "labels stay in DATA_LABEL_FG when the process has exited"
        );
    }

    #[test]
    fn status_value_maps_each_state_onto_the_process_gradient() {
        let theme = Theme::default();
        let grad = theme.gradient(tc::GRAD_PROCESS);
        let frame = make_frame();
        let mut proc = make_procs()[0].clone();

        proc.state = ProcState::Running;
        let running = draw_detail_panel(&make_panel(&proc, 120, 7, &frame, &theme, false));
        assert!(
            running.contains(&format!("{}Running", grad[0])),
            "Running takes the calm end of the gradient"
        );

        proc.state = ProcState::Suspended;
        let suspended = draw_detail_panel(&make_panel(&proc, 120, 7, &frame, &theme, false));
        assert!(
            suspended.contains(&format!("{}Suspended", grad[50])),
            "Suspended takes the gradient midpoint"
        );

        proc.state = ProcState::Running;
        let exited = draw_detail_panel(&make_panel(&proc, 120, 7, &frame, &theme, true));
        assert!(
            exited.contains(&format!("{}\u{2717} Exited", grad[100])),
            "Exited takes the hot end of the gradient"
        );

        // The three positions must be visually distinct in the default
        // theme, or the mapping conveys nothing.
        assert_ne!(grad[0], grad[50]);
        assert_ne!(grad[50], grad[100]);
    }

    #[test]
    fn status_value_ignores_the_gradient_when_proc_colors_is_off() {
        let theme = Theme::default();
        let grad = theme.gradient(tc::GRAD_PROCESS);
        let frame = ProcFrame {
            proc_colors: false,
            ..make_frame()
        };
        let proc = make_procs()[0].clone();
        let out = draw_detail_panel(&make_panel(&proc, 120, 7, &frame, &theme, false));

        assert!(
            !out.contains(&format!("{}Running", grad[0])),
            "proc_colors off must leave Status uncoloured"
        );
        assert!(
            out.contains(&format!("{}Running", theme.color(tc::MAIN_FG))),
            "Status falls back to the surrounding text colour"
        );
    }

    #[test]
    fn detail_panel_resolves_parent_name_when_supplied() {
        let procs = make_procs();
        let theme = Theme::default();
        let frame = make_frame();
        let mut params = make_panel(&procs[0], 120, 7, &frame, &theme, false);
        params.parent_name = Some("explorer.exe");
        let plain = strip_ansi(&draw_detail_panel(&params));

        assert!(
            plain.contains("Parent   explorer.exe (1)"),
            "parent PID should resolve to a name: {plain}"
        );
    }

    #[test]
    fn detail_panel_falls_back_to_bare_ppid_without_a_parent() {
        let procs = make_procs();
        let theme = Theme::default();
        let frame = make_frame();
        let plain = strip_ansi(&draw_detail_panel(&make_panel(
            &procs[0], 120, 7, &frame, &theme, false,
        )));

        assert!(
            plain.contains("Parent   1"),
            "an unresolved parent shows the raw PID: {plain}"
        );
    }

    #[test]
    fn detail_panel_merges_io_into_one_cell() {
        let mut procs = make_procs();
        procs[0].io_read = 1024 * 1024 * 128;
        procs[0].io_write = 1024 * 1024 * 41;
        let theme = Theme::default();
        let frame = make_frame();
        let plain = strip_ansi(&draw_detail_panel(&make_panel(
            &procs[0], 120, 7, &frame, &theme, false,
        )));

        assert!(plain.contains("IO R/W   128M / 41M"), "{plain}");
    }

    #[test]
    fn detail_panel_shows_cpu_time() {
        let mut procs = make_procs();
        // 100ns intervals: 1h 02m 03s.
        procs[0].cpu_time = (3600 + 123) * 10_000_000;
        let theme = Theme::default();
        let frame = make_frame();
        let plain = strip_ansi(&draw_detail_panel(&make_panel(
            &procs[0], 120, 7, &frame, &theme, false,
        )));

        assert!(plain.contains("CPU Time 1:02:03"), "{plain}");
    }

    #[test]
    fn format_cpu_time_does_not_wrap_hours() {
        assert_eq!(format_cpu_time(0), "0:00:00");
        assert_eq!(format_cpu_time(59 * 10_000_000), "0:00:59");
        assert_eq!(format_cpu_time(3661 * 10_000_000), "1:01:01");
        assert_eq!(format_cpu_time(147 * 3600 * 10_000_000), "147:00:00");
    }

    #[test]
    fn split_command_handles_quoted_and_bare_paths() {
        assert_eq!(
            split_command("\"C:\\a b\\x.exe\" --f 1"),
            ("C:\\a b\\x.exe", "--f 1")
        );
        assert_eq!(split_command("C:\\a\\x.exe --f"), ("C:\\a\\x.exe", "--f"));
        assert_eq!(split_command("x.exe"), ("x.exe", ""));
        assert_eq!(split_command(""), ("", ""));
        // Unterminated quote: take the remainder as the path rather
        // than mis-splitting the arguments.
        assert_eq!(split_command("\"C:\\a\\x.exe"), ("C:\\a\\x.exe", ""));
    }

    #[test]
    fn parent_dir_strips_the_leaf() {
        assert_eq!(parent_dir("C:\\a\\b\\x.exe"), "C:\\a\\b");
        assert_eq!(parent_dir("C:/a/x.exe"), "C:/a");
        assert_eq!(parent_dir("x.exe"), "");
    }

    #[test]
    fn mid_elide_keeps_both_ends() {
        let path = "C:\\one\\two\\three\\four\\five\\six";
        let out = mid_elide(path, 20);
        assert_eq!(tools::ulen(&out), 20);
        assert!(out.starts_with("C:\\one"), "{out}");
        assert!(out.ends_with("six"), "{out}");
        assert!(out.contains('\u{2026}'), "{out}");
        // Short enough to fit is returned untouched.
        assert_eq!(mid_elide("C:\\a", 20), "C:\\a");
    }

    #[test]
    fn meter_cell_right_aligns_its_value_at_a_fixed_width() {
        // The value column is fixed, so a one-digit and a three-digit
        // percentage must end at the same column — nothing shifts as
        // the number grows.
        let theme = Theme::default();
        let frame = make_frame();
        let mut low = make_procs()[0].clone();
        low.cpu_p = 5.0;
        let mut high = make_procs()[0].clone();
        high.cpu_p = 100.0;

        let a = strip_ansi(&draw_detail_panel(&make_panel(
            &low, 120, 7, &frame, &theme, false,
        )));
        let b = strip_ansi(&draw_detail_panel(&make_panel(
            &high, 120, 7, &frame, &theme, false,
        )));

        let end_of = |s: &str, needle: &str| s.find(needle).map(|i| i + needle.len());
        assert_eq!(
            end_of(&a, "5.0%"),
            end_of(&b, "100.0%"),
            "meter values must share a right edge:\n{a}\n{b}"
        );
    }

    #[test]
    fn detail_panel_rows_shrink_as_columns_grow() {
        // Three columns pack the nine cells into three grid rows; two
        // columns need five; one column needs nine. Path is dropped in
        // the single-column tier.
        assert_eq!(detail_panel_rows(120), 7);
        assert_eq!(detail_panel_rows(80), 9);
        assert_eq!(detail_panel_rows(30), 12);
    }

    #[test]
    fn detail_col_widths_keep_the_meter_columns_equal() {
        let (w, n) = detail_col_widths(138, 3);
        assert_eq!(n, 3);
        assert_eq!(w[0], w[1], "the two meter columns must match exactly");
        assert_eq!(w[0] + w[1] + w[2] + DETAIL_COL_GAP * 2, 138);
    }

    #[test]
    fn detail_panel_renders_from_caller_supplied_proc() {
        // The renderer takes a `&ProcInfo` directly; the upstream
        // resolver decides whether that reference points at a live
        // row or a cached `last_seen`. The renderer must not care:
        // construct an explicit "stale-looking" cached proc that
        // the row list does NOT contain, and confirm the panel
        // draws its values just the same.
        let cached = ProcInfo {
            pid: 999,
            name: "ghost.exe".into(),
            cmd: "ghost.exe --orphan".into(),
            user: "Cached".into(),
            mem: 1024 * 1024 * 8,
            cpu_p: 1.5,
            ..Default::default()
        };
        let theme = Theme::default();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&cached, 80, 9, &frame, &theme, true));
        let plain = strip_ansi(&output);

        assert!(plain.contains("ghost.exe"));
        assert!(plain.contains("Args     --orphan"));
        assert!(plain.contains("PID 999"));
        assert!(
            plain.contains("\u{2717} Exited"),
            "dead flag must surface the exited status: {plain}"
        );
    }

    #[test]
    fn detail_panel_omits_exited_status_for_live_cached_proc() {
        // Same renderer with `dead = false` must not insert the
        // status line. This pairs with the test above to confirm
        // the renderer is a pure function of (proc, dead).
        let proc = ProcInfo {
            pid: 42,
            name: "live.exe".into(),
            cmd: "live.exe".into(),
            ..Default::default()
        };
        let theme = Theme::default();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&proc, 80, 8, &frame, &theme, false));
        let plain = strip_ansi(&output);
        assert!(
            !plain.contains("Process exited"),
            "dead = false must omit the exited status line: {plain}"
        );
    }
}
