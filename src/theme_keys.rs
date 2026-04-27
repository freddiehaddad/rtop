//! Typed theme color key constants.
//!
//! Use these instead of raw string literals to get compile-time typo protection:
//! ```
//! use crate::theme_keys as tc;
//! theme.c(tc::MAIN_FG)  // instead of theme.c("main_fg")
//! ```

#![allow(dead_code)]

/// Color keys for theme.c() lookups.
pub const MAIN_BG: &str = "main_bg";
pub const MAIN_FG: &str = "main_fg";
pub const TITLE: &str = "title";
pub const HI_FG: &str = "hi_fg";
pub const SELECTED_BG: &str = "selected_bg";
pub const SELECTED_FG: &str = "selected_fg";
pub const INACTIVE_FG: &str = "inactive_fg";
pub const GRAPH_TEXT: &str = "graph_text";
pub const METER_BG: &str = "meter_bg";
pub const PROC_MISC: &str = "proc_misc";
pub const CPU_BOX: &str = "cpu_box";
pub const MEM_BOX: &str = "mem_box";
pub const NET_BOX: &str = "net_box";
pub const PROC_BOX: &str = "proc_box";
pub const GPU_BOX: &str = "gpu_box";
pub const DISK_BOX: &str = "disk_box";
pub const HELP_BOX: &str = "help_box";
pub const OPTIONS_BOX: &str = "options_box";

/// Gradient keys for theme.g() lookups.
pub const GRAD_CPU: &str = "cpu";
pub const GRAD_TEMP: &str = "temp";
pub const GRAD_FREE: &str = "free";
pub const GRAD_CACHED: &str = "cached";
pub const GRAD_AVAILABLE: &str = "available";
pub const GRAD_USED: &str = "used";
pub const GRAD_DOWNLOAD: &str = "download";
pub const GRAD_UPLOAD: &str = "upload";
pub const GRAD_PROCESS: &str = "process";
