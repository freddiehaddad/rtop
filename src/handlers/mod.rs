pub(crate) mod filter;
pub(crate) mod help;
pub(crate) mod main_menu;
pub(crate) mod normal;
pub(crate) mod options;

use crate::{
    app::{NetworkViewState, OverlayState, ProcessViewState, RenderState, RuntimeState},
    config,
    dirty::Dirty,
    runner, theme,
};

/// The current menu overlay state.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum MenuState {
    None,
    Main,
    Help,
    Options,
    Filter,
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
    pub(crate) snapshot: Option<&'a runner::CollectionSnapshot>,
    pub(crate) worker: &'a runner::CollectionWorker,
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
        let snapshot = self.snapshot?;
        self.process
            .entries
            .get(self.process.selected)
            .and_then(|entry| snapshot.proc_data.procs.get(entry.proc_index))
            .map(|proc| proc.pid)
    }
}

/// Redraw the underlying UI after closing a menu overlay.
///
/// Clears the screen, renders all boxes, and optionally re-draws the
/// main menu if we're returning to it. Returns the output string.
pub(crate) fn redraw_after_overlay(ctx: &mut InputContext) -> String {
    use crate::app::{RenderParams, render_all};

    let mut out = String::new();
    if let (Some(layout), Some(snapshot)) = (ctx.render.cached_layout.as_ref(), ctx.snapshot) {
        let params = RenderParams {
            dirty: Dirty::ALL_BOXES,
            layout,
            snapshot,
            proc_entries: &ctx.process.entries,
            proc_cpu_histories: &ctx.process.cpu_histories,
            selected_iface: ctx.network.selected_iface.as_str(),
            config: ctx.config,
            theme: ctx.theme,
            rounded: ctx.runtime.rounded,
            update_ms: ctx.runtime.update_ms,
            is_filtering: false,
        };
        out.push_str("\x1b[2J");
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
