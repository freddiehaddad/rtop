/// Shared area description for UI box draw functions.
pub struct BoxArea {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub rounded: bool,
}

/// Display state for the process list view.
pub struct ProcView<'a> {
    pub start: usize,
    pub selected: usize,
    pub sort_by: &'a str,
    pub sort_reversed: bool,
    pub tree_mode: bool,
    pub detailed_pid: u32,
    pub filter: &'a str,
    pub filtering: bool,
}

pub mod cpu_box;
pub mod gpu_box;
pub mod mem_box;
pub mod net_box;
pub mod proc_box;
