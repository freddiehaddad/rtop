use super::ProcFrame;
use super::rows::{display_proc_cpu, format_proc_memory};
use crate::domain::process::{PriorityClass, ProcInfo};
use crate::draw::buffer::AnsiBuffer;
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

const DETAIL_LABEL_W: usize = 9;
const DETAIL_COL_GAP: usize = 2;
const DETAIL_TWO_COL_MIN_WIDTH: usize = 48;

const NARROW_DETAIL_FIELD_ORDER: [usize; 9] = [0, 1, 4, 5, 2, 3, 8, 6, 7];

/// Parameters for [`draw_detail_panel`].
pub(super) struct DetailPanelParams<'a> {
    pub proc: &'a ProcInfo,
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
        x,
        y,
        width,
        rows,
        settings,
        theme,
        dead,
    } = *params;
    let fg = theme.color(tc::MAIN_FG);
    let hi = theme.color(tc::HI_FG);
    let dead_fg = theme.color(tc::DEAD_PROC_FG);
    let proc_grad = theme.gradient(tc::GRAD_PROCESS);
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
            label: fg,
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
        draw_detail_field(&mut buf, &cmd_field, inner_w, fg);
    }

    // Status: ✗ Process exited — only for dead processes in the
    // paused snapshot. Displaces one grid row to make room. Uses
    // dead_proc_fg so the user's eye links the detail-panel status
    // line and the dead row in the list (same color, same `✗`
    // glyph).
    let status_offset = if dead && content_rows > 2 {
        let status_field = DetailField {
            label: "Status",
            value: "\u{2717} Process exited".to_string(),
            color: dead_fg,
        };
        buf.mv(detail_x, y + 4);
        draw_detail_field(&mut buf, &status_field, inner_w, dead_fg);
        1
    } else {
        0
    };

    let fields = detail_fields(proc, settings, fg, hi, proc_grad);
    let grid_rows = content_rows.saturating_sub(2 + status_offset);
    let grid_y = y + 4 + status_offset;
    if inner_w >= DETAIL_TWO_COL_MIN_WIDTH {
        for (row, pair) in fields.chunks(2).take(grid_rows).enumerate() {
            let left = &pair[0];
            let right = pair.get(1);
            draw_detail_pair(&mut buf, left, right, detail_x, grid_y + row, inner_w, fg);
        }
    } else {
        for (row, field_index) in NARROW_DETAIL_FIELD_ORDER
            .iter()
            .copied()
            .take(grid_rows)
            .enumerate()
        {
            if let Some(field) = fields.get(field_index) {
                buf.mv(detail_x, grid_y + row);
                draw_detail_field(&mut buf, field, inner_w, fg);
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
    settings: &ProcFrame,
    fg: &'a str,
    hi: &'a str,
    proc_grad: &'a [String],
) -> Vec<DetailField<'a>> {
    let display_cpu = display_proc_cpu(proc.cpu_p, settings);
    let cpu_pct = display_cpu.round().clamp(0.0, 100.0) as usize;
    let cpu_color = if settings.proc_colors {
        proc_grad.get(cpu_pct).map(String::as_str).unwrap_or(fg)
    } else {
        fg
    };
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
            value: format!("{display_cpu:.1}%"),
            color: cpu_color,
        },
        DetailField {
            label: "Memory",
            value: format_proc_memory(proc.mem, settings),
            color: fg,
        },
        DetailField {
            label: "IO Read",
            value: tools::floating_humanizer(proc.io_read, true, 0, false, false, settings.base_10),
            color: fg,
        },
        DetailField {
            label: "IO Write",
            value: tools::floating_humanizer(
                proc.io_write,
                true,
                0,
                false,
                false,
                settings.base_10,
            ),
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
    fn detail_panel_truncates_command_before_coloring() {
        let mut procs = make_procs();
        procs[0].cmd = format!("alpha.exe {} tail-marker", "x".repeat(80));

        let theme = Theme::default();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&procs[0], 36, 8, &frame, &theme, false));
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
        let theme = Theme::default();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&procs[2], 42, 8, &frame, &theme, false));
        let plain = strip_ansi(&output);

        assert!(plain.contains("User     Admin"));
        assert!(plain.contains("Status   Running"));
        assert!(plain.contains("CPU      25.0%"));
        assert!(plain.contains("Memory   200M"));
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
        let output = draw_detail_panel(&make_panel(&procs[0], 80, 8, &frame, &theme, true));
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("Status"),
            "dead process detail panel must include a Status line: {plain}"
        );
        assert!(
            plain.contains("\u{2717} Process exited"),
            "dead process status line must contain `✗ Process exited`: {plain}"
        );
    }

    #[test]
    fn detail_panel_status_line_uses_dead_proc_fg() {
        let theme = Theme::default();
        let procs = make_procs();
        let frame = make_frame();
        let output = draw_detail_panel(&make_panel(&procs[0], 80, 8, &frame, &theme, true));
        let dead_fg = theme.color(tc::DEAD_PROC_FG);
        assert!(
            output.contains(&format!("{dead_fg}Status")),
            "Status label should be preceded by dead_proc_fg"
        );
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
        let output = draw_detail_panel(&make_panel(&cached, 80, 8, &frame, &theme, true));
        let plain = strip_ansi(&output);

        assert!(plain.contains("ghost.exe"));
        assert!(plain.contains("Cmd      ghost.exe --orphan"));
        assert!(plain.contains("PID 999"));
        assert!(
            plain.contains("\u{2717} Process exited"),
            "dead flag must surface the exited status line: {plain}"
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
