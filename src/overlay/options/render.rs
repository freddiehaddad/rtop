use crate::app::TerminalSize;
use crate::config::{BoolKey, Config, ConfigKey, EnumKey, IntKey, KeyKind, StringKey};
use crate::draw::box_drawing::{self, symbols};
use crate::term;
use crate::theme::Theme;
use crate::theme_keys as tc;

use super::OptionsState;
use super::edit::OptionEditState;

// ---------------------------------------------------------------------------
// Option type classification
// ---------------------------------------------------------------------------

/// How an option can be edited.
#[derive(Clone, Copy, PartialEq)]
pub enum OptKind {
    Bool,
    Int,
    /// Cycle through a fixed list of choices.
    Browsable,
    /// Free-form string (not editable via left/right in this version).
    StringVal,
}

// Each category is a `&[ConfigKey]`. The per-key help text lives on
// `ConfigKey::kind()` and the per-key options-menu help text lives
// in `crate::overlay::options::options_text::desc`. The editable shape is
// derived from `ConfigKey::kind()` plus `browsable_values(key)`.

// ---------------------------------------------------------------------------
// Browsable option value lists
// ---------------------------------------------------------------------------

/// Return the list of valid values for a browsable option key.
///
/// `EnumKey` always has choices (closed set); `StringKey` may
/// optionally have choices (today: only `ColorTheme`); other kinds
/// have none — the inline editor handles them via free-form input.
pub fn browsable_values(key: ConfigKey) -> &'static [&'static str] {
    match key {
        ConfigKey::Enum(k) => k.choices(),
        ConfigKey::String(k) => k.choices().unwrap_or(&[]),
        ConfigKey::Bool(_) | ConfigKey::Int(_) => &[],
    }
}

fn classify(key: ConfigKey) -> OptKind {
    match key.kind() {
        KeyKind::Bool => OptKind::Bool,
        KeyKind::Int => OptKind::Int,
        KeyKind::String if !browsable_values(key).is_empty() => OptKind::Browsable,
        KeyKind::String => OptKind::StringVal,
        KeyKind::Enum => OptKind::Browsable,
    }
}

/// Classify how an option key can be edited.
pub fn opt_kind(key: ConfigKey) -> OptKind {
    classify(key)
}

// ---------------------------------------------------------------------------
// Category definitions  (mirroring btop, minus Linux-only options)
// ---------------------------------------------------------------------------

/// One options-menu category tab. Bundles its display name, the
/// single-letter hotkey that jumps to it from anywhere in the
/// menu, and the option keys it hosts. The hotkey letter is
/// rendered highlighted inside the tab name (the case-insensitive
/// first match) so the user can see which key activates the tab.
///
/// Two `#[cfg(test)]` invariants pin the contract: every hotkey
/// is unique across [`CATEGORIES`], and every hotkey is a
/// case-insensitive substring of its `name`. A future rename
/// that breaks the mnemonic will fail at test time.
pub struct Category {
    pub name: &'static str,
    pub hotkey: char,
    pub options: &'static [ConfigKey],
}

/// All options-menu categories in tab order. Single source of
/// truth: the renderer iterates these for the tab bar, the
/// hotkey-jump action looks up by `hotkey`, and the binding
/// table's letter set is pinned equal to
/// `CATEGORIES.iter().map(|c| c.hotkey)` by test.
pub const CATEGORIES: &[Category] = &[
    Category {
        name: "general",
        hotkey: 'r',
        options: GENERAL,
    },
    Category {
        name: "statusbar",
        hotkey: 's',
        options: STATUSBAR,
    },
    Category {
        name: "cpu",
        hotkey: 'c',
        options: CPU,
    },
    Category {
        name: "mem",
        hotkey: 'm',
        options: MEM,
    },
    Category {
        name: "net",
        hotkey: 'n',
        options: NET,
    },
    Category {
        name: "proc",
        hotkey: 'p',
        options: PROC,
    },
    Category {
        name: "gpu",
        hotkey: 'g',
        options: GPU,
    },
    Category {
        name: "disk",
        hotkey: 'd',
        options: DISK,
    },
];

/// Look up the [`CATEGORIES`] index whose [`Category::hotkey`]
/// matches `key` (case-sensitive — bindings are declared with
/// the exact char a `Char(_)` event will carry). Returns `None`
/// when no category claims the letter; callers treat that as a
/// no-op (the binding table is the gate, but the handler stays
/// safe if a future edit broadens the binding's key set).
pub fn category_index_for_hotkey(key: char) -> Option<usize> {
    CATEGORIES.iter().position(|c| c.hotkey == key)
}

/// Cells of cursor advance between adjacent tab-bar cells. The
/// per-cell rendering always wraps the name in 1 cell of padding
/// on each side (plain space when unselected, bracket when
/// selected), so the visible gap between adjacent non-selected
/// tab names is `1 + INTER_TAB_GAP + 1 = 4` cells. When one of
/// the two adjacent tabs is selected, its bracket replaces its
/// padding space and the visible gap shrinks to 3 on that side —
/// but the absolute column of every non-selected name stays the
/// same regardless of selection.
pub const INTER_TAB_GAP: usize = 2;

