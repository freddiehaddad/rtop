pub(crate) mod keybinds;
pub(crate) mod normal;

use crate::{
    app::{
        LiveData, NetworkViewState, OverlayState, ProcessViewState, RenderState, RuntimeView,
        TerminalSize,
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
    pub(crate) filter: &'a mut crate::app::WidgetFilter,
    pub(crate) size: TerminalSize,
    /// Set to `true` by an action to signal that the application
    /// should exit after the current dispatch completes.
    pub(crate) quit: &'a mut bool,
}

impl InputContext<'_> {
    pub(crate) fn selected_proc_pid(&self) -> Option<u32> {
        let procs: &[crate::domain::process::ProcInfo] = self
            .process
            .display_procs
            .as_deref()
            .or_else(|| self.live.proc_data.as_ref().map(|s| s.procs.as_slice()))?;
        self.process
            .entries
            .get(self.process.selected)
            .and_then(|entry| procs.get(entry.proc_index))
            .map(|proc| proc.pid)
    }

    pub(crate) fn selected_proc_info(&self) -> Option<(u32, &str)> {
        let procs: &[crate::domain::process::ProcInfo] = self
            .process
            .display_procs
            .as_deref()
            .or_else(|| self.live.proc_data.as_ref().map(|s| s.procs.as_slice()))?;
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
    /// `return_to` is `Main`, transition to the main menu;
    /// otherwise close all the way to no overlay and trigger a
    /// full widget redraw.
    pub(crate) fn close_overlay(&mut self) {
        let return_to = match &self.overlay.active {
            ActiveModal::Help(s) => s.return_to,
            ActiveModal::Options(s) => s.return_to(),
            // Main, Filter, None always close to Normal.
            _ => ReturnTarget::Normal,
        };
        match return_to {
            ReturnTarget::Main => {
                self.overlay.active = ActiveModal::Main(MainMenuState::new());
                self.render.dirty.mark_overlay();
            }
            ReturnTarget::Normal => {
                let was_filter = matches!(self.overlay.active, ActiveModal::Filter(_));
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
