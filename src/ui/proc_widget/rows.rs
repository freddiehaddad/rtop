use super::layout::{ProcColumns, ProcWidgetLayout};
use super::{ProcColors, ProcFrame};
use crate::domain::process::{ProcDisplayEntry, ProcInfo};
use crate::draw::buffer::AnsiBuffer;
use crate::tools;
use std::collections::HashSet;

pub(super) struct ProcessRowsParams<'a> {
    pub(super) procs: &'a [ProcInfo],
    pub(super) entries: &'a [ProcDisplayEntry],
    pub(super) layout: &'a ProcWidgetLayout,
    pub(super) start: usize,
    pub(super) selected: usize,
    pub(super) followed_pid: u32,
    pub(super) tree_mode: bool,
    pub(super) settings: &'a ProcFrame,
    pub(super) colors: &'a ProcColors<'a>,
    /// PIDs from the paused snapshot that no longer exist in the
    /// live snapshot. Empty when not paused. Rows whose PID is in
    /// this set render with `dead_proc_fg` and a `✗ ` name prefix.
    pub(super) dead_pids: &'a HashSet<u32>,
}

struct ProcessRowParams<'a> {
    proc: &'a ProcInfo,
    entry: &'a ProcDisplayEntry,
    absolute_index: usize,
    row_y: usize,
    layout: &'a ProcWidgetLayout,
    selected: usize,
    followed_pid: u32,
    tree_mode: bool,
    settings: &'a ProcFrame,
    colors: &'a ProcColors<'a>,
    /// `true` when this row's PID is in `dead_pids`. Drives the
    /// dead-row foreground color and the `✗ ` name-column prefix.
    dead: bool,
}

/// Universal prefix for dead-row name columns (Ballot X + space).
/// Two cells of name-column width per dead row; the displayed name
/// is truncated by the same amount to fit.
const DEAD_NAME_PREFIX: &str = "\u{2717} ";
const DEAD_NAME_PREFIX_WIDTH: usize = 2;

struct RowText<'a> {
    pid: String,
    tree_prefix: &'a str,
    name: String,
    cmd: String,
    cpu: String,
    mem: String,
    prefix_w: usize,
    name_avail: usize,
}

pub(super) fn draw_rows(buf: &mut AnsiBuffer, params: &ProcessRowsParams<'_>) {
    for (i, entry) in params
        .entries
        .iter()
        .skip(params.start)
        .take(params.layout.max_rows)
        .enumerate()
    {
        let Some(proc) = params.procs.get(entry.proc_index) else {
            continue;
        };
        draw_process_row(
            buf,
            &ProcessRowParams {
                proc,
                entry,
                absolute_index: i + params.start,
                row_y: params.layout.first_row_y + i,
                layout: params.layout,
                selected: params.selected,
                followed_pid: params.followed_pid,
                tree_mode: params.tree_mode,
                settings: params.settings,
                colors: params.colors,
                dead: params.dead_pids.contains(&proc.pid),
            },
        );
    }
}