/// Render `name` with the first case-insensitive occurrence of
/// `hotkey` painted in `hi` and the rest painted in `base`.
///
/// The case-insensitive substring invariant is pinned by
/// [`tests::every_category_hotkey_is_substring_of_name`]. If the
/// invariant somehow fails (e.g., a future rename forgets to
/// update the hotkey AND the test was bypassed), this falls back
/// to painting the entire name in `base` so the menu still
/// renders — the test is the contract; the fallback is
/// belt-and-suspenders.
pub fn highlight_hotkey(name: &str, hotkey: char, base: &str, hi: &str) -> String {
    let lowered = hotkey.to_ascii_lowercase();
    let split_at = name
        .char_indices()
        .find(|(_, c)| c.eq_ignore_ascii_case(&lowered))
        .map(|(idx, c)| (idx, c.len_utf8()));
    match split_at {
        Some((idx, w)) => {
            let prefix = &name[..idx];
            let letter = &name[idx..idx + w];
            let suffix = &name[idx + w..];
            format!("{base}{prefix}{hi}{letter}{base}{suffix}")
        }
        None => format!("{base}{name}"),
    }
}

/// Options in the "general" category.
pub const GENERAL: &[ConfigKey] = &[
    ConfigKey::String(StringKey::ColorTheme),
    ConfigKey::Bool(BoolKey::ThemeBackground),
    ConfigKey::Bool(BoolKey::VimKeys),
    ConfigKey::String(StringKey::CustomLayout),
    ConfigKey::Int(IntKey::UpdateMs),
    ConfigKey::Bool(BoolKey::RoundedCorners),
    ConfigKey::Bool(BoolKey::TerminalSync),
    ConfigKey::Enum(EnumKey::GraphSymbol),
    ConfigKey::Bool(BoolKey::Base10Sizes),
    ConfigKey::Bool(BoolKey::BackgroundUpdate),
    ConfigKey::Enum(EnumKey::LogLevel),
];

/// Options in the "cpu" category.
pub const CPU: &[ConfigKey] = &[
    ConfigKey::Enum(EnumKey::GraphSymbolCpu),
    ConfigKey::Enum(EnumKey::CpuGraphUpper),
    ConfigKey::Enum(EnumKey::CpuGraphLower),
    ConfigKey::Bool(BoolKey::CpuInvertLower),
    ConfigKey::Bool(BoolKey::CpuSingleGraph),
    ConfigKey::Bool(BoolKey::CpuAutoScale),
    ConfigKey::Bool(BoolKey::CheckTemp),
    ConfigKey::Bool(BoolKey::ShowCoretemp),
    ConfigKey::Enum(EnumKey::TempScale),
    ConfigKey::Bool(BoolKey::ShowCpuFreq),
    ConfigKey::String(StringKey::CustomCpuName),
    ConfigKey::Bool(BoolKey::ShowCpuWatts),
    ConfigKey::Int(IntKey::CpuUpdateMs),
];

/// Options in the "mem" category.
pub const MEM: &[ConfigKey] = &[
    ConfigKey::Bool(BoolKey::ShowSwap),
    ConfigKey::Int(IntKey::MemUpdateMs),
];

/// Options in the "net" category.
pub const NET: &[ConfigKey] = &[
    ConfigKey::Enum(EnumKey::GraphSymbolNet),
    ConfigKey::Bool(BoolKey::SwapUploadDownload),
    ConfigKey::Int(IntKey::NetDownload),
    ConfigKey::Int(IntKey::NetUpload),
    ConfigKey::Bool(BoolKey::NetAuto),
    ConfigKey::Bool(BoolKey::NetSync),
    ConfigKey::Int(IntKey::NetUpdateMs),
];

/// Options in the "proc" category.
pub const PROC: &[ConfigKey] = &[
    ConfigKey::Enum(EnumKey::ProcSorting),
    ConfigKey::Bool(BoolKey::ProcReversed),
    ConfigKey::Bool(BoolKey::ProcTree),
    ConfigKey::Bool(BoolKey::ProcAggregate),
    ConfigKey::Bool(BoolKey::ProcColors),
    ConfigKey::Bool(BoolKey::ProcGradient),
    ConfigKey::Bool(BoolKey::ProcPerCore),
    ConfigKey::Bool(BoolKey::ProcMemBytes),
    ConfigKey::String(StringKey::ProcFilter),
    ConfigKey::Int(IntKey::ProcUpdateMs),
];

/// Options in the "gpu" category.
///
/// Ordered as eight interleaved (custom-name, refresh-interval)
/// pairs per device so each GPU's settings render as adjacent
/// rows in the options modal — the user edits "GPU N's name and
/// refresh interval" together rather than scrolling between two
/// disjoint blocks.
pub const GPU: &[ConfigKey] = &[
    ConfigKey::String(StringKey::CustomGpuName0),
    ConfigKey::Int(IntKey::Gpu0UpdateMs),
    ConfigKey::String(StringKey::CustomGpuName1),
    ConfigKey::Int(IntKey::Gpu1UpdateMs),
    ConfigKey::String(StringKey::CustomGpuName2),
    ConfigKey::Int(IntKey::Gpu2UpdateMs),
    ConfigKey::String(StringKey::CustomGpuName3),
    ConfigKey::Int(IntKey::Gpu3UpdateMs),
    ConfigKey::String(StringKey::CustomGpuName4),
    ConfigKey::Int(IntKey::Gpu4UpdateMs),
    ConfigKey::String(StringKey::CustomGpuName5),
    ConfigKey::Int(IntKey::Gpu5UpdateMs),
    ConfigKey::String(StringKey::CustomGpuName6),
    ConfigKey::Int(IntKey::Gpu6UpdateMs),
    ConfigKey::String(StringKey::CustomGpuName7),
    ConfigKey::Int(IntKey::Gpu7UpdateMs),
];

