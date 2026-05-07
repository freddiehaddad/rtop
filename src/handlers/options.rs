//! Per-action handlers for the options overlay and the helpers
//! shared with the inline-editor overlay
//! (`apply_post_change_effects`).

use crate::{
    config::{BoolKey, ConfigKey, EnumKey, IntKey, StringKey},
    dirty::Dirty,
    handlers::normal::sync_all_intervals,
    handlers::options_edit::{EditKind, OptionEditState},
    handlers::{HandleResult, InputContext, MenuState, TerminalOp},
    input::Key,
    menu, theme,
};

const OPTIONS_CATEGORY_COUNT: usize = 7;

// ---------------------------------------------------------------------------
// Per-action handlers (referenced by handlers/keybinds/table.rs)
// ---------------------------------------------------------------------------

pub(super) fn quit_action(_ctx: &mut InputContext, _key: &Key) -> HandleResult {
    HandleResult::quit()
}

pub(super) fn close_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let return_to = ctx.overlay.menu_return_to;
    ctx.overlay.set_menu_state(return_to);
    tracing::debug!(
        subsystem = %crate::log::Subsystem::Ui,
        menu = "options",
        opened = false,
        "menu transition",
    );
    if return_to == MenuState::None {
        ctx.render.dirty |= Dirty::LAYOUT | Dirty::ALL_WIDGETS;
    }
    HandleResult::redraw()
}

pub(super) fn cat_next_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.overlay.options_cat = (ctx.overlay.options_cat + 1) % OPTIONS_CATEGORY_COUNT;
    ctx.overlay.options_page = 0;
    ctx.overlay.options_selected = 0;
    options_menu_output(ctx)
}

pub(super) fn cat_prev_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    ctx.overlay.options_cat = if ctx.overlay.options_cat == 0 {
        OPTIONS_CATEGORY_COUNT - 1
    } else {
        ctx.overlay.options_cat - 1
    };
    ctx.overlay.options_page = 0;
    ctx.overlay.options_selected = 0;
    options_menu_output(ctx)
}

pub(super) fn cat_select_digit_action(ctx: &mut InputContext, key: &Key) -> HandleResult {
    let Key::Char(c) = key else {
        return HandleResult::none();
    };
    let new_cat = (*c as usize) - ('0' as usize);
    if new_cat >= OPTIONS_CATEGORY_COUNT {
        return HandleResult::none();
    }
    if new_cat != ctx.overlay.options_cat {
        ctx.overlay.options_cat = new_cat;
        ctx.overlay.options_page = 0;
        ctx.overlay.options_selected = 0;
    }
    options_menu_output(ctx)
}

pub(super) fn select_up_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    if ctx.overlay.options_selected > 0 {
        ctx.overlay.options_selected -= 1;
    } else {
        // wrap to previous page or last page
        let pages = menu::options_menu::page_count(ctx.overlay.options_cat, ctx.th);
        if ctx.overlay.options_page > 0 {
            ctx.overlay.options_page -= 1;
        } else if pages > 1 {
            ctx.overlay.options_page = pages - 1;
        }
        ctx.overlay.options_selected = menu::options_menu::select_max(
            ctx.overlay.options_cat,
            ctx.overlay.options_page,
            ctx.th,
        );
    }
    options_menu_output(ctx)
}

pub(super) fn select_down_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let sm =
        menu::options_menu::select_max(ctx.overlay.options_cat, ctx.overlay.options_page, ctx.th);
    if ctx.overlay.options_selected < sm {
        ctx.overlay.options_selected += 1;
    } else {
        // wrap to next page or first page
        let pages = menu::options_menu::page_count(ctx.overlay.options_cat, ctx.th);
        if ctx.overlay.options_page < pages - 1 {
            ctx.overlay.options_page += 1;
        } else if pages > 1 {
            ctx.overlay.options_page = 0;
        }
        ctx.overlay.options_selected = 0;
    }
    options_menu_output(ctx)
}

pub(super) fn page_up_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let pages = menu::options_menu::page_count(ctx.overlay.options_cat, ctx.th);
    if pages > 1 {
        ctx.overlay.options_page = if ctx.overlay.options_page > 0 {
            ctx.overlay.options_page - 1
        } else {
            pages - 1
        };
        ctx.overlay.options_selected = 0;
    }
    options_menu_output(ctx)
}

pub(super) fn page_down_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    let pages = menu::options_menu::page_count(ctx.overlay.options_cat, ctx.th);
    if pages > 1 {
        ctx.overlay.options_page = if ctx.overlay.options_page < pages - 1 {
            ctx.overlay.options_page + 1
        } else {
            0
        };
        ctx.overlay.options_selected = 0;
    }
    options_menu_output(ctx)
}

