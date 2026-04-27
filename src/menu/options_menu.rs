use crate::config::Config;
use crate::config_keys::{bool_keys as bk, int_keys as ik, str_keys as sk};
use crate::draw::box_drawing::{self, symbols};
use crate::term;
use crate::theme::Theme;
use crate::theme_keys as tc;

// ---------------------------------------------------------------------------
// Option type classification
// ---------------------------------------------------------------------------

/// How an option can be edited.
#[derive(Clone, Copy, PartialEq)]
pub enum OptKind {
    Bool,
    Int,
    /// Cycle through a fixed list of choices.
    Browsable,
    /// Free-form string (not editable via left/right in this version).
    StringVal,
}

/// A single option definition.
pub struct OptDef {
    pub key: &'static str,
    pub desc: &'static [&'static str],
}

// ---------------------------------------------------------------------------
// Browsable option value lists
// ---------------------------------------------------------------------------

/// Return the list of valid values for a browsable option key.
pub fn browsable_values(key: &str) -> &'static [&'static str] {
    match key {
        sk::COLOR_THEME => crate::theme::THEME_NAMES,
        sk::GRAPH_SYMBOL
        | sk::GRAPH_SYMBOL_CPU
        | sk::GRAPH_SYMBOL_MEM
        | sk::GRAPH_SYMBOL_NET
        | sk::GRAPH_SYMBOL_PROC
        | sk::GRAPH_SYMBOL_GPU => &["default", "braille", "block", "tty"],
        sk::CPU_GRAPH_UPPER | sk::CPU_GRAPH_LOWER => &["Auto", "total", "user", "system"],
        sk::TEMP_SCALE => &["celsius", "fahrenheit", "kelvin", "rankine"],
        sk::PROC_SORTING => crate::collect::process::SORT_OPTIONS,
        sk::LOG_LEVEL => &["ERROR", "WARNING", "INFO", "DEBUG"],
        sk::CPU_SENSOR | sk::SELECTED_BATTERY | sk::NET_IFACE => &["Auto"],
        _ => &[],
    }
}

fn classify(key: &str, config: &Config) -> OptKind {
    if config.bools.contains_key(key) {
        return OptKind::Bool;
    }
    if config.ints.contains_key(key) {
        return OptKind::Int;
    }
    if !browsable_values(key).is_empty() {
        return OptKind::Browsable;
    }
    OptKind::StringVal
}

// ---------------------------------------------------------------------------
// Category definitions  (mirroring btop, minus Linux-only options)
// ---------------------------------------------------------------------------

/// Category tab names for the options menu.
pub const CAT_NAMES: &[&str] = &["general", "cpu", "mem", "net", "proc", "gpu", "disk"];

