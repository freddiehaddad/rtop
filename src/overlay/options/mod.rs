//! Options overlay subsystem: state, render, and per-key actions.
//!
//! The options overlay is a centered list of every editable
//! configuration option, organized into categories. Selection is
//! tracked by `(cat, page, selected)` indices into the
//! options-menu layout. When the user activates an editable option
//! (Enter on an Int / StringVal), an [`edit::OptionEditState`] is
//! attached and the overlay enters *edit mode*; while editing, the
//! [`OverlayKind::OptionsEdit`] discriminator routes typed-character
//! input to the edit handlers.
//!
//! [`OverlayKind::OptionsEdit`]: super::OverlayKind::OptionsEdit

pub mod edit;
pub mod options_text;
pub mod render;

pub use render::{OptKind, cycle_browsable, opt_key, opt_kind, page_count, select_max, step_int};

use crate::{
    config::{BoolKey, ConfigKey, EnumKey, IntKey, StringKey},
    handlers::InputContext,
    input::Key,
    theme,
};

use super::{ActiveModal, ReturnTarget};
use edit::{EditKind, OptionEditState};

const OPTIONS_CATEGORY_COUNT: usize = render::CATEGORIES.len();

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Persistent state for the options overlay.
#[derive(Debug, Clone)]
pub struct OptionsState {
    cat: usize,
    selected: usize,
    page: usize,
    return_to: ReturnTarget,
    edit: Option<OptionEditState>,
}

impl OptionsState {
    /// Construct fresh options state at the first category, first
    /// page, first row, with no active edit.
    pub fn new(return_to: ReturnTarget) -> Self {
        Self {
            cat: 0,
            selected: 0,
            page: 0,
            return_to,
            edit: None,
        }
    }

    /// Currently-selected category index.
    pub fn cat(&self) -> usize {
        self.cat
    }

    /// Currently-selected row within the current page.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Currently-displayed page within the current category.
    pub fn page(&self) -> usize {
        self.page
    }

    /// Where to return on close.
    pub fn return_to(&self) -> ReturnTarget {
        self.return_to
    }

    /// Borrow the active edit state, if any.
    pub fn edit(&self) -> Option<&OptionEditState> {
        self.edit.as_ref()
    }

    /// Mutably borrow the active edit state, if any.
    pub fn edit_mut(&mut self) -> Option<&mut OptionEditState> {
        self.edit.as_mut()
    }

    /// `true` if the inline edit buffer is active.
    pub fn is_editing(&self) -> bool {
        self.edit.is_some()
    }

    /// Move to next category (wrapping). Resets page + selected to 0.
    pub fn cat_next(&mut self, cat_count: usize) {
        if cat_count == 0 {
            return;
        }
        self.cat = (self.cat + 1) % cat_count;
        self.page = 0;
        self.selected = 0;
    }

    /// Move to previous category (wrapping). Resets page + selected
    /// to 0.
    pub fn cat_prev(&mut self, cat_count: usize) {
        if cat_count == 0 {
            return;
        }
        self.cat = if self.cat == 0 {
            cat_count - 1
        } else {
            self.cat - 1
        };
        self.page = 0;
        self.selected = 0;
    }

    /// Set the category by index. Returns `true` if the category
    /// actually changed (caller can skip a redraw if `false`).
    /// Resets page + selected to 0 on a real change.
    pub fn set_cat(&mut self, cat: usize, cat_count: usize) -> bool {
        let new_cat = if cat_count == 0 { 0 } else { cat % cat_count };
        if new_cat == self.cat {
            return false;
        }
        self.cat = new_cat;
        self.page = 0;
        self.selected = 0;
        true
    }

    /// Set the selected row index. The caller is responsible for
    /// clamping `selected` to the page's selectable range.
    pub fn set_selected(&mut self, selected: usize) {
        self.selected = selected;
    }

    /// Set the page index. The caller is responsible for clamping
    /// `page` to the valid range. Resets selected to 0.
    pub fn set_page(&mut self, page: usize) {
        self.page = page;
        self.selected = 0;
    }

    /// Begin an inline edit with `edit` as the initial state.
    /// Replaces any existing edit (caller should normally only call
    /// this when `is_editing()` is `false`).
    pub fn enter_edit(&mut self, edit: OptionEditState) {
        self.edit = Some(edit);
    }

