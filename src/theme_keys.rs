//! Typed theme color and gradient key constants.
//!
//! Use these instead of raw string literals:
//! ```
//! use crate::theme_keys as tc;
//! theme.color(tc::MAIN_FG);
//! theme.gradient(tc::GRAD_CPU);
//! ```

/// Color key for theme color lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColorKey {
    name: &'static str,
}

impl ColorKey {
    /// Theme file name for this color key.
    pub const fn name(self) -> &'static str {
        self.name
    }
}

/// Gradient key for theme gradient lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GradientKey {
    name: &'static str,
}

impl GradientKey {
    /// Theme file prefix for this gradient key.
    pub const fn name(self) -> &'static str {
        self.name
    }
}

/// Color keys for `Theme::color()` lookups.
pub const MAIN_BG: ColorKey = ColorKey { name: "main_bg" };
pub const MAIN_FG: ColorKey = ColorKey { name: "main_fg" };
pub const TITLE: ColorKey = ColorKey { name: "title" };
pub const HI_FG: ColorKey = ColorKey { name: "hi_fg" };
pub const SELECTED_BG: ColorKey = ColorKey {
    name: "selected_bg",
};
pub const SELECTED_FG: ColorKey = ColorKey {
    name: "selected_fg",
};
pub const INACTIVE_FG: ColorKey = ColorKey {
    name: "inactive_fg",
};
pub const GRAPH_TEXT: ColorKey = ColorKey { name: "graph_text" };
pub const METER_BG: ColorKey = ColorKey { name: "meter_bg" };
pub const PROC_MISC: ColorKey = ColorKey { name: "proc_misc" };
/// Color for tree view connector lines (├─, └─, │).
pub const PROC_TREE_FG: ColorKey = ColorKey {
    name: "proc_tree_fg",
};
pub const CPU_BOX: ColorKey = ColorKey { name: "cpu_box" };
pub const MEM_BOX: ColorKey = ColorKey { name: "mem_box" };
pub const NET_BOX: ColorKey = ColorKey { name: "net_box" };
pub const PROC_BOX: ColorKey = ColorKey { name: "proc_box" };
pub const GPU_BOX: ColorKey = ColorKey { name: "gpu_box" };
pub const DISK_BOX: ColorKey = ColorKey { name: "disk_box" };
pub const HELP_BOX: ColorKey = ColorKey { name: "help_box" };
pub const OPTIONS_BOX: ColorKey = ColorKey {
    name: "options_box",
};
pub const DIV_LINE: ColorKey = ColorKey { name: "div_line" };
pub const PROC_PAUSE_BG: ColorKey = ColorKey {
    name: "proc_pause_bg",
};
pub const PROC_FOLLOW_BG: ColorKey = ColorKey {
    name: "proc_follow_bg",
};
pub const PROC_BANNER_BG: ColorKey = ColorKey {
    name: "proc_banner_bg",
};
pub const PROC_BANNER_FG: ColorKey = ColorKey {
    name: "proc_banner_fg",
};
pub const FOLLOWED_BG: ColorKey = ColorKey {
    name: "followed_bg",
};
pub const FOLLOWED_FG: ColorKey = ColorKey {
    name: "followed_fg",
};

/// All direct color keys known to the theme system.
pub const COLOR_KEYS: &[ColorKey] = &[
    MAIN_BG,
    MAIN_FG,
    TITLE,
    HI_FG,
    SELECTED_BG,
    SELECTED_FG,
    INACTIVE_FG,
    GRAPH_TEXT,
    METER_BG,
    PROC_MISC,
    PROC_TREE_FG,
    CPU_BOX,
    MEM_BOX,
    NET_BOX,
    PROC_BOX,
    GPU_BOX,
    DISK_BOX,
    HELP_BOX,
    OPTIONS_BOX,
    DIV_LINE,
    PROC_PAUSE_BG,
    PROC_FOLLOW_BG,
    PROC_BANNER_BG,
    PROC_BANNER_FG,
    FOLLOWED_BG,
    FOLLOWED_FG,
];

/// Gradient keys for `Theme::gradient()` lookups.
pub const GRAD_CPU: GradientKey = GradientKey { name: "cpu" };
pub const GRAD_TEMP: GradientKey = GradientKey { name: "temp" };
pub const GRAD_FREE: GradientKey = GradientKey { name: "free" };
pub const GRAD_CACHED: GradientKey = GradientKey { name: "cached" };
pub const GRAD_AVAILABLE: GradientKey = GradientKey { name: "available" };
pub const GRAD_USED: GradientKey = GradientKey { name: "used" };
pub const GRAD_DOWNLOAD: GradientKey = GradientKey { name: "download" };
pub const GRAD_UPLOAD: GradientKey = GradientKey { name: "upload" };
pub const GRAD_PROCESS: GradientKey = GradientKey { name: "process" };
pub const GRAD_GPU: GradientKey = GradientKey { name: "gpu" };
pub const GRAD_GPU_CLOCK: GradientKey = GradientKey { name: "gpu_clock" };
pub const GRAD_GPU_POWER: GradientKey = GradientKey { name: "gpu_power" };
pub const GRAD_GPU_VRAM: GradientKey = GradientKey { name: "gpu_vram" };
pub const GRAD_DISK_READ: GradientKey = GradientKey { name: "disk_read" };
pub const GRAD_DISK_WRITE: GradientKey = GradientKey { name: "disk_write" };
pub const GRAD_DISK_BUSY: GradientKey = GradientKey { name: "disk_busy" };

/// All gradient keys known to the theme system.
pub const GRADIENT_KEYS: &[GradientKey] = &[
    GRAD_CPU,
    GRAD_TEMP,
    GRAD_FREE,
    GRAD_CACHED,
    GRAD_AVAILABLE,
    GRAD_USED,
    GRAD_DOWNLOAD,
    GRAD_UPLOAD,
    GRAD_PROCESS,
    GRAD_GPU,
    GRAD_GPU_CLOCK,
    GRAD_GPU_POWER,
    GRAD_GPU_VRAM,
    GRAD_DISK_READ,
    GRAD_DISK_WRITE,
    GRAD_DISK_BUSY,
];
