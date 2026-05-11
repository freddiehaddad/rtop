//! Main menu overlay subsystem: state, render, and per-key actions.
//!
//! The main menu (`m` to open; Esc or `m` to close) presents three
//! actions: Options, Help, Quit. Selection is held in
//! [`MainMenuState`] as a typed [`MainMenuItem`] enum — not a
//! `usize` index — so the renderer and activation handler cannot
//! drift out of sync with the menu's actual contents.

use crate::app::TerminalSize;
use crate::banner;
use crate::handlers::InputContext;
use crate::input::Key;
use crate::overlay::{ActiveModal, ReturnTarget};
use crate::term;
use crate::theme::Theme;
use crate::theme_keys as tc;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Items in the main menu, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuItem {
    Options,
    Help,
    Quit,
}

impl MainMenuItem {
    /// All items in display order. The first item is the default
    /// selection on open; the last item wraps to the first on
    /// `select_next`.
    pub const fn all() -> [MainMenuItem; 3] {
        [
            MainMenuItem::Options,
            MainMenuItem::Help,
            MainMenuItem::Quit,
        ]
    }

    /// Position in [`Self::all`] — used by the renderer to map the
    /// variant to its on-screen row index.
    pub fn index(self) -> usize {
        Self::all()
            .iter()
            .position(|&x| x == self)
            .expect("MainMenuItem::all enumerates every variant")
    }
}

/// Persistent state for the main menu overlay: just the current
/// selection.
#[derive(Debug, Clone)]
pub struct MainMenuState {
    selected: MainMenuItem,
}

impl MainMenuState {
    /// Initial state — selection on the first item.
    pub fn new() -> Self {
        Self {
            selected: MainMenuItem::all()[0],
        }
    }

    /// Currently-selected item.
    pub fn selected(&self) -> MainMenuItem {
        self.selected
    }

    /// Move selection down one item, wrapping from the last item to
    /// the first.
    pub fn select_next(&mut self) {
        let items = MainMenuItem::all();
        let next_index = (self.selected.index() + 1) % items.len();
        self.selected = items[next_index];
    }

    /// Move selection up one item, wrapping from the first item to
    /// the last.
    pub fn select_prev(&mut self) {
        let items = MainMenuItem::all();
        let prev_index = if self.selected.index() == 0 {
            items.len() - 1
        } else {
            self.selected.index() - 1
        };
        self.selected = items[prev_index];
    }
}

impl Default for MainMenuState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// Menu item ASCII art: normal (thin lines) and selected (thick lines)
const MENU_NORMAL: [&[&str]; 3] = [
    &[
        "┌─┐┌─┐┌┬┐┬┌─┐┌┐┌┌─┐",
        "│ │├─┘ │ ││ ││││└─┐",
        "└─┘┴   ┴ ┴└─┘┘└┘└─┘",
    ],
    &["┬ ┬┌─┐┬  ┌─┐", "├─┤├┤ │  ├─┘", "┴ ┴└─┘┴─┘┴  "],
    &["┌─┐ ┬ ┬ ┬┌┬┐", "│─┼┐│ │ │ │ ", "└─┘└└─┘ ┴ ┴ "],
];

const MENU_SELECTED: [&[&str]; 3] = [
    &[
        "╔═╗╔═╗╔╦╗╦╔═╗╔╗╔╔═╗",
        "║ ║╠═╝ ║ ║║ ║║║║╚═╗",
        "╚═╝╩   ╩ ╩╚═╝╝╚╝╚═╝",
    ],
    &["╦ ╦╔═╗╦  ╔═╗", "╠═╣╠╣ ║  ╠═╝", "╩ ╩╚═╝╩═╝╩  "],
    &["╔═╗ ╦ ╦ ╦╔╦╗ ", "║═╬╗║ ║ ║ ║  ", "╚═╝╚╚═╝ ╩ ╩  "],
];

const MENU_WIDTHS: [usize; 3] = [19, 12, 12];

