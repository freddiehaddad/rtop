//! Typed theme color and gradient key constants.
//!
//! Use these instead of raw string literals:
//! ```
//! use crate::theme_keys as tc;
//! theme.color(tc::MAIN_FG);
//! theme.gradient(tc::GRAD_CPU);
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
pub const CPU_BOX: ColorKey = ColorKey(9);
pub const MEM_BOX: ColorKey = ColorKey(10);
pub const NET_BOX: ColorKey = ColorKey(11);
pub const PROC_BOX: ColorKey = ColorKey(12);
pub const GPU_BOX: ColorKey = ColorKey(13);
pub const DISK_BOX: ColorKey = ColorKey(14);
pub const HELP_BOX: ColorKey = ColorKey(15);
pub const OPTIONS_BOX: ColorKey = ColorKey(16);
pub const PROC_FOLLOW_BG: ColorKey = ColorKey(17);
pub const FOLLOWED_BG: ColorKey = ColorKey(18);
pub const FOLLOWED_FG: ColorKey = ColorKey(19);

/// Gradient keys for `Theme::gradient()` lookups.
pub const GRAD_CPU: GradientKey = GradientKey(0);
pub const GRAD_TEMP: GradientKey = GradientKey(1);
pub const GRAD_FREE: GradientKey = GradientKey(2);
pub const GRAD_CACHED: GradientKey = GradientKey(3);
pub const GRAD_AVAILABLE: GradientKey = GradientKey(4);
pub const GRAD_USED: GradientKey = GradientKey(5);
pub const GRAD_DOWNLOAD: GradientKey = GradientKey(6);
pub const GRAD_UPLOAD: GradientKey = GradientKey(7);
pub const GRAD_PROCESS: GradientKey = GradientKey(8);
pub const GRAD_GPU: GradientKey = GradientKey(9);
pub const GRAD_GPU_CLOCK: GradientKey = GradientKey(10);
pub const GRAD_GPU_POWER: GradientKey = GradientKey(11);
pub const GRAD_GPU_VRAM: GradientKey = GradientKey(12);
pub const GRAD_DISK_READ: GradientKey = GradientKey(13);
pub const GRAD_DISK_WRITE: GradientKey = GradientKey(14);
pub const GRAD_DISK_BUSY: GradientKey = GradientKey(15);
