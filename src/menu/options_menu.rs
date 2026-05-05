use crate::config::{Config, ConfigKey, KeyKind};
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
    pub key: ConfigKey,
    pub desc: &'static [&'static str],
}

// ---------------------------------------------------------------------------
// Browsable option value lists
// ---------------------------------------------------------------------------

/// Return the list of valid values for a browsable option key.
pub fn browsable_values(key: ConfigKey) -> &'static [&'static str] {
    key.browsable_values()
}

fn classify(key: ConfigKey, _config: &Config) -> OptKind {
    match key.kind() {
        KeyKind::Bool => OptKind::Bool,
        KeyKind::Int => OptKind::Int,
        KeyKind::String if !browsable_values(key).is_empty() => OptKind::Browsable,
        KeyKind::String => OptKind::StringVal,
        KeyKind::Enum => OptKind::Browsable,
    }
}

/// Classify how an option key can be edited.
pub fn opt_kind(key: ConfigKey, config: &Config) -> OptKind {
    classify(key, config)
}

// ---------------------------------------------------------------------------
// Category definitions  (mirroring btop, minus Linux-only options)
// ---------------------------------------------------------------------------

/// Category tab names for the options menu.
pub const CAT_NAMES: &[&str] = &["general", "cpu", "mem", "net", "proc", "gpu", "disk"];

/// Options in the "general" category.
pub const GENERAL: &[OptDef] = &[
    OptDef {
        key: ConfigKey::ColorTheme,
        desc: &[
            "Color theme.",
            "",
            "Choose from all bundled themes.",
            "",
            "\"default\" for the built-in theme.",
        ],
    },
    OptDef {
        key: ConfigKey::ThemeBackground,
        desc: &[
            "Theme background color.",
            "",
            "Set to False for terminal background",
            "transparency.",
        ],
    },
    OptDef {
        key: ConfigKey::VimKeys,
        desc: &[
            "Vim key bindings.",
            "",
            "h/j/k/l for directional control,",
            "g/G for top/bottom of list,",
            "Ctrl+F/B/D/U for page scrolling.",
        ],
    },
    OptDef {
        key: ConfigKey::Presets,
        desc: &[
            "Layout presets.",
            "",
            "Preset 0 is all boxes with default",
            "settings. Max 9 presets.",
            "",
            "Format: \"box_name:P:G,box_name:P:G\"",
        ],
    },
    OptDef {
        key: ConfigKey::ShownBoxes,
        desc: &[
            "Visible boxes.",
            "",
            "Available: \"cpu mem net proc disk\".",
            "Separate values with whitespace.",
        ],
    },
    OptDef {
        key: ConfigKey::UpdateMs,
        desc: &[
            "Update interval in milliseconds.",
            "",
            "Recommended 2000 ms or above for",
            "better graph sample times.",
            "",
            "Range: 100 ms to 86400000 ms.",
        ],
    },
    OptDef {
        key: ConfigKey::RoundedCorners,
        desc: &["Rounded corners on boxes.", "", "True or False."],
    },
    OptDef {
        key: ConfigKey::TerminalSync,
        desc: &[
            "Terminal output synchronization.",
            "",
            "Reduces flickering on supported",
            "terminals.",
        ],
    },
    OptDef {
        key: ConfigKey::GraphSymbol,
        desc: &[
            "Default graph symbol.",
            "",
            "\"braille\" or \"block\".",
            "Per-widget overrides use \"default\"",
            "to inherit this setting.",
        ],
    },
    OptDef {
        key: ConfigKey::ClockFormat,
        desc: &[
            "Clock display format.",
            "",
            "Shown in the CPU box. Uses format",
            "specifiers: %H, %M, %S, %X.",
            "",
            "Empty string to disable.",
        ],
    },
    OptDef {
        key: ConfigKey::Base10Sizes,
        desc: &[
            "Base 10 size units.",
            "",
            "Uses KB = 1000 instead of",
            "KiB = 1024.",
        ],
    },
    OptDef {
        key: ConfigKey::BackgroundUpdate,
        desc: &[
            "Update while menus are open.",
            "",
            "Continue refreshing data when the",
            "options or help menu is visible.",
        ],
    },
    OptDef {
        key: ConfigKey::LogLevel,
        desc: &[
            "Logging level.",
            "",
            "Sets verbosity for rtop.log.",
            "",
            "\"off\", \"error\", \"warn\",",
            "\"info\", \"debug\", or \"trace\".",
            "",
            "\"off\" disables file logging.",
            "Changes apply immediately.",
        ],
    },
    OptDef {
        key: ConfigKey::SaveConfigOnExit,
        desc: &[
            "Save settings on exit.",
            "",
            "Automatically write current settings",
            "to the config file on exit.",
        ],
    },
];

