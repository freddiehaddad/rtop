//! Help-menu overlay.
//!
//! Box dimensions and content are entirely **derived from
//! [`crate::handlers::keybinds::BINDINGS`]** — every binding whose
//! `help: Option<HelpEntry>` is `Some(_)` becomes a help row, and
//! the rows are grouped by `HelpEntry::category` in first-seen
//! order. Adding or removing a binding or changing a help entry
//! automatically resizes the box; no constants need updating.

use crate::draw::box_drawing;
use crate::handlers::keybinds::BINDINGS;
use crate::term;
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------
//
// All shape parameters of the help menu live here. The box width
// and height are otherwise *fully derived* from the help-row list,
// so adding or removing entries automatically resizes the box.

/// Cells of horizontal padding between the left/right border and
/// the nearest content character.
const SIDE_PAD: usize = 1;
/// Cells between the right edge of the key column and the left
/// edge of the description column.
const KEY_DESC_GAP: usize = 2;
/// Minimum extra width needed by `box_drawing::section_divider` on
/// top of the section name (`├──┐ name ┌─┤` overhead).
const DIVIDER_OVERHEAD: usize = 6;

/// Single help-menu row — flattened from a [`Binding`]'s
/// `Some(HelpEntry)` for layout/render convenience.
struct HelpRow {
    keys: &'static str,
    desc: &'static str,
    category: &'static str,
}

/// Flatten every binding with `help: Some(_)` into a [`HelpRow`].
/// Order is the [`BINDINGS`] declaration order, which is also the
/// authoring contract for help-menu section ordering.
fn help_rows() -> Vec<HelpRow> {
    BINDINGS
        .iter()
        .filter_map(|b| {
            b.help.map(|h| HelpRow {
                keys: h.keys,
                desc: h.description,
                category: h.category,
            })
        })
        .collect()
}

/// Compute the smallest `(width, height)` the help box needs to
/// render `rows` without truncation, including borders.
///
/// Width is the larger of:
///   * `2 (borders) + 2 * SIDE_PAD + key_col + KEY_DESC_GAP + desc_col`
///   * `2 (borders) + section_name + DIVIDER_OVERHEAD` (max over sections)
///
/// Height is `2 (borders) + section_count + rows.len()` — one
/// row per divider, one row per keybind, no inter-section blanks.
fn dimensions(rows: &[HelpRow]) -> (usize, usize) {
    let key_col = rows
        .iter()
        .map(|r| tools::ulen(r.keys, false))
        .max()
        .unwrap_or(0);
    let desc_col = rows
        .iter()
        .map(|r| tools::ulen(r.desc, false))
        .max()
        .unwrap_or(0);
    let longest_section = rows
        .iter()
        .map(|r| tools::ulen(r.category, false))
        .max()
        .unwrap_or(0);

    let content_width = 2 * SIDE_PAD + key_col + KEY_DESC_GAP + desc_col;
    let divider_width = longest_section + DIVIDER_OVERHEAD;
    let width = 2 + content_width.max(divider_width);

    let section_count = rows
        .iter()
        .map(|r| r.category)
        .scan("", |prev, cur| {
            let new = cur != *prev;
            *prev = cur;
            Some(new)
        })
        .filter(|&new| new)
        .count();
    let height = 2 + section_count + rows.len();

    (width, height)
}

/// Width of the key column, derived from the longest key text.
fn key_col_width(rows: &[HelpRow]) -> usize {
    rows.iter()
        .map(|r| tools::ulen(r.keys, false))
        .max()
        .unwrap_or(0)
}

/// Draw the help menu centered on screen, populated from
/// [`BINDINGS`].
///
/// Box dimensions are derived from the help-row list via
/// [`dimensions`], then clamped to the terminal size. Adding or
/// removing keybinds (or HelpEntries) automatically resizes the
/// box; no constants need updating.
pub fn draw(term_width: usize, term_height: usize, theme: &Theme, rounded: bool) -> String {
    let rows = help_rows();
    let (preferred_w, preferred_h) = dimensions(&rows);
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
    let key_col = key_col_width(&rows);
    let key_x = x + 2 + SIDE_PAD;
    let desc_x = key_x + key_col + KEY_DESC_GAP;

    let mut row = inner_top;
    let mut current_section = "";

    for hr in &rows {
        // Section divider when section changes.
        if hr.category != current_section {
            if row > inner_bottom {
                break;
            }
            current_section = hr.category;
            out.push_str(&term::mv(x + 1, row));
            out.push_str(&box_drawing::section_divider(
                hr.category,
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
            tools::ljust(hr.keys, key_col, false),
            term::mv(desc_x, row),
            fg,
            hr.desc,
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
    fn help_rows_has_all_sections() {
        let rows = help_rows();
        let sections: Vec<&str> = rows.iter().map(|r| r.category).collect();
        assert!(sections.contains(&"Global"), "missing Global section");
        assert!(sections.contains(&"Process"), "missing Process section");
        assert!(sections.contains(&"Disk"), "missing Disk section");
        assert!(sections.contains(&"Network"), "missing Network section");
    }

    #[test]
    fn help_rows_no_empty_entries() {
        let rows = help_rows();
        for r in &rows {
            assert!(!r.keys.is_empty(), "empty keys in help row");
            assert!(!r.desc.is_empty(), "empty desc for keys: {}", r.keys);
            assert!(
                !r.category.is_empty(),
                "empty category for keys: {}",
                r.keys
            );
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
    fn dimensions_height_equals_borders_plus_sections_plus_rows() {
        let rows = help_rows();
        let (_, h) = dimensions(&rows);
        let section_count = rows
            .iter()
            .map(|r| r.category)
            .scan("", |prev, cur| {
                let new = cur != *prev;
                *prev = cur;
                Some(new)
            })
            .filter(|&new| new)
            .count();
        assert_eq!(h, 2 + section_count + rows.len());
    }

    #[test]
    fn dimensions_width_fits_longest_row() {
        let rows = help_rows();
        let (w, _) = dimensions(&rows);
        let key_col = rows
            .iter()
            .map(|r| tools::ulen(r.keys, false))
            .max()
            .unwrap();
        let desc_col = rows
            .iter()
            .map(|r| tools::ulen(r.desc, false))
            .max()
            .unwrap();
        // 2 borders + 2 paddings + key + gap + desc must fit.
        assert!(w >= 2 + 2 * SIDE_PAD + key_col + KEY_DESC_GAP + desc_col);
    }

    #[test]
    fn dimensions_width_fits_longest_section_divider() {
        let rows = help_rows();
        let (w, _) = dimensions(&rows);
        let longest_section = rows
            .iter()
            .map(|r| tools::ulen(r.category, false))
            .max()
            .unwrap();
        // 2 borders + section name + divider overhead must fit.
        assert!(w >= 2 + longest_section + DIVIDER_OVERHEAD);
    }

    #[test]
    fn help_rows_groups_are_contiguous() {
        // The help layout contract: each category appears as one
        // contiguous block. Authoring bindings out of order would
        // break the divider rendering (we'd render the same
        // divider twice).
        let rows = help_rows();
        let mut seen: Vec<&str> = Vec::new();
        let mut current = "";
        for r in &rows {
            if r.category != current {
                assert!(
                    !seen.contains(&r.category),
                    "category {:?} appears in two non-contiguous blocks in BINDINGS",
                    r.category,
                );
                seen.push(r.category);
                current = r.category;
            }
        }
    }
}
