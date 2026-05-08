use crate::ui::{ProcView, WidgetArea};

// Process list column widths.
const COL_PID: usize = 7;
const COL_CPU: usize = 6;
const COL_MEM: usize = 6;
/// Minimum inner width to show the Command column.
const CMD_COL_THRESHOLD: usize = 55;
/// Inner width above which the Program column expands.
const WIDE_PROG_THRESHOLD: usize = 70;
const PROG_WIDE: usize = 16;
const PROG_NARROW: usize = 8;
/// Number of 1-space gaps between columns (PID, Name, [Cmd], Cpu, Mem).
const COL_SPACING: usize = 4;
const COL_SPACING_NO_CMD: usize = 3;
/// Max height for the process detail panel (fields like User, Status, etc.).
const MAX_DETAIL_ROWS: usize = 8;
/// Height overhead when the detail panel is active: header + divider + detail
/// divider + bottom border + spacing rows.
const DETAIL_OVERHEAD: usize = 6;
/// Row overhead subtracted from widget height: header(1) + column header(1) +
/// header divider(1) + bottom border(1).
const WIDGET_ROW_OVERHEAD: usize = 4;

/// Rows available for process entries between the header divider and bottom border.
pub(crate) fn visible_row_count(widget_height: usize, detail_rows: usize) -> usize {
    widget_height.saturating_sub(detail_rows + WIDGET_ROW_OVERHEAD)
}

pub(super) struct ProcColumns {
    pub(super) pid_w: usize,
    pub(super) name_w: usize,
    pub(super) cmd_w: usize,
    pub(super) cpu_w: usize,
    pub(super) mem_w: usize,
    pub(super) has_cmd_col: bool,
}

impl ProcColumns {
    pub(super) fn calculate(inner_w: usize, tree_mode: bool) -> Self {
        let pid_w = COL_PID;
        let cpu_w = COL_CPU;
        let mem_w = COL_MEM;
        let has_cmd_col = inner_w > CMD_COL_THRESHOLD;
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

        Self {
            pid_w,
            name_w,
            cmd_w,
            cpu_w,
            mem_w,
            has_cmd_col,
        }
    }
}

pub(super) struct ProcWidgetLayout {
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

impl ProcWidgetLayout {
    pub(super) fn calculate(area: &WidgetArea, view: &ProcView) -> Self {
        let inner_w = area.width.saturating_sub(4);
        let detail_rows = if view.detail.is_some() {
            MAX_DETAIL_ROWS.min(area.height.saturating_sub(DETAIL_OVERHEAD))
        } else {
            0
        };
        let columns = ProcColumns::calculate(inner_w, view.tree_mode);

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
    pub(super) sort_by: crate::collect::process_display::ProcSort,
}

impl SortState {
    pub(super) fn new(view: &ProcView) -> Self {
        Self {
            arrow: if view.sort_reversed { "▼" } else { "▲" },
            sort_by: view.sort_by,
        }
    }

    pub(super) fn is_sort(&self, col: &str) -> bool {
        use crate::collect::process_display::ProcSort;
        match col {
            "pid" => self.sort_by == ProcSort::Pid,
            "name" => self.sort_by == ProcSort::Name,
            "command" => self.sort_by == ProcSort::Command,
            "cpu" => matches!(self.sort_by, ProcSort::Cpu),
            "mem" | "memory" => self.sort_by == ProcSort::Memory,
            _ => false,
        }
    }
}
