//! The single source of truth for every keybind in the application.
//!
//! Adding a binding here automatically:
//! * routes the matching key event to the action,
//! * (if `help: Some(_)`) adds the binding to the help menu in the
//!   declared category, in declaration order.
//!
//! ## Authoring rules
//!
//! * One binding per logical action, even if it has multiple
//!   triggers — collapse `Up`/`vim k`/`PgUp` into a single
//!   `&[KeySpec]`.
//! * `keys` is the trigger list. Use `KeySpec::VimOnly` to gate
//!   the trigger on `ui.vim_keys`.
//! * `states` is the menu states the binding is active in.
//! * `help` is hand-curated. The `keys` display string is for
//!   humans; the `category` groups bindings in the help menu.
//!   Set `help: None` for menu-internal commands (Tab cycle,
//!   Esc close, inline-edit cursor moves) that don't belong in
//!   user-facing help.
//! * **Never** bind a printable `Key::Char(_)` or `Key::Space` in
//!   [`MenuState::Filter`] or [`MenuState::OptionsEdit`] — those
//!   states' fallback (`fallback_typed_char`) consumes typed
//!   characters into the buffer. The validation test
//!   `text_states_have_no_printable_bindings` enforces this.

use crate::handlers::normal;
use crate::input::Key;
use crate::overlay::{
    OverlayKind, filter, help, main_menu,
    options::{self, edit as options_edit},
};

use super::{Binding, HelpEntry, KeySpec, Prehook, prehooks};

const NORMAL: &[OverlayKind] = &[OverlayKind::None];
const MAIN: &[OverlayKind] = &[OverlayKind::Main];
const HELP: &[OverlayKind] = &[OverlayKind::Help];
const OPTIONS: &[OverlayKind] = &[OverlayKind::Options];
const OPTIONS_EDIT: &[OverlayKind] = &[OverlayKind::OptionsEdit];
const FILTER: &[OverlayKind] = &[OverlayKind::Filter];

const NORMAL_OR_MAIN: &[OverlayKind] = &[OverlayKind::None, OverlayKind::Main];

pub(crate) static PREHOOKS: &[Prehook] = &[
    Prehook {
        states: NORMAL,
        hook: prehooks::armed_terminate_disarm,
    },
    Prehook {
        states: NORMAL,
        hook: prehooks::cancel_follow_on_nav,
    },
];