fn draw_process_row(buf: &mut AnsiBuffer, params: &ProcessRowParams<'_>) {
    let columns = &params.layout.columns;
    let cpu_p = params.entry.cpu_override.unwrap_or(params.proc.cpu_p);
    let mem = params.entry.mem_override.unwrap_or(params.proc.mem);
    let display_cpu = display_proc_cpu(cpu_p, params.settings);
    let mem_str = format_proc_memory(mem, params.settings);
    // Dead rows that are not selected/followed render in
    // dead_proc_fg; the dim signal then layers under the dagger
    // prefix below. Selected / followed states win over the dead
    // foreground (their bg highlight takes precedence in their
    // dedicated row renderers); the prefix carries the "dead even
    // when highlighted" signal.
    let proc_color = if params.dead {
        params.colors.dead_fg
    } else {
        process_row_color(
            display_cpu,
            params.absolute_index,
            params.selected,
            params.layout.max_rows,
            params.settings,
            params.colors.proc_grad,
            params.colors.fg,
        )
    };

    let (tree_prefix, bare_name) = if params.tree_mode && !params.entry.prefix.is_empty() {
        (params.entry.prefix.as_str(), params.proc.name.as_str())
    } else {
        ("", params.proc.name.as_str())
    };
    let prefix_w = tools::ulen(tree_prefix, false);
    let name_avail = columns.name_w.saturating_sub(prefix_w);
    // Dead rows reserve the leftmost two cells of their name field
    // for the `✗ ` prefix; the displayed name is truncated by the
    // same amount so the column layout doesn't shift.
    let (display_name, name_avail_after_prefix) = if params.dead {
        let avail = name_avail.saturating_sub(DEAD_NAME_PREFIX_WIDTH);
        let name = tools::uresize(bare_name, avail, false);
        let prefixed = format!("{DEAD_NAME_PREFIX}{name}");
        (prefixed, name_avail)
    } else {
        (tools::uresize(bare_name, name_avail, false), name_avail)
    };
    let pid_str = format!("{:<pid_w$}", params.proc.pid, pid_w = columns.pid_w);
    let cpu_str = format!("{:>cpu_w$.1}", display_cpu, cpu_w = columns.cpu_w);
    let mem_str_fmt = format!("{:>mem_w$}", mem_str, mem_w = columns.mem_w);
    let cmd_display = command_display(params.proc, columns);
    // `display_name` may contain the multi-byte `✗` prefix (3 bytes,
    // 1 visible cell) on dead rows, so right-pad by visible width
    // — not byte length — to keep the cmd / cpu / mem columns
    // aligned with their alive-row neighbours.
    let name_padded = tools::ljust(&display_name, name_avail_after_prefix, true);

    let is_followed = params.followed_pid > 0 && params.proc.pid == params.followed_pid;

    if is_followed {
        draw_followed_process_row(
            buf,
            params,
            &RowText {
                pid: pid_str,
                tree_prefix,
                name: name_padded,
                cmd: cmd_display,
                cpu: cpu_str,
                mem: mem_str_fmt,
                prefix_w,
                name_avail: name_avail_after_prefix,
            },
        );
    } else if params.absolute_index == params.selected {
        draw_selected_process_row(
            buf,
            params,
            &RowText {
                pid: pid_str,
                tree_prefix,
                name: name_padded,
                cmd: cmd_display,
                cpu: cpu_str,
                mem: mem_str_fmt,
                prefix_w,
                name_avail: name_avail_after_prefix,
            },
        );
    } else {
        draw_unselected_process_row(
            buf,
            params,
            &RowText {
                pid: pid_str,
                tree_prefix,
                name: name_padded,
                cmd: cmd_display,
                cpu: cpu_str,
                mem: mem_str_fmt,
                prefix_w,
                name_avail: name_avail_after_prefix,
            },
            proc_color,
        );
    }
}

fn command_display(proc: &ProcInfo, columns: &ProcColumns) -> String {
    if !columns.has_cmd_col || columns.cmd_w == 0 {
        return String::new();
    }

    let raw = if proc.cmd != proc.name { &proc.cmd } else { "" };
    tools::uresize(raw, columns.cmd_w, false)
}

fn draw_selected_process_row(
    buf: &mut AnsiBuffer,
    params: &ProcessRowParams<'_>,
    row: &RowText<'_>,
) {
    let columns = &params.layout.columns;
    let bg_esc = &params.colors.sel_bg_esc;
    buf.mv(params.layout.x + 3, params.row_y)
        .text(bg_esc)
        .color(params.colors.sel_fg);
    buf.text(&row.pid).text(" ");
    if !row.tree_prefix.is_empty() {
        buf.text(row.tree_prefix);
    }
    draw_process_name_padding(buf, row, columns.name_w);
    buf.text(" ");
    if columns.has_cmd_col && columns.cmd_w > 0 {
        buf.text(&format!("{:<cmd_w$}", row.cmd, cmd_w = columns.cmd_w));
        buf.text(" ");
    }
    buf.text(&row.cpu).text(" ").text(&row.mem);
    buf.reset();
}