pub(super) fn enter_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    // Enter on Int / StringVal opens the inline editor.
    // Enter on Bool / Browsable falls through to the same
    // step-right behaviour as the arrow keys.
    let Some(opt_key) = menu::options_menu::opt_key(
        ctx.overlay.options_cat,
        ctx.overlay.options_page,
        ctx.overlay.options_selected,
        ctx.th,
    ) else {
        return HandleResult::none();
    };
    let kind = menu::options_menu::opt_kind(opt_key, ctx.config);
    match kind {
        menu::options_menu::OptKind::Int => enter_inline_edit(ctx, opt_key, EditKind::Integer),
        menu::options_menu::OptKind::StringVal => enter_inline_edit(ctx, opt_key, EditKind::Text),
        menu::options_menu::OptKind::Bool | menu::options_menu::OptKind::Browsable => {
            step_selected_option(ctx, 1)
        }
    }
}

pub(super) fn step_left_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    step_selected_option(ctx, -1)
}

pub(super) fn step_right_action(ctx: &mut InputContext, _key: &Key) -> HandleResult {
    step_selected_option(ctx, 1)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a HandleResult that redraws the options menu overlay.
fn options_menu_output(ctx: &mut InputContext) -> HandleResult {
    let menu_out = menu::options_menu::draw(&menu::options_menu::DrawParams {
        term_width: ctx.tw,
        term_height: ctx.th,
        cat: ctx.overlay.options_cat,
        selected: ctx.overlay.options_selected,
        page: ctx.overlay.options_page,
        config: ctx.config,
        theme: ctx.theme,
        option_edit: ctx.overlay.option_edit(),
    });
    HandleResult::synced(menu_out)
}

/// Step (Bool toggle, Int delta, Browsable cycle, StringVal no-op)
/// the selected option in `dir` direction. Always re-renders the
/// menu so the new value is visible.
fn step_selected_option(ctx: &mut InputContext, dir: i64) -> HandleResult {
    let mut extra_ops: Vec<TerminalOp> = Vec::new();

    if let Some(opt_key) = menu::options_menu::opt_key(
        ctx.overlay.options_cat,
        ctx.overlay.options_page,
        ctx.overlay.options_selected,
        ctx.th,
    ) {
        let kind = menu::options_menu::opt_kind(opt_key, ctx.config);
        apply_option_change(opt_key, kind, dir, ctx, &mut extra_ops);
    }

    let menu_out = menu::options_menu::draw(&menu::options_menu::DrawParams {
        term_width: ctx.tw,
        term_height: ctx.th,
        cat: ctx.overlay.options_cat,
        selected: ctx.overlay.options_selected,
        page: ctx.overlay.options_page,
        config: ctx.config,
        theme: ctx.theme,
        option_edit: ctx.overlay.option_edit(),
    });
    extra_ops.push(TerminalOp::Synced(menu_out));
    HandleResult {
        quit: false,
        ops: extra_ops,
        redraw_overlay: false,
    }
}

/// Open the inline editor on the selected option, seeding the
/// buffer with the current value so the user can backspace to clear
/// or just keep typing.
fn enter_inline_edit(
    ctx: &mut InputContext,
    opt_key: ConfigKey,
    edit_kind: EditKind,
) -> HandleResult {
    let buffer = opt_key.get_display(ctx.config);
    ctx.overlay
        .enter_option_edit(OptionEditState::new(opt_key, edit_kind, buffer));
    tracing::info!(
        subsystem = %crate::log::Subsystem::Input,
        action = "option_edit_open",
        option = opt_key.name(),
        "inline editor opened",
    );
    let menu_out = menu::options_menu::draw(&menu::options_menu::DrawParams {
        term_width: ctx.tw,
        term_height: ctx.th,
        cat: ctx.overlay.options_cat,
        selected: ctx.overlay.options_selected,
        page: ctx.overlay.options_page,
        config: ctx.config,
        theme: ctx.theme,
        option_edit: ctx.overlay.option_edit(),
    });
    HandleResult::synced(menu_out)
}

/// Apply a config option change (shared by arrow keys and vim h/l).
fn apply_option_change(
    opt_key: ConfigKey,
    kind: menu::options_menu::OptKind,
    dir: i64,
    ctx: &mut InputContext,
    extra_ops: &mut Vec<TerminalOp>,
) {
    match kind {
        menu::options_menu::OptKind::Bool => {
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
        menu::options_menu::OptKind::Int => {
            menu::options_menu::step_int(opt_key, ctx.config, dir);
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
        menu::options_menu::OptKind::Browsable => {
            menu::options_menu::cycle_browsable(opt_key, ctx.config, dir as i32);
            tracing::info!(
                subsystem = %crate::log::Subsystem::Input,
                action = "option_cycle",
                option = opt_key.name(),
                "option cycled",
            );
        }
        menu::options_menu::OptKind::StringVal => {
            // Direct typed entry is the inline-editor's job, not the
            // arrow-step path; nothing to do here.
        }
    }
    apply_post_change_effects(opt_key, ctx, extra_ops);
}

/// Apply live side-effects and dirty flags after `key` has changed.
///
/// Called from both the arrow-step path (`apply_option_change`) and
/// the inline-editor commit path. Centralising the logic guarantees
/// both paths produce the same observable behaviour.
///
/// Live side-effects (collector intervals, theme reload, log level,
/// runtime caches, terminal base-style writes) take effect immediately.
///
/// Dirty flags: the menu-close handler already sets
/// `Dirty::LAYOUT | Dirty::ALL_WIDGETS` on every path back to
/// `MenuState::None`, so this helper only needs to set
/// `Dirty::PROC_LIST` when the change affects the process display
/// list — without it the proc widget would render the old list with
/// the new sort/filter applied to it.
pub(crate) fn apply_post_change_effects(
    key: ConfigKey,
    ctx: &mut InputContext,
    extra_ops: &mut Vec<TerminalOp>,
) {
    match key {
        ConfigKey::String(StringKey::ColorTheme) => {
            let name = ctx.config.ui.color_theme.clone();
            *ctx.theme = theme::Theme::from_name(&name);
            extra_ops.push(TerminalOp::Raw(
                ctx.theme.base_style(ctx.config.ui.theme_background),
            ));
        }
        ConfigKey::Bool(BoolKey::ThemeBackground) => {
            extra_ops.push(TerminalOp::Raw(
                ctx.theme.base_style(ctx.config.ui.theme_background),
            ));
        }
        ConfigKey::Bool(BoolKey::RoundedCorners) => {
            ctx.runtime.rounded = ctx.config.ui.rounded_corners;
        }
        ConfigKey::Enum(EnumKey::LogLevel) => {
            crate::log::set_level(ctx.config.log.log_level).expect("log level change must succeed");
        }
        ConfigKey::Int(IntKey::UpdateMs) => {
            ctx.runtime.update_ms = ctx.config.refresh.update_ms as u64;
            sync_all_intervals(ctx);
        }
        ConfigKey::Int(IntKey::CpuUpdateMs) => {
            let ms = ctx
                .config
                .effective_interval(ctx.config.refresh.cpu_update_ms);
            ctx.manager
                .set_interval(crate::event::SubsystemKind::Cpu, ms);
        }
        ConfigKey::Int(IntKey::MemUpdateMs) => {
            let ms = ctx
                .config
                .effective_interval(ctx.config.refresh.mem_update_ms);
            ctx.manager
                .set_interval(crate::event::SubsystemKind::Mem, ms);
        }
        ConfigKey::Int(IntKey::DiskUpdateMs) => {
            let ms = ctx
                .config
                .effective_interval(ctx.config.refresh.disk_update_ms);
            ctx.manager
                .set_interval(crate::event::SubsystemKind::Disk, ms);
        }
        ConfigKey::Int(IntKey::NetUpdateMs) => {
            let ms = ctx
                .config
                .effective_interval(ctx.config.refresh.net_update_ms);
            ctx.manager
                .set_interval(crate::event::SubsystemKind::Net, ms);
        }
        ConfigKey::Int(IntKey::GpuUpdateMs) => {
            let ms = ctx
                .config
                .effective_interval(ctx.config.refresh.gpu_update_ms);
            ctx.manager
                .set_interval(crate::event::SubsystemKind::Gpu, ms);
        }
        ConfigKey::Int(IntKey::ProcUpdateMs) => {
            let ms = ctx
                .config
                .effective_interval(ctx.config.refresh.proc_update_ms);
            ctx.manager
                .set_interval(crate::event::SubsystemKind::Proc, ms);
        }
        _ => {}
    }

    // Sync RuntimeView <- config.view if the change touched a
    // runtime-toggle key. This keeps the runtime mirror current
    // after the user edits a view-state field via the options
    // menu.
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
            | ConfigKey::Bool(BoolKey::KeepDeadProcUsage)
            | ConfigKey::Bool(BoolKey::ProcMemBytes)
            | ConfigKey::Bool(BoolKey::ProcGradient)
            | ConfigKey::Bool(BoolKey::ProcColors)
            | ConfigKey::Bool(BoolKey::ProcPerCore)
    ) {
        ctx.render.dirty |= Dirty::PROC_LIST;
    }
}

/// `true` for any [`ConfigKey`] backed by a [`crate::config::ViewConfig`]
/// field. After the options menu mutates one of these, the runtime
/// mirror in [`crate::app::RuntimeView`] needs to pick up the change.
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
