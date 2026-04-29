use super::ProcBoxSettings;
use crate::ui::{BoxArea, ProcView};

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
pub(super) const PROC_CPU_GRAPH_W: usize = 5;
/// Max height for the process detail panel (fields like User, Status, etc.).
const MAX_DETAIL_ROWS: usize = 8;
/// Height overhead when the detail panel is active: header + divider + detail
/// divider + bottom border + spacing rows.
const DETAIL_OVERHEAD: usize = 6;
/// Row overhead subtracted from box height: header(1) + column header(1) +
/// header divider(1) + bottom border(1).
const BOX_ROW_OVERHEAD: usize = 4;

/// Rows available for process entries between the header divider and bottom border.
pub(crate) fn visible_row_count(box_height: usize, detail_rows: usize) -> usize {
    box_height.saturating_sub(detail_rows + BOX_ROW_OVERHEAD)
}

pub(super) struct ProcColumns {
    pub(super) pid_w: usize,
    pub(super) name_w: usize,
    pub(super) cmd_w: usize,
    pub(super) graph_w: usize,
    pub(super) cpu_w: usize,
    pub(super) mem_w: usize,
    pub(super) has_cmd_col: bool,
    pub(super) show_cpu_graphs: bool,
}

impl ProcColumns {
    pub(super) fn calculate(
        inner_w: usize,
        tree_mode: bool,
        settings: &ProcBoxSettings<'_>,
    ) -> Self {
        let pid_w = COL_PID;
        let cpu_w = COL_CPU;
        let mem_w = COL_MEM;
        let has_cmd_col = inner_w > CMD_COL_THRESHOLD;
        let show_cpu_graphs = settings.proc_cpu_graphs
            && has_cmd_col
            && inner_w > CMD_COL_THRESHOLD + PROC_CPU_GRAPH_W;
        let graph_w = if show_cpu_graphs { PROC_CPU_GRAPH_W } else { 0 };
        let graph_spacing = usize::from(show_cpu_graphs);
        let (name_w, cmd_w) = if has_cmd_col {
            let prog = if tree_mode {
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
            let cmd = inner_w.saturating_sub(
                pid_w + prog + graph_w + cpu_w + mem_w + COL_SPACING + graph_spacing,
            );
            (prog, cmd)
        } else {
            let prog = if tree_mode {
                inner_w
                    .saturating_sub(
                        pid_w + graph_w + cpu_w + mem_w + COL_SPACING_NO_CMD + graph_spacing,
                    )
                    .max(PROG_NARROW + 8)
            } else {
                inner_w.saturating_sub(
                    pid_w + graph_w + cpu_w + mem_w + COL_SPACING_NO_CMD + graph_spacing,
                )
            };
            (prog, 0)
        };

        Self {
            pid_w,
            name_w,
            cmd_w,
            graph_w,
            cpu_w,
            mem_w,
            has_cmd_col,
            show_cpu_graphs,
        }
    }
}

pub(super) struct ProcBoxLayout {
    pub(super) x: usize,
    pub(super) y: usize,
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) inner_w: usize,
    pub(super) detail_rows: usize,
    pub(super) header_y: usize,
    pub(super) divider_y: usize,
    pub(super) detail_divider_y: Option<usize>,
    pub(super) first_row_y: usize,
    pub(super) bottom_y: usize,
    pub(super) max_rows: usize,
    pub(super) columns: ProcColumns,
}

impl ProcBoxLayout {
    pub(super) fn calculate(
        area: &BoxArea,
        view: &ProcView,
        settings: &ProcBoxSettings<'_>,
    ) -> Self {
        let inner_w = area.width.saturating_sub(4);
        let detail_rows = if view.detailed_pid > 0 {
            MAX_DETAIL_ROWS.min(area.height.saturating_sub(DETAIL_OVERHEAD))
        } else {
            0
        };
        let columns = ProcColumns::calculate(inner_w, view.tree_mode, settings);

        Self {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height,
            inner_w,
            detail_rows,
            header_y: area.y + 2 + detail_rows,
            divider_y: area.y + 3 + detail_rows,
            detail_divider_y: (detail_rows > 0).then_some(area.y + 1 + detail_rows),
            first_row_y: area.y + 4 + detail_rows,
            bottom_y: area.y + area.height,
            max_rows: visible_row_count(area.height, detail_rows),
            columns,
        }
    }

    pub(super) fn is_too_small(&self) -> bool {
        self.inner_w == 0 || self.height < 3
    }
}

pub(super) struct SortState {
    pub(super) arrow: &'static str,
    pub(super) sort_lower: String,
}

impl SortState {
    pub(super) fn new(view: &ProcView) -> Self {
        Self {
            arrow: if view.sort_reversed { "▼" } else { "▲" },
            sort_lower: view.sort_by.to_lowercase(),
        }
    }

    pub(super) fn is_sort(&self, col: &str) -> bool {
        match col {
            "pid" => self.sort_lower == "pid",
            "name" => self.sort_lower == "name",
            "command" => self.sort_lower == "command",
            "cpu" => self.sort_lower.starts_with("cpu"),
            "mem" | "memory" => self.sort_lower == "memory",
            _ => false,
        }
    }
}