/// Options in the "general" category.
pub const GENERAL: &[OptDef] = &[
    OptDef {
        key: sk::COLOR_THEME,
        desc: &[
            "Set color theme.",
            "",
            "Choose from all bundled themes.",
            "",
            "\"Default\" for builtin default theme.",
        ],
    },
    OptDef {
        key: bk::THEME_BACKGROUND,
        desc: &[
            "If the theme set background should be shown.",
            "",
            "Set to False if you want terminal background",
            "transparency.",
        ],
    },
    OptDef {
        key: bk::TRUECOLOR,
        desc: &[
            "Sets if 24-bit truecolor should be used.",
            "",
            "Will convert 24-bit colors to 256 color",
            "(6x6x6 color cube) if False.",
        ],
    },
    OptDef {
        key: bk::LOWCOLOR,
        desc: &["Use 256-color mode instead of truecolor."],
    },
    OptDef {
        key: bk::FORCE_TTY,
        desc: &[
            "TTY mode.",
            "",
            "Set to true to force tty mode regardless",
            "if a real tty has been detected or not.",
        ],
    },
    OptDef {
        key: bk::VIM_KEYS,
        desc: &[
            "Enable vim keys.",
            "Set to True to enable \"h,j,k,l\" keys for",
            "directional control in lists.",
        ],
    },
    OptDef {
        key: bk::DISABLE_MOUSE,
        desc: &["Disable all mouse events."],
    },
    OptDef {
        key: sk::PRESETS,
        desc: &[
            "Define presets for the layout of the boxes.",
            "",
            "Preset 0 is always all boxes shown with",
            "default settings. Max 9 presets.",
            "",
            "Format: \"box_name:P:G,box_name:P:G\"",
        ],
    },
    OptDef {
        key: sk::SHOWN_BOXES,
        desc: &[
            "Manually set which boxes to show.",
            "",
            "Available values are \"cpu mem net proc\".",
            "Separate values with whitespace.",
        ],
    },
    OptDef {
        key: ik::UPDATE_MS,
        desc: &[
            "Update time in milliseconds.",
            "",
            "Recommended 2000 ms or above for better",
            "sample times for graphs.",
            "",
            "Min value: 100 ms",
            "Max value: 86400000 ms = 24 hours.",
        ],
    },
    OptDef {
        key: bk::ROUNDED_CORNERS,
        desc: &["Rounded corners on boxes.", "", "True or False"],
    },
    OptDef {
        key: bk::TERMINAL_SYNC,
        desc: &[
            "Output synchronization.",
            "",
            "Use terminal synchronized output sequences",
            "to reduce flickering on supported terminals.",
        ],
    },
    OptDef {
        key: sk::GRAPH_SYMBOL,
        desc: &[
            "Default symbols to use for graph creation.",
            "",
            "\"braille\", \"block\" or \"tty\".",
        ],
    },
    OptDef {
        key: sk::CLOCK_FORMAT,
        desc: &[
            "Draw a clock at top of screen.",
            "(Only visible if cpu box is enabled!)",
            "",
            "Formatting according to strftime, empty",
            "string to disable.",
        ],
    },
    OptDef {
        key: bk::BASE_10_SIZES,
        desc: &[
            "Use base 10 for bits and bytes sizes.",
            "",
            "Uses KB = 1000 instead of KiB = 1024.",
        ],
    },
    OptDef {
        key: bk::BACKGROUND_UPDATE,
        desc: &[
            "Update main ui when menus are showing.",
            "",
            "True or False.",
        ],
    },
    OptDef {
        key: bk::SHOW_BATTERY,
        desc: &[
            "Show battery stats.",
            "(Only visible if cpu box is enabled!)",
        ],
    },
    OptDef {
        key: sk::SELECTED_BATTERY,
        desc: &[
            "Select battery.",
            "",
            "Which battery to use if multiple are present.",
            "\"Auto\" for auto detection.",
        ],
    },
    OptDef {
        key: bk::SHOW_BATTERY_WATTS,
        desc: &["Show battery power.", "", "Show discharge/charging power."],
    },
    OptDef {
        key: sk::LOG_LEVEL,
        desc: &[
            "Set loglevel for error.log",
            "",
            "\"ERROR\", \"WARNING\", \"INFO\" and \"DEBUG\".",
        ],
    },
    OptDef {
        key: bk::SAVE_CONFIG_ON_EXIT,
        desc: &[
            "Save config on exit.",
            "",
            "Automatically save current settings to",
            "config file on exit.",
        ],
    },
];