    /// End the inline edit, returning the discarded buffer.
    pub fn exit_edit(&mut self) -> Option<OptionEditState> {
        self.edit.take()
    }
}

// ---------------------------------------------------------------------------
// Per-action handlers (referenced by handlers/keybinds/table.rs)
// ---------------------------------------------------------------------------

pub(crate) fn quit_action(ctx: &mut InputContext, _: &Key) {
    *ctx.quit = true;
}

pub(crate) fn close_action(ctx: &mut InputContext, _: &Key) {
    ctx.close_overlay();
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "options",
        opened = false,
        "menu transition",
    );
}

pub(crate) fn cat_next_action(ctx: &mut InputContext, _: &Key) {
    if let ActiveModal::Options(s) = &mut ctx.overlay.active {
        s.cat_next(OPTIONS_CATEGORY_COUNT);
        ctx.render.dirty.mark_overlay();
    }
}

pub(crate) fn cat_prev_action(ctx: &mut InputContext, _: &Key) {
    if let ActiveModal::Options(s) = &mut ctx.overlay.active {
        s.cat_prev(OPTIONS_CATEGORY_COUNT);
        ctx.render.dirty.mark_overlay();
    }
}

pub(crate) fn cat_select_hotkey_action(ctx: &mut InputContext, key: &Key) {
    let Key::Char(c) = key else {
        return;
    };
    let Some(new_cat) = render::category_index_for_hotkey(*c) else {
        return;
    };
    if let ActiveModal::Options(s) = &mut ctx.overlay.active
        && s.set_cat(new_cat, OPTIONS_CATEGORY_COUNT)
    {
        ctx.render.dirty.mark_overlay();
    }
}

pub(crate) fn select_up_action(ctx: &mut InputContext, _: &Key) {
    let term_height = ctx.size.height;
    let ActiveModal::Options(s) = &mut ctx.overlay.active else {
        return;
    };
    if s.selected() > 0 {
        s.set_selected(s.selected() - 1);
    } else {
        // Wrap to previous page or last page.
        let pages = page_count(s.cat(), term_height);
        let new_page = if s.page() > 0 {
            s.page() - 1
        } else {
            pages.saturating_sub(1)
        };
        let new_selected = select_max(s.cat(), new_page, term_height);
        s.set_page(new_page);
        s.set_selected(new_selected);
    }
    ctx.render.dirty.mark_overlay();
}

pub(crate) fn select_down_action(ctx: &mut InputContext, _: &Key) {
    let term_height = ctx.size.height;
    let ActiveModal::Options(s) = &mut ctx.overlay.active else {
        return;
    };
    let sm = select_max(s.cat(), s.page(), term_height);
    if s.selected() < sm {
        s.set_selected(s.selected() + 1);
    } else {
        // Wrap to next page or first page.
        let pages = page_count(s.cat(), term_height);
        let new_page = if s.page() < pages.saturating_sub(1) {
            s.page() + 1
        } else if pages > 1 {
            0
        } else {
            s.page()
        };
        s.set_page(new_page);
        s.set_selected(0);
    }
    ctx.render.dirty.mark_overlay();
}

pub(crate) fn page_up_action(ctx: &mut InputContext, _: &Key) {
    let term_height = ctx.size.height;
    let ActiveModal::Options(s) = &mut ctx.overlay.active else {
        return;
    };
    let pages = page_count(s.cat(), term_height);
    if pages > 1 {
        let new_page = if s.page() > 0 {
            s.page() - 1
        } else {
            pages - 1
        };
        s.set_page(new_page);
        ctx.render.dirty.mark_overlay();
    }
}

pub(crate) fn page_down_action(ctx: &mut InputContext, _: &Key) {
    let term_height = ctx.size.height;
    let ActiveModal::Options(s) = &mut ctx.overlay.active else {
        return;
    };
    let pages = page_count(s.cat(), term_height);
    if pages > 1 {
        let new_page = if s.page() < pages - 1 {
            s.page() + 1
        } else {
            0
        };
        s.set_page(new_page);
        ctx.render.dirty.mark_overlay();
    }
}

