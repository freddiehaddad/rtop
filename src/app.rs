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

pub(crate) use dirty_exec::RenderParams;
pub(crate) use state::{
    AppState, GpuViewState, LiveData, NetworkViewState, OverlayState, ProcessViewState,
    RenderState, RuntimeView, WidgetFilter,
};

use crate::config;
use crate::event::{AppEvent, PerSubsystem};
use crate::handlers::{self, InputContext};
use crate::input;
use crate::runner;
use crate::term;
use crate::theme;

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
    // Resolve the GPU interval through `effective_interval` here so
    // the spawn site sees an already-resolved value and never
    // re-implements the "0 = inherit global" rule. The cycling-GPU
    // widget shares one interval across every detected device, so
    // every GPU thread starts with the same value.
    let gpu_update_ms = config.effective_interval(config.refresh.gpu_update_ms);
    let mut manager = runner::CollectorManager::start(
        config.refresh.update_ms as u64,
        event_tx.clone(),
        gpu_update_ms,
    );
    lifecycle::spawn_input_thread(event_tx);
    let mut state = AppState::new(config, manager.gpu_count());
    tracing::info!(subsystem = %crate::log::Subsystem::Startup, "ready");

    let mut ready = PerSubsystem::<bool>::with_default(manager.gpu_count());
    while let Ok(first) = event_rx.recv() {
        // Drain all queued events to batch work before rendering.
        let mut has_resize = matches!(first, AppEvent::Resize);
        // Reset the per-cycle ready bitmap in place so the GPU
        // `Vec<bool>` doesn't reallocate every iteration.
        ready.reset();
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
            root: config.layout_spec().clone(),
            hints: state.live.layout_hints(config, &state.view, &state.filter),
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

        // Render dirty widgets / overlay.
        //
        // The render gate now always runs when something is
        // dirty, regardless of overlay state. The new compose
        // path in `dirty_exec::write_dirty_frame` handles the
        // overlay case: when a centered modal is active it
        // composes a dimmed widget snapshot + the modal layer
        // into one atomic frame; otherwise it takes the
        // pre-existing widget-only path.
        if !state.render.dirty.is_empty() {
            dirty_exec::execute_dirty_work(&mut state, config, size);
            // If the runtime view filter has hidden every widget in
            // the active layout, the engine produces an empty
            // Layout. Substitute a centered help overlay so the
            // user sees something actionable instead of a blank
            // screen — Shift+R restores everything, and the per-
            // widget toggle keys for the active preset are listed.
            //
            // This gate only applies when no centered modal is
            // active (otherwise we want the modal-with-dim
            // composition).
            let layout_empty = state.render.cached_layout().is_some_and(|l| l.is_empty());
            if layout_empty && state.overlay.render_ui() {
                render_gates::render_if_dirty_all_hidden(&mut state, config, terminal, theme, size);
            } else {
                dirty_exec::write_dirty_frame(&mut state, config, terminal, theme, size);
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
    let mut quit = false;
    {
        let mut ctx = InputContext {
            config,
            theme,
            manager,
            live: &state.live,
            view: &mut state.view,
            render: &mut state.render,
            overlay: &mut state.overlay,
            process: &mut state.process,
            network: &mut state.network,
            gpu: &mut state.gpu,
            filter: &mut state.filter,
            size,
            quit: &mut quit,
        };
        handlers::keybinds::dispatch(key, &mut ctx);
    }
    terminal.set_sync(config.ui.terminal_sync);

    if quit {
        AppCommand::Quit
    } else {
        AppCommand::Continue
    }
}