/// Options in the "cpu" category.
pub const CPU: &[OptDef] = &[
    OptDef {
        key: bk::CPU_BOTTOM,
        desc: &[
            "Cpu box location.",
            "",
            "Show cpu box at bottom of screen instead",
            "of top.",
        ],
    },
    OptDef {
        key: sk::GRAPH_SYMBOL_CPU,
        desc: &[
            "Graph symbol to use for graphs in cpu box.",
            "",
            "\"default\", \"braille\", \"block\" or \"tty\".",
        ],
    },
    OptDef {
        key: sk::CPU_GRAPH_UPPER,
        desc: &[
            "Cpu upper graph.",
            "",
            "Sets the CPU stat shown in upper half of",
            "the CPU graph.",
        ],
    },
    OptDef {
        key: sk::CPU_GRAPH_LOWER,
        desc: &[
            "Cpu lower graph.",
            "",
            "Sets the CPU stat shown in lower half of",
            "the CPU graph.",
        ],
    },
    OptDef {
        key: bk::CPU_INVERT_LOWER,
        desc: &[
            "Toggles orientation of the lower CPU graph.",
            "",
            "True or False.",
        ],
    },
    OptDef {
        key: bk::CPU_SINGLE_GRAPH,
        desc: &[
            "Completely disable the lower CPU graph.",
            "",
            "Shows only upper CPU graph and resizes it",
            "to fit to box height.",
        ],
    },
    OptDef {
        key: bk::CHECK_TEMP,
        desc: &["Enable cpu temperature reporting.", "", "True or False."],
    },
    OptDef {
        key: sk::CPU_SENSOR,
        desc: &[
            "Cpu temperature sensor.",
            "",
            "Select the sensor that corresponds to",
            "your cpu temperature.",
            "",
            "Set to \"Auto\" for auto detection.",
        ],
    },
    OptDef {
        key: bk::SHOW_CORETEMP,
        desc: &[
            "Show temperatures for cpu cores.",
            "",
            "Only works if check_temp is True and",
            "the system is reporting core temps.",
        ],
    },
    OptDef {
        key: sk::CPU_CORE_MAP,
        desc: &[
            "Custom mapping between core and coretemp.",
            "",
            "Format: \"X:Y\"",
            "X=core with wrong temp.",
            "Y=core with correct temp.",
        ],
    },
    OptDef {
        key: sk::TEMP_SCALE,
        desc: &[
            "Which temperature scale to use.",
            "",
            "Celsius, Fahrenheit, Kelvin or Rankine.",
        ],
    },
    OptDef {
        key: bk::SHOW_CPU_FREQ,
        desc: &[
            "Show CPU frequency.",
            "",
            "Can cause slowdowns on systems with many",
            "cores and certain kernel versions.",
        ],
    },
    OptDef {
        key: sk::CUSTOM_CPU_NAME,
        desc: &[
            "Custom cpu model name in cpu percentage box.",
            "",
            "Empty string to disable.",
        ],
    },
    OptDef {
        key: bk::SHOW_UPTIME,
        desc: &[
            "Shows the system uptime in the CPU box.",
            "",
            "True or False.",
        ],
    },
    OptDef {
        key: bk::SHOW_CPU_WATTS,
        desc: &[
            "Shows the CPU power consumption in watts.",
            "",
            "True or False.",
        ],
    },
];