/// Options in the "disk" category.
pub const DISK: &[ConfigKey] = &[
    ConfigKey::Enum(EnumKey::GraphSymbolDisk),
    ConfigKey::Bool(BoolKey::ShowIoStat),
    ConfigKey::Bool(BoolKey::IoMode),
    ConfigKey::Bool(BoolKey::IoGraphCombined),
    ConfigKey::Bool(BoolKey::DiskIoMode),
    ConfigKey::String(StringKey::DiskFilter),
    ConfigKey::Int(IntKey::DiskUpdateMs),
];

/// Options in the "statusbar" category. Hosts every key the
/// statusbar widget consults. The master toggle leads; the five
/// sub-item visibility bools follow in render order
/// (left-section → right-section); the clock format closes the
/// list because it's the only entry the user types into.
pub const STATUSBAR: &[ConfigKey] = &[
    ConfigKey::Bool(BoolKey::ShowStatusbar),
    ConfigKey::Bool(BoolKey::StatusbarShowMenu),
    ConfigKey::Bool(BoolKey::StatusbarShowPreset),
    ConfigKey::Bool(BoolKey::StatusbarShowUpdateInterval),
    ConfigKey::Bool(BoolKey::StatusbarShowUptime),
    ConfigKey::Bool(BoolKey::StatusbarShowClock),
    ConfigKey::String(StringKey::StatusbarClockFormat),
];

// ---------------------------------------------------------------------------
// Value helpers
// ---------------------------------------------------------------------------

/// Get the display value for an option.
pub fn get_value(key: ConfigKey, config: &Config) -> String {
    key.get_display(config)
}

/// Cycle a browsable option left or right. Returns true if changed.
///
/// Only `EnumKey` and constrained `StringKey`s (today: `ColorTheme`)
/// are browsable; calling this on any other key is a no-op.
pub fn cycle_browsable(key: ConfigKey, config: &mut Config, direction: i32) -> bool {
    let vals = browsable_values(key);
    if vals.is_empty() {
        return false;
    }
    let current = key.get_display(config);
    let idx = vals.iter().position(|&v| v == current).unwrap_or(0);
    let new_idx = if direction > 0 {
        (idx + 1) % vals.len()
    } else if idx == 0 {
        vals.len() - 1
    } else {
        idx - 1
    };
    let target = vals[new_idx];
    match key {
        ConfigKey::Enum(k) => k
            .set_canonical(config, target)
            .expect("EnumKey::choices entries must round-trip through set_canonical"),
        ConfigKey::String(k) => k
            .set(config, target)
            .expect("StringKey::choices entries must round-trip through set"),
        ConfigKey::Bool(_) | ConfigKey::Int(_) => {
            // browsable_values returned a non-empty slice above only
            // for Enum / String keys, so this branch is unreachable
            // by construction.
            unreachable!("non-browsable key reached cycle_browsable mutation path")
        }
    }
    true
}