/// Options in the "cpu" category.
pub const CPU: &[OptDef] = &[
    OptDef {
        key: ConfigKey::CpuBottom,
        desc: &[
            "CPU box at bottom.",
            "",
            "Show the CPU box at the bottom of",
            "the screen instead of the top.",
        ],
    },
    OptDef {
        key: ConfigKey::GraphSymbolCpu,
        desc: &[
            "CPU graph symbol.",
            "",
            "\"default\", \"braille\", or \"block\".",
        ],
    },
    OptDef {
        key: ConfigKey::CpuGraphUpper,
        desc: &[
            "Upper CPU graph source.",
            "",
            "CPU stat shown in the upper half",
            "of the CPU graph.",
        ],
    },
    OptDef {
        key: ConfigKey::CpuGraphLower,
        desc: &[
            "Lower CPU graph source.",
            "",
            "CPU stat shown in the lower half",
            "of the CPU graph.",
        ],
    },
    OptDef {
        key: ConfigKey::CpuInvertLower,
        desc: &[
            "Invert lower CPU graph.",
            "",
            "Flips the orientation of the lower",
            "CPU graph so it grows downward.",
        ],
    },
    OptDef {
        key: ConfigKey::CpuSingleGraph,
        desc: &[
            "Single CPU graph.",
            "",
            "Disable the lower CPU graph and",
            "expand the upper graph to full",
            "box height.",
        ],
    },
    OptDef {
        key: ConfigKey::CheckTemp,
        desc: &[
            "CPU temperature monitoring.",
            "",
            "Enable temperature reporting in",
            "the CPU box.",
        ],
    },
    OptDef {
        key: ConfigKey::ShowCoretemp,
        desc: &[
            "Per-core temperatures.",
            "",
            "Show individual core temperatures.",
            "Requires temperature monitoring",
            "to be enabled.",
        ],
    },
    OptDef {
        key: ConfigKey::TempScale,
        desc: &[
            "Temperature scale.",
            "",
            "Celsius, Fahrenheit, Kelvin,",
            "or Rankine.",
        ],
    },
    OptDef {
        key: ConfigKey::ShowCpuFreq,
        desc: &[
            "CPU frequency display.",
            "",
            "Show the current CPU clock speed",
            "in the core panel.",
        ],
    },
    OptDef {
        key: ConfigKey::CustomCpuName,
        desc: &[
            "Custom CPU name.",
            "",
            "Override the detected CPU model",
            "name. Empty string to disable.",
        ],
    },
    OptDef {
        key: ConfigKey::ShowUptime,
        desc: &[
            "System uptime display.",
            "",
            "Show system uptime in the CPU box.",
        ],
    },
    OptDef {
        key: ConfigKey::ShowCpuWatts,
        desc: &[
            "CPU power consumption.",
            "",
            "Show wattage in the CPU box.",
            "Requires LibreHardwareMonitor.",
        ],
    },
    OptDef {
        key: ConfigKey::CpuUpdateMs,
        desc: &[
            "CPU update interval (ms).",
            "",
            "0 = use global update_ms.",
            "Range: 100 to 86400000.",
        ],
    },
];