/// Options in the "mem" category.
pub const MEM: &[OptDef] = &[
    OptDef {
        key: bk::MEM_BELOW_NET,
        desc: &[
            "Mem box location.",
            "",
            "Show mem box below net box instead of above.",
        ],
    },
    OptDef {
        key: sk::GRAPH_SYMBOL_MEM,
        desc: &[
            "Graph symbol to use for graphs in mem box.",
            "",
            "\"default\", \"braille\", \"block\" or \"tty\".",
        ],
    },
    OptDef {
        key: bk::MEM_GRAPHS,
        desc: &["Show graphs for memory values.", "", "True or False."],
    },
    OptDef {
        key: bk::SHOW_DISKS,
        desc: &["Split memory box to also show disks.", "", "True or False."],
    },
    OptDef {
        key: bk::SHOW_IO_STAT,
        desc: &[
            "Toggle IO activity graphs.",
            "",
            "Show small IO graphs for disk activity",
            "when not in IO mode.",
        ],
    },
    OptDef {
        key: bk::IO_MODE,
        desc: &[
            "Toggles io mode for disks.",
            "",
            "Shows big graphs for disk read/write speeds",
            "instead of used/free percentage meters.",
        ],
    },
    OptDef {
        key: bk::IO_GRAPH_COMBINED,
        desc: &[
            "Toggle combined read and write graphs.",
            "",
            "Only has effect if \"io mode\" is True.",
        ],
    },
    OptDef {
        key: sk::IO_GRAPH_SPEEDS,
        desc: &[
            "Set top speeds for the io graphs.",
            "",
            "Manually set which speed in MiB/s that",
            "equals 100 percent in the io graphs.",
            "(100 MiB/s by default).",
        ],
    },
    OptDef {
        key: bk::SHOW_SWAP,
        desc: &[
            "If swap memory should be shown in memory box.",
            "",
            "True or False.",
        ],
    },
    OptDef {
        key: bk::SWAP_DISK,
        desc: &[
            "Show swap as a disk.",
            "",
            "Ignores show_swap value above.",
            "Inserts itself after first disk.",
        ],
    },
    OptDef {
        key: bk::ONLY_PHYSICAL,
        desc: &[
            "Filter out non physical disks.",
            "",
            "Set this to False to include network disks,",
            "RAM disks and similar.",
        ],
    },
    OptDef {
        key: bk::DISK_FREE_PRIV,
        desc: &[
            "Type of available disk space.",
            "",
            "Set to true to show how much disk space is",
            "available for privileged users.",
        ],
    },
    OptDef {
        key: sk::DISKS_FILTER,
        desc: &[
            "Optional filter for shown disks.",
            "",
            "Should be full path of a mountpoint.",
            "Separate multiple values with whitespace.",
        ],
    },
];

/// Options in the "net" category.
pub const NET: &[OptDef] = &[
    OptDef {
        key: sk::GRAPH_SYMBOL_NET,
        desc: &[
            "Graph symbol to use for graphs in net box.",
            "",
            "\"default\", \"braille\", \"block\" or \"tty\".",
        ],
    },
    OptDef {
        key: bk::SWAP_UPLOAD_DOWNLOAD,
        desc: &["Swap the positions of the upload and download", "graphs."],
    },
    OptDef {
        key: ik::NET_DOWNLOAD,
        desc: &[
            "Fixed network graph download value.",
            "",
            "Value in Mebibits, default \"100\".",
            "",
            "Can be toggled with auto button.",
        ],
    },
    OptDef {
        key: ik::NET_UPLOAD,
        desc: &[
            "Fixed network graph upload value.",
            "",
            "Value in Mebibits, default \"100\".",
            "",
            "Can be toggled with auto button.",
        ],
    },
    OptDef {
        key: bk::NET_AUTO,
        desc: &[
            "Start in network graphs auto rescaling mode.",
            "",
            "Ignores any values set above at start and",
            "rescales down to 10Kibibytes at the lowest.",
        ],
    },
    OptDef {
        key: bk::NET_SYNC,
        desc: &[
            "Network scale sync.",
            "",
            "Syncs the scaling for download and upload to",
            "whichever currently has the highest scale.",
        ],
    },
    OptDef {
        key: sk::NET_IFACE,
        desc: &[
            "Network Interface.",
            "",
            "Manually set the starting Network Interface.",
            "",
            "Will otherwise automatically choose the NIC",
            "with the highest total download since boot.",
        ],
    },
];