pub(crate) fn enter_action(ctx: &mut InputContext, _: &Key) {
    // Enter on Int / StringVal opens the inline editor.
    // Enter on Bool / Browsable falls through to the same
    // step-right behaviour as the arrow keys.
    let (cat, page, selected) = match &ctx.overlay.active {
        ActiveModal::Options(s) => (s.cat(), s.page(), s.selected()),
        _ => return,
    };
    let Some(opt_key) = opt_key(cat, page, selected, ctx.size.height) else {
        return;
    };
    let kind = opt_kind(opt_key);
    match kind {
        OptKind::Int => enter_inline_edit(ctx, opt_key, EditKind::Integer),
        OptKind::StringVal => enter_inline_edit(ctx, opt_key, EditKind::Text),
        OptKind::Bool | OptKind::Browsable => {
            step_selected_option(ctx, 1);
        }
    }
}

pub(crate) fn step_left_action(ctx: &mut InputContext, _: &Key) {
    step_selected_option(ctx, -1);
}

pub(crate) fn step_right_action(ctx: &mut InputContext, _: &Key) {
    step_selected_option(ctx, 1);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Step (Bool toggle, Int delta, Browsable cycle, StringVal no-op)
/// the selected option in `dir` direction.
fn step_selected_option(ctx: &mut InputContext, dir: i64) {
    let (cat, page, selected) = match &ctx.overlay.active {
        ActiveModal::Options(s) => (s.cat(), s.page(), s.selected()),
        _ => return,
    };
    if let Some(key) = opt_key(cat, page, selected, ctx.size.height) {
        let kind = opt_kind(key);
        apply_option_change(key, kind, dir, ctx);
    }
    ctx.render.dirty.mark_overlay();
}

/// Open the inline editor on the selected option, seeding the
/// buffer with the current value so the user can backspace to clear
/// or just keep typing.
fn enter_inline_edit(ctx: &mut InputContext, opt_key: ConfigKey, edit_kind: EditKind) {
    let buffer = opt_key.get_display(ctx.config);
    if let ActiveModal::Options(s) = &mut ctx.overlay.active {
        s.enter_edit(OptionEditState::new(opt_key, edit_kind, buffer));
    }
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "option_edit_open",
        option = opt_key.name(),
        "inline editor opened",
    );
    ctx.render.dirty.mark_overlay();
}

/// Apply a config option change (shared by arrow keys and vim h/l).
fn apply_option_change(opt_key: ConfigKey, kind: OptKind, dir: i64, ctx: &mut InputContext) {
    match kind {
        OptKind::Bool => {
            if let ConfigKey::Bool(k) = opt_key {
                k.toggle(ctx.config);
            }
            tracing::info!(
                subsystem = %crate::log::Subsystem::Input,
                action = "option_toggle",
                option = opt_key.name(),
                "option toggled",
            );
        }
        OptKind::Int => {
            step_int(opt_key, ctx.config, dir);
            let value = match opt_key {
                ConfigKey::Int(k) => k.get(ctx.config),
                _ => 0,
            };
            tracing::info!(
                subsystem = %crate::log::Subsystem::Input,
                action = "option_step",
                option = opt_key.name(),
                value,
                "option stepped",
            );
        }
        OptKind::Browsable => {
            cycle_browsable(opt_key, ctx.config, dir as i32);
            tracing::info!(
                subsystem = %crate::log::Subsystem::Input,
                action = "option_cycle",
                option = opt_key.name(),
                "option cycled",
            );
        }
        OptKind::StringVal => {
            // Direct typed entry is the inline-editor's job, not the
            // arrow-step path; nothing to do here.
        }
    }
    apply_post_change_effects(opt_key, ctx);
}

