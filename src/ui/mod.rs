use crate::collect::CollectStatus;
use crate::draw::box_drawing;
use crate::draw::buffer::AnsiBuffer;

/// Toggle key digits shown as superscripts in box titles.
///
/// Each constant is the digit key that toggles the corresponding widget.
/// Used by both the renderers (superscript label) and the input handler
/// (keybind dispatch) to keep them in sync.
pub const CPU_KEY: u8 = 1;
pub const MEM_KEY: u8 = 2;
pub const NET_KEY: u8 = 3;
pub const PROC_KEY: u8 = 4;
pub const DISK_KEY: u8 = 5;
/// First GPU toggle key. GPU N uses `GPU_KEY_BASE + N`.
pub const GPU_KEY_BASE: u8 = 6;

/// Shared area description for UI box draw functions.
pub struct BoxArea {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub rounded: bool,
}

impl BoxArea {
    pub fn from_dim(dim: &crate::draw::layout::BoxDimensions, rounded: bool) -> Self {
        Self {
            x: dim.x,
            y: dim.y,
            width: dim.width,
            height: dim.height,
            rounded,
        }
    }
}

/// Draw a status indicator inset on the top border when the collector is
/// degraded or failed. Placed after the box title (left side).
///
/// `title` is the box title text (e.g. "cpu", "mem") used to calculate
/// the offset past the existing title inset.
pub fn draw_status_inset(
    buf: &mut AnsiBuffer,
    status: &CollectStatus,
    title: &str,
    x: usize,
    y: usize,
    box_color: &str,
    title_color: &str,
) {
    if *status == CollectStatus::Ok {
        return;
    }
    let icon = match status {
        CollectStatus::Degraded(_) => "\u{26a0}",
        CollectStatus::Failed(_) => "\u{2717}",
        CollectStatus::Ok => unreachable!(),
    };
    let inset = box_drawing::title_inset(icon, box_color, title_color, false);
    // Position after the box title: title_left(1) + bold_superscript(~1) + title_text + title_right(1)
    // create_box places the title at x+3, so the end of the title region is approximately:
    let title_end_x = x + 3 + box_drawing::inset_width(title) + 1;
    buf.mv(title_end_x, y + 1).text(&inset);
}

/// Display state for the process list view.
pub struct ProcView<'a> {
    pub start: usize,
    pub selected: usize,
    pub sort_by: &'a str,
    pub sort_reversed: bool,
    pub tree_mode: bool,
    pub detailed_pid: u32,
    pub followed_pid: u32,
    pub filter: &'a str,
    pub filtering: bool,
    pub armed_name: &'a str,
    pub armed_force: bool,
}

pub mod cpu_box;
pub mod disk_box;
pub mod gpu_box;
pub mod mem_box;
pub mod net_box;
pub mod proc_box;