/// Options in the "proc" category.
pub const PROC: &[OptDef] = &[
    OptDef {
        key: bk::PROC_LEFT,
        desc: &[
            "Proc box location.",
            "",
            "Show proc box on left side of screen",
            "instead of right.",
        ],
    },
    OptDef {
        key: sk::GRAPH_SYMBOL_PROC,
        desc: &[
            "Graph symbol to use for graphs in proc box.",
            "",
            "\"default\", \"braille\", \"block\" or \"tty\".",
        ],
    },
    OptDef {
        key: sk::PROC_SORTING,
        desc: &[
            "Processes sorting option.",
            "",
            "Possible values:",
            "\"pid\", \"name\", \"command\", \"threads\",",
            "\"user\", \"memory\", \"cpu lazy\", \"cpu direct\".",
        ],
    },
    OptDef {
        key: bk::PROC_REVERSED,
        desc: &["Reverse processes sorting order.", "", "True or False."],
    },
    OptDef {
        key: bk::PROC_TREE,
        desc: &[
            "Processes tree view.",
            "",
            "Set true to show processes grouped by",
            "parents with lines drawn between parent",
            "and child process.",
        ],
    },
    OptDef {
        key: bk::PROC_AGGREGATE,
        desc: &[
            "Aggregate child's resources in parent.",
            "",
            "In tree-view, include all child resources",
            "with the parent even while expanded.",
        ],
    },
    OptDef {
        key: bk::PROC_COLORS,
        desc: &["Enable colors in process view.", "", "True or False."],
    },
    OptDef {
        key: bk::PROC_GRADIENT,
        desc: &[
            "Enable process view gradient fade.",
            "",
            "Fades from top or current selection.",
        ],
    },
    OptDef {
        key: bk::PROC_PER_CORE,
        desc: &[
            "Process usage per core.",
            "",
            "If process cpu usage should be of the core",
            "it's running on or usage of the total",
            "available cpu power.",
        ],
    },
    OptDef {
        key: bk::PROC_MEM_BYTES,
        desc: &[
            "Show memory as bytes in process list.",
            "",
            "Will show percentage of total memory",
            "if False.",
        ],
    },
    OptDef {
        key: bk::KEEP_DEAD_PROC_USAGE,
        desc: &[
            "Cpu and Mem usage for dead processes",
            "",
            "Set true if process should preserve the cpu",
            "and memory usage of when it died while paused.",
        ],
    },
    OptDef {
        key: bk::PROC_CPU_GRAPHS,
        desc: &["Show cpu graph for each process.", "", "True or False"],
    },
    OptDef {
        key: bk::PROC_FILTER_KERNEL,
        desc: &[
            "Filter kernel processes from output.",
            "",
            "Set to True to filter out internal",
            "processes started by the kernel.",
        ],
    },
    OptDef {
        key: bk::PROC_FOLLOW_DETAILED,
        desc: &[
            "Follow selected process with detailed view",
            "",
            "If True, when opening the detailed view",
            "the process will be followed in the list.",
        ],
    },
    OptDef {
        key: sk::PROC_FILTER,
        desc: &["Filter processes by name.", "", "Prefix with ! for regex."],
    },
];

/// Options in the "gpu" category.
pub const GPU: &[OptDef] = &[
    OptDef {
        key: bk::GPU_MIRROR_GRAPH,
        desc: &["Mirror GPU graph.", "", "True or False."],
    },
    OptDef {
        key: sk::GRAPH_SYMBOL_GPU,
        desc: &[
            "Graph symbol to use for graphs in gpu box.",
            "",
            "\"default\", \"braille\", \"block\" or \"tty\".",
        ],
    },
    OptDef {
        key: sk::CUSTOM_GPU_NAME0,
        desc: &["Custom GPU name for GPU 0.", "", "Empty string to disable."],
    },
    OptDef {
        key: sk::CUSTOM_GPU_NAME1,
        desc: &["Custom GPU name for GPU 1.", "", "Empty string to disable."],
    },
    OptDef {
        key: sk::CUSTOM_GPU_NAME2,
        desc: &["Custom GPU name for GPU 2.", "", "Empty string to disable."],
    },
    OptDef {
        key: sk::CUSTOM_GPU_NAME3,
        desc: &["Custom GPU name for GPU 3.", "", "Empty string to disable."],
    },
    OptDef {
        key: sk::CUSTOM_GPU_NAME4,
        desc: &["Custom GPU name for GPU 4.", "", "Empty string to disable."],
    },
    OptDef {
        key: sk::CUSTOM_GPU_NAME5,
        desc: &["Custom GPU name for GPU 5.", "", "Empty string to disable."],
    },
];