fn draw_followed_process_row(
    buf: &mut AnsiBuffer,
    params: &ProcessRowParams<'_>,
    row: &RowText<'_>,
) {
    let columns = &params.layout.columns;
    let bg_esc = &params.colors.followed_bg_esc;
    buf.mv(params.layout.x + 3, params.row_y)
        .text(bg_esc)
        .color(params.colors.followed_fg);
    buf.text(&row.pid).text(" ");
    if !row.tree_prefix.is_empty() {
        buf.text(row.tree_prefix);
    }
    draw_process_name_padding(buf, row, columns.name_w);
    buf.text(" ");
    if columns.has_cmd_col && columns.cmd_w > 0 {
        buf.text(&format!("{:<cmd_w$}", row.cmd, cmd_w = columns.cmd_w));
        buf.text(" ");
    }
    buf.text(&row.cpu).text(" ").text(&row.mem);
    buf.reset();
}

fn draw_unselected_process_row(
    buf: &mut AnsiBuffer,
    params: &ProcessRowParams<'_>,
    row: &RowText<'_>,
    proc_color: &str,
) {
    let columns = &params.layout.columns;
    buf.mv(params.layout.x + 3, params.row_y).color(proc_color);
    buf.text(&row.pid).text(" ");
    if !row.tree_prefix.is_empty() {
        buf.color(params.colors.tree_fg)
            .text(row.tree_prefix)
            .color(proc_color);
    }
    draw_process_name_padding(buf, row, columns.name_w);
    buf.text(" ");
    if columns.has_cmd_col && columns.cmd_w > 0 {
        buf.text(&format!("{:<cmd_w$}", row.cmd, cmd_w = columns.cmd_w));
        buf.text(" ");
    }
    buf.text(&row.cpu).text(" ").text(&row.mem);
}

fn draw_process_name_padding(buf: &mut AnsiBuffer, row: &RowText<'_>, name_w: usize) {
    buf.text(&row.name);
    if row.prefix_w + row.name_avail < name_w {
        buf.text(&" ".repeat(name_w - row.prefix_w - row.name_avail));
    }
}

pub(super) fn display_proc_cpu(cpu_per_core: f64, settings: &ProcFrame) -> f64 {
    if !cpu_per_core.is_finite() {
        return 0.0;
    }
    let core_count = settings.core_count.max(1);
    let value = if settings.proc_per_core {
        cpu_per_core
    } else {
        cpu_per_core / core_count as f64
    };
    let max_value = if settings.proc_per_core {
        100.0 * core_count as f64
    } else {
        100.0
    };
    value.clamp(0.0, max_value)
}

pub(super) fn format_proc_memory(mem: u64, settings: &ProcFrame) -> String {
    if settings.proc_mem_bytes {
        return if mem > 0 {
            tools::floating_humanizer(mem, true, 0, false, false, settings.base_10)
        } else {
            "0B".into()
        };
    }

    let pct = if settings.total_mem == 0 {
        0.0
    } else {
        (mem as f64 * 100.0 / settings.total_mem as f64).clamp(0.0, 100.0)
    };
    format!("{pct:.1}%")
}

