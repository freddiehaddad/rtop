use crate::draw::box_drawing;
use crate::term;
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

/// A single keybind entry: (key display, description, section).
pub struct Keybind {
    pub key: &'static str,
    pub desc: &'static str,
    pub section: &'static str,
}

/// All keybinds in the application, grouped by section.
/// This is the single source of truth — the help menu renders from this.
pub const KEYBINDS: &[Keybind] = &[
    // Global
    Keybind {
        key: "q / Ctrl-C",
        desc: "Quit",
        section: "Global",
    },
    Keybind {
        key: "m / Esc",
        desc: "Toggle main menu",
        section: "Global",
    },
    Keybind {
        key: "? / F1",
        desc: "Toggle help",
        section: "Global",
    },
    Keybind {
        key: "o / F2",
        desc: "Toggle options",
        section: "Global",
    },
    Keybind {
        key: "p / Shift-P",
        desc: "Cycle presets forward/back",
        section: "Global",
    },
    Keybind {
        key: "Ctrl-S",
        desc: "Save current layout as preset",
        section: "Global",
    },
    Keybind {
        key: "Ctrl-X",
        desc: "Delete current preset",
        section: "Global",
    },
    Keybind {
        key: "Ctrl-R",
        desc: "Reload config",
        section: "Global",
    },
    Keybind {
        key: "+/-",
        desc: "Adjust update speed",
        section: "Global",
    },
    Keybind {
        key: "1-6",
        desc: "Toggle box (cpu/mem/net/proc/gpu/disk)",
        section: "Global",
    },
    // Process
    Keybind {
        key: "Up/Down",
        desc: "Select process",
        section: "Process",
    },
    Keybind {
        key: "PgUp/PgDn",
        desc: "Page through processes",
        section: "Process",
    },
    Keybind {
        key: "Home/End",
        desc: "Jump to first/last",
        section: "Process",
    },
    Keybind {
        key: "Left/Right",
        desc: "Cycle sort column",
        section: "Process",
    },
    Keybind {
        key: "r",
        desc: "Reverse sort order",
        section: "Process",
    },
    Keybind {
        key: "f / /",
        desc: "Enter filter mode",
        section: "Process",
    },
    Keybind {
        key: "e",
        desc: "Toggle tree view",
        section: "Process",
    },
    Keybind {
        key: "i",
        desc: "Toggle IO mode",
        section: "Process",
    },
    Keybind {
        key: "c",
        desc: "Toggle per-core CPU",
        section: "Process",
    },
    Keybind {
        key: "t",
        desc: "Terminate process",
        section: "Process",
    },
    Keybind {
        key: "Enter",
        desc: "Show process details",
        section: "Process",
    },
    // Network
    Keybind {
        key: "n",
        desc: "Next network interface",
        section: "Network",
    },
    Keybind {
        key: "b",
        desc: "Previous network interface",
        section: "Network",
    },
    Keybind {
        key: "a",
        desc: "Toggle net auto-scale",
        section: "Network",
    },
    Keybind {
        key: "y",
        desc: "Toggle net sync scale",
        section: "Network",
    },
    Keybind {
        key: "z",
        desc: "Reset network counters",
        section: "Network",
    },
    // Filter mode
    Keybind {
        key: "Esc",
        desc: "Cancel filter",
        section: "Filter",
    },
    Keybind {
        key: "Enter",
        desc: "Apply filter",
        section: "Filter",
    },
    Keybind {
        key: "Backspace",
        desc: "Delete character",
        section: "Filter",
    },
    Keybind {
        key: "Delete",
        desc: "Clear filter",
        section: "Filter",
    },
];

/// Draw the help menu centered on screen, populated from KEYBINDS.
pub fn draw(term_width: usize, term_height: usize, theme: &Theme, rounded: bool) -> String {
    let w = 60.min(term_width);
    let h = 40.min(term_height);
    let x = (term_width.saturating_sub(w)) / 2;
    let y = (term_height.saturating_sub(h)) / 2;

    let hi = theme.color(tc::HI_FG);
    let title_c = theme.color(tc::TITLE);
    let fg = theme.color(tc::MAIN_FG);
    let help_c = theme.color(tc::HELP_BOX);

    let mut out = box_drawing::create_box(&box_drawing::BoxConfig {
        x,
        y,
        width: w,
        height: h,
        line_color: help_c,
        fill: true,
        title: "help",
        title2: "",
        num: 0,
        rounded,
        hi_color: hi,
        title_color: title_c,
    });

    // Build lines from KEYBINDS, grouped by section
    let divider_w = w.saturating_sub(2); // between ├ and ┤
    let max_lines = h.saturating_sub(3);
    let mut row = 0;
    let mut current_section = "";

    for kb in KEYBINDS {
        if row >= max_lines {
            break;
        }
        // Section header: ├──┐ Section ┌──────────────────────┤
        if kb.section != current_section {
            if !current_section.is_empty() && row < max_lines {
                row += 1;
            }
            if row >= max_lines {
                break;
            }
            current_section = kb.section;
            out.push_str(&term::mv(x + 1, y + 2 + row));
            out.push_str(&box_drawing::section_divider(
                kb.section, divider_w, help_c, title_c,
            ));
            row += 1;
        }
        if row >= max_lines {
            break;
        }
        // Key + description
        let key_col = 16; // fixed width for key column
        let key_display = tools::ljust(kb.key, key_col, false);
        out.push_str(&format!(
            "{}  {}{}{}{}",
            term::mv(x + 2, y + 2 + row),
            hi,
            key_display,
            fg,
            kb.desc,
        ));
        row += 1;
    }

    out.push_str("\x1b[0m");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keybinds_has_all_sections() {
        let sections: Vec<&str> = KEYBINDS.iter().map(|kb| kb.section).collect();
        assert!(sections.contains(&"Global"), "missing Global section");
        assert!(sections.contains(&"Process"), "missing Process section");
        assert!(sections.contains(&"Network"), "missing Network section");
        assert!(sections.contains(&"Filter"), "missing Filter section");
    }

    #[test]
    fn keybinds_no_empty_entries() {
        for kb in KEYBINDS {
            assert!(!kb.key.is_empty(), "empty key in keybinds");
            assert!(!kb.desc.is_empty(), "empty desc for key: {}", kb.key);
            assert!(!kb.section.is_empty(), "empty section for key: {}", kb.key);
        }
    }

    #[test]
    fn draw_renders_all_sections() {
        let theme = Theme::new();
        let out = draw(80, 45, &theme, true);
        assert!(out.contains("Global"), "should contain Global header");
        assert!(out.contains("Process"), "should contain Process header");
        assert!(out.contains("Network"), "should contain Network header");
    }

    #[test]
    fn draw_renders_keybind_descriptions() {
        let theme = Theme::new();
        let out = draw(80, 45, &theme, true);
        assert!(out.contains("Quit"), "should contain Quit");
        assert!(
            out.contains("Toggle main menu"),
            "should contain menu toggle"
        );
        assert!(
            out.contains("Select process"),
            "should contain process selection"
        );
    }
}