/// Options in the "disk" category.
pub const DISK: &[OptDef] = &[
    OptDef {
        key: sk::DISKS_FILTER,
        desc: &[
            "Optional filter for shown disks.",
            "",
            "Should be full path of a mountpoint.",
            "Separate multiple values with whitespace.",
        ],
    },
    OptDef {
        key: bk::ONLY_PHYSICAL,
        desc: &[
            "Filter out non physical disks.",
            "",
            "Set this to False to include network disks,",
            "RAM disks and similar.",
        ],
    },
    OptDef {
        key: bk::DISK_IO_MODE,
        desc: &[
            "Show IO activity.",
            "",
            "Shows disk IO activity instead of",
            "usage percentage.",
        ],
    },
];

/// All categories in order.
pub fn categories() -> &'static [&'static [OptDef]] {
    &[GENERAL, CPU, MEM, NET, PROC, GPU, DISK]
}

// ---------------------------------------------------------------------------
// Value helpers
// ---------------------------------------------------------------------------

/// Get the display value for an option.
pub fn get_value(key: &str, config: &Config) -> String {
    if config.bools.contains_key(key) {
        if config.get_bool(key) {
            "True".to_string()
        } else {
            "False".to_string()
        }
    } else if config.ints.contains_key(key) {
        config.get_int(key).to_string()
    } else {
        config.get_string(key).to_string()
    }
}

/// Cycle a browsable option left or right. Returns true if changed.
pub fn cycle_browsable(key: &str, config: &mut Config, direction: i32) -> bool {
    let vals = browsable_values(key);
    if vals.is_empty() {
        return false;
    }
    let current = config.get_string(key).to_string();
    let idx = vals.iter().position(|&v| v == current).unwrap_or(0);
    let new_idx = if direction > 0 {
        (idx + 1) % vals.len()
    } else {
        if idx == 0 { vals.len() - 1 } else { idx - 1 }
    };
    config.set_string(key, vals[new_idx]);
    true
}