/// Step an int option by `delta`.
///
/// No-op for non-int keys (the menu dispatches by `OptKind` before
/// reaching this function — the match here exists to satisfy the
/// type-safe `IntKey` API rather than as a runtime guard).
pub fn step_int(key: ConfigKey, config: &mut Config, delta: i64) {
    let ConfigKey::Int(k) = key else {
        return;
    };
    let value = k.get(config) + delta * k.step();
    k.set(config, value);
    config.validate();
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Center-justify a string in `width` columns, padding with spaces.
fn cjust(s: &str, width: usize) -> String {
    let slen = s.chars().count();
    if slen >= width {
        return s.chars().take(width).collect();
    }
    let pad_left = (width - slen) / 2;
    let pad_right = width - slen - pad_left;
    format!("{}{}{}", " ".repeat(pad_left), s, " ".repeat(pad_right))
}

/// Render `buffer` as a `width`-character window for inline editing.
///
/// The cursor (a char index into `buffer`) is rendered with an
/// ANSI underline marker (`term::UNDERLINE` / `term::UNDERLINE_OFF`).
/// When `cursor` lies past the rendered text the marker wraps a
/// space so it stays visible — this is the same precedent the proc
/// filter uses (see `src/ui/proc_widget/borders.rs`).
///
/// When the buffer is wider than `width`, the window scrolls so
/// the cursor stays on-screen: cursor positions `< width` show from
/// the start; positions `>= width` shift the start so the cursor
/// lands on the last column.
pub(crate) fn render_edit_value(buffer: &str, cursor: usize, width: usize) -> String {
    let chars: Vec<char> = buffer.chars().collect();
    let cc = chars.len();
    let scroll = if cc <= width || cursor < width {
        0
    } else {
        cursor + 1 - width
    };
    let mut out = String::with_capacity(width + term::UNDERLINE.len() + term::UNDERLINE_OFF.len());
    for col in 0..width {
        let pos = scroll + col;
        let ch = chars.get(pos).copied().unwrap_or(' ');
        if pos == cursor {
            out.push_str(term::UNDERLINE);
            out.push(ch);
            out.push_str(term::UNDERLINE_OFF);
        } else {
            out.push(ch);
        }
    }
    out
}

/// Capitalize first letter of each word, replace underscores with spaces.
fn capitalize_option(key: ConfigKey) -> String {
    key.name()
        .replace('_', " ")
        .split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => {
                    let upper: String = f.to_uppercase().collect();
                    format!("{}{}", upper, c.as_str())
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parameters for [`draw`].
///
/// Bundled into a struct because the option-set is wide enough to
/// trigger `clippy::too_many_arguments` (the codebase forbids
/// `#[allow(...)]` lint suppressions). All fields are required;
/// `option_edit` is `None` outside the inline-editor menu state.
pub struct DrawParams<'a> {
    pub term_width: usize,
    pub term_height: usize,
    pub cat: usize,
    pub selected: usize,
    pub page: usize,
    pub config: &'a Config,
    pub theme: &'a Theme,
    pub option_edit: Option<&'a OptionEditState>,
}

/// Render the options overlay to an unstyled ANSI buffer using
/// the typed [`OptionsState`].
///
/// Thin wrapper around [`draw`] that constructs [`DrawParams`]
/// from the typed state — keeps the legacy (cat, selected, page,
/// option_edit) parameterisation in the body of the renderer
/// while exposing the typed entry point at the module boundary.
pub fn render(state: &OptionsState, term: TerminalSize, config: &Config, theme: &Theme) -> String {
    draw(&DrawParams {
        term_width: term.width,
        term_height: term.height,
        cat: state.cat(),
        selected: state.selected(),
        page: state.page(),
        config,
        theme,
        option_edit: state.edit(),
    })
}

/// Draw the btop-style options menu.
///
/// The box is 78 chars wide, centered on screen.
/// Left panel: 30 chars (option name + value rows).
/// Right panel: description of selected option.
/// Vertical divider at column x+30.
///
/// `option_edit` (in [`DrawParams`]) is `Some` only while the
/// options overlay is in inline-edit sub-state. When set, the value
/// cell on the matching row is rendered as a left-aligned editable
/// buffer with an underline cursor; the right panel additionally
/// shows any validation error.
pub fn draw(p: &DrawParams) -> String {
    let term_width = p.term_width;
    let term_height = p.term_height;
    let cat = p.cat;
    let selected = p.selected;
    let page = p.page;
    let config = p.config;
    let theme = p.theme;
    let option_edit = p.option_edit;
    let cat = cat.min(CATEGORIES.len() - 1);
    let options = CATEGORIES[cat].options;

    let box_w: usize = 78;
    let x = term_width.saturating_sub(box_w) / 2;

    // Compute available height for options (each takes 2 rows)
    let max_items = CATEGORIES
        .iter()
        .map(|c| c.options.len())
        .max()
        .unwrap_or(0);
    let desired_h = max_items * 2 + 4; // 4 = tab row + divider + top/bottom borders
    let height = desired_h.min(term_height.saturating_sub(8));
    let height = if height % 2 != 0 { height - 1 } else { height };
    let y = term_height.saturating_sub(height + 6) / 2;

    let current_items = options.len();
    let item_height = ((height - 4) / 2).min(max_items);
    let pages = if current_items == 0 {
        1
    } else {
        current_items.div_ceil(item_height)
    };
    let page = page.min(pages - 1);
    let select_max = item_height
        .min(current_items.saturating_sub(item_height * page))
        .saturating_sub(1);
    let selected = selected.min(select_max);

    let hi = theme.color(tc::HI_FG);
    let title_c = theme.color(tc::TITLE);
    let fg = theme.color(tc::MAIN_FG);
    // `theme.color()` returns a *foreground* ANSI escape; for the
    // selected-row background we need the matching **background**
    // escape (`\x1b[48;2;...m`). Use `theme.background()` to flip
    // the SGR parameter (38 -> 48). Without this the selected row
    // attempts to set FG twice and ends up rendering
    // `selected_fg`-on-`main_bg`, which is invisible on themes whose
    // `selected_fg` matches `main_bg` (greyscale, gruvbox_material_dark,
    // orange) or whose `selected_fg` matches `main_bg` numerically
    // even with a different selected_bg defined (flat_remix_light).
    let sel_bg = theme.background(tc::SELECTED_BG);
    let sel_fg = theme.color(tc::SELECTED_FG);
    let opts_c = theme.color(tc::OPTIONS_BOX);
    let reset = term::RESET;

    let mut out = String::with_capacity(4096);

    // Main box: create at (x, y+6) with height
    let tab_title = format!("{}tab{}{}", hi, fg, symbols::RIGHT_ARROW);
    out.push_str(&box_drawing::create_box(&box_drawing::BoxConfig {
        x,
        y: y + 6,
        width: box_w,
        height,
        line_color: opts_c,
        fill: true,
        title: &tab_title,
        title2: "",
        num: 0,
        rounded: config.ui.rounded_corners,
        hi_color: "",
        title_color: "",
    }));

    // Horizontal divider at row y+8 with T-junctions
    let h_left = symbols::H_LINE.repeat(29);
    let h_right = symbols::H_LINE.repeat(box_w - 32);
    let divider_row = y + 8 + 1;
    out.push_str(&term::mv(x + 1, divider_row));
    out.push_str(opts_c);
    out.push_str(symbols::DIV_LEFT);
    out.push_str(opts_c);
    out.push_str(&h_left);
    out.push_str(symbols::DIV_UP);
    out.push_str(&h_right);
    out.push_str(opts_c);
    out.push_str(symbols::DIV_RIGHT);
    // Bottom T-junction on vertical divider
    out.push_str(&term::mv(x + 31, y + 6 + height));
    out.push_str(opts_c);
    out.push_str(symbols::DIV_DOWN);

    // Vertical divider line at x+30 for each content row
    for i in 0..(height - 4) {
        out.push_str(&format!(
            "{}{}{}",
            term::mv(x + 31, y + 9 + 1 + i),
            opts_c,
            symbols::V_LINE,
        ));
    }

    // Category tab bar at row y+7. Each cell is rendered as
    // `<lead><name><trail>` where `lead`/`trail` are `[`/`]` for
    // the selected tab and plain spaces otherwise. The brackets
    // replace the padding spaces — selecting a different tab
    // never shifts the absolute column of any neighbour. Adjacent
    // cells are separated by `INTER_TAB_GAP` extra cells, giving
    // a uniform 4-cell visible gap between non-selected adjacent
    // tab names (trailing pad + INTER_TAB_GAP + leading pad). The
    // hotkey letter inside each name is painted in `hi`; the rest
    // of the name is in `title_c`.
    out.push_str(&term::mv(x + 4, y + 7 + 1));
    for (i, cat_def) in CATEGORIES.iter().enumerate() {
        let selected = i == cat;
        let (lead, trail) = if selected { ('[', ']') } else { (' ', ' ') };
        let highlighted = highlight_hotkey(cat_def.name, cat_def.hotkey, title_c, hi);
        out.push_str(&format!(
            "{bold}{hi}{lead}{highlighted}{hi}{trail}{reset}",
            bold = term::BOLD,
        ));
        if i + 1 < CATEGORIES.len() {
            out.push_str(&format!("\x1b[{}C", INTER_TAB_GAP));
        }
    }

    // Page indicator
    if pages > 1 {
        out.push_str(&format!(
            "{}{}{} {} page {}/{} {} {}",
            term::mv(x + 2, y + 6 + height),
            hi,
            symbols::UP_ARROW,
            title_c,
            page + 1,
            pages,
            hi,
            symbols::DOWN_ARROW,
        ));
    }

    // Option rows
    let cy_start = y + 9 + 1; // first content row (1-based terminal row)
    for c in 0..item_height {
        let i = item_height * page + c;
        if i >= options.len() {
            break;
        }
        let key = options[i];
        let kind = classify(key);
        let value = get_value(key, config);
        let is_selected = c == selected;

        let name_display = capitalize_option(key);

        // Browsable index suffix
        let mut name_suffix = String::new();
        if is_selected && kind == OptKind::Browsable {
            let vals = browsable_values(key);
            let idx = vals.iter().position(|&v| v == value).unwrap_or(0);
            name_suffix = format!(" {}/{}", idx + 1, vals.len());
        }

        // Row 1: option name (29 chars in left panel)
        let full_name = format!("{}{}", name_display, name_suffix);
        let name_str = cjust(&full_name, 29);
        out.push_str(&format!(
            "{}{}{}{}",
            term::mv(x + 2, cy_start + c * 2),
            if is_selected {
                format!("{}{}{}", sel_bg, sel_fg, term::BOLD)
            } else {
                format!("{}{}", title_c, term::BOLD)
            },
            name_str,
            reset,
        ));

        // Row 2: value cell. Two presentations:
        //   - normal: centered in 25 chars with arrow / enter inset
        //   - editing (selected row + matching option_edit): left-
        //     aligned editable buffer with underline cursor
        let val_row = cy_start + c * 2 + 1;
        let active_edit: Option<&OptionEditState> = if is_selected {
            option_edit.filter(|e| e.key() == key)
        } else {
            None
        };
        if let Some(edit) = active_edit {
            // Selected row + active inline edit: render the editable
            // buffer with the selected colour pair so the value cell
            // stays readable even when the theme's `selected_fg`
            // matches `main_bg` (without the bg the cell would be
            // foreground-on-foreground and invisible — see
            // `themes/greyscale.toml` for the canonical case).
            let value_color = if edit.error().is_some() { hi } else { sel_fg };
            let cell = render_edit_value(edit.buffer(), edit.cursor(), 25);
            out.push_str(&format!(
                "{}{}{}  {}{}",
                term::mv(x + 2, val_row),
                sel_bg,
                value_color,
                cell,
                reset,
            ));
        } else {
            let value_display = cjust(&value, 25);
            // Selected row: render the value with both `sel_bg` and
            // `sel_fg` (matching the name row above). Without
            // `sel_bg`, themes whose `selected_fg` matches
            // `main_bg` (greyscale, gruvbox_material_dark, orange,
            // flat_remix_light) would render an invisible value
            // cell.
            let (color, bg): (&str, &str) = if is_selected {
                (sel_fg, sel_bg.as_str())
            } else {
                (fg, "")
            };
            out.push_str(&format!(
                "{}{}{}  {}  {}",
                term::mv(x + 2, val_row),
                bg,
                color,
                value_display,
                reset,
            ));
        }

        // Decorations and right-panel description for the selected item.
        if is_selected {
            if let Some(edit) = active_edit {
                // Inline-editor decorations: no left/right arrows
                // (cursor keys move within the buffer instead).
                // Right panel: description + blank line + error.
                out.push_str(&format!("{}{}{}", reset, title_c, term::BOLD));
                let desc_top = y + 8 + 1;
                let desc_bottom = y + 6 + height;
                let mut row = desc_top;
                for (di, desc_line) in super::options_text::desc(key).iter().enumerate() {
                    if row >= desc_bottom {
                        break;
                    }
                    if di == 1 {
                        out.push_str(&format!("{}{}", fg, term::BOLD_OFF));
                    }
                    out.push_str(&format!("{}{}", term::mv(x + 33, row + 1), desc_line));
                    row += 1;
                }
                if let Some(msg) = edit.error()
                    && row + 1 < desc_bottom
                {
                    out.push_str(&format!(
                        "{}{}{}{}",
                        term::mv(x + 33, row + 2),
                        hi,
                        msg,
                        reset,
                    ));
                }
                out.push_str(reset);
            } else {
                // Decorations sit *inside* the selected value row,
                // so they must repaint `sel_bg` themselves — the
                // earlier `reset` after the value cell wiped the
                // background state and a bare foreground escape
                // would render the icons on the terminal default
                // background, breaking the selection band.
                match kind {
                    OptKind::Bool | OptKind::Browsable | OptKind::Int => {
                        out.push_str(&format!(
                            "{}{}{}{}{}{}{}{}{}",
                            term::BOLD,
                            term::mv(x + 3, val_row),
                            sel_bg,
                            hi,
                            symbols::LEFT_ARROW,
                            term::mv(x + 29, val_row),
                            sel_bg,
                            hi,
                            symbols::RIGHT_ARROW,
                        ));
                        out.push_str(reset);
                    }
                    OptKind::StringVal => {
                        out.push_str(&format!(
                            "{}{}{}{}{}",
                            term::BOLD,
                            term::mv(x + 29, val_row),
                            sel_bg,
                            hi,
                            symbols::ENTER,
                        ));
                        out.push_str(reset);
                    }
                }

                // Description in right panel
                out.push_str(&format!("{}{}{}", reset, title_c, term::BOLD));
                for (di, desc_line) in super::options_text::desc(key).iter().enumerate() {
                    let desc_row = y + 8 + 1 + di; // start at the row after the divider
                    if desc_row >= y + 6 + height {
                        break;
                    }
                    // First description line is title-colored, rest are main_fg
                    if di == 1 {
                        out.push_str(&format!("{}{}", fg, term::BOLD_OFF));
                    }
                    out.push_str(&format!("{}{}", term::mv(x + 33, desc_row + 1), desc_line,));
                }
                out.push_str(reset);
            }
        }
    }

    out.push_str(reset);
    out
}

/// Return the option key at `(cat, index)`.
pub fn opt_key(cat: usize, page: usize, selected: usize, term_height: usize) -> Option<ConfigKey> {
    if cat >= CATEGORIES.len() {
        return None;
    }
    let options = CATEGORIES[cat].options;
    let global_max = CATEGORIES
        .iter()
        .map(|c| c.options.len())
        .max()
        .unwrap_or(0);
    let height = (global_max * 2 + 4).min(term_height.saturating_sub(8));
    let height = if height % 2 != 0 { height - 1 } else { height };
    let item_height = ((height - 4) / 2).min(global_max);
    let idx = item_height * page + selected;
    options.get(idx).copied()
}

/// Get item_height (visible items per page) for a category.
pub fn items_per_page(cat: usize, term_height: usize) -> usize {
    if cat >= CATEGORIES.len() {
        return 1;
    }
    let global_max = CATEGORIES
        .iter()
        .map(|c| c.options.len())
        .max()
        .unwrap_or(0);
    let height = (global_max * 2 + 4).min(term_height.saturating_sub(8));
    let height = if height % 2 != 0 { height - 1 } else { height };
    ((height - 4) / 2).min(global_max).max(1)
}

/// Number of pages for a category.
pub fn page_count(cat: usize, term_height: usize) -> usize {
    if cat >= CATEGORIES.len() {
        return 1;
    }
    let max_items = CATEGORIES[cat].options.len();
    let ipp = items_per_page(cat, term_height);
    max_items.div_ceil(ipp)
}

/// Max selectable index on a given page.
pub fn select_max(cat: usize, page: usize, term_height: usize) -> usize {
    if cat >= CATEGORIES.len() {
        return 0;
    }
    let max_items = CATEGORIES[cat].options.len();
    let ipp = items_per_page(cat, term_height);
    let remaining = max_items.saturating_sub(ipp * page);
    ipp.min(remaining).saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_keys_roundtrip_through_config_parser() {
        for category in CATEGORIES {
            for option in category.options {
                assert_eq!(ConfigKey::parse(option.name()), Some(*option));
            }
        }
    }

    // ── Category schema invariants ──────────────────────────────

    #[test]
    fn every_category_has_unique_hotkey() {
        let mut seen: Vec<char> = Vec::new();
        for cat in CATEGORIES {
            assert!(
                !seen.contains(&cat.hotkey),
                "duplicate hotkey {:?} on category {:?}",
                cat.hotkey,
                cat.name,
            );
            seen.push(cat.hotkey);
        }
    }

    #[test]
    fn every_category_hotkey_is_substring_of_name() {
        // Pin the mnemonic contract: a future rename that breaks
        // the visual highlight (no letter to colour) fails here.
        for cat in CATEGORIES {
            let lowered = cat.hotkey.to_ascii_lowercase();
            let found = cat.name.chars().any(|c| c.eq_ignore_ascii_case(&lowered));
            assert!(
                found,
                "hotkey {:?} is not a substring of name {:?}",
                cat.hotkey, cat.name,
            );
        }
    }

    #[test]
    fn binding_table_letter_set_equals_categories_hotkey_set() {
        // Pin the cross-table contract so a future edit cannot bind
        // a letter the handler doesn't recognise, or define a hotkey
        // that no binding fires.
        use crate::handlers::keybinds::{BINDINGS, KeySpec};
        use crate::input::Key;
        use crate::overlay::OverlayKind;
        use crate::overlay::options::cat_select_hotkey_action;

        let mut bound: Vec<char> = Vec::new();
        for binding in BINDINGS {
            // Find the binding registered against
            // `cat_select_hotkey_action` for OPTIONS state.
            let action_matches = std::ptr::fn_addr_eq(
                binding.action,
                cat_select_hotkey_action as crate::handlers::keybinds::ActionFn,
            );
            if !action_matches {
                continue;
            }
            assert!(
                binding.states.contains(&OverlayKind::Options),
                "hotkey-action binding must be scoped to OPTIONS state",
            );
            for spec in binding.keys {
                match *spec {
                    KeySpec::Always(Key::Char(c)) => bound.push(c),
                    KeySpec::Always(_) | KeySpec::VimOnly(_) => {
                        panic!("hotkey-action binding must only carry Always(Char(_)) keys")
                    }
                }
            }
        }

        let mut declared: Vec<char> = CATEGORIES.iter().map(|c| c.hotkey).collect();
        bound.sort();
        declared.sort();
        assert_eq!(
            bound, declared,
            "binding `keys` slice must match CATEGORIES hotkeys exactly",
        );
    }

    // ── highlight_hotkey ────────────────────────────────────────

    #[test]
    fn highlight_hotkey_emits_letter_in_hi_color() {
        // Pin the visual contract: the hotkey letter is wrapped
        // in `hi`, the surrounding name segments are wrapped in
        // `base`. Use distinguishable sentinel escapes so the
        // assertion is unambiguous.
        let out = highlight_hotkey("statusbar", 's', "<base>", "<hi>");
        assert_eq!(out, "<base><hi>s<base>tatusbar");

        // Letter mid-word.
        let out = highlight_hotkey("general", 'r', "<base>", "<hi>");
        assert_eq!(out, "<base>gene<hi>r<base>al");

        // Case-insensitive match.
        let out = highlight_hotkey("Status", 'S', "<base>", "<hi>");
        assert_eq!(out, "<base><hi>S<base>tatus");
    }

    #[test]
    fn highlight_hotkey_falls_back_to_uncoloured_when_letter_missing() {
        // The substring invariant pins this never happens for
        // CATEGORIES, but the helper must still produce something
        // sane if invoked with an inconsistent (name, hotkey) pair.
        let out = highlight_hotkey("xyz", 'q', "<base>", "<hi>");
        assert_eq!(out, "<base>xyz");
    }

    // ── Tab-bar visual layout ───────────────────────────────────

    fn strip_ansi(s: &str) -> String {
        // Materialise the visible cells from an ANSI string so the
        // tab-bar tests can reason about column positions. SGR
        // escapes (`\x1b[...m`) are dropped; cursor-forward escapes
        // (`\x1b[<N>C`) are expanded to N spaces because they
        // advance the cursor over empty cells that the tab-bar
        // layout depends on. Other CSI sequences are dropped.
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\x1b' {
                out.push(c);
                continue;
            }
            // Expect `[`; if not, skip the next char and continue.
            if chars.next() != Some('[') {
                continue;
            }
            // Collect parameter bytes (digits, `;`) until we hit
            // the final byte (an ASCII alphabetic per ECMA-48
            // final-byte range 0x40-0x7E).
            let mut params = String::new();
            let final_byte = loop {
                match chars.next() {
                    Some(c2) if c2.is_ascii_alphabetic() => break c2,
                    Some(c2) => params.push(c2),
                    None => return out,
                }
            };
            // Cursor-forward (CUF): `\x1b[<N>C` → N spaces. A
            // missing parameter defaults to 1 per ECMA-48.
            if final_byte == 'C' {
                let n: usize = params.parse().unwrap_or(1);
                for _ in 0..n {
                    out.push(' ');
                }
            }
            // All other CSI sequences (SGR, cursor positioning,
            // etc.) contribute nothing to visible cells in this
            // test's row of interest.
        }
        out
    }

    /// Render the menu with `cat = selected` against an 80×30
    /// terminal and return the plain-text tab-bar row.
    fn render_tab_bar_plain(selected: usize) -> String {
        let config = Config::default();
        let theme = Theme::new();
        let raw = draw(&DrawParams {
            term_width: 120,
            term_height: 40,
            cat: selected,
            selected: 0,
            page: 0,
            config: &config,
            theme: &theme,
            option_edit: None,
        });
        let plain = strip_ansi(&raw);
        // Find the line that contains "general" and one of the
        // other tab names — that's the tab bar.
        plain
            .lines()
            .find(|l| l.contains("general") && l.contains("disk"))
            .map(str::to_string)
            .expect("tab bar must contain `general` and `disk`")
    }

    #[test]
    fn tab_bar_uses_uniform_inter_tab_spacing() {
        // Each adjacent pair of tab names must have exactly 4
        // visible cells between them when neither tab is selected
        // (trailing pad + INTER_TAB_GAP + leading pad =
        // 1 + 2 + 1 = 4). We render with `cat = 7` (disk) so
        // the entire general↔gpu prefix is non-selected.
        let bar = render_tab_bar_plain(CATEGORIES.len() - 1);
        // Walk through each adjacent non-selected pair and
        // assert exact 4-cell gap.
        for window in CATEGORIES.windows(2) {
            let [a, b] = window else {
                continue;
            };
            // Skip the pair that includes the selected (last) cat.
            if std::ptr::eq(b as *const _, &CATEGORIES[CATEGORIES.len() - 1]) {
                continue;
            }
            let combined = format!("{}    {}", a.name, b.name);
            assert!(
                bar.contains(&combined),
                "expected 4-space gap between {:?} and {:?} in tab bar: {bar:?}",
                a.name,
                b.name,
            );
        }
    }

    #[test]
    fn selecting_a_tab_does_not_shift_neighbouring_tab_columns() {
        // Render with two different selections and assert that
        // every non-selected tab name lands at the same column
        // offset in both renders. The brackets on the selected
        // tab eat into the surrounding gap rather than push
        // neighbours.
        let bar_a = render_tab_bar_plain(0); // general selected
        let bar_b = render_tab_bar_plain(4); // net selected
        for (i, cat) in CATEGORIES.iter().enumerate() {
            if i == 0 || i == 4 {
                continue; // skip the selected one in each render
            }
            let pos_a = bar_a
                .find(cat.name)
                .unwrap_or_else(|| panic!("missing {:?} in bar_a: {bar_a:?}", cat.name));
            let pos_b = bar_b
                .find(cat.name)
                .unwrap_or_else(|| panic!("missing {:?} in bar_b: {bar_b:?}", cat.name));
            assert_eq!(
                pos_a, pos_b,
                "tab {:?} shifted between selections (a@{pos_a}, b@{pos_b})",
                cat.name,
            );
        }
    }

    #[test]
    fn selected_tab_renders_inside_brackets() {
        // Pin: selected tab name is wrapped in [ ] (current visual
        // contract preserved through the schema rewrite).
        let bar = render_tab_bar_plain(1); // statusbar selected
        assert!(
            bar.contains("[statusbar]"),
            "expected `[statusbar]` in selected tab bar: {bar:?}",
        );
    }

    #[test]
    fn unselected_tab_renders_without_brackets() {
        // Pin: when not selected, the tab name has no surrounding
        // brackets — the visible cells around it are plain spaces.
        let bar = render_tab_bar_plain(0); // general selected, statusbar NOT
        assert!(
            !bar.contains("[statusbar]"),
            "non-selected statusbar must not have brackets: {bar:?}",
        );
        assert!(
            bar.contains("statusbar"),
            "statusbar must still appear in tab bar: {bar:?}",
        );
    }

    fn count_visible_chars(s: &str) -> usize {
        // Strip ANSI underline markers; count remaining chars.
        s.replace(term::UNDERLINE, "")
            .replace(term::UNDERLINE_OFF, "")
            .chars()
            .count()
    }

    #[test]
    fn render_edit_value_pads_short_buffer_to_full_width() {
        let out = render_edit_value("ab", 2, 25);
        assert_eq!(count_visible_chars(&out), 25);
    }

    #[test]
    fn render_edit_value_marks_cursor_with_underline() {
        let out = render_edit_value("ab", 1, 25);
        // Cursor on 'b' → underline wraps it.
        let expected = format!("{}b{}", term::UNDERLINE, term::UNDERLINE_OFF);
        assert!(out.contains(&expected));
    }

    #[test]
    fn render_edit_value_underlines_blank_when_cursor_at_end() {
        let out = render_edit_value("ab", 2, 25);
        let expected = format!("{} {}", term::UNDERLINE, term::UNDERLINE_OFF);
        assert!(out.contains(&expected));
    }

    #[test]
    fn render_edit_value_handles_empty_buffer() {
        let out = render_edit_value("", 0, 25);
        // Cursor at column 0 wraps a space.
        let expected = format!("{} {}", term::UNDERLINE, term::UNDERLINE_OFF);
        assert!(out.contains(&expected));
        assert_eq!(count_visible_chars(&out), 25);
    }

    #[test]
    fn render_edit_value_scrolls_when_buffer_exceeds_width() {
        let buffer: String = "0123456789012345678901234567890".to_string(); // 31 chars
        let out = render_edit_value(&buffer, 31, 25);
        // Cursor at end (pos 31). With width=25, scroll = 31+1-25 = 7.
        // Visible window starts at char 7, so the rendered prefix is
        // "78901234..." and the underline marks the trailing space
        // (cursor sits past the last char).
        assert!(out.contains("78901"));
        assert_eq!(count_visible_chars(&out), 25);
    }

    #[test]
    fn render_edit_value_supports_multibyte_chars() {
        // Crab is 1 char, 4 bytes. The renderer is char-indexed.
        let out = render_edit_value("a🦀b", 2, 25);
        // Cursor on 'b' → underline wraps 'b'.
        let expected = format!("{}b{}", term::UNDERLINE, term::UNDERLINE_OFF);
        assert!(out.contains(&expected));
        // Crab is preserved verbatim.
        assert!(out.contains('🦀'));
        assert_eq!(count_visible_chars(&out), 25);
    }

    /// Regression: the selected-row VALUE cell used to render with
    /// only `selected_fg` and no background, while the selected-row
    /// NAME cell *attempted* to set the background but used the
    /// wrong ANSI escape — `theme.color(SELECTED_BG)` returns a
    /// *foreground* escape, not a background one. The net effect was
    /// that the selected row rendered as `selected_fg` on `main_bg`
    /// regardless of `selected_bg` — invisible on themes where
    /// `selected_fg` matches `main_bg` (greyscale,
    /// gruvbox_material_dark, orange) and on flat_remix_light
    /// where the visual collision happens at the cell-fill level.
    /// Both name and value cells must emit the real background
    /// escape (`\x1b[48;2;...m`) for the selected row.
    #[test]
    fn selected_row_paints_real_background_escape() {
        use crate::config::Config;
        use crate::theme::Theme;

        // Greyscale is the canonical case: `selected_bg = #ffffff`,
        // `selected_fg = #000000`, `main_bg = #000000`. Without the
        // 48;2 escape the row renders black-on-black.
        let theme = Theme::from_name("greyscale");
        let bg_escape = theme.background(crate::theme_keys::SELECTED_BG);
        assert!(
            bg_escape.contains("48;2"),
            "background() must produce a 48;2 (BG) escape, got {bg_escape:?}",
        );

        let config = Config::new();
        let out = draw(&DrawParams {
            term_width: 120,
            term_height: 30,
            cat: 0,
            selected: 0,
            page: 0,
            config: &config,
            theme: &theme,
            option_edit: None,
        });

        // The selected row emits the BG escape at least twice — once
        // for the name row, once for the value row. Pre-fix the
        // output contained the FG escape (38;2) for SELECTED_BG and
        // never the BG escape (48;2).
        let occurrences = out.matches(&bg_escape).count();
        assert!(
            occurrences >= 2,
            "selected row must paint sel_bg (48;2 escape) on both \
             name and value cells; got {occurrences} occurrences of \
             {bg_escape:?}",
        );
    }
}
