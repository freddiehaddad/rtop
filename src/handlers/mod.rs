pub(crate) mod keybinds;
pub(crate) mod normal;

use crate::{
    app::{
        GpuViewState, LiveData, NetworkViewState, OverlayState, ProcessViewState, RenderState,
        RuntimeView, TerminalSize, WidgetFilter,
    },
    config,
    overlay::{
        ActiveModal, ReturnTarget, filter::FilterState, help::HelpState, main_menu::MainMenuState,
        options::OptionsState,
    },
    runner, theme,
};

/// Shared mutable state passed to each per-overlay input handler.
///
/// Handlers mutate this state directly. They never write to the
/// terminal — that is the render path's job. Quit is signaled by
/// setting `*ctx.quit = true`; the event loop checks the flag
/// after each dispatch.
pub(crate) struct InputContext<'a> {
    pub(crate) config: &'a mut config::Config,
    pub(crate) theme: &'a mut theme::Theme,
    pub(crate) live: &'a LiveData,
    pub(crate) manager: &'a runner::CollectorManager,
    pub(crate) view: &'a mut RuntimeView,
    pub(crate) render: &'a mut RenderState,
    pub(crate) overlay: &'a mut OverlayState,
    pub(crate) process: &'a mut ProcessViewState,
    pub(crate) network: &'a mut NetworkViewState,
    pub(crate) gpu: &'a mut GpuViewState,
    pub(crate) filter: &'a mut WidgetFilter,
    pub(crate) size: TerminalSize,
    /// Set to `true` by an action to signal that the application
    /// should exit after the current dispatch completes.
    pub(crate) quit: &'a mut bool,
}

impl InputContext<'_> {
    pub(crate) fn selected_proc_pid(&self) -> Option<u32> {
        let procs = self.process.procs_source(self.live)?;
        self.process
            .entries
            .get(self.process.selected)
            .and_then(|entry| procs.get(entry.proc_index))
            .map(|proc| proc.pid)
    }

    pub(crate) fn selected_proc_info(&self) -> Option<(u32, &str)> {
        let procs = self.process.procs_source(self.live)?;
        self.process
            .entries
            .get(self.process.selected)
            .and_then(|entry| procs.get(entry.proc_index))
            .map(|proc| (proc.pid, proc.name.as_str()))
    }

    // ---------------------------------------------------------------
    // Overlay transitions
    //
    // Every overlay open/close goes through one of these helpers so
    // the dirty-flag and dim-cache contract is encoded in one place.
    // ---------------------------------------------------------------

    /// Open the main menu.
    pub(crate) fn open_main_menu(&mut self) {
        self.overlay.active = ActiveModal::Main(MainMenuState::new());
        self.render.dirty.mark_overlay();
    }

    /// Open the help overlay returning to `return_to` on close.
    pub(crate) fn open_help_menu(&mut self, return_to: ReturnTarget) {
        self.overlay.active = ActiveModal::Help(HelpState::new(return_to));
        self.render.dirty.mark_overlay();
    }

    /// Open the options overlay returning to `return_to` on close.
    /// Also syncs `RuntimeView -> config.view` so the menu shows
    /// current values for runtime-toggle keys.
    pub(crate) fn open_options_menu(&mut self, return_to: ReturnTarget) {
        self.view.sync_to_config(&mut self.config.view);
        self.overlay.active = ActiveModal::Options(OptionsState::new(return_to));
        self.render.dirty.mark_overlay();
    }

    /// Open the inline filter overlay. The filter is rendered by
    /// the proc widget, so this marks the proc widget dirty rather
    /// than the overlay layer.
    pub(crate) fn open_filter(&mut self) {
        self.overlay.active = ActiveModal::Filter(FilterState);
        self.render.dirty.mark_proc_widget();
    }

    /// Close the active overlay. If the active overlay's
    /// `return_to` is `Main`, transition to the main menu using
    /// the saved [`MainMenuState`] that was captured when the user
    /// drilled down from the main menu — this preserves the
    /// selection across the round-trip. Otherwise close all the
    /// way to no overlay and trigger a full widget redraw.
    pub(crate) fn close_overlay(&mut self) {
        let outcome = close_outcome(&self.overlay.active);
        match outcome {
            CloseOutcome::ToMain(saved_main) => {
                self.overlay.active = ActiveModal::Main(saved_main);
                self.render.dirty.mark_overlay();
            }
            CloseOutcome::ToNone { was_filter } => {
                self.overlay.active = ActiveModal::None;
                if was_filter {
                    // Filter close: only the inline prompt area
                    // changes — the proc widget repaints without
                    // it.
                    self.render.dirty.mark_proc_widget();
                } else {
                    // Centered modal close: full layout redraw to
                    // clear the modal area and repaint widgets at
                    // full brightness.
                    self.render.dirty.mark_layout();
                }
            }
        }
    }
}

