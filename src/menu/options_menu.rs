use crate::config::{BoolKey, Config, ConfigKey, EnumKey, IntKey, KeyKind, StringKey};
use crate::draw::box_drawing::{self, symbols};
use crate::handlers::options_edit::OptionEditState;
use crate::term;
use crate::theme::Theme;
use crate::theme_keys as tc;

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
// in `crate::menu::options_text::desc`. The editable shape is
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

fn classify(key: ConfigKey, _config: &Config) -> OptKind {
    match key.kind() {
        KeyKind::Bool => OptKind::Bool,
        KeyKind::Int => OptKind::Int,
        KeyKind::String if !browsable_values(key).is_empty() => OptKind::Browsable,
        KeyKind::String => OptKind::StringVal,
        KeyKind::Enum => OptKind::Browsable,
    }
}

/// Classify how an option key can be edited.
pub fn opt_kind(key: ConfigKey, config: &Config) -> OptKind {
    classify(key, config)
}

// ---------------------------------------------------------------------------
// Category definitions  (mirroring btop, minus Linux-only options)
// ---------------------------------------------------------------------------

/// Category tab names for the options menu.
pub const CAT_NAMES: &[&str] = &["general", "cpu", "mem", "net", "proc", "gpu", "disk"];

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
    ConfigKey::String(StringKey::ClockFormat),
    ConfigKey::Bool(BoolKey::Base10Sizes),
    ConfigKey::Bool(BoolKey::BackgroundUpdate),
    ConfigKey::Enum(EnumKey::LogLevel),
    ConfigKey::Bool(BoolKey::SaveConfigOnExit),
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
    ConfigKey::Bool(BoolKey::ShowUptime),
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
    ConfigKey::Bool(BoolKey::KeepDeadProcUsage),
    ConfigKey::String(StringKey::ProcFilter),
    ConfigKey::Int(IntKey::ProcUpdateMs),
];

/// Options in the "gpu" category.
pub const GPU: &[ConfigKey] = &[
    ConfigKey::String(StringKey::CustomGpuName0),
    ConfigKey::String(StringKey::CustomGpuName1),
    ConfigKey::String(StringKey::CustomGpuName2),
    ConfigKey::String(StringKey::CustomGpuName3),
    ConfigKey::String(StringKey::CustomGpuName4),
    ConfigKey::String(StringKey::CustomGpuName5),
    ConfigKey::String(StringKey::CustomGpuName6),
    ConfigKey::String(StringKey::CustomGpuName7),
    ConfigKey::Int(IntKey::GpuUpdateMs),
];

/// Options in the "disk" category.
pub const DISK: &[ConfigKey] = &[
    ConfigKey::Enum(EnumKey::GraphSymbolDisk),
    ConfigKey::Bool(BoolKey::ShowIoStat),
    ConfigKey::Bool(BoolKey::IoMode),
    ConfigKey::Bool(BoolKey::IoGraphCombined),
    ConfigKey::Bool(BoolKey::DiskIoMode),
    ConfigKey::String(StringKey::DisksFilter),
    ConfigKey::Int(IntKey::DiskUpdateMs),
];

/// All categories in order.
pub fn categories() -> &'static [&'static [ConfigKey]] {
    &[GENERAL, CPU, MEM, NET, PROC, GPU, DISK]
}

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

