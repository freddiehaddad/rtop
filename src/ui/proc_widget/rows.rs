use super::layout::{ProcColumns, ProcWidgetLayout};
use super::{ProcColors, ProcWidgetSettings};
use crate::domain::process::{ProcDisplayEntry, ProcInfo};
use crate::draw::buffer::AnsiBuffer;
use crate::tools;

pub(super) struct ProcessRowsParams<'a> {
    pub(super) procs: &'a [ProcInfo],
    pub(super) entries: &'a [ProcDisplayEntry],
    pub(super) layout: &'a ProcWidgetLayout,
    pub(super) start: usize,
    pub(super) selected: usize,
    pub(super) followed_pid: u32,
    pub(super) tree_mode: bool,
    pub(super) settings: &'a ProcWidgetSettings,
    pub(super) colors: &'a ProcColors<'a>,
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
    settings: &'a ProcWidgetSettings,
    colors: &'a ProcColors<'a>,
}

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
    let proc_color = process_row_color(
        display_cpu,
        params.absolute_index,
        params.selected,
        params.layout.max_rows,
        params.settings,
        params.colors.proc_grad,
        params.colors.fg,
    );

    let (tree_prefix, bare_name) = if params.tree_mode && !params.entry.prefix.is_empty() {
        (params.entry.prefix.as_str(), params.proc.name.as_str())
    } else {
        ("", params.proc.name.as_str())
    };
    let prefix_w = tools::ulen(tree_prefix, false);
    let name_avail = columns.name_w.saturating_sub(prefix_w);
    let display_name = tools::uresize(bare_name, name_avail, false);
    let pid_str = format!("{:<pid_w$}", params.proc.pid, pid_w = columns.pid_w);
    let cpu_str = format!("{:>cpu_w$.1}", display_cpu, cpu_w = columns.cpu_w);
    let mem_str_fmt = format!("{:>mem_w$}", mem_str, mem_w = columns.mem_w);
    let cmd_display = command_display(params.proc, columns);
    let name_padded = tools::ljust(&display_name, name_avail, false);

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
                name_avail,
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
                name_avail,
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
                name_avail,
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

pub(super) fn display_proc_cpu(cpu_per_core: f64, settings: &ProcWidgetSettings) -> f64 {
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

pub(super) fn format_proc_memory(mem: u64, settings: &ProcWidgetSettings) -> String {
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
    settings: &ProcWidgetSettings,
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

    fn make_settings() -> ProcWidgetSettings {
        ProcWidgetSettings {
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
        let settings = ProcWidgetSettings {
            proc_per_core: false,
            core_count: 24,
            ..make_settings()
        };

        assert_eq!(display_proc_cpu(300.0, &settings), 12.5);
        assert_eq!(display_proc_cpu(2400.0, &settings), 100.0);
    }

    #[test]
    fn display_proc_cpu_matches_per_core_semantics() {
        let settings = ProcWidgetSettings {
            proc_per_core: true,
            core_count: 24,
            ..make_settings()
        };

        assert_eq!(display_proc_cpu(300.0, &settings), 300.0);
        assert_eq!(display_proc_cpu(3000.0, &settings), 2400.0);
    }

    #[test]
    fn display_proc_cpu_handles_invalid_values() {
        let settings = ProcWidgetSettings {
            proc_per_core: false,
            core_count: 0,
            ..make_settings()
        };

        assert_eq!(display_proc_cpu(f64::NAN, &settings), 0.0);
        assert_eq!(display_proc_cpu(-10.0, &settings), 0.0);
        assert_eq!(display_proc_cpu(100.0, &settings), 100.0);
    }

    #[test]
    fn proc_gradient_setting_changes_row_color_mode() {
        let gradient: Vec<String> = (0..=100).map(|i| i.to_string()).collect();
        let settings = make_settings();
        let no_gradient = ProcWidgetSettings {
            proc_gradient: false,
            ..make_settings()
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
}
