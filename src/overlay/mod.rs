//! Per-modal overlay subsystems.
//!
//! The application's modal/overlay layer lives here. Each per-modal
//! submodule (`main_menu`, `help`, `options/{mod,edit}`, `filter`)
//! owns its own typed state struct and (in later stages) its
//! per-key actions and its render function.
//!
//! [`ActiveModal`] is the single source of truth for "what overlay
//! is open and what's its state." It replaces the scattered
//! `OverlayState` fields (`menu_state`, `menu_return_to`,
//! `main_menu_selected`, `options_cat`, `options_selected`,
//! `options_page`, `option_edit`) with one typed root.
//!
//! [`OverlayKind`] is a derived projection used by the keybind
//! dispatch table to match bindings against the current overlay
//! context. It distinguishes [`OverlayKind::Options`] (browsing the
//! options list) from [`OverlayKind::OptionsEdit`] (typing into an
//! inline edit buffer) so printable-character bindings stay grouped
//! with the right state.
//!
//! [`ReturnTarget`] is the close-target for overlays that can be
//! reached either directly (e.g. `?` opens Help from Normal mode) or
//! via the main menu (`m` → ↓ → Enter opens Help from Main). On
//! close, the overlay returns to wherever it was opened from.

pub mod filter;
pub mod help;
pub mod main_menu;
pub mod options;

use crate::app::TerminalSize;
use crate::config::Config;
use crate::theme::Theme;

/// What overlay is currently active and its per-overlay state.
///
/// Every overlay open/close transition goes through a typed mutator
/// on `AppState` (`open_main`, `open_help`, `open_options`,
/// `open_filter`, `close_overlay`); there is no public constructor
/// path that bypasses the cache + dirty contract.
#[derive(Debug, Clone, Default)]
pub enum ActiveModal {
    #[default]
    None,
    Main(main_menu::MainMenuState),
    Help(help::HelpState),
    Options(options::OptionsState),
    Filter(filter::FilterState),
}

/// A flat discriminator over [`ActiveModal`] variants, with
/// [`ActiveModal::Options`] further split by whether an inline edit
/// buffer is active.
///
/// Used by the keybind dispatch table: `Binding.states: &[OverlayKind]`
/// `+ state.overlay.active.kind()` selects which bindings fire for a
/// given key. This split-by-edit-mode is what lets the table keep
/// printable-character bindings in `Options` (which would conflict
/// with typed-character input) separate from `OptionsEdit` (where
/// printable characters are consumed by the edit buffer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OverlayKind {
    None,
    Main,
    Help,
    Options,
    OptionsEdit,
    Filter,
}

/// Where an overlay returns to when it closes. Help and Options can
/// be opened either directly from Normal mode (return to `Normal`)
/// or from the Main menu (return to `Main`, carrying the
/// [`MainMenuState`] snapshot to restore on close so the user's
/// selection survives the round-trip). Main itself always returns
/// to `Normal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReturnTarget {
    Normal,
    Main(main_menu::MainMenuState),
}

impl ActiveModal {
    /// Project to the keybind-table dispatch discriminator.
    pub fn kind(&self) -> OverlayKind {
        match self {
            ActiveModal::None => OverlayKind::None,
            ActiveModal::Main(_) => OverlayKind::Main,
            ActiveModal::Help(_) => OverlayKind::Help,
            ActiveModal::Options(s) if s.is_editing() => OverlayKind::OptionsEdit,
            ActiveModal::Options(_) => OverlayKind::Options,
            ActiveModal::Filter(_) => OverlayKind::Filter,
        }
    }

    /// `true` for overlays that should dim the underlying widget
    /// layer when active.
    ///
    /// `Main`, `Help`, and `Options` (including its inline-edit
    /// sub-state) are centered modals that take focus and should
    /// dim the underlay. `Filter` is an inline prompt living inside
    /// the proc widget and does not dim. `None` is no overlay.
    pub fn dims_underlay(&self) -> bool {
        matches!(
            self,
            ActiveModal::Main(_) | ActiveModal::Help(_) | ActiveModal::Options(_)
        )
    }
}

/// Render the active overlay's modal layer to an unstyled ANSI
/// buffer. Returns an empty string for [`ActiveModal::None`] (no
/// overlay) and [`ActiveModal::Filter`] (inline prompt rendered by
/// the proc widget, not a centered modal).
///
/// Called by the central render path
/// (`crate::app::dirty_exec::compose_modal_frame`) which then
/// styles the result via `theme.style_output` and composes it over
/// the dimmed underlay snapshot.
pub fn render(active: &ActiveModal, term: TerminalSize, config: &Config, theme: &Theme) -> String {
    match active {
        ActiveModal::None | ActiveModal::Filter(_) => String::new(),
        ActiveModal::Main(s) => main_menu::render(s, term, theme),
        ActiveModal::Help(_) => help::render(term, config, theme),
        ActiveModal::Options(s) => options::render::render(s, term, config, theme),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_none() {
        let m = ActiveModal::default();
        assert!(matches!(m, ActiveModal::None));
        assert_eq!(m.kind(), OverlayKind::None);
        assert!(!m.dims_underlay());
    }

    #[test]
    fn kind_dispatch_for_each_variant() {
        assert_eq!(
            ActiveModal::Main(main_menu::MainMenuState::new()).kind(),
            OverlayKind::Main,
        );
        assert_eq!(
            ActiveModal::Help(help::HelpState::new(ReturnTarget::Normal)).kind(),
            OverlayKind::Help,
        );
        assert_eq!(
            ActiveModal::Options(options::OptionsState::new(ReturnTarget::Normal)).kind(),
            OverlayKind::Options,
        );
        assert_eq!(
            ActiveModal::Filter(filter::FilterState).kind(),
            OverlayKind::Filter,
        );
    }

    #[test]
    fn options_with_active_edit_buffer_kinds_as_options_edit() {
        let mut opts = options::OptionsState::new(ReturnTarget::Normal);
        opts.enter_edit(options::edit::OptionEditState::placeholder());
        let m = ActiveModal::Options(opts);
        assert_eq!(m.kind(), OverlayKind::OptionsEdit);
    }

    #[test]
    fn dims_underlay_only_for_centered_modals() {
        assert!(!ActiveModal::None.dims_underlay());
        assert!(!ActiveModal::Filter(filter::FilterState).dims_underlay());
        assert!(ActiveModal::Main(main_menu::MainMenuState::new()).dims_underlay());
        assert!(ActiveModal::Help(help::HelpState::new(ReturnTarget::Normal)).dims_underlay());
        assert!(
            ActiveModal::Options(options::OptionsState::new(ReturnTarget::Normal)).dims_underlay()
        );
    }
}
