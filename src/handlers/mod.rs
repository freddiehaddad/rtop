pub(crate) mod filter;
pub(crate) mod help;
pub(crate) mod main_menu;
pub(crate) mod normal;
pub(crate) mod options;

use crate::{config, dirty::Dirty, domain::process::ProcDisplayEntry, draw, runner, theme};

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
    pub(crate) proc_entries: &'a [ProcDisplayEntry],
    pub(crate) worker: &'a runner::CollectionWorker,
    pub(crate) menu_state: &'a mut MenuState,
    pub(crate) dirty: &'a mut Dirty,
    pub(crate) rounded: &'a mut bool,
    pub(crate) update_ms: &'a mut u64,
    pub(crate) main_menu_selected: &'a mut usize,
    pub(crate) options_cat: &'a mut usize,
    pub(crate) options_selected: &'a mut usize,
    pub(crate) options_page: &'a mut usize,
    pub(crate) proc_selected: &'a mut usize,
    pub(crate) proc_start: &'a mut usize,
    pub(crate) selected_iface: &'a mut String,
    pub(crate) filter_text: &'a mut String,
    pub(crate) cached_layout: &'a Option<draw::layout::Layout>,
    /// Where Options/Help was opened from — return here on escape.
    pub(crate) menu_return_to: &'a mut MenuState,
    pub(crate) tw: usize,
    pub(crate) th: usize,
}

impl InputContext<'_> {
    pub(crate) fn selected_proc_pid(&self) -> Option<u32> {
        let snapshot = self.snapshot?;
        self.proc_entries
            .get(*self.proc_selected)
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
    if let (Some(layout), Some(snapshot)) = (ctx.cached_layout.as_ref(), ctx.snapshot) {
        let params = RenderParams {
            dirty: Dirty::ALL_BOXES,
            layout,
            snapshot,
            proc_entries: ctx.proc_entries,
            selected_iface: ctx.selected_iface.as_str(),
            config: ctx.config,
            theme: ctx.theme,
            rounded: *ctx.rounded,
            update_ms: *ctx.update_ms,
            is_filtering: false,
        };
        out.push_str("\x1b[2J");
        out.push_str(&render_all(&params, ctx.proc_selected, ctx.proc_start));
        if *ctx.menu_return_to == MenuState::Main {
            out.push_str(&crate::menu::main_menu::draw_with_selection(
                ctx.tw,
                ctx.th,
                *ctx.main_menu_selected,
                ctx.theme,
            ));
        }
    }
    out
}
