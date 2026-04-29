use super::layout::{PROC_CPU_GRAPH_W, ProcBoxLayout, ProcColumns};
use super::{ProcBoxSettings, ProcColors};
use crate::domain::process::{ProcDisplayEntry, ProcInfo};
use crate::draw::buffer::AnsiBuffer;
use crate::draw::graph::Graph;
use crate::tools;
use std::collections::VecDeque;

pub(super) struct ProcessRowsParams<'a> {
    pub(super) procs: &'a [ProcInfo],
    pub(super) entries: &'a [ProcDisplayEntry],
    pub(super) layout: &'a ProcBoxLayout,
    pub(super) start: usize,
    pub(super) selected: usize,
    pub(super) tree_mode: bool,
    pub(super) settings: &'a ProcBoxSettings<'a>,
    pub(super) colors: &'a ProcColors<'a>,
}

struct ProcessRowParams<'a> {
    proc: &'a ProcInfo,
    entry: &'a ProcDisplayEntry,
    absolute_index: usize,
    row_y: usize,
    layout: &'a ProcBoxLayout,
    selected: usize,
    tree_mode: bool,
    settings: &'a ProcBoxSettings<'a>,
    colors: &'a ProcColors<'a>,
}

struct RowText<'a> {
    pid: String,
    tree_prefix: &'a str,
    name: String,
    cmd: String,
    graph: String,
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
                tree_mode: params.tree_mode,
                settings: params.settings,
                colors: params.colors,
            },
        );
    }
}