/// Step an int option by `delta`.
pub fn step_int(key: &str, config: &mut Config, delta: i64) {
    let step = if key == ik::UPDATE_MS { 100 } else { 1 };
    let value = config.get_int(key) + delta * step;
    config.set_int(key, value);
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Center-justify a string in `width` columns, padding with spaces.
fn cjust(s: &str, width: usize) -> String {
    let slen = s.chars().count();
    if slen >= width {
        return s.chars().take(width).collect();
    }
    let pad_left = (width - slen) / 2;
    let pad_right = width - slen - pad_left;
    format!("{}{}{}", " ".repeat(pad_left), s, " ".repeat(pad_right))
}

/// Capitalize first letter of each word, replace underscores with spaces.
fn capitalize_option(key: &str) -> String {
    key.replace('_', " ")
        .split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => {
                    let upper: String = f.to_uppercase().collect();
                    format!("{}{}", upper, c.as_str())
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Draw the btop-style options menu.
///
/// The box is 78 chars wide, centered on screen.
/// Left panel: 30 chars (option name + value rows).
/// Right panel: description of selected option.
/// Vertical divider at column x+30.
pub fn draw(
    term_width: usize,
    term_height: usize,
    cat: usize,
    selected: usize,
    page: usize,
    config: &Config,
    theme: &Theme,
) -> String {
    let cats = categories();
    let cat = cat.min(cats.len() - 1);
    let options = cats[cat];

    let box_w: usize = 78;
    let x = term_width.saturating_sub(box_w) / 2;

    // Compute available height for options (each takes 2 rows)
    let max_items = cats.iter().map(|c| c.len()).max().unwrap_or(0);
    let desired_h = max_items * 2 + 4; // 4 = tab row + divider + top/bottom borders
    let height = desired_h.min(term_height.saturating_sub(8));
    let height = if height % 2 != 0 { height - 1 } else { height };
    let y = term_height.saturating_sub(height + 6) / 2;

    let current_items = options.len();
    let item_height = ((height - 4) / 2).min(max_items);
    let pages = if current_items == 0 {
        1
    } else {
        current_items.div_ceil(item_height)
    };
    let page = page.min(pages - 1);
    let select_max = item_height
        .min(current_items.saturating_sub(item_height * page))
        .saturating_sub(1);
    let selected = selected.min(select_max);

    let hi = theme.c(tc::HI_FG);
    let title_c = theme.c(tc::TITLE);
    let fg = theme.c(tc::MAIN_FG);
    let sel_bg = theme.c(tc::SELECTED_BG);
    let sel_fg = theme.c(tc::SELECTED_FG);
    let opts_c = theme.c(tc::OPTIONS_BOX);
    let reset = "\x1b[0m";

    let mut out = String::with_capacity(4096);

    // Main box: create at (x, y+6) with height
    let tab_title = format!("{}tab{}{}", hi, fg, symbols::RIGHT_ARROW);
    out.push_str(&box_drawing::create_box(&box_drawing::BoxConfig {
        x,
        y: y + 6,
        width: box_w,
        height,
        line_color: opts_c,
        fill: true,
        title: &tab_title,
        title2: "",
        num: 0,
        rounded: config.get_bool(bk::ROUNDED_CORNERS),
        hi_color: "",
        title_color: "",
    }));

    // Horizontal divider at row y+8 with T-junctions
    let h_left = symbols::H_LINE.repeat(29);
    let h_right = symbols::H_LINE.repeat(box_w - 32);
    let divider_row = y + 8 + 1;
    out.push_str(&term::mv(x + 1, divider_row));
    out.push_str(opts_c);
    out.push_str(symbols::DIV_LEFT);
    out.push_str(opts_c);
    out.push_str(&h_left);
    out.push_str(symbols::DIV_UP);
    out.push_str(&h_right);
    out.push_str(opts_c);
    out.push_str(symbols::DIV_RIGHT);
    // Bottom T-junction on vertical divider
    out.push_str(&term::mv(x + 31, y + 6 + height));
    out.push_str(opts_c);
    out.push_str(symbols::DIV_DOWN);

    // Vertical divider line at x+30 for each content row
    for i in 0..(height - 4) {
        out.push_str(&format!(
            "{}{}{}",
            term::mv(x + 31, y + 9 + 1 + i),
            opts_c,
            symbols::V_LINE,
        ));
    }

    // Category tab bar at row y+7
    out.push_str(&term::mv(x + 4, y + 7 + 1));
    for (i, &name) in CAT_NAMES.iter().enumerate() {
        if i == cat {
            out.push_str(&format!(
                "\x1b[1m{}[{}{}{}]{}",
                hi, title_c, name, hi, reset
            ));
        } else {
            out.push_str(&format!("\x1b[1m{}{}{}{}{}", hi, i, title_c, name, reset));
        }
        let spacing = 8_usize.saturating_sub(name.len() + 1);
        out.push_str(&format!("\x1b[{}C", spacing));
    }

    // Page indicator
    if pages > 1 {
        out.push_str(&format!(
            "{}{}{} {} page {}/{} {} {}",
            term::mv(x + 2, y + 6 + height),
            hi,
            symbols::UP_ARROW,
            title_c,
            page + 1,
            pages,
            hi,
            symbols::DOWN_ARROW,
        ));
    }

    // Option rows
    let cy_start = y + 9 + 1; // first content row (1-based terminal row)
    for c in 0..item_height {
        let i = item_height * page + c;
        if i >= options.len() {
            break;
        }
        let opt = &options[i];
        let kind = classify(opt.key, config);
        let value = get_value(opt.key, config);
        let is_selected = c == selected;

        let name_display = capitalize_option(opt.key);

        // Browsable index suffix
        let mut name_suffix = String::new();
        if is_selected && kind == OptKind::Browsable {
            let vals = browsable_values(opt.key);
            let idx = vals.iter().position(|&v| v == value).unwrap_or(0);
            name_suffix = format!(" {}/{}", idx + 1, vals.len());
        }

        // Row 1: option name (29 chars in left panel)
        let full_name = format!("{}{}", name_display, name_suffix);
        let name_str = cjust(&full_name, 29);
        out.push_str(&format!(
            "{}{}{}{}",
            term::mv(x + 2, cy_start + c * 2),
            if is_selected {
                format!("{}{}\x1b[1m", sel_bg, sel_fg)
            } else {
                format!("{}\x1b[1m", title_c)
            },
            name_str,
            reset,
        ));

        // Row 2: value (centered in 25 chars within left panel, with arrow indicators)
        let value_display = cjust(&value, 25);
        out.push_str(&format!(
            "{}{}  {}  {}",
            term::mv(x + 2, cy_start + c * 2 + 1),
            if is_selected { &sel_fg } else { &fg },
            value_display,
            reset,
        ));

        // Draw arrows and enter symbol for selected item
        if is_selected {
            let val_row = cy_start + c * 2 + 1;
            match kind {
                OptKind::Bool | OptKind::Browsable | OptKind::Int => {
                    out.push_str(&format!(
                        "\x1b[1m{}{}{}{}{}{}{}",
                        term::mv(x + 2, val_row),
                        hi,
                        symbols::LEFT_ARROW,
                        term::mv(x + 29, val_row),
                        hi,
                        symbols::RIGHT_ARROW,
                        reset,
                    ));
                }
                OptKind::StringVal => {
                    out.push_str(&format!(
                        "\x1b[1m{}{}{}{}",
                        term::mv(x + 29, val_row),
                        hi,
                        symbols::ENTER,
                        reset,
                    ));
                }
            }

            // Description in right panel
            out.push_str(&format!("{}{}\x1b[1m", reset, title_c));
            for (di, desc_line) in opt.desc.iter().enumerate() {
                let desc_row = y + 8 + 1 + di; // start at the row after the divider
                if desc_row >= y + 6 + height {
                    break;
                }
                // First description line is title-colored, rest are main_fg
                if di == 1 {
                    out.push_str(&format!("{}\x1b[22m", fg));
                }
                out.push_str(&format!("{}{}", term::mv(x + 33, desc_row + 1), desc_line,));
            }
            out.push_str(reset);
        }
    }

    out.push_str(reset);
    out
}

/// Return the option key at `(cat, index)`.
pub fn opt_key(
    cat: usize,
    page: usize,
    selected: usize,
    term_height: usize,
) -> Option<&'static str> {
    let cats = categories();
    if cat >= cats.len() {
        return None;
    }
    let options = cats[cat];
    let global_max = cats.iter().map(|c| c.len()).max().unwrap_or(0);
    let height = (global_max * 2 + 4).min(term_height.saturating_sub(8));
    let height = if height % 2 != 0 { height - 1 } else { height };
    let item_height = ((height - 4) / 2).min(global_max);
    let idx = item_height * page + selected;
    options.get(idx).map(|o| o.key)
}

/// Get item_height (visible items per page) for a category.
pub fn items_per_page(cat: usize, term_height: usize) -> usize {
    let cats = categories();
    if cat >= cats.len() {
        return 1;
    }
    let global_max = cats.iter().map(|c| c.len()).max().unwrap_or(0);
    let height = (global_max * 2 + 4).min(term_height.saturating_sub(8));
    let height = if height % 2 != 0 { height - 1 } else { height };
    ((height - 4) / 2).min(global_max).max(1)
}

/// Number of pages for a category.
pub fn page_count(cat: usize, term_height: usize) -> usize {
    let cats = categories();
    if cat >= cats.len() {
        return 1;
    }
    let max_items = cats[cat].len();
    let ipp = items_per_page(cat, term_height);
    max_items.div_ceil(ipp)
}

/// Max selectable index on a given page.
pub fn select_max(cat: usize, page: usize, term_height: usize) -> usize {
    let cats = categories();
    if cat >= cats.len() {
        return 0;
    }
    let max_items = cats[cat].len();
    let ipp = items_per_page(cat, term_height);
    let remaining = max_items.saturating_sub(ipp * page);
    ipp.min(remaining).saturating_sub(1)
}
