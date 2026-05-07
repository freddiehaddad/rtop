//! Application event loop and per-keystroke dispatch.
//!
//! The `app` module owns the orchestration of the running program:
//! it spawns collector threads, drains the event channel, gates the
//! render pipeline, and dispatches input. The actual policy and
//! rendering live in submodules:
//!
//! * [`state`] — `AppState` and its sub-structs.
//! * [`pull`] — drain ready collector slots into `state.live`.
//! * [`render_gates`] — too-small / waiting-for-data screens.
//! * [`dirty_exec`] — pre-render normalisation and the per-frame
//!   ANSI output (`render_all`).
//! * [`lifecycle`] — input thread, terminal IO, save-on-exit.
//!
//! Handlers reach into app state through `crate::app::*`; the
//! `pub(crate) use` re-exports below preserve those import paths.

mod dirty_exec;
mod lifecycle;
mod pull;
mod render_gates;
mod state;

pub(crate) use dirty_exec::{RenderInputs, render_all};
pub(crate) use state::{
    AppState, LiveData, NetworkViewState, OverlayState, ProcessViewState, RenderState,
    RuntimeState, WidgetFilter,
};

use crate::config;
use crate::event::{AppEvent, PerSubsystem};
use crate::handlers::{self, InputContext, MenuState};
use crate::input;
use crate::runner;
use crate::term;
use crate::theme;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalSize {
    pub(crate) width: usize,
    pub(crate) height: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppCommand {
    Continue,
    Quit,
}

/// Run the main event loop: collect data, render UI, and handle input.
///
/// Events arrive from three sources through a single channel:
/// - Input thread: key presses and terminal resize
/// - Collector threads: per-subsystem ready notifications
///
/// The loop blocks on `rx.recv()` (zero CPU when idle), drains all
/// queued events, then renders any dirty widgets in one frame.
pub fn run(config: &mut config::Config, terminal: &mut term::Terminal, theme: &mut theme::Theme) {
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let mut manager =
        runner::CollectorManager::start(config.refresh.update_ms as u64, event_tx.clone());
    lifecycle::spawn_input_thread(event_tx);
    let mut state = AppState::new(config, Instant::now());
    tracing::info!(subsystem = %crate::log::Subsystem::Startup, "ready");

    while let Ok(first) = event_rx.recv() {
        // Drain all queued events to batch work before rendering.
        let mut has_resize = matches!(first, AppEvent::Resize);
        let mut ready = PerSubsystem::<bool>::default();
        let mut keys: Vec<input::Key> = Vec::new();
        match first {
            AppEvent::Resize => {}
            AppEvent::SubsystemReady(kind) => *ready.get_mut(kind) = true,
            AppEvent::Key(k) => keys.push(k),
        }
        for event in std::iter::from_fn(|| event_rx.try_recv().ok()) {
            match event {
                AppEvent::Resize => has_resize = true,
                AppEvent::SubsystemReady(kind) => *ready.get_mut(kind) = true,
                AppEvent::Key(k) => keys.push(k),
            }
        }

        // Process resize before keys — keys may draw overlays that need current dimensions.
        if has_resize {
            let changed = terminal.refresh();
            if changed {
                let (w, h) = terminal.size();
                tracing::debug!(
                    subsystem = %crate::log::Subsystem::Ui,
                    w,
                    h,
                    "terminal resized",
                );
            }
            state.render.mark_resize();
        }
        let size = terminal_size(terminal);

        // Always consume slot data into LiveData regardless of overlay state.
        let render_ui = config.ui.background_update || state.overlay.render_ui();
        pull::pull_subsystem_data(&mut state, config, &manager, render_ui, &ready);

        // Required terminal size for the active layout, derived per
        // frame from the active widget set + hardware-derived hints
        // (core count, gpu count, disk count, swap, temps, watts).
        let min_size = crate::draw::layout::min_terminal_size(&crate::draw::layout::LayoutConfig {
            term_width: size.width,
            term_height: size.height,
            root: config.layout_spec(),
            hints: state.live.layout_hints(config),
            hidden: state.compose_hidden(config),
        });

        // Handle too-small terminal: render message, only accept quit.
        if render_gates::is_too_small(size, min_size) {
            render_gates::render_if_dirty_small(
                &mut state, config, terminal, theme, size, min_size,
            );
            if keys.contains(&input::Key::Char('q')) {
                break;
            }
            continue;
        }

        // Handle waiting for first data: render message, only accept quit.
        if !state.live.is_ready() {
            render_gates::render_if_dirty_waiting(&mut state, config, terminal, theme, size);
            if keys.contains(&input::Key::Char('q')) {
                break;
            }
            continue;
        }

        // Process key events.
        for key in &keys {
            if handle_input_key(key, &mut state, config, terminal, theme, &manager, size)
                == AppCommand::Quit
            {
                tracing::info!(subsystem = %crate::log::Subsystem::Startup, "exiting");
                manager.shutdown();
                lifecycle::save_config_on_exit(config, &state);
                return;
            }
        }

        // Render dirty widgets.
        if state.overlay.render_ui() && !state.render.dirty.is_empty() {
            dirty_exec::execute_dirty_work(&mut state, config, size);
            // If the runtime view filter has hidden every widget in
            // the active layout, the engine produces an empty
            // Layout. Substitute a centered help overlay so the
            // user sees something actionable instead of a blank
            // screen — Shift+R restores everything, and the per-
            // widget toggle keys for the active preset are listed.
            let layout_empty = state
                .render
                .cached_layout
                .as_ref()
                .is_some_and(|l| l.is_empty());
            if layout_empty {
                render_gates::render_if_dirty_all_hidden(&mut state, config, terminal, theme, size);
            } else {
                dirty_exec::write_dirty_frame(&mut state, config, terminal, theme);
            }
        }
    }

    tracing::info!(subsystem = %crate::log::Subsystem::Startup, "exiting");
    manager.shutdown();
    lifecycle::save_config_on_exit(config, &state);
}

fn terminal_size(terminal: &term::Terminal) -> TerminalSize {
    let (width, height) = terminal.size();
    TerminalSize {
        width: width as usize,
        height: height as usize,
    }
}

fn handle_input_key(
    key: &input::Key,
    state: &mut AppState,
    config: &mut config::Config,
    terminal: &mut term::Terminal,
    theme: &mut theme::Theme,
    manager: &runner::CollectorManager,
    size: TerminalSize,
) -> AppCommand {
    let mut ctx = InputContext {
        config,
        theme,
        manager,
        live: &state.live,
        runtime: &mut state.runtime,
        render: &mut state.render,
        overlay: &mut state.overlay,
        process: &mut state.process,
        network: &mut state.network,
        filter: &mut state.filter,
        tw: size.width,
        th: size.height,
    };
    let result = dispatch_handler(key, &mut ctx);
    terminal.set_sync(ctx.config.ui.terminal_sync);
    lifecycle::execute_terminal_ops(terminal, ctx.config, ctx.theme, &result);
    if result.redraw_overlay {
        let out = handlers::redraw_after_overlay(&mut ctx);
        let out = lifecycle::style_terminal_output(&out, ctx.config, ctx.theme);
        if let Err(e) = terminal.write_synced(&out) {
            tracing::warn!(
                subsystem = %crate::log::Subsystem::Terminal,
                error = %e,
                "terminal write failed",
            );
        }
    }

    if result.quit {
        AppCommand::Quit
    } else {
        AppCommand::Continue
    }
}

fn dispatch_handler(key: &input::Key, ctx: &mut InputContext) -> handlers::HandleResult {
    match ctx.overlay.menu_state {
        MenuState::Main => handlers::main_menu::handle(key, ctx),
        MenuState::Help => handlers::help::handle(key, ctx),
        MenuState::Options => handlers::options::handle(key, ctx),
        MenuState::OptionsEdit => handlers::options_edit::handle(key, ctx),
        MenuState::Filter => handlers::filter::handle(key, ctx),
        MenuState::None => handlers::normal::handle(key, ctx),
    }
}
