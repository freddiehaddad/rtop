mod borders;
mod detail;
mod layout;
mod rows;

use crate::collect::CollectStatus;
use crate::domain::process::{ProcDisplayEntry, ProcInfo};
use crate::draw::box_drawing;
use crate::draw::box_drawing::symbols;
use crate::draw::buffer::AnsiBuffer;
use crate::theme::Theme;
use crate::theme_keys as tc;

use self::borders::{BottomBorderParams, draw_bottom_border, draw_top_border};
use self::detail::{draw_detail_panel, find_detailed_proc};
use self::layout::{ProcWidgetLayout, SortState};
use self::rows::{ProcessRowsParams, draw_rows};

use super::{ProcView, WidgetArea};

pub(crate) use layout::visible_row_count;

/// Process widget per-frame view passed to [`draw`].
pub struct ProcFrame {
    pub proc_per_core: bool,
    pub core_count: usize,
    pub proc_mem_bytes: bool,
    pub total_mem: u64,
    pub proc_colors: bool,
    pub proc_gradient: bool,
    pub base_10: bool,
}

/// Draw the process list widget into an ANSI string matching btop's layout.
///
/// Layout:
/// ╭─ proc ───────────────────────────────────╮
/// │ PID    Program              Cpu%    Mem%  │
/// │ 1234   chrome.exe           12.3   1.2G  │
/// │ 5678   code.exe              8.1   0.9G  │
/// │ ...                                      │
/// ╰──────────────────────── 25/350 ──────────╯
/// Draw the process list widget with sort indicator on the active column.
pub fn draw(
    procs: &[ProcInfo],
    entries: &[ProcDisplayEntry],
    area: &WidgetArea,
    theme: &Theme,
    settings: &ProcFrame,
    view: &ProcView,
    status: &CollectStatus,
) -> String {
    let colors = ProcColors::from_theme(theme);
    let layout = ProcWidgetLayout::calculate(area, view);
    let mut buf = AnsiBuffer::new();

    draw_frame(&mut buf, area, &colors, status);

    if layout.is_too_small() {
        return buf.finish();
    }

    draw_detail_section(
        &mut buf,
        &DetailSectionParams {
            procs,
            entries,
            layout: &layout,
            detailed_pid: view.detailed_pid,
            settings,
            theme,
        },
    );
    draw_header(&mut buf, &layout, view, settings, &colors);
    draw_dividers(&mut buf, &layout, &colors);
    draw_rows(
        &mut buf,
        &ProcessRowsParams {
            procs,
            entries,
            layout: &layout,
            start: view.start,
            selected: view.selected,
            followed_pid: view.followed_pid,
            tree_mode: view.tree_mode,
            settings,
            colors: &colors,
        },
    );
    draw_proc_borders(&mut buf, &layout, view, entries.len(), theme);

    buf.finish()
}

struct ProcColors<'a> {
    border_color: &'a str,
    fg: &'a str,
    title_color: &'a str,
    hi: &'a str,
    sel_bg_esc: String,
    sel_fg: &'a str,
    followed_bg_esc: String,
    followed_fg: &'a str,
    tree_fg: &'a str,
    proc_grad: &'a [String],
}

impl<'a> ProcColors<'a> {
    fn from_theme(theme: &'a Theme) -> Self {
        Self {
            border_color: theme.color(tc::PROC_WIDGET),
            fg: theme.color(tc::MAIN_FG),
            title_color: theme.color(tc::TITLE),
            hi: theme.color(tc::HI_FG),
            sel_bg_esc: theme.background(tc::SELECTED_BG),
            sel_fg: theme.color(tc::SELECTED_FG),
            followed_bg_esc: theme.background(tc::FOLLOWED_BG),
            followed_fg: theme.color(tc::FOLLOWED_FG),
            tree_fg: theme.color(tc::PROC_TREE_FG),
            proc_grad: theme.gradient(tc::GRAD_PROCESS),
        }
    }
}

struct DetailSectionParams<'a> {
    procs: &'a [ProcInfo],
    entries: &'a [ProcDisplayEntry],
    layout: &'a ProcWidgetLayout,
    detailed_pid: u32,
    settings: &'a ProcFrame,
    theme: &'a Theme,
}

