//! Typed config key constants.
//!
//! Use these instead of raw string literals to get compile-time typo protection:
//! ```
//! use crate::config_keys::bool_keys as bk;
//! config.get_bool(bk::SHOW_SWAP)  // instead of config.get_bool("show_swap")
//! ```

/// Boolean config keys.
pub mod bool_keys {
    pub const THEME_BACKGROUND: &str = "theme_background";
    pub const TRUECOLOR: &str = "truecolor";
    pub const ROUNDED_CORNERS: &str = "rounded_corners";
    pub const PROC_REVERSED: &str = "proc_reversed";
    pub const PROC_TREE: &str = "proc_tree";
    pub const PROC_COLORS: &str = "proc_colors";
    pub const PROC_GRADIENT: &str = "proc_gradient";
    pub const PROC_PER_CORE: &str = "proc_per_core";
    pub const PROC_MEM_BYTES: &str = "proc_mem_bytes";
    pub const PROC_CPU_GRAPHS: &str = "proc_cpu_graphs";
    pub const PROC_LEFT: &str = "proc_left";
    pub const PROC_FILTER_KERNEL: &str = "proc_filter_kernel";
    pub const PROC_FOLLOW_DETAILED: &str = "proc_follow_detailed";
    pub const PROC_AGGREGATE: &str = "proc_aggregate";
    pub const KEEP_DEAD_PROC_USAGE: &str = "keep_dead_proc_usage";
    pub const CPU_INVERT_LOWER: &str = "cpu_invert_lower";
    pub const CPU_SINGLE_GRAPH: &str = "cpu_single_graph";
    pub const CPU_BOTTOM: &str = "cpu_bottom";
    pub const SHOW_UPTIME: &str = "show_uptime";
    pub const SHOW_CPU_WATTS: &str = "show_cpu_watts";
    pub const CHECK_TEMP: &str = "check_temp";
    pub const SHOW_CORETEMP: &str = "show_coretemp";
    pub const SHOW_CPU_FREQ: &str = "show_cpu_freq";
    pub const MEM_GRAPHS: &str = "mem_graphs";
    pub const MEM_BELOW_NET: &str = "mem_below_net";
    pub const SHOW_SWAP: &str = "show_swap";
    pub const SWAP_DISK: &str = "swap_disk";
    pub const SHOW_DISKS: &str = "show_disks";
    pub const ONLY_PHYSICAL: &str = "only_physical";
    pub const SHOW_IO_STAT: &str = "show_io_stat";
    pub const IO_MODE: &str = "io_mode";
    pub const IO_GRAPH_COMBINED: &str = "io_graph_combined";
    pub const SWAP_UPLOAD_DOWNLOAD: &str = "swap_upload_download";
    pub const BASE_10_SIZES: &str = "base_10_sizes";
    pub const NET_AUTO: &str = "net_auto";
    pub const NET_SYNC: &str = "net_sync";
    pub const SHOW_BATTERY: &str = "show_battery";
    pub const SHOW_BATTERY_WATTS: &str = "show_battery_watts";
    pub const VIM_KEYS: &str = "vim_keys";
    pub const FORCE_TTY: &str = "force_tty";
    pub const LOWCOLOR: &str = "lowcolor";
    pub const BACKGROUND_UPDATE: &str = "background_update";
    pub const TERMINAL_SYNC: &str = "terminal_sync";
    pub const SAVE_CONFIG_ON_EXIT: &str = "save_config_on_exit";
    pub const DISABLE_MOUSE: &str = "disable_mouse";
    pub const DISK_FREE_PRIV: &str = "disk_free_priv";
    pub const GPU_MIRROR_GRAPH: &str = "gpu_mirror_graph";
    pub const DISK_IO_MODE: &str = "disk_io_mode";
}

/// String config keys.
pub mod str_keys {
    pub const COLOR_THEME: &str = "color_theme";
    pub const SHOWN_BOXES: &str = "shown_boxes";
    pub const GRAPH_SYMBOL: &str = "graph_symbol";
    pub const GRAPH_SYMBOL_CPU: &str = "graph_symbol_cpu";
    pub const GRAPH_SYMBOL_GPU: &str = "graph_symbol_gpu";
    pub const GRAPH_SYMBOL_MEM: &str = "graph_symbol_mem";
    pub const GRAPH_SYMBOL_NET: &str = "graph_symbol_net";
    pub const GRAPH_SYMBOL_PROC: &str = "graph_symbol_proc";
    pub const PROC_SORTING: &str = "proc_sorting";
    pub const CPU_GRAPH_UPPER: &str = "cpu_graph_upper";
    pub const CPU_GRAPH_LOWER: &str = "cpu_graph_lower";
    pub const CPU_SENSOR: &str = "cpu_sensor";
    pub const SELECTED_BATTERY: &str = "selected_battery";
    pub const CPU_CORE_MAP: &str = "cpu_core_map";
    pub const TEMP_SCALE: &str = "temp_scale";
    pub const CLOCK_FORMAT: &str = "clock_format";
    pub const CUSTOM_CPU_NAME: &str = "custom_cpu_name";
    pub const DISKS_FILTER: &str = "disks_filter";
    pub const IO_GRAPH_SPEEDS: &str = "io_graph_speeds";
    pub const NET_IFACE: &str = "net_iface";
    pub const LOG_LEVEL: &str = "log_level";
    pub const PROC_FILTER: &str = "proc_filter";
    pub const PRESETS: &str = "presets";
    pub const INITIAL_SHOWN_BOXES: &str = "initial_shown_boxes";
    pub const CUSTOM_GPU_NAME0: &str = "custom_gpu_name0";
    pub const CUSTOM_GPU_NAME1: &str = "custom_gpu_name1";
    pub const CUSTOM_GPU_NAME2: &str = "custom_gpu_name2";
    pub const CUSTOM_GPU_NAME3: &str = "custom_gpu_name3";
    pub const CUSTOM_GPU_NAME4: &str = "custom_gpu_name4";
    pub const CUSTOM_GPU_NAME5: &str = "custom_gpu_name5";
}

/// Integer config keys.
pub mod int_keys {
    pub const UPDATE_MS: &str = "update_ms";
    pub const NET_DOWNLOAD: &str = "net_download";
    pub const NET_UPLOAD: &str = "net_upload";
    pub const DETAILED_PID: &str = "detailed_pid";
    pub const SELECTED_PID: &str = "selected_pid";
    pub const FOLLOWED_PID: &str = "followed_pid";
    pub const PROC_START: &str = "proc_start";
    pub const PROC_SELECTED: &str = "proc_selected";
    pub const CURRENT_PRESET: &str = "current_preset";
}