/// Options in the "mem" category.
pub const MEM: &[OptDef] = &[
    OptDef {
        key: ConfigKey::MemBelowNet,
        desc: &[
            "Memory box below network.",
            "",
            "Position the memory box below the",
            "network box instead of above.",
        ],
    },
    OptDef {
        key: ConfigKey::ShowSwap,
        desc: &[
            "Swap memory display.",
            "",
            "Show swap usage in the memory box.",
        ],
    },
    OptDef {
        key: ConfigKey::MemUpdateMs,
        desc: &[
            "Memory update interval (ms).",
            "",
            "0 = use global update_ms.",
            "Range: 100 to 86400000.",
        ],
    },
];

/// Options in the "net" category.
pub const NET: &[OptDef] = &[
    OptDef {
        key: ConfigKey::GraphSymbolNet,
        desc: &[
            "Network graph symbol.",
            "",
            "\"default\", \"braille\", or \"block\".",
        ],
    },
    OptDef {
        key: ConfigKey::SwapUploadDownload,
        desc: &["Swap upload and download positions."],
    },
    OptDef {
        key: ConfigKey::NetDownload,
        desc: &[
            "Fixed download graph scale.",
            "",
            "Value in Mebibits. Default: 100.",
            "Overridden when auto scaling is on.",
        ],
    },
    OptDef {
        key: ConfigKey::NetUpload,
        desc: &[
            "Fixed upload graph scale.",
            "",
            "Value in Mebibits. Default: 100.",
            "Overridden when auto scaling is on.",
        ],
    },
    OptDef {
        key: ConfigKey::NetAuto,
        desc: &[
            "Auto scale network graphs.",
            "",
            "Automatically adjust graph scale",
            "based on current traffic.",
        ],
    },
    OptDef {
        key: ConfigKey::NetSync,
        desc: &[
            "Sync network graph scales.",
            "",
            "Use the same scale for both upload",
            "and download graphs.",
        ],
    },
    OptDef {
        key: ConfigKey::NetUpdateMs,
        desc: &[
            "Network update interval (ms).",
            "",
            "0 = use global update_ms.",
            "Range: 100 to 86400000.",
        ],
    },
];

/// Options in the "proc" category.
pub const PROC: &[OptDef] = &[
    OptDef {
        key: ConfigKey::ProcLeft,
        desc: &[
            "Process box on left.",
            "",
            "Show the process box on the left",
            "side of the screen.",
        ],
    },
    OptDef {
        key: ConfigKey::ProcSorting,
        desc: &[
            "Process sort column.",
            "",
            "\"pid\", \"name\", \"cpu lazy\",",
            "\"cpu responsive\", \"mem\",",
            "or \"threads\".",
        ],
    },
    OptDef {
        key: ConfigKey::ProcReversed,
        desc: &["Reverse sort order.", "", "True or False."],
    },
    OptDef {
        key: ConfigKey::ProcTree,
        desc: &[
            "Tree view.",
            "",
            "Group processes by parent with",
            "lines drawn between parent and",
            "child processes.",
        ],
    },
    OptDef {
        key: ConfigKey::ProcAggregate,
        desc: &[
            "Aggregate child resources.",
            "",
            "In tree view, include child CPU",
            "and memory usage in the parent",
            "process totals.",
        ],
    },
    OptDef {
        key: ConfigKey::ProcColors,
        desc: &[
            "Process row colors.",
            "",
            "Color process rows based on",
            "CPU usage.",
        ],
    },
    OptDef {
        key: ConfigKey::ProcGradient,
        desc: &[
            "Process color gradient.",
            "",
            "Fade row colors based on distance",
            "from the selected process.",
        ],
    },
    OptDef {
        key: ConfigKey::ProcPerCore,
        desc: &[
            "Per-core CPU usage.",
            "",
            "Show CPU usage relative to one",
            "core instead of total CPU power.",
            "Values can exceed 100%.",
        ],
    },
    OptDef {
        key: ConfigKey::ProcMemBytes,
        desc: &[
            "Memory as bytes.",
            "",
            "Show memory in bytes instead of",
            "percentage of total memory.",
        ],
    },
    OptDef {
        key: ConfigKey::KeepDeadProcUsage,
        desc: &[
            "Preserve dead process usage.",
            "",
            "Keep CPU and memory values for",
            "processes that have exited.",
        ],
    },
    OptDef {
        key: ConfigKey::ProcFilter,
        desc: &[
            "Process filter.",
            "",
            "Filter by name. Prefix with !",
            "for inverse match.",
        ],
    },
    OptDef {
        key: ConfigKey::ProcUpdateMs,
        desc: &[
            "Process update interval (ms).",
            "",
            "0 = use global update_ms.",
            "Range: 100 to 86400000.",
        ],
    },
];