fn draw_frame(
    buf: &mut AnsiBuffer,
    area: &WidgetArea,
    colors: &ProcColors<'_>,
    status: &CollectStatus,
) {
    buf.text(&box_drawing::create_box(&box_drawing::BoxConfig {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height,
        line_color: colors.border_color,
        fill: true,
        title: "proc",
        title2: "",
        num: crate::ui::PROC_KEY,
        rounded: area.rounded,
        hi_color: colors.hi,
        title_color: colors.title_color,
    }));

    super::draw_status_inset(
        buf,
        status,
        "proc",
        area.x,
        area.y,
        colors.border_color,
        colors.title_color,
    );
}

fn draw_detail_section(buf: &mut AnsiBuffer, params: &DetailSectionParams<'_>) {
    if params.detailed_pid == 0 || params.layout.detail_rows == 0 {
        return;
    }

    if let Some(proc) = find_detailed_proc(params.procs, params.entries, params.detailed_pid) {
        buf.text(&draw_detail_panel(
            proc,
            params.layout.x,
            params.layout.y,
            params.layout.width,
            params.layout.detail_rows,
            params.settings,
            params.theme,
        ));
    }
}

fn draw_header(
    buf: &mut AnsiBuffer,
    layout: &ProcWidgetLayout,
    view: &ProcView,
    settings: &ProcFrame,
    colors: &ProcColors<'_>,
) {
    let sort = SortState::new(view);
    let columns = &layout.columns;
    let pid_label = if sort.is_sort("pid") {
        format!("PID{}", sort.arrow)
    } else {
        "PID".into()
    };
    let name_label = if sort.is_sort("name") {
        format!("Program{}", sort.arrow)
    } else {
        "Program".into()
    };
    let cpu_label = if sort.is_sort("cpu") {
        format!("CPU%{}", sort.arrow)
    } else {
        "CPU%".into()
    };
    let mem_label = if sort.is_sort("mem") {
        format!("{}{}", mem_header_label(settings), sort.arrow)
    } else {
        mem_header_label(settings).into()
    };

    let mut col_x = layout.x + 3;

    let pid_str = format!("{:<pid_w$}", pid_label, pid_w = columns.pid_w);
    let pid_color = if sort.is_sort("pid") {
        colors.hi
    } else {
        colors.title_color
    };
    buf.mv(col_x, layout.header_y)
        .color(pid_color)
        .text(&pid_str);
    col_x += columns.pid_w + 1;

    let name_str = format!("{:<name_w$}", name_label, name_w = columns.name_w);
    let name_color = if sort.is_sort("name") {
        colors.hi
    } else {
        colors.title_color
    };
    buf.mv(col_x, layout.header_y)
        .color(name_color)
        .text(&name_str);
    col_x += columns.name_w + 1;

    if columns.has_cmd_col && columns.cmd_w > 0 {
        let cmd_label = if sort.is_sort("command") {
            format!("Command Line{}", sort.arrow)
        } else {
            "Command Line".into()
        };
        let cmd_str = format!("{:<cmd_w$}", cmd_label, cmd_w = columns.cmd_w);
        let cmd_color = if sort.is_sort("command") {
            colors.hi
        } else {
            colors.title_color
        };
        buf.mv(col_x, layout.header_y)
            .color(cmd_color)
            .text(&cmd_str);
        col_x += columns.cmd_w + 1;
    }

    let cpu_str = format!("{:>cpu_w$}", cpu_label, cpu_w = columns.cpu_w);
    let cpu_color = if sort.is_sort("cpu") {
        colors.hi
    } else {
        colors.title_color
    };
    buf.mv(col_x, layout.header_y)
        .color(cpu_color)
        .text(&cpu_str);
    col_x += columns.cpu_w + 1;

    let mem_str = format!("{:>mem_w$}", mem_label, mem_w = columns.mem_w);
    let mem_color = if sort.is_sort("mem") {
        colors.hi
    } else {
        colors.title_color
    };
    buf.mv(col_x, layout.header_y)
        .color(mem_color)
        .text(&mem_str)
        .reset();
}

fn mem_header_label(settings: &ProcFrame) -> &'static str {
    if settings.proc_mem_bytes {
        "Mem"
    } else {
        "Mem%"
    }
}

