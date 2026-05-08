//! Process-lifecycle helpers: input thread, terminal IO operations,
//! theme styling, and config save-on-exit.
//!
//! These are stateless, IO-adjacent helpers that the event loop
//! invokes at well-defined moments (startup, per-keystroke,
//! shutdown). They are intentionally kept apart from `state.rs` and
//! `dirty_exec.rs` so the data-pipeline modules don't pull in
//! terminal types.

use crate::config;
use crate::event::AppEvent;
use crate::input;
use crate::theme;
use crate::tools;
use crossterm::event::Event;

/// Spawn a thread that blocks on `crossterm::event::read()` and forwards
/// key presses and resize events through the event channel.
///
/// The thread is not joined — it exits when `tx` is dropped (all senders
/// gone) or when the process exits.
pub(crate) fn spawn_input_thread(tx: std::sync::mpsc::Sender<AppEvent>) {
    std::thread::spawn(move || {
        loop {
            match crossterm::event::read() {
                Ok(Event::Key(key)) => {
                    if let Some(k) = input::translate_key(key)
                        && tx.send(AppEvent::Key(k)).is_err()
                    {
                        break;
                    }
                }
                Ok(Event::Resize(_, _)) if tx.send(AppEvent::Resize).is_err() => {
                    break;
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        subsystem = %crate::log::Subsystem::Input,
                        error = %e,
                        "crossterm::event::read failed",
                    );
                }
            }
        }
    });
}

pub(crate) fn style_terminal_output(
    output: &str,
    config: &config::Config,
    theme: &theme::Theme,
) -> String {
    theme.style_output(output, config.ui.theme_background)
}

pub(crate) fn save_config_on_exit(config: &mut config::Config, state: &super::AppState) {
    // Mirror runtime view filter into config so toggle gestures
    // (1-9, 0, Shift+R) survive restart. AppState owns the live
    // filter; Config carries the persisted form.
    config.hidden_widgets = state.filter.hidden;
    // Mirror RuntimeView -> config.view so runtime-toggle state
    // (proc_tree, io_mode, net_iface, etc.) survives restart.
    state.view.sync_to_config(&mut config.view);
    let conf_path = tools::config_dir().join("rtop.toml");
    match config.write(&conf_path) {
        Ok(()) => tracing::info!(
            subsystem = %crate::log::Subsystem::Config,
            path = %conf_path.display(),
            "config saved",
        ),
        Err(e) => tracing::warn!(
            subsystem = %crate::log::Subsystem::Config,
            error = %e,
            path = %conf_path.display(),
            "config save failed",
        ),
    }
}
