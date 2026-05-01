pub(crate) mod filter;
pub(crate) mod help;
pub(crate) mod main_menu;
pub(crate) mod normal;
pub(crate) mod options;

use crate::{
    app::{LiveData, NetworkViewState, OverlayState, ProcessViewState, RenderState, RuntimeState},
    config,
    dirty::Dirty,
    runner, term, theme,
};

/// The current menu overlay state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MenuState {
    None,
    Main,
    Help,
    Options,
    Filter,
}

impl MenuState {
    /// Whether transitioning from `self` to `to` is a valid menu navigation.
    pub(crate) fn can_transition_to(self, to: MenuState) -> bool {
        matches!(
            (self, to),
            // From normal view
            (MenuState::None, MenuState::Main)
                | (MenuState::None, MenuState::Help)
                | (MenuState::None, MenuState::Options)
                | (MenuState::None, MenuState::Filter)
                // From main menu
                | (MenuState::Main, MenuState::None)
                | (MenuState::Main, MenuState::Help)
                | (MenuState::Main, MenuState::Options)
                // From submenus back to parent
                | (MenuState::Help, MenuState::None)
                | (MenuState::Help, MenuState::Main)
                | (MenuState::Options, MenuState::None)
                | (MenuState::Options, MenuState::Main)
                // From filter back to normal
                | (MenuState::Filter, MenuState::None)
        )
    }
}

/// A terminal write operation produced by a handler.
pub(crate) enum TerminalOp {
    /// Write without terminal sync sequences.
    Raw(String),
    /// Write wrapped in terminal sync sequences (atomic update).
    Synced(String),
}

/// Result returned by input handlers instead of performing side effects.
///
/// Handlers mutate state (dirty flags, config, menu navigation) directly,
/// but never write to the terminal. Instead they return terminal operations
/// for the event loop in app.rs to execute.
pub(crate) struct HandleResult {
    pub(crate) quit: bool,
    pub(crate) ops: Vec<TerminalOp>,
    pub(crate) redraw_overlay: bool,
}

impl HandleResult {
    pub(crate) fn none() -> Self {
        Self {
            quit: false,
            ops: Vec::new(),
            redraw_overlay: false,
        }
    }

    pub(crate) fn quit() -> Self {
        Self {
            quit: true,
            ops: Vec::new(),
            redraw_overlay: false,
        }
    }

    pub(crate) fn raw(output: String) -> Self {
        Self {
            quit: false,
            ops: vec![TerminalOp::Raw(output)],
            redraw_overlay: false,
        }
    }

    pub(crate) fn synced(output: String) -> Self {
        Self {
            quit: false,
            ops: vec![TerminalOp::Synced(output)],
            redraw_overlay: false,
        }
    }

    pub(crate) fn redraw() -> Self {
        Self {
            quit: false,
            ops: Vec::new(),
            redraw_overlay: true,
        }
    }
}

/// Shared mutable state passed to each per-MenuState input handler.
pub(crate) struct InputContext<'a> {
    pub(crate) config: &'a mut config::Config,
    pub(crate) theme: &'a mut theme::Theme,
    pub(crate) live: &'a LiveData,
    pub(crate) manager: &'a runner::CollectorManager,
    pub(crate) runtime: &'a mut RuntimeState,
    pub(crate) render: &'a mut RenderState,
    pub(crate) overlay: &'a mut OverlayState,
    pub(crate) process: &'a mut ProcessViewState,
    pub(crate) network: &'a mut NetworkViewState,
    pub(crate) tw: usize,
    pub(crate) th: usize,
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
}

/// Redraw the underlying UI after closing a menu overlay.
///
/// Clears the screen, renders all boxes, and optionally re-draws the
/// main menu if we're returning to it. Returns the output string.
pub(crate) fn redraw_after_overlay(ctx: &mut InputContext) -> String {
    use crate::app::{RenderParams, render_all};

    let mut out = String::new();
    if let Some(layout) = ctx.render.cached_layout.as_ref() {
        let params = RenderParams {
            dirty: Dirty::ALL_BOXES,
            layout,
            cpu: ctx.live.cpu.as_deref(),
            mem: ctx.live.mem.as_deref(),
            disk: ctx.live.disk.as_deref(),
            net: ctx.live.net.as_deref(),
            gpu: ctx.live.gpu.as_deref(),
            proc_data: ctx.live.proc_data.as_deref(),
            proc_entries: &ctx.process.entries,
            proc_display_procs: ctx.process.display_procs.as_deref(),
            selected_iface: ctx.network.selected_iface.as_str(),
            config: ctx.config,
            theme: ctx.theme,
            rounded: ctx.runtime.rounded,
            update_ms: ctx.runtime.update_ms,
            is_filtering: false,
            core_count: ctx.live.core_count,
            total_mem: ctx.live.total_mem,
            detailed_pid: ctx.process.detailed_pid,
            followed_pid: ctx.process.followed_pid,
            armed_terminate: ctx
                .process
                .armed_terminate
                .as_ref()
                .map(|(_, name, force)| (name.as_str(), *force)),
        };
        out.push_str(term::CLEAR_SCREEN);
        out.push_str(&render_all(
            &params,
            &mut ctx.process.selected,
            &mut ctx.process.start,
        ));
        if ctx.overlay.menu_return_to == MenuState::Main {
            out.push_str(&crate::menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                ctx.overlay.main_menu_selected,
                ctx.theme,
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::MenuState;

    #[test]
    fn valid_transitions_from_none() {
        assert!(MenuState::None.can_transition_to(MenuState::Main));
        assert!(MenuState::None.can_transition_to(MenuState::Help));
        assert!(MenuState::None.can_transition_to(MenuState::Options));
        assert!(MenuState::None.can_transition_to(MenuState::Filter));
    }

    #[test]
    fn valid_transitions_from_main() {
        assert!(MenuState::Main.can_transition_to(MenuState::None));
        assert!(MenuState::Main.can_transition_to(MenuState::Help));
        assert!(MenuState::Main.can_transition_to(MenuState::Options));
    }

    #[test]
    fn valid_transitions_from_submenus() {
        assert!(MenuState::Help.can_transition_to(MenuState::None));
        assert!(MenuState::Help.can_transition_to(MenuState::Main));
        assert!(MenuState::Options.can_transition_to(MenuState::None));
        assert!(MenuState::Options.can_transition_to(MenuState::Main));
    }

    #[test]
    fn valid_transitions_from_filter() {
        assert!(MenuState::Filter.can_transition_to(MenuState::None));
    }

    #[test]
    fn invalid_transitions_rejected() {
        // Filter can only go to None
        assert!(!MenuState::Filter.can_transition_to(MenuState::Main));
        assert!(!MenuState::Filter.can_transition_to(MenuState::Help));
        // Help/Options cannot go to Filter
        assert!(!MenuState::Help.can_transition_to(MenuState::Filter));
        assert!(!MenuState::Options.can_transition_to(MenuState::Filter));
        // Main cannot go to Filter
        assert!(!MenuState::Main.can_transition_to(MenuState::Filter));
        // Identity transitions are not valid (state should change)
        assert!(!MenuState::None.can_transition_to(MenuState::None));
        assert!(!MenuState::Main.can_transition_to(MenuState::Main));
    }
}