/// Options in the "gpu" category.
pub const GPU: &[OptDef] = &[
    OptDef {
        key: ConfigKey::CustomGpuName0,
        desc: &[
            "Custom GPU name for GPU 0.",
            "",
            "Override the detected GPU name.",
            "Empty string to disable.",
        ],
    },
    OptDef {
        key: ConfigKey::CustomGpuName1,
        desc: &[
            "Custom GPU name for GPU 1.",
            "",
            "Override the detected GPU name.",
            "Empty string to disable.",
        ],
    },
    OptDef {
        key: ConfigKey::CustomGpuName2,
        desc: &[
            "Custom GPU name for GPU 2.",
            "",
            "Override the detected GPU name.",
            "Empty string to disable.",
        ],
    },
    OptDef {
        key: ConfigKey::CustomGpuName3,
        desc: &[
            "Custom GPU name for GPU 3.",
            "",
            "Override the detected GPU name.",
            "Empty string to disable.",
        ],
    },
    OptDef {
        key: ConfigKey::CustomGpuName4,
        desc: &[
            "Custom GPU name for GPU 4.",
            "",
            "Override the detected GPU name.",
            "Empty string to disable.",
        ],
    },
    OptDef {
        key: ConfigKey::CustomGpuName5,
        desc: &[
            "Custom GPU name for GPU 5.",
            "",
            "Override the detected GPU name.",
            "Empty string to disable.",
        ],
    },
    OptDef {
        key: ConfigKey::CustomGpuName6,
        desc: &[
            "Custom GPU name for GPU 6.",
            "",
            "Override the detected GPU name.",
            "Empty string to disable.",
        ],
    },
    OptDef {
        key: ConfigKey::CustomGpuName7,
        desc: &[
            "Custom GPU name for GPU 7.",
            "",
            "Override the detected GPU name.",
            "Empty string to disable.",
        ],
    },
    OptDef {
        key: ConfigKey::GpuUpdateMs,
        desc: &[
            "GPU update interval (ms).",
            "",
            "0 = use global update_ms.",
            "Range: 100 to 86400000.",
        ],
    },
];