/// What [`InputContext::close_overlay`] should do, derived purely
/// from the currently-active overlay.
///
/// Extracted as a typed enum returned by a pure function so the
/// "what comes after close" decision can be unit-tested without
/// constructing a full [`InputContext`] (which requires real
/// terminal and collector resources).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseOutcome {
    /// Restore the saved main-menu state. Used when the active
    /// overlay was opened from the main menu and its `return_to`
    /// carries the snapshot taken at open time.
    ToMain(MainMenuState),
    /// Close all the way to no overlay. `was_filter` is true when
    /// the closing overlay was the inline filter prompt (only the
    /// proc widget needs repaint); false when it was a centered
    /// modal (full layout redraw to clear the modal area).
    ToNone { was_filter: bool },
}

pub(crate) fn close_outcome(active: &ActiveModal) -> CloseOutcome {
    let return_to = match active {
        ActiveModal::Help(s) => s.return_to,
        ActiveModal::Options(s) => s.return_to(),
        // Main, Filter, None always close to Normal — Main has no
        // return target of its own; Filter and None never recurse
        // into another modal.
        _ => ReturnTarget::Normal,
    };
    match return_to {
        ReturnTarget::Main(saved_main) => CloseOutcome::ToMain(saved_main),
        ReturnTarget::Normal => CloseOutcome::ToNone {
            was_filter: matches!(active, ActiveModal::Filter(_)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::main_menu::{MainMenuItem, MainMenuState};
    use crate::overlay::{help::HelpState, options::OptionsState};

    /// The bug this guards against: closing Help/Options that was
    /// opened from a Main menu with a non-default selection used
    /// to drop the selection (close_overlay constructed a fresh
    /// `MainMenuState::new()` instead of restoring the saved one).
    /// The fix makes `ReturnTarget::Main` carry the snapshot so the
    /// close path consumes it; this test pins that contract.
    #[test]
    fn close_help_with_main_return_target_restores_saved_selection() {
        let mut saved = MainMenuState::new();
        saved.select_next(); // Options -> Help
        assert_eq!(saved.selected(), MainMenuItem::Help);

        let active = ActiveModal::Help(HelpState::new(ReturnTarget::Main(saved)));
        assert_eq!(close_outcome(&active), CloseOutcome::ToMain(saved));
    }

    #[test]
    fn close_options_with_main_return_target_restores_saved_selection() {
        let mut saved = MainMenuState::new();
        saved.select_next();
        saved.select_next(); // Options -> Help -> Quit
        assert_eq!(saved.selected(), MainMenuItem::Quit);

        let active = ActiveModal::Options(OptionsState::new(ReturnTarget::Main(saved)));
        assert_eq!(close_outcome(&active), CloseOutcome::ToMain(saved));
    }

    #[test]
    fn close_help_with_normal_return_target_closes_all_the_way() {
        let active = ActiveModal::Help(HelpState::new(ReturnTarget::Normal));
        assert_eq!(
            close_outcome(&active),
            CloseOutcome::ToNone { was_filter: false }
        );
    }

    #[test]
    fn close_options_with_normal_return_target_closes_all_the_way() {
        let active = ActiveModal::Options(OptionsState::new(ReturnTarget::Normal));
        assert_eq!(
            close_outcome(&active),
            CloseOutcome::ToNone { was_filter: false }
        );
    }

    #[test]
    fn close_main_closes_all_the_way() {
        let active = ActiveModal::Main(MainMenuState::new());
        assert_eq!(
            close_outcome(&active),
            CloseOutcome::ToNone { was_filter: false }
        );
    }

    #[test]
    fn close_filter_marks_was_filter_true() {
        let active = ActiveModal::Filter(crate::overlay::filter::FilterState);
        assert_eq!(
            close_outcome(&active),
            CloseOutcome::ToNone { was_filter: true }
        );
    }
}
