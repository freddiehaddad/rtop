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

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------
//
// All shape parameters of the help menu live here. The box width
// and height are otherwise *fully derived* from `KEYBINDS`, so
// adding or removing entries automatically resizes the box.

/// Cells of horizontal padding between the left/right border and
/// the nearest content character.
const SIDE_PAD: usize = 1;
/// Cells between the right edge of the key column and the left
/// edge of the description column.
const KEY_DESC_GAP: usize = 2;
/// Minimum extra width needed by `box_drawing::section_divider` on
/// top of the section name (`├──┐ name ┌─┤` overhead).
const DIVIDER_OVERHEAD: usize = 6;

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
        key: "1-5",
        desc: "Toggle widget (cpu/mem/net/proc/disk)",
        section: "Global",
    },
    Keybind {
        key: "6-9",
        desc: "Toggle GPU 0-3",
        section: "Global",
    },
    Keybind {
        key: "0",
        desc: "Toggle GPU 4-7",
        section: "Global",
    },
    Keybind {
        key: "Shift-R",
        desc: "Restore all hidden widgets",
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
        key: "c",
        desc: "Toggle per-core CPU",
        section: "Process",
    },
    Keybind {
        key: "t",
        desc: "Terminate process (graceful)",
        section: "Process",
    },
    Keybind {
        key: "T",
        desc: "Kill process (force)",
        section: "Process",
    },
    Keybind {
        key: "Enter",
        desc: "Show process details",
        section: "Process",
    },
    Keybind {
        key: "F",
        desc: "Follow/unfollow process",
        section: "Process",
    },
    // Disk
    Keybind {
        key: "i",
        desc: "Toggle IO mode",
        section: "Disk",
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
        key: "s",
        desc: "Toggle net sync scale",
        section: "Network",
    },
    Keybind {
        key: "z",
        desc: "Reset network counters",
        section: "Network",
    },
];

/// Compute the smallest `(width, height)` the help box needs to
/// render `KEYBINDS` without truncation, including borders.
///
/// Width is the larger of:
///   * `2 (borders) + 2 * SIDE_PAD + key_col + KEY_DESC_GAP + desc_col`
///   * `2 (borders) + section_name + DIVIDER_OVERHEAD` (max over sections)
///
/// Height is `2 (borders) + section_count + keybind_count` — one
/// row per divider, one row per keybind, no inter-section blanks.
fn dimensions() -> (usize, usize) {
    let key_col = KEYBINDS
        .iter()
        .map(|kb| tools::ulen(kb.key, false))
        .max()
        .unwrap_or(0);
    let desc_col = KEYBINDS
        .iter()
        .map(|kb| tools::ulen(kb.desc, false))
        .max()
        .unwrap_or(0);
    let longest_section = KEYBINDS
        .iter()
        .map(|kb| tools::ulen(kb.section, false))
        .max()
        .unwrap_or(0);

    let content_width = 2 * SIDE_PAD + key_col + KEY_DESC_GAP + desc_col;
    let divider_width = longest_section + DIVIDER_OVERHEAD;
    let width = 2 + content_width.max(divider_width);

    let section_count = KEYBINDS
        .iter()
        .map(|kb| kb.section)
        .scan("", |prev, cur| {
            let new = cur != *prev;
            *prev = cur;
            Some(new)
        })
        .filter(|&new| new)
        .count();
    let height = 2 + section_count + KEYBINDS.len();

    (width, height)
}

/// Width of the key column, derived from the longest key text.
fn key_col_width() -> usize {
    KEYBINDS
        .iter()
        .map(|kb| tools::ulen(kb.key, false))
        .max()
        .unwrap_or(0)
}

/// Draw the help menu centered on screen, populated from KEYBINDS.
///
/// Box dimensions are derived from `KEYBINDS` via `dimensions()`,
/// then clamped to the terminal size. Adding or removing keybinds
/// or sections automatically resizes the box; no constants need
/// updating.
pub fn draw(term_width: usize, term_height: usize, theme: &Theme, rounded: bool) -> String {
    let (preferred_w, preferred_h) = dimensions();
    let w = preferred_w.min(term_width);
    let h = preferred_h.min(term_height);
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

    // Inner content area. `create_box` uses 1-based offsets: the
    // left border lands at column `x+1` and the top border at row
    // `y+1`. So the first cell strictly inside the box is `(x+2,
    // y+2)`, and the last writable cell is `(x+w-1, y+h-1)`.
    let inner_top = y + 2;
    let inner_bottom = y + h.saturating_sub(1); // last writable row
    let divider_inner_w = w.saturating_sub(2);
    let key_col = key_col_width();
    let key_x = x + 2 + SIDE_PAD;
    let desc_x = key_x + key_col + KEY_DESC_GAP;

    let mut row = inner_top;
    let mut current_section = "";

    for kb in KEYBINDS {
        // Section divider when section changes.
        if kb.section != current_section {
            if row > inner_bottom {
                break;
            }
            current_section = kb.section;
            out.push_str(&term::mv(x + 1, row));
            out.push_str(&box_drawing::section_divider(
                kb.section,
                divider_inner_w,
                help_c,
                title_c,
            ));
            row += 1;
        }

        // Key + description.
        if row > inner_bottom {
            break;
        }
        out.push_str(&format!(
            "{}{}{}{}{}{}",
            term::mv(key_x, row),
            hi,
            tools::ljust(kb.key, key_col, false),
            term::mv(desc_x, row),
            fg,
            kb.desc,
        ));
        row += 1;
    }

    out.push_str(term::RESET);
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
        assert!(sections.contains(&"Disk"), "missing Disk section");
        assert!(sections.contains(&"Network"), "missing Network section");
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
        let out = draw(200, 60, &theme, true);
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

    #[test]
    fn dimensions_height_equals_borders_plus_sections_plus_keybinds() {
        let (_, h) = dimensions();
        let section_count = KEYBINDS
            .iter()
            .map(|kb| kb.section)
            .scan("", |prev, cur| {
                let new = cur != *prev;
                *prev = cur;
                Some(new)
            })
            .filter(|&new| new)
            .count();
        assert_eq!(h, 2 + section_count + KEYBINDS.len());
    }

    #[test]
    fn dimensions_width_fits_longest_keybind_row() {
        let (w, _) = dimensions();
        let key_col = KEYBINDS
            .iter()
            .map(|kb| tools::ulen(kb.key, false))
            .max()
            .unwrap();
        let desc_col = KEYBINDS
            .iter()
            .map(|kb| tools::ulen(kb.desc, false))
            .max()
            .unwrap();
        // 2 borders + 2 paddings + key + gap + desc must fit.
        assert!(w >= 2 + 2 * SIDE_PAD + key_col + KEY_DESC_GAP + desc_col);
    }

    #[test]
    fn dimensions_width_fits_longest_section_divider() {
        let (w, _) = dimensions();
        let longest_section = KEYBINDS
            .iter()
            .map(|kb| tools::ulen(kb.section, false))
            .max()
            .unwrap();
        // 2 borders + section name + divider overhead must fit.
        assert!(w >= 2 + longest_section + DIVIDER_OVERHEAD);
    }
}
