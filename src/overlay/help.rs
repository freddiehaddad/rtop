//! Help overlay subsystem: state, render, and per-key actions.
//!
//! The help overlay is a centered list of every keybind the
//! application responds to (sourced from
//! `crate::handlers::keybinds::BINDINGS`). It can be opened either
//! directly from Normal mode (`?`, `F1`) or from the Main menu
//! (Esc → ↓ → Enter), and on close it returns to wherever it was
//! opened from.
//!
//! Box dimensions and content are entirely **derived from the
//! binding table** — every binding whose `help: Option<HelpEntry>`
//! is `Some(_)` becomes a help row, grouped by `category` in
//! first-seen order. Adding or removing a binding or changing a
//! help entry automatically resizes the box.

use crate::app::TerminalSize;
use crate::config::Config;
use crate::draw::box_drawing;
use crate::handlers::InputContext;
use crate::handlers::keybinds::BINDINGS;
use crate::input::Key;
use crate::overlay::ReturnTarget;
use crate::term;
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Persistent state for the help overlay: just the close target.
#[derive(Debug, Clone)]
pub struct HelpState {
    pub return_to: ReturnTarget,
}

impl HelpState {
    /// Construct help state for an overlay that returns to
    /// `return_to` on close.
    pub fn new(return_to: ReturnTarget) -> Self {
        Self { return_to }
    }
}

// ---------------------------------------------------------------------------
// Render — layout constants
// ---------------------------------------------------------------------------

/// Cells of horizontal padding between the left/right border and
/// the nearest content character.
const SIDE_PAD: usize = 1;
/// Cells between the right edge of the key column and the left
/// edge of the description column.
const KEY_DESC_GAP: usize = 2;
/// Minimum extra width needed by `box_drawing::section_divider` on
/// top of the section name (`├──┐ name ┌─┤` overhead).
const DIVIDER_OVERHEAD: usize = 6;

/// Single help-menu row — flattened from a binding's
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

/// Render the help overlay to an unstyled ANSI buffer, populated
/// from [`BINDINGS`]. Box dimensions are derived from the help-row
/// list via [`dimensions`], then clamped to the terminal size.
pub fn render(_state: &HelpState, term: TerminalSize, config: &Config, theme: &Theme) -> String {
    let rows = help_rows();
    let (preferred_w, preferred_h) = dimensions(&rows);
    let w = preferred_w.min(term.width);
    let h = preferred_h.min(term.height);
    let x = (term.width.saturating_sub(w)) / 2;
    let y = (term.height.saturating_sub(h)) / 2;

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
        rounded: config.ui.rounded_corners,
        hi_color: hi,
        title_color: title_c,
    });

    let inner_top = y + 2;
    let inner_bottom = y + h.saturating_sub(1);
    let divider_inner_w = w.saturating_sub(2);
    let key_col = key_col_width(&rows);
    let key_x = x + 2 + SIDE_PAD;
    let desc_x = key_x + key_col + KEY_DESC_GAP;

    let mut row = inner_top;
    let mut current_section = "";

    for hr in &rows {
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

// ---------------------------------------------------------------------------
// Per-action handlers (referenced by handlers/keybinds/table.rs)
// ---------------------------------------------------------------------------

pub(crate) fn close_action(ctx: &mut InputContext, _key: &Key) {
    ctx.close_overlay();
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "help",
        opened = false,
        "menu transition",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_return_target() {
        let s = HelpState::new(ReturnTarget::Normal);
        assert_eq!(s.return_to, ReturnTarget::Normal);
        let s = HelpState::new(ReturnTarget::Main);
        assert_eq!(s.return_to, ReturnTarget::Main);
    }

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
    fn render_emits_all_sections() {
        let theme = Theme::new();
        let config = Config::new();
        let state = HelpState::new(ReturnTarget::Normal);
        let out = render(
            &state,
            TerminalSize {
                width: 80,
                height: 45,
            },
            &config,
            &theme,
        );
        assert!(out.contains("Global"));
        assert!(out.contains("Process"));
        assert!(out.contains("Network"));
    }

    #[test]
    fn render_emits_keybind_descriptions() {
        let theme = Theme::new();
        let config = Config::new();
        let state = HelpState::new(ReturnTarget::Normal);
        let out = render(
            &state,
            TerminalSize {
                width: 200,
                height: 60,
            },
            &config,
            &theme,
        );
        assert!(out.contains("Quit"));
        assert!(out.contains("Toggle main menu"));
        assert!(out.contains("Select process"));
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
    fn help_rows_groups_are_contiguous() {
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