/// Options in the "disk" category.
pub const DISK: &[OptDef] = &[
    OptDef {
        key: ConfigKey::GraphSymbolDisk,
        desc: &[
            "Disk graph symbol.",
            "",
            "\"default\", \"braille\", or \"block\".",
        ],
    },
    OptDef {
        key: ConfigKey::ShowIoStat,
        desc: &[
            "Disk IO activity indicators.",
            "",
            "Show read/write throughput data",
            "alongside disk usage meters.",
        ],
    },
    OptDef {
        key: ConfigKey::IoMode,
        desc: &[
            "IO mode toggle.",
            "",
            "Switch between usage meters and",
            "IO throughput graphs with the",
            "\"i\" key.",
        ],
    },
    OptDef {
        key: ConfigKey::IoGraphCombined,
        desc: &[
            "Combined IO graph.",
            "",
            "Merge read and write into a single",
            "graph. Only applies in IO mode.",
        ],
    },
    OptDef {
        key: ConfigKey::DiskIoMode,
        desc: &[
            "Persistent IO mode.",
            "",
            "Always show IO throughput graphs",
            "instead of usage meters.",
        ],
    },
    OptDef {
        key: ConfigKey::DisksFilter,
        desc: &[
            "Disk filter.",
            "",
            "Filter which disks are shown.",
            "Use drive letters (e.g. \"C:\").",
            "Prefix with ! to exclude.",
            "Separate with whitespace.",
        ],
    },
    OptDef {
        key: ConfigKey::DiskUpdateMs,
        desc: &[
            "Disk update interval (ms).",
            "",
            "0 = use global update_ms.",
            "Range: 100 to 86400000.",
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
pub fn get_value(key: ConfigKey, config: &Config) -> String {
    key.get_display(config)
}

/// Cycle a browsable option left or right. Returns true if changed.
pub fn cycle_browsable(key: ConfigKey, config: &mut Config, direction: i32) -> bool {
    if !matches!(key.kind(), KeyKind::String | KeyKind::Enum) {
        return false;
    }
    let vals = key.browsable_values();
    if vals.is_empty() {
        return false;
    }
    let current = key.get_display(config);
    let idx = vals.iter().position(|&v| v == current).unwrap_or(0);
    let new_idx = if direction > 0 {
        (idx + 1) % vals.len()
    } else if idx == 0 {
        vals.len() - 1
    } else {
        idx - 1
    };
    key.set_string(config, vals[new_idx])
        .expect("browsable_values entries must round-trip through set_string");
    true
}

/// Step an int option by `delta`.
pub fn step_int(key: ConfigKey, config: &mut Config, delta: i64) {
    let step = match key {
        ConfigKey::UpdateMs
        | ConfigKey::CpuUpdateMs
        | ConfigKey::MemUpdateMs
        | ConfigKey::DiskUpdateMs
        | ConfigKey::NetUpdateMs
        | ConfigKey::GpuUpdateMs
        | ConfigKey::ProcUpdateMs => 100,
        _ => 1,
    };
    let value = key.get_int(config) + delta * step;
    key.set_int(config, value);
    config.validate();
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
fn capitalize_option(key: ConfigKey) -> String {
    key.name()
        .replace('_', " ")
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

    let hi = theme.color(tc::HI_FG);
    let title_c = theme.color(tc::TITLE);
    let fg = theme.color(tc::MAIN_FG);
    let sel_bg = theme.color(tc::SELECTED_BG);
    let sel_fg = theme.color(tc::SELECTED_FG);
    let opts_c = theme.color(tc::OPTIONS_BOX);
    let reset = term::RESET;

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
        rounded: config.rounded_corners,
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
                "{}{}[{}{}{}]{}",
                term::BOLD,
                hi,
                title_c,
                name,
                hi,
                reset
            ));
        } else {
            out.push_str(&format!(
                "{}{}{}{}{}{}",
                term::BOLD,
                hi,
                i,
                title_c,
                name,
                reset
            ));
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
                format!("{}{}{}", sel_bg, sel_fg, term::BOLD)
            } else {
                format!("{}{}", title_c, term::BOLD)
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
                        "{}{}{}{}{}{}{}",
                        term::BOLD,
                        term::mv(x + 3, val_row),
                        hi,
                        symbols::LEFT_ARROW,
                        term::mv(x + 29, val_row),
                        hi,
                        symbols::RIGHT_ARROW,
                    ));
                    out.push_str(reset);
                }
                OptKind::StringVal => {
                    out.push_str(&format!(
                        "{}{}{}{}",
                        term::BOLD,
                        term::mv(x + 29, val_row),
                        hi,
                        symbols::ENTER,
                    ));
                    out.push_str(reset);
                }
            }

            // Description in right panel
            out.push_str(&format!("{}{}{}", reset, title_c, term::BOLD));
            for (di, desc_line) in opt.desc.iter().enumerate() {
                let desc_row = y + 8 + 1 + di; // start at the row after the divider
                if desc_row >= y + 6 + height {
                    break;
                }
                // First description line is title-colored, rest are main_fg
                if di == 1 {
                    out.push_str(&format!("{}{}", fg, term::BOLD_OFF));
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
pub fn opt_key(cat: usize, page: usize, selected: usize, term_height: usize) -> Option<ConfigKey> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_keys_roundtrip_through_config_parser() {
        for category in categories() {
            for option in *category {
                assert_eq!(ConfigKey::parse(option.key.name()), Some(option.key));
            }
        }
    }
}
