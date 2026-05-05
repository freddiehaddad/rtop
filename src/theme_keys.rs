//! Typed theme color and gradient key constants.
//!
//! Use these instead of raw string literals:
//! ```
//! use crate::theme_keys as tc;
//! theme.color(tc::MAIN_FG);
//! theme.gradient(tc::GRAD_CPU_UPPER);
//! ```

/// Index-based color key for theme color lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColorKey(pub(crate) usize);

impl ColorKey {
    /// Array index for this color key.
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Index-based gradient key for theme gradient lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GradientKey(pub(crate) usize);

impl GradientKey {
    /// Array index for this gradient key.
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Color keys for `Theme::color()` lookups.
pub const MAIN_BG: ColorKey = ColorKey(0);
pub const MAIN_FG: ColorKey = ColorKey(1);
pub const TITLE: ColorKey = ColorKey(2);
pub const HI_FG: ColorKey = ColorKey(3);
pub const SELECTED_BG: ColorKey = ColorKey(4);
pub const SELECTED_FG: ColorKey = ColorKey(5);
pub const GRAPH_TEXT: ColorKey = ColorKey(6);
pub const METER_BG: ColorKey = ColorKey(7);
/// Color for tree view connector lines (├─, └─, │).
pub const PROC_TREE_FG: ColorKey = ColorKey(8);
pub const CPU_WIDGET: ColorKey = ColorKey(9);
pub const MEM_WIDGET: ColorKey = ColorKey(10);
pub const NET_WIDGET: ColorKey = ColorKey(11);
pub const PROC_WIDGET: ColorKey = ColorKey(12);
pub const GPU_WIDGET: ColorKey = ColorKey(13);
pub const DISK_WIDGET: ColorKey = ColorKey(14);
pub const HELP_BOX: ColorKey = ColorKey(15);
pub const OPTIONS_BOX: ColorKey = ColorKey(16);
pub const FOLLOWED_BG: ColorKey = ColorKey(17);
pub const FOLLOWED_FG: ColorKey = ColorKey(18);

// Gradient keys for `Theme::gradient()` lookups.
// Grouped by widget: CPU, memory, network, GPU, disk, other.

// CPU
pub const GRAD_CPU_UPPER: GradientKey = GradientKey(0);
pub const GRAD_CPU_LOWER: GradientKey = GradientKey(1);

// Memory
pub const GRAD_USED: GradientKey = GradientKey(2);
pub const GRAD_AVAILABLE: GradientKey = GradientKey(3);
pub const GRAD_CACHED: GradientKey = GradientKey(4);
pub const GRAD_FREE: GradientKey = GradientKey(5);

// Network
pub const GRAD_DOWNLOAD: GradientKey = GradientKey(6);
pub const GRAD_UPLOAD: GradientKey = GradientKey(7);

// GPU
pub const GRAD_GPU: GradientKey = GradientKey(8);
pub const GRAD_GPU_CLOCK: GradientKey = GradientKey(9);
pub const GRAD_GPU_POWER: GradientKey = GradientKey(10);
pub const GRAD_GPU_VRAM: GradientKey = GradientKey(11);

// Disk
pub const GRAD_DISK_READ: GradientKey = GradientKey(12);
pub const GRAD_DISK_WRITE: GradientKey = GradientKey(13);
pub const GRAD_DISK_BUSY: GradientKey = GradientKey(14);

// Other
pub const GRAD_TEMP: GradientKey = GradientKey(15);
pub const GRAD_PROCESS: GradientKey = GradientKey(16);