fn draw_dividers(buf: &mut AnsiBuffer, layout: &ProcWidgetLayout, colors: &ProcColors<'_>) {
    draw_divider_at(buf, layout, layout.divider_y, colors.border_color);
    if let Some(detail_divider_y) = layout.detail_divider_y {
        draw_divider_at(buf, layout, detail_divider_y, colors.border_color);
    }
}

fn draw_divider_at(
    buf: &mut AnsiBuffer,
    layout: &ProcWidgetLayout,
    row_y: usize,
    border_color: &str,
) {
    buf.mv(layout.x + 1, row_y)
        .color(border_color)
        .text(symbols::DIV_LEFT)
        .color(border_color)
        .text(&symbols::H_LINE.repeat(layout.width.saturating_sub(2)))
        .color(border_color)
        .text(symbols::DIV_RIGHT);
}

fn draw_proc_borders(
    buf: &mut AnsiBuffer,
    layout: &ProcWidgetLayout,
    view: &ProcView,
    entry_count: usize,
    theme: &Theme,
) {
    buf.text(&draw_top_border(
        layout.x,
        layout.y,
        layout.width,
        view.sort_by,
        view.tree_mode,
        theme,
    ));

    let visible = entry_count.min(layout.max_rows);
    buf.text(&draw_bottom_border(
        &BottomBorderParams {
            x: layout.x,
            bottom_y: layout.bottom_y,
            width: layout.width,
            filter: view.filter,
            filtering: view.filtering,
            followed_pid: view.followed_pid,
            visible,
            total: entry_count,
            armed_name: view.armed_name,
            armed_force: view.armed_force,
        },
        theme,
    ));
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

    fn make_entries_for(procs: &[ProcInfo]) -> Vec<ProcDisplayEntry> {
        (0..procs.len()).map(ProcDisplayEntry::flat).collect()
    }

    fn make_numbered_procs(count: usize) -> Vec<ProcInfo> {
        (0..count)
            .map(|i| ProcInfo {
                pid: ((i + 1) * 100) as u32,
                name: format!("proc{i}.exe"),
                cmd: format!("proc{i}.exe"),
                threads: 1,
                user: "User".into(),
                mem: 1024 * 1024,
                cpu_p: i as f64,
                state: ProcState::Running,
                priority: PriorityClass::Normal,
                ppid: 1,
                cpu_time: 0,
                io_read: 0,
                io_write: 0,
            })
            .collect()
    }

    fn make_area() -> WidgetArea {
        WidgetArea {
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
            sort_by: crate::collect::process_display::ProcSort::Cpu,
            sort_reversed: false,
            tree_mode: false,
            detailed_pid: 0,
            followed_pid: 0,
            filter: "",
            filtering: false,
            armed_name: "",
            armed_force: false,
        }
    }

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
    fn draw_contains_proc_title() {
        let output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &make_view(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(plain.contains("proc"), "output should contain 'proc' title");
    }

    #[test]
    fn draw_contains_process_names() {
        let output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &make_view(),
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
        let output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &make_view(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains('▲') || plain.contains('▼'),
            "output should contain a sort direction indicator (▲ or ▼)"
        );
    }

    #[test]
    fn command_column_visibility_follows_width_threshold() {
        let wide_output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &make_view(),
            &CollectStatus::Ok,
        );
        let narrow_area = WidgetArea {
            width: 50,
            ..make_area()
        };
        let narrow_output = draw(
            &make_procs(),
            &make_entries(),
            &narrow_area,
            &Theme::default(),
            &make_frame(),
            &make_view(),
            &CollectStatus::Ok,
        );

        let wide_plain = strip_ansi(&wide_output);
        let narrow_plain = strip_ansi(&narrow_output);
        assert!(wide_plain.contains("Command Line"));
        assert!(wide_plain.contains("--flag"));
        assert!(!narrow_plain.contains("Command Line"));
        assert!(!narrow_plain.contains("--flag"));
    }

    #[test]
    fn detail_divider_only_draws_when_detail_panel_is_active() {
        let mut detail_view = make_view();
        detail_view.detailed_pid = 100;
        let detail_output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &detail_view,
            &CollectStatus::Ok,
        );
        let plain_output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &make_view(),
            &CollectStatus::Ok,
        );

        let detail_dividers = strip_ansi(&detail_output)
            .matches(symbols::DIV_LEFT)
            .count();
        let plain_dividers = strip_ansi(&plain_output).matches(symbols::DIV_LEFT).count();
        assert_eq!(detail_dividers, plain_dividers + 1);
    }

    #[test]
    fn process_rows_fill_last_line_above_bottom_border() {
        let procs = make_numbered_procs(4);
        let entries = make_entries_for(&procs);
        let area = WidgetArea {
            x: 1,
            y: 1,
            width: 80,
            height: 8,
            rounded: true,
        };

        let output = draw(
            &procs,
            &entries,
            &area,
            &Theme::default(),
            &make_frame(),
            &make_view(),
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);

        assert!(
            output.contains("\x1b[8;4H"),
            "fourth process row should be drawn on the last usable row"
        );
        assert!(plain.contains("proc3.exe"));
        assert!(plain.contains("4/4"));
    }

    #[test]
    fn detail_panel_uses_aligned_labels() {
        let mut view = make_view();
        view.detailed_pid = 100;
        let output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &view,
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
    fn proc_per_core_setting_changes_rendered_cpu() {
        let mut procs = make_procs();
        procs[0].cpu_p = 400.0;

        let per_core = draw(
            &procs,
            &make_entries(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &make_view(),
            &CollectStatus::Ok,
        );
        let total_power = draw(
            &procs,
            &make_entries(),
            &make_area(),
            &Theme::default(),
            &ProcFrame {
                proc_per_core: false,
                core_count: 4,
                ..make_frame()
            },
            &make_view(),
            &CollectStatus::Ok,
        );

        assert!(strip_ansi(&per_core).contains("400.0"));
        assert!(strip_ansi(&total_power).contains("100.0"));
    }

    #[test]
    fn proc_mem_bytes_setting_changes_header_and_values() {
        let bytes_output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &Theme::default(),
            &make_frame(),
            &make_view(),
            &CollectStatus::Ok,
        );
        let pct_output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &Theme::default(),
            &ProcFrame {
                proc_mem_bytes: false,
                total_mem: 1024 * 1024 * 1024,
                ..make_frame()
            },
            &make_view(),
            &CollectStatus::Ok,
        );
        let bytes_plain = strip_ansi(&bytes_output);
        let pct_plain = strip_ansi(&pct_output);

        assert!(bytes_plain.contains("Mem"));
        assert!(!bytes_plain.contains("Mem%"));
        assert!(bytes_plain.contains("100M"));
        assert!(pct_plain.contains("Mem%"));
        assert!(pct_plain.contains("9.8%"));
    }

    #[test]
    fn proc_colors_setting_disables_cpu_row_coloring() {
        let theme = Theme::default();
        let cpu_color = theme.gradient(tc::GRAD_PROCESS)[5].clone();
        let mut view = make_view();
        view.selected = usize::MAX;

        let colored = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &theme,
            &ProcFrame {
                proc_gradient: false,
                ..make_frame()
            },
            &view,
            &CollectStatus::Ok,
        );
        let plain = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &theme,
            &ProcFrame {
                proc_colors: false,
                ..make_frame()
            },
            &view,
            &CollectStatus::Ok,
        );

        assert!(colored.contains(&cpu_color));
        assert!(!plain.contains(&cpu_color));
    }

    #[test]
    fn count_inset_uses_title_color() {
        // Defends border-inset color consistency: pre-fix the bottom-right
        // process count "N/M" rendered in MAIN_FG while every other widget's
        // border insets use TITLE for label/value text.
        let theme = Theme::default();
        let output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &theme,
            &make_frame(),
            &make_view(),
            &CollectStatus::Ok,
        );
        let title = theme.color(tc::TITLE);
        // 3 procs in the fixture; layout fits all 3 → "3/3".
        assert!(
            output.contains(&format!("{}{}", title, "3/3")),
            "process count '3/3' inset should be preceded by TITLE"
        );
    }

    #[test]
    fn following_inset_renders_as_chip_matching_followed_row() {
        // When following a process, the bottom-border "following" inset is
        // rendered as a colored chip whose background matches FOLLOWED_BG
        // and whose text matches FOLLOWED_FG — same color identity as the
        // followed row in the list. The chip ends with a hard ANSI reset
        // so the bg does not bleed into the count "N/M" inset rendered to
        // the right.
        let theme = Theme::default();
        let mut view = make_view();
        view.followed_pid = 100;
        let output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &theme,
            &make_frame(),
            &view,
            &CollectStatus::Ok,
        );
        let bg = theme.background(tc::FOLLOWED_BG);
        let fg = theme.color(tc::FOLLOWED_FG);
        let expected = format!("{bg}{fg}following\x1b[0m");
        assert!(
            output.contains(&expected),
            "following inset should be FOLLOWED_BG bg + FOLLOWED_FG fg + 'following' + reset"
        );
    }

    #[test]
    fn following_chip_does_not_break_count_inset_in_narrow_box() {
        // Regression guard against chip width math drifting: with a narrow
        // proc widget and a large process count, both the chip and the right-
        // aligned count must still render and not collide.
        let theme = Theme::default();
        let area = WidgetArea {
            x: 1,
            y: 1,
            width: 60,
            height: 8,
            rounded: true,
        };
        let procs = make_numbered_procs(50);
        let entries = make_entries_for(&procs);
        let mut view = make_view();
        view.followed_pid = 100;
        let output = draw(
            &procs,
            &entries,
            &area,
            &theme,
            &make_frame(),
            &view,
            &CollectStatus::Ok,
        );
        let plain = strip_ansi(&output);
        assert!(
            plain.contains("following"),
            "narrow widget should still render the following chip"
        );
        // 4-row content area (height 8 - 4 overhead) shows 4 of 50 procs.
        assert!(
            plain.contains("4/50"),
            "narrow widget should still render the count '4/50'"
        );
    }

    #[test]
    fn followed_row_uses_followed_bg_not_selected_bg() {
        // System invariant (handlers/normal.rs): F always sets followed_pid =
        // selected pid; any nav key clears followed_pid. So followed_pid > 0
        // implies the cursor is on the followed row. The dispatch in
        // rows.rs::draw_process_row must therefore prefer the followed branch
        // over the selected branch for that row, otherwise pressing F has no
        // visual effect on the row (only on the border chip), which is
        // exactly the bug this guards against.
        let theme = Theme::default();
        let mut view = make_view();
        view.selected = 0;
        view.followed_pid = 100; // matches make_procs()[0].pid
        let output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &theme,
            &make_frame(),
            &view,
            &CollectStatus::Ok,
        );
        let followed_bg = theme.background(tc::FOLLOWED_BG);
        let selected_bg = theme.background(tc::SELECTED_BG);
        assert!(
            output.contains(&followed_bg),
            "followed row should render with FOLLOWED_BG background"
        );
        assert!(
            !output.contains(&selected_bg),
            "no row should render with SELECTED_BG when the followed row is also the selected row"
        );
    }

    #[test]
    fn proc_column_headers_use_title_color() {
        // Carve-out: proc column headers (PID, Program, Command Line, CPU%,
        // Mem) stay TITLE, not MAIN_FG. They're a structural header row above
        // the body data, not row labels themselves.
        let theme = Theme::default();
        let output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &theme,
            &make_frame(),
            &make_view(),
            &CollectStatus::Ok,
        );
        let title = theme.color(tc::TITLE);
        // The header is "PID    Program          ..." padded to column widths.
        // PID, Program, and "Command Line" are left-justified so their text
        // appears immediately after the color escape; right-justified columns
        // (CPU%, Mem) have leading spaces between the escape and the text and
        // are intentionally excluded.
        for label in &["PID", "Program", "Command Line"] {
            assert!(
                output.contains(&format!("{title}{label}")),
                "proc column header {label:?} should be preceded by TITLE"
            );
        }
    }

    #[test]
    fn detail_panel_field_labels_use_main_fg() {
        // Body label rule: detail panel field labels (Cmd, User, Status,
        // Threads, etc.) render in MAIN_FG. Pre-shift these were TITLE.
        let theme = Theme::default();
        let mut view = make_view();
        view.detailed_pid = 100;
        let output = draw(
            &make_procs(),
            &make_entries(),
            &make_area(),
            &theme,
            &make_frame(),
            &view,
            &CollectStatus::Ok,
        );
        let fg = theme.color(tc::MAIN_FG);
        for label in &["Cmd", "User", "Status", "Threads"] {
            assert!(
                output.contains(&format!("{fg}{label}")),
                "detail panel field label {label:?} should be preceded by MAIN_FG"
            );
        }
    }
}