#[rustfmt::skip]
pub(crate) static BINDINGS: &[Binding] = &[
    // -----------------------------------------------------------------
    // Global (Normal mode + Main menu where shared)
    // -----------------------------------------------------------------
    Binding {
        keys: &[KeySpec::Always(Key::Char('q'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Global", keys: "q / Ctrl+C", description: "Quit" }),
        action: normal::quit_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Escape), KeySpec::Always(Key::Char('m'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Global", keys: "m / Esc", description: "Toggle main menu" }),
        action: normal::open_main_menu_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('?')), KeySpec::Always(Key::F(1))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Global", keys: "? / F1", description: "Toggle help" }),
        action: normal::open_help_menu_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('o')), KeySpec::Always(Key::F(2))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Global", keys: "o / F2", description: "Toggle options" }),
        action: normal::open_options_menu_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('p'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Global", keys: "p / Shift+P", description: "Cycle presets forward/back" }),
        action: normal::preset_forward_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('P'))],
        states: NORMAL,
        help: None,
        action: normal::preset_back_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::CtrlR)],
        states: NORMAL,
        help: Some(HelpEntry { category: "Global", keys: "Ctrl+R", description: "Reload config" }),
        action: normal::config_reload_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('+'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Global", keys: "+ / -", description: "Adjust update speed" }),
        action: normal::update_rate_up_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('-'))],
        states: NORMAL,
        help: None,
        action: normal::update_rate_down_action,
    },
    Binding {
        keys: &[
            KeySpec::Always(Key::Char('1')),
            KeySpec::Always(Key::Char('2')),
            KeySpec::Always(Key::Char('3')),
            KeySpec::Always(Key::Char('4')),
            KeySpec::Always(Key::Char('5')),
        ],
        states: NORMAL,
        help: Some(HelpEntry { category: "Global", keys: "1-5", description: "Toggle widget (cpu/mem/net/proc/disk)" }),
        action: normal::toggle_widget_main_action,
    },
    Binding {
        keys: &[
            KeySpec::Always(Key::Char('6')),
            KeySpec::Always(Key::Char('7')),
            KeySpec::Always(Key::Char('8')),
            KeySpec::Always(Key::Char('9')),
        ],
        states: NORMAL,
        help: Some(HelpEntry { category: "Global", keys: "6-9", description: "Toggle GPU 0-3" }),
        action: normal::toggle_widget_gpu_low_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('0'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Global", keys: "0", description: "Toggle GPU 4-7" }),
        action: normal::toggle_widget_gpu_high_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('R'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Global", keys: "Shift+R", description: "Restore all hidden widgets" }),
        action: normal::restore_widgets_action,
    },

    // -----------------------------------------------------------------
    // Process navigation
    // -----------------------------------------------------------------
    Binding {
        keys: &[KeySpec::Always(Key::Up), KeySpec::VimOnly(Key::Char('k'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "Up / Down", description: "Select process" }),
        action: normal::nav_up_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Down), KeySpec::VimOnly(Key::Char('j'))],
        states: NORMAL,
        help: None,
        action: normal::nav_down_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::PageUp)],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "PgUp / PgDn", description: "Page through processes" }),
        action: normal::nav_page_up_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::PageDown)],
        states: NORMAL,
        help: None,
        action: normal::nav_page_down_action,
    },
    Binding {
        keys: &[KeySpec::VimOnly(Key::CtrlB)],
        states: NORMAL,
        help: None,
        action: normal::nav_page_up_action,
    },
    Binding {
        keys: &[KeySpec::VimOnly(Key::CtrlF)],
        states: NORMAL,
        help: None,
        action: normal::nav_page_down_action,
    },
    Binding {
        keys: &[KeySpec::VimOnly(Key::CtrlD)],
        states: NORMAL,
        help: None,
        action: normal::nav_half_page_down_action,
    },
    Binding {
        keys: &[KeySpec::VimOnly(Key::CtrlU)],
        states: NORMAL,
        help: None,
        action: normal::nav_half_page_up_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Home), KeySpec::VimOnly(Key::Char('g'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "Home / End", description: "Jump to first/last" }),
        action: normal::nav_home_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::End), KeySpec::VimOnly(Key::Char('G'))],
        states: NORMAL,
        help: None,
        action: normal::nav_end_action,
    },

    // -----------------------------------------------------------------
    // Process modes, sorting, actions
    // -----------------------------------------------------------------
    Binding {
        keys: &[KeySpec::Always(Key::Left)],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "Left / Right", description: "Cycle sort column" }),
        action: normal::sort_back_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Right)],
        states: NORMAL,
        help: None,
        action: normal::sort_forward_action,
    },
    Binding {
        keys: &[KeySpec::VimOnly(Key::Char('h'))],
        states: NORMAL,
        help: None,
        action: normal::sort_back_action,
    },
    Binding {
        keys: &[KeySpec::VimOnly(Key::Char('l'))],
        states: NORMAL,
        help: None,
        action: normal::sort_forward_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('r'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "r", description: "Toggle reverse sort" }),
        action: normal::toggle_reverse_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('f')), KeySpec::Always(Key::Char('/'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "f / /", description: "Filter processes" }),
        action: normal::open_filter_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('e'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "e", description: "Toggle tree view" }),
        action: normal::toggle_tree_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('c'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "c", description: "Toggle per-core CPU" }),
        action: normal::toggle_per_core_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('t'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "t", description: "Terminate process (graceful, double-tap)" }),
        action: normal::terminate_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('T'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "Shift+T", description: "Kill process (force, double-tap)" }),
        action: normal::kill_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Enter)],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "Enter", description: "Show/hide process details" }),
        action: normal::detail_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('F'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Process", keys: "Shift+F", description: "Follow/unfollow process" }),
        action: normal::follow_action,
    },

    // -----------------------------------------------------------------
    // Disk
    // -----------------------------------------------------------------
    Binding {
        keys: &[KeySpec::Always(Key::Char('i'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Disk", keys: "i", description: "Toggle disk IO mode" }),
        action: normal::toggle_io_action,
    },

    // -----------------------------------------------------------------
    // Network
    // -----------------------------------------------------------------
    Binding {
        keys: &[KeySpec::Always(Key::Char('b'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Network", keys: "n / b", description: "Cycle network interfaces" }),
        action: normal::iface_back_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('n'))],
        states: NORMAL,
        help: None,
        action: normal::iface_forward_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('a'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Network", keys: "a", description: "Toggle network auto scale" }),
        action: normal::net_auto_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('s'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Network", keys: "s", description: "Toggle network sync scale" }),
        action: normal::net_sync_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('z'))],
        states: NORMAL,
        help: Some(HelpEntry { category: "Network", keys: "z", description: "Reset network totals" }),
        action: normal::net_zero_action,
    },

    // -----------------------------------------------------------------
    // Main menu (overlay-internal; not surfaced in help)
    // -----------------------------------------------------------------
    Binding {
        keys: &[KeySpec::Always(Key::Char('q'))],
        states: MAIN,
        help: None,
        action: main_menu::quit_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Escape), KeySpec::Always(Key::Char('m'))],
        states: MAIN,
        help: None,
        action: main_menu::close_action,
    },
    Binding {
        keys: &[
            KeySpec::Always(Key::Up),
            KeySpec::Always(Key::ShiftTab),
            KeySpec::VimOnly(Key::Char('k')),
        ],
        states: MAIN,
        help: None,
        action: main_menu::select_prev_action,
    },
    Binding {
        keys: &[
            KeySpec::Always(Key::Down),
            KeySpec::Always(Key::Tab),
            KeySpec::VimOnly(Key::Char('j')),
        ],
        states: MAIN,
        help: None,
        action: main_menu::select_next_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Enter), KeySpec::Always(Key::Space)],
        states: MAIN,
        help: None,
        action: main_menu::activate_selected_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('o')), KeySpec::Always(Key::F(2))],
        states: MAIN,
        help: None,
        action: main_menu::open_options_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Char('?')), KeySpec::Always(Key::F(1))],
        states: MAIN,
        help: None,
        action: main_menu::open_help_action,
    },

    // -----------------------------------------------------------------
    // Help overlay (close-only)
    // -----------------------------------------------------------------
    Binding {
        keys: &[KeySpec::Always(Key::Char('q'))],
        states: HELP,
        help: None,
        action: normal::quit_action,
    },
    Binding {
        keys: &[
            KeySpec::Always(Key::Escape),
            KeySpec::Always(Key::Char('?')),
            KeySpec::Always(Key::F(1)),
        ],
        states: HELP,
        help: None,
        action: help::close_action,
    },

    // -----------------------------------------------------------------
    // Options overlay
    // -----------------------------------------------------------------
    Binding {
        keys: &[KeySpec::Always(Key::Char('q'))],
        states: OPTIONS,
        help: None,
        action: options::quit_action,
    },
    Binding {
        keys: &[
            KeySpec::Always(Key::Escape),
            KeySpec::Always(Key::Backspace),
            KeySpec::Always(Key::Char('o')),
            KeySpec::Always(Key::F(2)),
        ],
        states: OPTIONS,
        help: None,
        action: options::close_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Tab)],
        states: OPTIONS,
        help: None,
        action: options::cat_next_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::ShiftTab)],
        states: OPTIONS,
        help: None,
        action: options::cat_prev_action,
    },
    Binding {
        keys: &[
            KeySpec::Always(Key::Char('0')),
            KeySpec::Always(Key::Char('1')),
            KeySpec::Always(Key::Char('2')),
            KeySpec::Always(Key::Char('3')),
            KeySpec::Always(Key::Char('4')),
            KeySpec::Always(Key::Char('5')),
            KeySpec::Always(Key::Char('6')),
        ],
        states: OPTIONS,
        help: None,
        action: options::cat_select_digit_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Up), KeySpec::VimOnly(Key::Char('k'))],
        states: OPTIONS,
        help: None,
        action: options::select_up_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Down), KeySpec::VimOnly(Key::Char('j'))],
        states: OPTIONS,
        help: None,
        action: options::select_down_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::PageUp)],
        states: OPTIONS,
        help: None,
        action: options::page_up_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::PageDown)],
        states: OPTIONS,
        help: None,
        action: options::page_down_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Enter)],
        states: OPTIONS,
        help: None,
        action: options::enter_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Left), KeySpec::VimOnly(Key::Char('h'))],
        states: OPTIONS,
        help: None,
        action: options::step_left_action,
    },
    Binding {
        keys: &[
            KeySpec::Always(Key::Right),
            KeySpec::Always(Key::Space),
            KeySpec::VimOnly(Key::Char('l')),
        ],
        states: OPTIONS,
        help: None,
        action: options::step_right_action,
    },

    // -----------------------------------------------------------------
    // Options inline editor (text-input state — text falls through to
    // `fallback_typed_char`; only command keys are bound here)
    // -----------------------------------------------------------------
    Binding {
        keys: &[KeySpec::Always(Key::Escape)],
        states: OPTIONS_EDIT,
        help: None,
        action: options_edit::cancel_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Enter)],
        states: OPTIONS_EDIT,
        help: None,
        action: options_edit::commit_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Backspace)],
        states: OPTIONS_EDIT,
        help: None,
        action: options_edit::backspace_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Delete)],
        states: OPTIONS_EDIT,
        help: None,
        action: options_edit::delete_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Left)],
        states: OPTIONS_EDIT,
        help: None,
        action: options_edit::move_left_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Right)],
        states: OPTIONS_EDIT,
        help: None,
        action: options_edit::move_right_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Home)],
        states: OPTIONS_EDIT,
        help: None,
        action: options_edit::move_home_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::End)],
        states: OPTIONS_EDIT,
        help: None,
        action: options_edit::move_end_action,
    },

    // -----------------------------------------------------------------
    // Process filter (text-input state — typed chars fall through to
    // `fallback_typed_char`; only command keys are bound here)
    // -----------------------------------------------------------------
    Binding {
        keys: &[KeySpec::Always(Key::Escape)],
        states: FILTER,
        help: None,
        action: filter::cancel_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Enter)],
        states: FILTER,
        help: None,
        action: filter::commit_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Backspace)],
        states: FILTER,
        help: None,
        action: filter::backspace_action,
    },
    Binding {
        keys: &[KeySpec::Always(Key::Delete)],
        states: FILTER,
        help: None,
        action: filter::delete_clear_action,
    },
];

// -----------------------------------------------------------------
// Keep the unused-import linter quiet for the convenience constants
// -----------------------------------------------------------------
const _: &[OverlayKind] = NORMAL_OR_MAIN;