/// Draw the btop-style options menu.
///
/// The box is 78 chars wide, centered on screen.
/// Left panel: 30 chars (option name + value rows).
/// Right panel: description of selected option.
/// Vertical divider at column x+30.
///
/// `option_edit` (in [`DrawParams`]) is `Some` only while
/// [`crate::handlers::MenuState::OptionsEdit`] is active. When set,
/// the value cell on the matching row is rendered as a left-aligned
/// editable buffer with an underline cursor; the right panel
/// additionally shows any validation error.
pub fn draw(p: &DrawParams) -> String {
    let term_width = p.term_width;
    let term_height = p.term_height;
    let cat = p.cat;
    let selected = p.selected;
    let page = p.page;
    let config = p.config;
    let theme = p.theme;
    let option_edit = p.option_edit;
    let cats = categories();
    let cat = cat.min(cats.len() - 1);
    let options = cats[cat];

    let box_w: usize = 78;
    let x = term_width.saturating_sub(box_w) / 2;

    // Compute available height for options (each takes 2 rows)
    let max_items = cats.iter().map(|c| c.len()).max().unwrap_or(0);
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
    let sel_bg = theme.color(tc::SELECTED_BG);
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

    // Category tab bar at row y+7
    out.push_str(&term::mv(x + 4, y + 7 + 1));
    for (i, &name) in CAT_NAMES.iter().enumerate() {
        if i == cat {
            out.push_str(&format!(
                "{}{}[{}{}{}]{}",
                term::BOLD,
                hi,
                title_c,
                name,
                hi,
                reset
            ));
        } else {
            out.push_str(&format!(
                "{}{}{}{}{}{}",
                term::BOLD,
                hi,
                i,
                title_c,
                name,
                reset
            ));
        }
        let spacing = 8_usize.saturating_sub(name.len() + 1);
        out.push_str(&format!("\x1b[{}C", spacing));
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
        let kind = classify(key, config);
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
            option_edit.filter(|e| e.key == key)
        } else {
            None
        };
        if let Some(edit) = active_edit {
            let value_color = if edit.error.is_some() { hi } else { sel_fg };
            let cell = render_edit_value(&edit.buffer, edit.cursor, 25);
            out.push_str(&format!(
                "{}{}  {}{}",
                term::mv(x + 2, val_row),
                value_color,
                cell,
                reset,
            ));
        } else {
            let value_display = cjust(&value, 25);
            out.push_str(&format!(
                "{}{}  {}  {}",
                term::mv(x + 2, val_row),
                if is_selected { &sel_fg } else { &fg },
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
                for (di, desc_line) in crate::menu::options_text::desc(key).iter().enumerate() {
                    if row >= desc_bottom {
                        break;
                    }
                    if di == 1 {
                        out.push_str(&format!("{}{}", fg, term::BOLD_OFF));
                    }
                    out.push_str(&format!("{}{}", term::mv(x + 33, row + 1), desc_line));
                    row += 1;
                }
                if let Some(msg) = edit.error
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
                match kind {
                    OptKind::Bool | OptKind::Browsable | OptKind::Int => {
                        out.push_str(&format!(
                            "{}{}{}{}{}{}{}",
                            term::BOLD,
                            term::mv(x + 3, val_row),
                            hi,
                            symbols::LEFT_ARROW,
                            term::mv(x + 29, val_row),
                            hi,
                            symbols::RIGHT_ARROW,
                        ));
                        out.push_str(reset);
                    }
                    OptKind::StringVal => {
                        out.push_str(&format!(
                            "{}{}{}{}",
                            term::BOLD,
                            term::mv(x + 29, val_row),
                            hi,
                            symbols::ENTER,
                        ));
                        out.push_str(reset);
                    }
                }

                // Description in right panel
                out.push_str(&format!("{}{}{}", reset, title_c, term::BOLD));
                for (di, desc_line) in crate::menu::options_text::desc(key).iter().enumerate() {
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
    let cats = categories();
    if cat >= cats.len() {
        return None;
    }
    let options = cats[cat];
    let global_max = cats.iter().map(|c| c.len()).max().unwrap_or(0);
    let height = (global_max * 2 + 4).min(term_height.saturating_sub(8));
    let height = if height % 2 != 0 { height - 1 } else { height };
    let item_height = ((height - 4) / 2).min(global_max);
    let idx = item_height * page + selected;
    options.get(idx).copied()
}

/// Get item_height (visible items per page) for a category.
pub fn items_per_page(cat: usize, term_height: usize) -> usize {
    let cats = categories();
    if cat >= cats.len() {
        return 1;
    }
    let global_max = cats.iter().map(|c| c.len()).max().unwrap_or(0);
    let height = (global_max * 2 + 4).min(term_height.saturating_sub(8));
    let height = if height % 2 != 0 { height - 1 } else { height };
    ((height - 4) / 2).min(global_max).max(1)
}

/// Number of pages for a category.
pub fn page_count(cat: usize, term_height: usize) -> usize {
    let cats = categories();
    if cat >= cats.len() {
        return 1;
    }
    let max_items = cats[cat].len();
    let ipp = items_per_page(cat, term_height);
    max_items.div_ceil(ipp)
}

/// Max selectable index on a given page.
pub fn select_max(cat: usize, page: usize, term_height: usize) -> usize {
    let cats = categories();
    if cat >= cats.len() {
        return 0;
    }
    let max_items = cats[cat].len();
    let ipp = items_per_page(cat, term_height);
    let remaining = max_items.saturating_sub(ipp * page);
    ipp.min(remaining).saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_keys_roundtrip_through_config_parser() {
        for category in categories() {
            for option in *category {
                assert_eq!(ConfigKey::parse(option.name()), Some(*option));
            }
        }
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
}