fn draw_process_row(buf: &mut AnsiBuffer, params: &ProcessRowParams<'_>) {
    let columns = &params.layout.columns;
    let display_cpu = display_proc_cpu(params.proc.cpu_p, params.settings);
    let mem_str = format_proc_memory(params.proc.mem, params.settings);
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
    let graph_str = if columns.show_cpu_graphs {
        proc_cpu_graph(
            params.proc.pid,
            params.settings,
            params.colors.proc_grad,
            params.colors.fg,
        )
    } else {
        String::new()
    };
    let cmd_display = command_display(params.proc, columns);
    let name_padded = tools::ljust(&display_name, name_avail, false);

    if params.absolute_index == params.selected {
        draw_selected_process_row(
            buf,
            params,
            &RowText {
                pid: pid_str,
                tree_prefix,
                name: name_padded,
                cmd: cmd_display,
                graph: graph_str,
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
                graph: graph_str,
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

    let raw = if proc.cmd.len() > proc.name.len() {
        proc.cmd[proc.name.len()..].trim()
    } else if proc.cmd != proc.name {
        &proc.cmd
    } else {
        ""
    };
    tools::uresize(raw, columns.cmd_w, false)
}

fn draw_selected_process_row(
    buf: &mut AnsiBuffer,
    params: &ProcessRowParams<'_>,
    row: &RowText<'_>,
) {
    let columns = &params.layout.columns;
    let bg_esc = &params.colors.sel_bg_esc;
    buf.mv(params.layout.x + 2, params.row_y)
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
    if columns.show_cpu_graphs {
        buf.text(&row.graph)
            .text(bg_esc)
            .color(params.colors.sel_fg)
            .text(" ");
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
    buf.mv(params.layout.x + 2, params.row_y).color(proc_color);
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
    if columns.show_cpu_graphs {
        buf.text(&row.graph).color(proc_color).text(" ");
    }
    buf.text(&row.cpu).text(" ").text(&row.mem);
}

fn draw_process_name_padding(buf: &mut AnsiBuffer, row: &RowText<'_>, name_w: usize) {
    buf.text(&row.name);
    if row.prefix_w + row.name_avail < name_w {
        buf.text(&" ".repeat(name_w - row.prefix_w - row.name_avail));
    }
}

pub(super) fn display_proc_cpu(cpu_per_core: f64, settings: &ProcBoxSettings<'_>) -> f64 {
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

pub(super) fn format_proc_memory(mem: u64, settings: &ProcBoxSettings<'_>) -> String {
    if settings.proc_mem_bytes {
        return if mem > 0 {
            tools::floating_humanizer(mem, true, 0, false, false, false)
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
    settings: &ProcBoxSettings<'_>,
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

fn proc_cpu_graph(
    pid: u32,
    settings: &ProcBoxSettings<'_>,
    proc_grad: &[String],
    fg: &str,
) -> String {
    let data: VecDeque<i64> = settings
        .cpu_histories
        .get(&pid)
        .filter(|history| !history.is_empty())
        .map(|history| {
            history
                .iter()
                .map(|value| {
                    display_proc_cpu(*value as f64, settings)
                        .round()
                        .clamp(0.0, 100.0) as i64
                })
                .collect()
        })
        .unwrap_or_else(|| VecDeque::from(vec![0; PROC_CPU_GRAPH_W]));

    let fallback_gradient;
    let gradient = if settings.proc_colors && !proc_grad.is_empty() {
        proc_grad
    } else {
        fallback_gradient = vec![fg.to_string(); 101];
        &fallback_gradient
    };

    let mut graph = Graph::new(
        PROC_CPU_GRAPH_W,
        1,
        settings.graph_symbol,
        false,
        true,
        100,
        0,
    );
    expand_cursor_right_padding(&graph.render_row_colored(&data, gradient))
}

fn expand_cursor_right_padding(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'\x1b' && bytes.get(i + 1) == Some(&b'[') {
            let digits_start = i + 2;
            let mut cursor = digits_start;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                cursor += 1;
            }

            if cursor > digits_start
                && bytes.get(cursor) == Some(&b'C')
                && let Ok(count) = input[digits_start..cursor].parse::<usize>()
            {
                out.push_str(&" ".repeat(count));
                i = cursor + 1;
                continue;
            }

            let mut end = i + 2;
            while end < bytes.len() {
                end += 1;
                if bytes[end - 1].is_ascii_alphabetic() {
                    break;
                }
            }
            out.push_str(&input[i..end]);
            i = end;
            continue;
        }

        let Some(ch) = input[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::graph::GraphSymbol;
    use crate::theme::Theme;
    use crate::theme_keys as tc;
    use std::collections::HashMap;

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

    fn empty_histories() -> &'static HashMap<u32, VecDeque<i64>> {
        static HISTORIES: std::sync::OnceLock<HashMap<u32, VecDeque<i64>>> =
            std::sync::OnceLock::new();
        HISTORIES.get_or_init(HashMap::new)
    }

    fn make_settings() -> ProcBoxSettings<'static> {
        ProcBoxSettings {
            proc_per_core: true,
            core_count: 4,
            proc_mem_bytes: true,
            total_mem: 1024 * 1024 * 1024,
            proc_colors: true,
            proc_gradient: true,
            proc_cpu_graphs: false,
            graph_symbol: GraphSymbol::Braille,
            cpu_histories: empty_histories(),
        }
    }

    fn make_settings_with_histories<'a>(
        histories: &'a HashMap<u32, VecDeque<i64>>,
    ) -> ProcBoxSettings<'a> {
        ProcBoxSettings {
            cpu_histories: histories,
            ..make_settings()
        }
    }

    #[test]
    fn display_proc_cpu_matches_total_power_semantics() {
        let settings = ProcBoxSettings {
            proc_per_core: false,
            core_count: 24,
            ..make_settings()
        };

        assert_eq!(display_proc_cpu(300.0, &settings), 12.5);
        assert_eq!(display_proc_cpu(2400.0, &settings), 100.0);
    }

    #[test]
    fn display_proc_cpu_matches_per_core_semantics() {
        let settings = ProcBoxSettings {
            proc_per_core: true,
            core_count: 24,
            ..make_settings()
        };

        assert_eq!(display_proc_cpu(300.0, &settings), 300.0);
        assert_eq!(display_proc_cpu(3000.0, &settings), 2400.0);
    }

    #[test]
    fn display_proc_cpu_handles_invalid_values() {
        let settings = ProcBoxSettings {
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
        let no_gradient = ProcBoxSettings {
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

    #[test]
    fn proc_cpu_graph_expands_cursor_padding_to_spaces() {
        let mut histories = HashMap::new();
        histories.insert(100, VecDeque::from(vec![0, 0, 0, 0, 0]));
        let settings = ProcBoxSettings {
            proc_cpu_graphs: true,
            graph_symbol: GraphSymbol::Block,
            ..make_settings_with_histories(&histories)
        };

        let graph = proc_cpu_graph(
            100,
            &settings,
            Theme::default().gradient(tc::GRAD_PROCESS),
            "fg",
        );

        assert!(
            !graph.contains("\x1b[1C"),
            "graph padding should paint row background instead of moving the cursor"
        );
        assert_eq!(tools::ulen(&strip_ansi(&graph), false), PROC_CPU_GRAPH_W);
        assert_ne!(strip_ansi(&graph), " ".repeat(PROC_CPU_GRAPH_W));
    }

    #[test]
    fn proc_cpu_graph_renders_baseline_without_history() {
        let histories = HashMap::new();
        let settings = ProcBoxSettings {
            proc_cpu_graphs: true,
            graph_symbol: GraphSymbol::Block,
            ..make_settings_with_histories(&histories)
        };

        let graph = proc_cpu_graph(
            100,
            &settings,
            Theme::default().gradient(tc::GRAD_PROCESS),
            "fg",
        );

        assert_eq!(tools::ulen(&strip_ansi(&graph), false), PROC_CPU_GRAPH_W);
        assert_ne!(strip_ansi(&graph), " ".repeat(PROC_CPU_GRAPH_W));
    }
}