/// Apply live side-effects and dirty flags after `key` has changed.
///
/// Called from both the arrow-step path (`apply_option_change`) and
/// the inline-editor commit path. Centralising the logic guarantees
/// both paths produce the same observable behaviour.
///
/// Live side-effects (collector intervals, theme reload, log level,
/// runtime caches) take effect immediately. Theme/background changes
/// mark the layout dirty so the next frame's `style_terminal_output`
/// prefix re-paints with the new base style and `CLEAR_SCREEN`
/// repaints cells outside widgets.
pub(crate) fn apply_post_change_effects(key: ConfigKey, ctx: &mut InputContext) {
    match key {
        ConfigKey::String(StringKey::ColorTheme) => {
            let name = ctx.config.ui.color_theme.clone();
            *ctx.theme = theme::Theme::from_name(&name);
            ctx.render.dirty.mark_layout();
        }
        ConfigKey::Bool(BoolKey::ThemeBackground) => {
            ctx.render.dirty.mark_layout();
        }
        ConfigKey::Bool(BoolKey::RoundedCorners) => {
            // No runtime mirror to update — `rounded_corners` is
            // read directly from `config.ui.rounded_corners` at
            // every render. The toggle's effect lands on the next
            // frame.
        }
        ConfigKey::Enum(EnumKey::LogLevel) => {
            crate::log::set_level(ctx.config.log.log_level).expect("log level change must succeed");
        }
        ConfigKey::Int(
            IntKey::UpdateMs
            | IntKey::CpuUpdateMs
            | IntKey::MemUpdateMs
            | IntKey::DiskUpdateMs
            | IntKey::NetUpdateMs
            | IntKey::GpuUpdateMs
            | IntKey::ProcUpdateMs,
        ) => {
            // Every refresh-interval edit (global or per-widget)
            // routes through the single resolver in the runner —
            // see [`crate::runner::CollectorManager::apply_refresh`].
            // Sending an unchanged value to a worker is harmless,
            // so the broadcast is unconditional.
            ctx.manager.apply_refresh(&ctx.config.refresh);
        }
        _ => {}
    }

    // Statusbar visibility / format changes alter the bar's
    // contribution to `min_terminal_size` (every visible item
    // contributes width via the `statusbar_*_label_width` fields
    // on `LayoutHints`). `mark_layout` recomputes the layout AND
    // marks every widget dirty, which is exactly what we want
    // here — the row above the statusbar may need to grow or
    // shrink, and the statusbar itself must repaint.
    if matches!(
        key,
        ConfigKey::Bool(BoolKey::ShowStatusbar)
            | ConfigKey::Bool(BoolKey::StatusbarShowMenu)
            | ConfigKey::Bool(BoolKey::StatusbarShowPreset)
            | ConfigKey::Bool(BoolKey::StatusbarShowUpdateInterval)
            | ConfigKey::Bool(BoolKey::StatusbarShowUptime)
            | ConfigKey::Bool(BoolKey::StatusbarShowClock)
            | ConfigKey::String(StringKey::StatusbarClockFormat)
    ) {
        ctx.render.dirty.mark_layout();
    }

    // Sync RuntimeView <- config.view if the change touched a
    // runtime-toggle key.
    if is_view_key(key) {
        ctx.view.sync_from_config(&ctx.config.view);
    }

    if matches!(
        key,
        ConfigKey::String(StringKey::ProcFilter)
            | ConfigKey::Enum(EnumKey::ProcSorting)
            | ConfigKey::Bool(BoolKey::ProcReversed)
            | ConfigKey::Bool(BoolKey::ProcTree)
            | ConfigKey::Bool(BoolKey::ProcAggregate)
            | ConfigKey::Bool(BoolKey::ProcMemBytes)
            | ConfigKey::Bool(BoolKey::ProcGradient)
            | ConfigKey::Bool(BoolKey::ProcColors)
            | ConfigKey::Bool(BoolKey::ProcPerCore)
    ) {
        ctx.render.dirty.mark_proc_data_changed();
    }
}