fn process_row_color<'a>(
    display_cpu: f64,
    row_index: usize,
    selected: usize,
    visible_rows: usize,
    settings: &ProcFrame,
    proc_grad: &'a [String],
    fg: &'a str,
) -> &'a str {
    if !settings.proc_colors || proc_grad.is_empty() {
        return fg;
    }

    let cpu_idx = display_cpu.round().clamp(0.0, 100.0) as usize;
    if !settings.proc_gradient {
        return proc_grad[cpu_idx.min(100)].as_str();
    }

    // Fade the gradient color based on distance from the selected row:
    // closer rows get brighter colors, distant rows fade toward the low end.
    let distance = row_index.abs_diff(selected);
    let fade_loss = distance.saturating_mul(100) / visible_rows.max(1);
    let idx = (cpu_idx + 100)
        .saturating_sub(fade_loss.min(100))
        .saturating_sub(100)
        .min(100);
    proc_grad[idx].as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_frame() -> ProcFrame {
        ProcFrame {
            proc_per_core: true,
            core_count: 4,
            proc_mem_bytes: true,
            total_mem: 1024 * 1024 * 1024,
            proc_colors: true,
            proc_gradient: true,
            base_10: false,
        }
    }

    #[test]
    fn display_proc_cpu_matches_total_power_semantics() {
        let settings = ProcFrame {
            proc_per_core: false,
            core_count: 24,
            ..make_frame()
        };

        assert_eq!(display_proc_cpu(300.0, &settings), 12.5);
        assert_eq!(display_proc_cpu(2400.0, &settings), 100.0);
    }

    #[test]
    fn display_proc_cpu_matches_per_core_semantics() {
        let settings = ProcFrame {
            proc_per_core: true,
            core_count: 24,
            ..make_frame()
        };

        assert_eq!(display_proc_cpu(300.0, &settings), 300.0);
        assert_eq!(display_proc_cpu(3000.0, &settings), 2400.0);
    }

    #[test]
    fn display_proc_cpu_handles_invalid_values() {
        let settings = ProcFrame {
            proc_per_core: false,
            core_count: 0,
            ..make_frame()
        };

        assert_eq!(display_proc_cpu(f64::NAN, &settings), 0.0);
        assert_eq!(display_proc_cpu(-10.0, &settings), 0.0);
        assert_eq!(display_proc_cpu(100.0, &settings), 100.0);
    }

    #[test]
    fn proc_gradient_setting_changes_row_color_mode() {
        let gradient: Vec<String> = (0..=100).map(|i| i.to_string()).collect();
        let settings = make_frame();
        let no_gradient = ProcFrame {
            proc_gradient: false,
            ..make_frame()
        };

        assert_eq!(
            process_row_color(50.0, 5, 0, 10, &settings, &gradient, "fg"),
            "0"
        );
        assert_eq!(
            process_row_color(50.0, 5, 0, 10, &no_gradient, &gradient, "fg"),
            "50"
        );
    }

    // ────────────────────────────────────────────────────────────
    // Dead-row indicator (paused-list ✗ prefix + dead_proc_fg)
    // ────────────────────────────────────────────────────────────

    use crate::collect::CollectStatus;
    use crate::domain::process::{PriorityClass, ProcInfo, ProcState};
    use crate::theme::Theme;
    use crate::theme_keys as tc;
    use crate::ui::WidgetArea;
    use std::collections::HashSet;
    use std::sync::OnceLock;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut in_esc = false;
        for ch in s.chars() {
            if in_esc {
                if ch.is_ascii_alphabetic() || ch == 'm' {
                    in_esc = false;
                }
                continue;
            }
            if ch == '\x1b' {
                in_esc = true;
                continue;
            }
            out.push(ch);
        }
        out
    }

    fn dead_test_procs() -> Vec<ProcInfo> {
        vec![
            ProcInfo {
                pid: 100,
                name: "alive.exe".into(),
                cmd: String::new(),
                cpu_p: 12.0,
                mem: 256 * 1024 * 1024,
                state: ProcState::Running,
                priority: PriorityClass::Normal,
                ..Default::default()
            },
            ProcInfo {
                pid: 200,
                name: "dead.exe".into(),
                cmd: String::new(),
                cpu_p: 80.0,
                mem: 1024 * 1024 * 1024,
                state: ProcState::Running,
                priority: PriorityClass::Normal,
                ..Default::default()
            },
        ]
    }

    fn render_with_dead(dead_pids: &HashSet<u32>, selected: usize, followed_pid: u32) -> String {
        let theme = Theme::default();
        let procs = dead_test_procs();
        let entries = vec![
            crate::domain::process::ProcDisplayEntry::flat(0),
            crate::domain::process::ProcDisplayEntry::flat(1),
        ];
        let area = WidgetArea {
            x: 0,
            y: 0,
            width: 80,
            height: 12,
            rounded: true,
        };
        static EMPTY: OnceLock<HashSet<u32>> = OnceLock::new();
        let _ = EMPTY.get_or_init(HashSet::new);
        let view = crate::ui::ProcView {
            start: 0,
            selected,
            sort_by: crate::collect::process_display::ProcSort::Cpu,
            sort_reversed: false,
            tree_mode: false,
            detail: None,
            followed_pid,
            filter: "",
            filtering: false,
            armed_name: "",
            armed_force: false,
            paused: !dead_pids.is_empty(),
            dead_pids,
            selected_pid: procs.get(selected).map_or(0, |p| p.pid),
        };
        let frame = make_frame();
        crate::ui::proc_widget::draw(
            &procs,
            &entries,
            &area,
            &theme,
            &frame,
            &view,
            &CollectStatus::Ok,
        )
    }

    #[test]
    fn dead_row_emits_dead_proc_fg_color_and_ballot_x_prefix() {
        let mut dead = HashSet::new();
        dead.insert(200);
        let out = render_with_dead(&dead, 0, 0);
        let theme = Theme::default();
        let dead_fg = theme.color(tc::DEAD_PROC_FG);
        // The dead row's text must be preceded by dead_proc_fg.
        assert!(
            out.contains(&format!("{dead_fg}200")),
            "dead row PID should be preceded by dead_proc_fg"
        );
        // The displayed name should carry the ✗ prefix.
        let plain = strip_ansi(&out);
        assert!(
            plain.contains("\u{2717} dead.exe") || plain.contains("\u{2717} dead"),
            "dead row name should be prefixed with `✗ `: {plain}"
        );
    }

    #[test]
    fn live_row_does_not_carry_ballot_x_prefix() {
        let dead = HashSet::new();
        let out = render_with_dead(&dead, 0, 0);
        let plain = strip_ansi(&out);
        assert!(
            !plain.contains("\u{2717}"),
            "live rows must not show the ballot-X prefix: {plain}"
        );
    }

    #[test]
    fn dead_row_keeps_ballot_x_prefix_when_selected() {
        // Dead + selected: bg goes to selected_bg (selection wins
        // on the bg channel), but the ✗ prefix still appears so
        // the user knows the row is dead even when highlighted.
        let mut dead = HashSet::new();
        dead.insert(200);
        let out = render_with_dead(&dead, 1, 0); // PID 200 is at index 1
        let plain = strip_ansi(&out);
        assert!(
            plain.contains("\u{2717} "),
            "selected dead row should still carry the ballot-X prefix: {plain}"
        );
    }

    #[test]
    fn dead_row_name_field_visible_width_matches_alive_row() {
        // Regression: `✗` is 3 UTF-8 bytes but renders as 1 cell.
        // Padding the name field by byte length under-fills the
        // dead row by 2 cells per `✗`, shifting the cmd / cpu /
        // mem columns leftward. This test asserts that the visible
        // text on each row has the same length: the trailing
        // segments after pid_w (name + sep + cmd_w + sep + cpu_w +
        // sep + mem_w) are columnar with fixed visible widths and
        // must match across alive and dead rows.
        let mut dead = HashSet::new();
        dead.insert(200);
        let out = render_with_dead(&dead, 0, 0);

        // Group emitted text by Y coordinate from the cursor-move
        // escapes (`\x1b[Y;XH`).
        let mut lines: std::collections::BTreeMap<u32, String> = std::collections::BTreeMap::new();
        let mut chars = out.chars();
        let mut current_y: Option<u32> = None;
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                if chars.next() != Some('[') {
                    continue;
                }
                let mut seq = String::new();
                let mut terminator = '\0';
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        terminator = c;
                        break;
                    }
                    seq.push(c);
                }
                if terminator == 'H'
                    && let Some((y_str, _x_str)) = seq.split_once(';')
                    && let Ok(y) = y_str.parse::<u32>()
                {
                    current_y = Some(y);
                }
                continue;
            }
            if let Some(y) = current_y {
                lines.entry(y).or_default().push(ch);
            }
        }

        let alive_line = lines
            .values()
            .find(|l| l.contains("alive.exe"))
            .expect("alive row present");
        let dead_line = lines
            .values()
            .find(|l| l.contains("dead.exe"))
            .expect("dead row present");

        let alive_w = unicode_width::UnicodeWidthStr::width(alive_line.trim_end());
        let dead_w = unicode_width::UnicodeWidthStr::width(dead_line.trim_end());

        assert_eq!(
            alive_w, dead_w,
            "alive and dead rows must have identical visible width \
             (alive={alive_w}, dead={dead_w}); dead row was: {dead_line:?}"
        );
    }
}