/// Render the main menu to an unstyled ANSI buffer. Colours are
/// derived from the theme's `hi_fg` (selected) and `main_fg`
/// (normal).
pub fn render(state: &MainMenuState, term: TerminalSize, theme: &Theme) -> String {
    let selected = state.selected().index();
    let hi_rgb = theme.rgb(tc::HI_FG);
    let fg_rgb = theme.rgb(tc::MAIN_FG);
    let colors_selected = banner::gradient3(hi_rgb);
    let colors_normal = banner::gradient3(fg_rgb);

    let mut out = String::new();

    // Position: banner centered at y = height/2 - 10
    let banner_y = term.height / 2;
    let banner_y = if banner_y > 10 { banner_y - 10 } else { 1 };

    // Draw banner
    out.push_str(&banner::generate(
        banner_y,
        (term.width.saturating_sub(35)) / 2,
        theme,
    ));

    // Menu items start below the banner (6 lines of banner + 1 gap)
    let mut cy = banner_y + 7;

    for i in 0..3 {
        let menu = if i == selected {
            &MENU_SELECTED[i]
        } else {
            &MENU_NORMAL[i]
        };
        let colors = if i == selected {
            &colors_selected
        } else {
            &colors_normal
        };
        let w = MENU_WIDTHS[i];
        let menu_x = (term.width.saturating_sub(w)) / 2;

        for (line_idx, line) in menu.iter().enumerate() {
            out.push_str(&format!(
                "{}{}{}",
                term::mv(menu_x + 1, cy),
                colors[line_idx],
                line,
            ));
            cy += 1;
        }
    }

    out.push_str(term::RESET);
    out
}

// ---------------------------------------------------------------------------
// Per-action handlers (referenced by handlers/keybinds/table.rs)
// ---------------------------------------------------------------------------

pub(crate) fn quit_action(ctx: &mut InputContext, _: &Key) {
    *ctx.quit = true;
}

pub(crate) fn close_action(ctx: &mut InputContext, _: &Key) {
    ctx.close_overlay();
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "main",
        opened = false,
        "menu transition",
    );
}

pub(crate) fn select_prev_action(ctx: &mut InputContext, _: &Key) {
    if let ActiveModal::Main(s) = &mut ctx.overlay.active {
        s.select_prev();
        ctx.render.dirty.mark_overlay();
    }
}

pub(crate) fn select_next_action(ctx: &mut InputContext, _: &Key) {
    if let ActiveModal::Main(s) = &mut ctx.overlay.active {
        s.select_next();
        ctx.render.dirty.mark_overlay();
    }
}

pub(crate) fn activate_selected_action(ctx: &mut InputContext, _: &Key) {
    let selected = match &ctx.overlay.active {
        ActiveModal::Main(s) => s.selected(),
        _ => return,
    };
    match selected {
        MainMenuItem::Options => open_options_from_main(ctx),
        MainMenuItem::Help => open_help_from_main(ctx),
        MainMenuItem::Quit => *ctx.quit = true,
    }
}

pub(crate) fn open_options_action(ctx: &mut InputContext, _: &Key) {
    open_options_from_main(ctx);
}

pub(crate) fn open_help_action(ctx: &mut InputContext, _: &Key) {
    open_help_from_main(ctx);
}

fn open_options_from_main(ctx: &mut InputContext) {
    ctx.open_options_menu(ReturnTarget::Main);
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "options",
        opened = true,
        "menu transition",
    );
}

fn open_help_from_main(ctx: &mut InputContext) {
    ctx.open_help_menu(ReturnTarget::Main);
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "help",
        opened = true,
        "menu transition",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_selects_first_item() {
        let s = MainMenuState::new();
        assert_eq!(s.selected(), MainMenuItem::Options);
    }

    #[test]
    fn select_next_walks_then_wraps() {
        let mut s = MainMenuState::new();
        s.select_next();
        assert_eq!(s.selected(), MainMenuItem::Help);
        s.select_next();
        assert_eq!(s.selected(), MainMenuItem::Quit);
        s.select_next();
        assert_eq!(s.selected(), MainMenuItem::Options, "must wrap");
    }

    #[test]
    fn select_prev_walks_then_wraps() {
        let mut s = MainMenuState::new();
        s.select_prev();
        assert_eq!(s.selected(), MainMenuItem::Quit, "must wrap from first");
        s.select_prev();
        assert_eq!(s.selected(), MainMenuItem::Help);
    }

    #[test]
    fn index_round_trips_through_all() {
        for (i, item) in MainMenuItem::all().iter().enumerate() {
            assert_eq!(item.index(), i);
        }
    }
}