/// `true` for any [`ConfigKey`] backed by a [`crate::config::ViewConfig`]
/// field.
fn is_view_key(key: ConfigKey) -> bool {
    matches!(
        key,
        ConfigKey::Bool(BoolKey::ProcTree)
            | ConfigKey::Bool(BoolKey::ProcReversed)
            | ConfigKey::Bool(BoolKey::ProcPerCore)
            | ConfigKey::Bool(BoolKey::IoMode)
            | ConfigKey::Bool(BoolKey::NetAuto)
            | ConfigKey::Bool(BoolKey::NetSync)
            | ConfigKey::Enum(EnumKey::ProcSorting)
            | ConfigKey::String(StringKey::ProcFilter)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_at_origin_with_no_edit() {
        let s = OptionsState::new(ReturnTarget::Normal);
        assert_eq!(s.cat(), 0);
        assert_eq!(s.page(), 0);
        assert_eq!(s.selected(), 0);
        assert_eq!(s.return_to(), ReturnTarget::Normal);
        assert!(!s.is_editing());
        assert!(s.edit().is_none());
    }

    #[test]
    fn cat_next_wraps_through_count() {
        let mut s = OptionsState::new(ReturnTarget::Normal);
        s.cat_next(3);
        assert_eq!(s.cat(), 1);
        s.cat_next(3);
        assert_eq!(s.cat(), 2);
        s.cat_next(3);
        assert_eq!(s.cat(), 0, "must wrap");
    }

    #[test]
    fn cat_prev_wraps_through_count() {
        let mut s = OptionsState::new(ReturnTarget::Normal);
        s.cat_prev(3);
        assert_eq!(s.cat(), 2);
        s.cat_prev(3);
        assert_eq!(s.cat(), 1);
    }

    #[test]
    fn cat_navigation_resets_page_and_selected() {
        let mut s = OptionsState::new(ReturnTarget::Normal);
        s.set_page(2);
        s.set_selected(5);
        s.cat_next(3);
        assert_eq!(s.page(), 0);
        assert_eq!(s.selected(), 0);
    }

    #[test]
    fn set_cat_returns_true_only_when_cat_changes() {
        let mut s = OptionsState::new(ReturnTarget::Normal);
        s.set_selected(3);
        assert!(!s.set_cat(0, 3));
        assert_eq!(s.selected(), 3);
        assert!(s.set_cat(2, 3));
        assert_eq!(s.cat(), 2);
        assert_eq!(s.selected(), 0);
    }

    #[test]
    fn enter_and_exit_edit_round_trip() {
        let mut s = OptionsState::new(ReturnTarget::Normal);
        s.enter_edit(OptionEditState::placeholder());
        assert!(s.is_editing());
        let returned = s.exit_edit();
        assert!(returned.is_some());
        assert!(!s.is_editing());
    }

    #[test]
    fn edit_mut_allows_mutation_through_options_state() {
        let mut s = OptionsState::new(ReturnTarget::Normal);
        s.enter_edit(OptionEditState::placeholder());
        s.edit_mut().expect("just entered").insert_char('x');
        assert_eq!(s.edit().expect("still editing").buffer(), "x");
    }

    // ── Hotkey-jump dispatch ────────────────────────────────────
    //
    // `cat_select_hotkey_action` requires a full `InputContext`
    // (config, theme, manager, live data, …) which is awkward to
    // construct in a unit test. Instead, exercise the equivalent
    // logic directly: the handler does
    //     `set_cat(category_index_for_hotkey(c)?, COUNT)`
    // so a test that pins both pieces is equivalent in coverage
    // and orders of magnitude lighter than building an
    // InputContext.

    #[test]
    fn category_index_for_hotkey_resolves_each_declared_letter() {
        for (expected_idx, cat_def) in render::CATEGORIES.iter().enumerate() {
            let resolved = render::category_index_for_hotkey(cat_def.hotkey);
            assert_eq!(
                resolved,
                Some(expected_idx),
                "hotkey {:?} for {:?} did not resolve to its index",
                cat_def.hotkey,
                cat_def.name,
            );
        }
    }

    #[test]
    fn category_index_for_hotkey_returns_none_for_unbound_letter() {
        // Defensive: pick a letter no category claims.
        let bound: Vec<char> = render::CATEGORIES.iter().map(|c| c.hotkey).collect();
        let unbound = ('a'..='z')
            .find(|c| !bound.contains(c))
            .expect("not all letters can be bound");
        assert!(render::category_index_for_hotkey(unbound).is_none());
    }

    #[test]
    fn set_cat_with_each_hotkey_jumps_to_the_matching_category() {
        // Pin the end-to-end behaviour the handler implements:
        // `category_index_for_hotkey(letter)` then `set_cat`
        // moves the OptionsState to the matching index.
        for (expected_idx, cat_def) in render::CATEGORIES.iter().enumerate() {
            let mut s = OptionsState::new(ReturnTarget::Normal);
            // Move somewhere else first so the assertion below
            // requires the jump to actually have happened.
            s.set_cat(
                (expected_idx + 1) % render::CATEGORIES.len(),
                render::CATEGORIES.len(),
            );
            let idx =
                render::category_index_for_hotkey(cat_def.hotkey).expect("hotkey is declared");
            s.set_cat(idx, render::CATEGORIES.len());
            assert_eq!(s.cat(), expected_idx);
        }
    }
}
