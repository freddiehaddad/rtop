//! Statusbar widget renderer.
//!
//! A borderless 1-row widget rendered as part of the slot tree
//! (typically the last child of the outermost vertical stack).
//! Hosts two sections:
//!
//! * **Left** — `[menu]`, `[← P NAME[*] p →]`, `[- Nms +]` in
//!   declaration order, single space between visible neighbours;
//!   hidden items collapse the gap entirely (no placeholders).
//!   Each item is wrapped in `[ ]` brackets painted in
//!   `STATUSBAR_SEP` for subtle visual chunking; the brackets
//!   stay subtle whether `theme_background` is on or off because
//!   each theme picks a `statusbar_sep` value tuned to read
//!   against both the theme bg and a generic terminal default
//!   bg. Keybind glyphs (`m`, `P`, `p`, `-`, `+`) inside the
//!   brackets render in the highlight colour to advertise the
//!   keypress affordance.
//! * **Right** — `[up Xd HH:MM]`, `[HH:MM:SS]` in declaration
//!   order, right-aligned so the rightmost glyph (the closing
//!   bracket of the last visible item) lands at
//!   `area.x + area.width - 1 - RIGHT_PAD`.
//!
//! The master `show_statusbar` config toggle is enforced at the
//! layout-engine layer (see `app::AppState::compose_hidden`) by
//! adding `WidgetKind::Statusbar` to the engine's `hidden` set
//! when off — the widget's row is reclaimed by parent containers
//! exactly like any other hidden widget. By the time `draw` is
//! reached the master toggle is necessarily on. Per-item
//! visibility, `update_ms`, the active preset name, the filter
//! indicator, and uptime arrive via `LayoutHints` and
//! [`crate::app::RenderParams`] — the renderer stays a pure
//! function of (frame, theme, area).
//!
//! No box drawing — the widget is a 1-row band, never a bordered
//! box. `draw_status_inset` is intentionally not invoked.

use crate::draw::box_drawing::symbols;
use crate::draw::buffer::AnsiBuffer;
use crate::theme::Theme;
use crate::theme_keys as tc;
use crate::tools;

use super::WidgetArea;

/// Per-frame view passed to [`draw`].
///
/// All values come from `RenderParams`; widths come pre-computed
/// on `LayoutHints` so the widget never re-derives them.
pub struct StatusbarFrame<'a> {
    pub show_menu: bool,
    pub show_preset: bool,
    pub show_update_interval: bool,
    pub show_uptime: bool,
    pub show_clock: bool,
    pub preset_name: &'a str,
    pub filter_active: bool,
    pub update_ms: u64,
    pub uptime_seconds: u64,
    pub clock_format: &'a str,
    /// Honor the global `ui.theme_background` toggle. When `false`,
    /// the bar paints over the terminal default background
    /// (`\x1b[49m`) instead of `STATUSBAR_BG` so users with
    /// terminal transparency see a continuously-transparent UI
    /// rather than an opaque band at the bottom. Foreground
    /// colours are unchanged — same convention `MAIN_BG` uses
    /// elsewhere.
    pub theme_background: bool,
}

/// Visible width of the `menu` keybind hint as rendered into the
/// statusbar (literal text "menu"). Pinned here so the
/// `min_width` calculation can compute it without re-deriving
/// from the colour-escape-laden render output.
pub const MENU_LABEL: &str = "menu";

/// Plain-text rendering of the preset-cycler item:
/// `← P NAME[*] p →`. Returned without ANSI escapes so the layout
/// engine can measure visible width via `tools::ulen`.
///
/// The renderer's [`format_preset_item`] produces a coloured
/// version of this same template; `preset_label_matches_format_preset_item_width`
/// pins the visible widths equal so the two cannot drift.
pub fn preset_label(preset_name: &str, filter_active: bool) -> String {
    let suffix = if filter_active { "*" } else { "" };
    format!(
        "{} P {}{} p {}",
        symbols::LEFT_ARROW,
        preset_name,
        suffix,
        symbols::RIGHT_ARROW,
    )
}

/// Plain-text rendering of the update-interval item:
/// `- Nms +`. See [`preset_label`] for the synchronisation
/// contract with the renderer.
pub fn update_label(update_ms: u64) -> String {
    format!("- {update_ms}ms +")
}

/// Plain-text rendering of the uptime item: `up XdHH:MM`. See
/// [`preset_label`] for the synchronisation contract with the
/// renderer.
pub fn uptime_label(uptime_seconds: u64) -> String {
    format!("up {}", tools::sec_to_dhms(uptime_seconds, false, true))
}

/// ANSI escape that resets only the background to the terminal's
/// default. Used when the user has turned off `theme_background`
/// so the statusbar follows the rest of the UI in letting the
/// terminal background show through.
const TERMINAL_DEFAULT_BG: &str = "\x1b[49m";

/// Render the statusbar into an ANSI string.
pub fn draw(area: &WidgetArea, theme: &Theme, frame: &StatusbarFrame) -> String {
    if area.width == 0 || area.height == 0 {
        return String::new();
    }

    // Background source follows the global `ui.theme_background`
    // toggle, mirroring how `Theme::base_style` handles `MAIN_BG`.
    // Foreground colours stay themed in both modes.
    let bg = if frame.theme_background {
        theme.background(tc::STATUSBAR_BG)
    } else {
        TERMINAL_DEFAULT_BG.to_string()
    };
    let fg = theme.color(tc::STATUSBAR_FG);
    let hi = theme.color(tc::STATUSBAR_HI);
    let sep = theme.color(tc::STATUSBAR_SEP);

    let mut buf = AnsiBuffer::new();

    // The layout engine uses 0-based coordinates (`area.x = 0`
    // is the leftmost widget); ANSI cursor positioning is
    // 1-based (column 1 is the first visible column). Every
    // widget converts at render time — `create_box` uses
    // `mv(x + 1, y + 1)` for the top-left border. The
    // borderless statusbar follows the same convention so its
    // first cell lands at the same column the widget above
    // would draw its leftmost border. The y axis adds an
    // additional +1 (already baked into the +1 below) so the
    // bar lands on its own row instead of colliding with the
    // bottom border of the widget above (`create_box` draws
    // its bottom at `mv(_, y + height)`).
    let start_x = area.x + 1;
    let row = area.y + 1;

    // Paint the 1-row background spanning the full width.
    buf.mv(start_x, row).text(&bg);
    buf.text(&" ".repeat(area.width));

    // Render left section anchored `LEFT_PAD` cells in from the
    // bar's left edge so items don't sit flush against whatever
    // lies to the left of the bar (terminal edge, or, in
    // custom layouts, the border of an adjacent column). Each
    // visible item is wrapped in `[ ]` brackets painted in
    // `STATUSBAR_SEP` (subtle in both `theme_background` modes);
    // adjacent visible items are joined by a single space.
    let (left_text, left_visible_width) = render_left(frame, &bg, fg, hi, sep);
    if left_visible_width > 0 {
        buf.mv(start_x + LEFT_PAD, row)
            .text(&bg)
            .text(fg)
            .text(&left_text);
    }

    // Render right section right-aligned with `RIGHT_PAD` cells
    // of padding from the bar's right edge so the clock doesn't
    // sit flush against whatever lies to the right of the bar.
    // Mirrors `LEFT_PAD` for visual symmetry.
    let (right_text, right_visible_width) = render_right(frame, &bg, fg, sep);
    if right_visible_width > 0 && right_visible_width + RIGHT_PAD <= area.width {
        let right_x = start_x + area.width - right_visible_width - RIGHT_PAD;
        buf.mv(right_x, row).text(&bg).text(fg).text(&right_text);
    }

    buf.finish()
}

/// Cells of horizontal padding between the bar's left edge and
/// the leftmost glyph of the left section.
const LEFT_PAD: usize = 1;
/// Cells of horizontal padding between the rightmost glyph of
/// the right section and the bar's right edge. Matches
/// [`LEFT_PAD`] for visual symmetry.
const RIGHT_PAD: usize = 1;
/// Visible-cell overhead a bracketed item adds: `[` + `]`.
const BRACKET_WIDTH: usize = 2;
/// Visible-cell width of the space between adjacent bracketed
/// items in a section. Each bracketed item is its own visual
/// chunk so a single space between is enough.
const ITEM_GAP: usize = 1;

/// Compose the left section from visible items in declaration
/// order, each wrapped in `[ ]` brackets and joined by a single
/// space. Returns `(ansi_text, visible_width)` so the caller can
/// position downstream content (the right section, or a future
/// centre-anchored item) without re-deriving the width. The
/// caller currently only needs the text.
fn render_left(frame: &StatusbarFrame, bg: &str, fg: &str, hi: &str, sep: &str) -> (String, usize) {
    let mut items: Vec<(String, usize)> = Vec::new();
    if frame.show_menu {
        items.push((format_menu_item(fg, hi), tools::ulen(MENU_LABEL)));
    }
    if frame.show_preset {
        let plain = preset_label(frame.preset_name, frame.filter_active);
        items.push((
            format_preset_item(frame.preset_name, frame.filter_active, fg, hi),
            tools::ulen(&plain),
        ));
    }
    if frame.show_update_interval {
        let plain = update_label(frame.update_ms);
        items.push((
            format_update_item(frame.update_ms, fg, hi),
            tools::ulen(&plain),
        ));
    }
    render_section(&items, bg, fg, sep)
}

/// Compose the right section. Returns `(ansi_text, visible_width)`
/// so the caller can right-align the section without re-deriving
/// the width.
fn render_right(frame: &StatusbarFrame, bg: &str, fg: &str, sep: &str) -> (String, usize) {
    let mut items: Vec<(String, usize)> = Vec::new();
    if frame.show_uptime {
        let label = uptime_label(frame.uptime_seconds);
        let w = tools::ulen(&label);
        if w > 0 {
            items.push((label, w));
        }
    }
    if frame.show_clock {
        let label = tools::format_clock(frame.clock_format);
        let w = tools::ulen(&label);
        if w > 0 {
            items.push((label, w));
        }
    }
    render_section(&items, bg, fg, sep)
}

/// Render a section's items as `[ ]`-bracketed chunks joined by
/// single spaces. Each item's `text` may contain its own colour
/// escapes (the left section's items do, since keybind glyphs
/// render in HI mid-word); the bracket glyphs are painted in
/// `sep` and the inter-item gap is plain `bg`. `width` is the
/// pre-computed visible width of `text` (with any embedded ANSI
/// stripped); summing it lets the caller right-align without
/// re-walking the bytes.
fn render_section(items: &[(String, usize)], bg: &str, fg: &str, sep: &str) -> (String, usize) {
    if items.is_empty() {
        return (String::new(), 0);
    }
    let mut out = String::new();
    let mut total = 0usize;
    for (i, (text, w)) in items.iter().enumerate() {
        if i > 0 {
            // Single space between bracketed items, painted on
            // the bar's background.
            out.push_str(bg);
            out.push(' ');
            total += ITEM_GAP;
        }
        // `[` + content + `]` — brackets in `sep`, content in `fg`
        // (or whatever colour escapes `text` already carries).
        out.push_str(bg);
        out.push_str(sep);
        out.push('[');
        out.push_str(bg);
        out.push_str(fg);
        out.push_str(text);
        out.push_str(bg);
        out.push_str(sep);
        out.push(']');
        total += BRACKET_WIDTH + w;
    }
    (out, total)
}

fn format_menu_item(fg: &str, hi: &str) -> String {
    // "menu" — `m` is the HI keybind, `enu` stays FG. Mirrors
    // the CPU widget's previous `keybind_inset` contract; the
    // 'm' key opens the main menu (see
    // `handlers/normal.rs::open_main_menu_action`).
    format!("{hi}m{fg}enu")
}

fn format_preset_item(preset_name: &str, filter_active: bool, fg: &str, hi: &str) -> String {
    let suffix = if filter_active { "*" } else { "" };
    // `← P NAME[*] p →` — keybind letters in HI, rest in FG.
    format!(
        "{fg}{} {hi}P{fg} {preset_name}{suffix} {hi}p{fg} {}",
        symbols::LEFT_ARROW,
        symbols::RIGHT_ARROW,
    )
}

fn format_update_item(update_ms: u64, fg: &str, hi: &str) -> String {
    // `- Nms +` — `-` / `+` are rate-down/up keybinds (see
    // `handlers/normal.rs`); both render in HI and the label stays
    // FG. The borderless statusbar uses literal ASCII `-` / `+`,
    // not the CPU-widget inset's U+2500 box-drawing glyph.
    format!("{hi}-{fg} {update_ms}ms {hi}+{fg}")
}

pub struct StatusbarWidget;

impl super::Widget for StatusbarWidget {
    fn kinds(&self) -> &'static [crate::domain::widget_kind::WidgetKind] {
        const KINDS: &[crate::domain::widget_kind::WidgetKind] =
            &[crate::domain::widget_kind::WidgetKind::Statusbar];
        KINDS
    }

    fn preferred_height(&self, _: &crate::draw::layout::LayoutHints) -> usize {
        1
    }

    fn min_width(&self, hints: &crate::draw::layout::LayoutHints) -> usize {
        // Sum of visible-item widths, with each item wrapped in
        // `[ ]` brackets (BRACKET_WIDTH = 2 cells per item) and
        // adjacent items separated by a single space (ITEM_GAP).
        // The engine integrates this into `min_terminal_size` so
        // the existing too-small gate fires when the bar can't
        // fit.
        //
        // The master `show_statusbar` toggle does NOT need a guard
        // here: when off, the engine has `WidgetKind::Statusbar`
        // in its `hidden` set (composed by
        // `app::AppState::compose_hidden`) and `place()` skips the
        // widget entirely before reaching `min_width`.
        let mut left_items: Vec<usize> = Vec::new();
        if hints.statusbar_show_menu {
            left_items.push(tools::ulen(MENU_LABEL));
        }
        if hints.statusbar_show_preset && hints.statusbar_preset_label_width > 0 {
            left_items.push(hints.statusbar_preset_label_width);
        }
        if hints.statusbar_show_update_interval && hints.statusbar_update_label_width > 0 {
            left_items.push(hints.statusbar_update_label_width);
        }
        let left_w = sum_bracketed(&left_items);

        let mut right_items: Vec<usize> = Vec::new();
        if hints.statusbar_show_uptime && hints.statusbar_uptime_label_width > 0 {
            right_items.push(hints.statusbar_uptime_label_width);
        }
        if hints.statusbar_show_clock && hints.statusbar_clock_label_width > 0 {
            right_items.push(hints.statusbar_clock_label_width);
        }
        let right_w = sum_bracketed(&right_items);

        // When both sections have content, the engine demands
        // they fit *side by side*. A single space between is the
        // minimum visual separation; the interior gap inflates
        // the rest of the bar to whatever extra width the
        // terminal provides. Each side is also offset from its
        // edge by `LEFT_PAD` / `RIGHT_PAD` cells; that padding
        // counts toward the bar's minimum width whenever the
        // corresponding section is present.
        let separator = if left_w > 0 && right_w > 0 { 1 } else { 0 };
        let left_padding = if left_w > 0 { LEFT_PAD } else { 0 };
        let right_padding = if right_w > 0 { RIGHT_PAD } else { 0 };
        left_padding + left_w + separator + right_w + right_padding
    }

    fn min_height(&self, _: &crate::draw::layout::LayoutHints) -> usize {
        1
    }

    fn render(&self, params: &crate::app::RenderParams<'_>, output: &mut String) {
        let Some(dim) = params
            .layout
            .dims_for(crate::domain::widget_kind::WidgetKind::Statusbar)
        else {
            return;
        };
        let area = super::WidgetArea::from_dim(dim, params.rounded);
        let uptime_seconds = params.statusbar.map_or(0, |s| s.info.uptime_seconds);
        let frame = StatusbarFrame {
            show_menu: params.config.statusbar.statusbar_show_menu,
            show_preset: params.config.statusbar.statusbar_show_preset,
            show_update_interval: params.config.statusbar.statusbar_show_update_interval,
            show_uptime: params.config.statusbar.statusbar_show_uptime,
            show_clock: params.config.statusbar.statusbar_show_clock,
            preset_name: params.config.active_preset().name(),
            filter_active: params.filter_active,
            update_ms: params.update_ms,
            uptime_seconds,
            clock_format: &params.config.statusbar.statusbar_clock_format,
            theme_background: params.config.ui.theme_background,
        };
        output.push_str(&draw(&area, params.theme, &frame));
    }
}

/// Width of a section composed of bracketed items joined by
/// single spaces: each item contributes `content + BRACKET_WIDTH`
/// cells, and there's an `ITEM_GAP` between every adjacent pair.
fn sum_bracketed(items: &[usize]) -> usize {
    if items.is_empty() {
        return 0;
    }
    items.iter().map(|w| w + BRACKET_WIDTH).sum::<usize>() + (items.len() - 1) * ITEM_GAP
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::layout::LayoutHints;
    use crate::theme::Theme;

    fn full_hints() -> LayoutHints {
        LayoutHints {
            statusbar_show_menu: true,
            statusbar_show_preset: true,
            statusbar_show_update_interval: true,
            statusbar_show_uptime: true,
            statusbar_show_clock: true,
            statusbar_preset_label_width: 18,
            statusbar_update_label_width: 9,
            statusbar_uptime_label_width: 11,
            statusbar_clock_label_width: 8,
            ..LayoutHints::default()
        }
    }

    fn area(width: usize) -> WidgetArea {
        WidgetArea {
            x: 0,
            y: 0,
            width,
            height: 1,
            rounded: true,
        }
    }

    fn full_frame<'a>() -> StatusbarFrame<'a> {
        StatusbarFrame {
            show_menu: true,
            show_preset: true,
            show_update_interval: true,
            show_uptime: true,
            show_clock: true,
            preset_name: "all",
            filter_active: false,
            update_ms: 2000,
            uptime_seconds: 86_400,
            clock_format: "",
            theme_background: true,
        }
    }

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Visible-cell width of `format_*_item`'s output, derived by
    /// stripping the ANSI escapes and counting glyphs.
    fn formatted_item_width(formatted: &str) -> usize {
        tools::ulen(&strip_ansi(formatted))
    }

    #[test]
    fn menu_label_matches_format_menu_item_width() {
        // Pin the synchronisation contract between the plain
        // `MENU_LABEL` (used by `LayoutHints` width math) and
        // `format_menu_item` (used by the renderer): they must
        // produce equal visible widths.
        let theme = Theme::new();
        let fg = theme.color(tc::STATUSBAR_FG);
        let hi = theme.color(tc::STATUSBAR_HI);
        assert_eq!(
            tools::ulen(MENU_LABEL),
            formatted_item_width(&format_menu_item(fg, hi)),
        );
    }

    #[test]
    fn preset_label_matches_format_preset_item_width() {
        // Same contract for the preset cycler. Cover both the
        // filter-active and filter-inactive shapes.
        let theme = Theme::new();
        let fg = theme.color(tc::STATUSBAR_FG);
        let hi = theme.color(tc::STATUSBAR_HI);
        for filter_active in [false, true] {
            for name in ["all", "default", "minimal", "very-long-preset-name"] {
                assert_eq!(
                    tools::ulen(&preset_label(name, filter_active)),
                    formatted_item_width(&format_preset_item(name, filter_active, fg, hi)),
                    "drift on name={name:?} filter_active={filter_active}",
                );
            }
        }
    }

    #[test]
    fn update_label_matches_format_update_item_width() {
        let theme = Theme::new();
        let fg = theme.color(tc::STATUSBAR_FG);
        let hi = theme.color(tc::STATUSBAR_HI);
        for ms in [50u64, 100, 500, 1_000, 2_000, 10_000, 999_999] {
            assert_eq!(
                tools::ulen(&update_label(ms)),
                formatted_item_width(&format_update_item(ms, fg, hi)),
                "drift at update_ms={ms}",
            );
        }
    }

    #[test]
    fn uptime_label_matches_render_right_uptime_width() {
        // The right section's uptime item is built inline in
        // `render_right` — pin that the inline format matches the
        // exposed `uptime_label` helper used by the layout engine.
        for secs in [0u64, 1, 60, 3_600, 86_400, 86_400 * 30] {
            let plain = uptime_label(secs);
            let inline = format!("up {}", tools::sec_to_dhms(secs, false, true));
            assert_eq!(
                tools::ulen(&plain),
                tools::ulen(&inline),
                "drift at uptime_seconds={secs}",
            );
        }
    }

    #[test]
    fn renders_one_row_below_area_y_to_avoid_border_collision() {
        // Regression: previously the renderer painted at `area.y`,
        // colliding with the bottom border of whichever widget the
        // layout placed above (e.g. the disk + proc widgets in the
        // `all` preset). Every other widget paints starting at
        // `area.y + 1` (`create_box` draws its top border at
        // `y + 1`); the borderless statusbar must follow the same
        // convention so it lands on its own row below.
        let theme = Theme::new();
        let frame = full_frame();
        // Place the bar at y = 5 so the assertion isn't satisfied
        // by both `[0;...H` and `[1;...H` happening to appear.
        let area = WidgetArea {
            x: 0,
            y: 5,
            width: 80,
            height: 1,
            rounded: true,
        };
        let raw = draw(&area, &theme, &frame);
        // Expect at least one move to row 6 (= area.y + 1).
        assert!(
            raw.contains("\x1b[6;"),
            "expected paint at row 6 (area.y + 1); raw: {raw:?}",
        );
        // And no paints to row 5 (= area.y), which would collide
        // with the widget above's bottom border.
        assert!(
            !raw.contains("\x1b[5;"),
            "must not paint at row 5 (area.y) — would collide with widget border above",
        );
    }

    #[test]
    fn renders_empty_band_when_all_items_off() {
        let theme = Theme::new();
        let mut frame = full_frame();
        frame.show_menu = false;
        frame.show_preset = false;
        frame.show_update_interval = false;
        frame.show_uptime = false;
        frame.show_clock = false;
        let plain = strip_ansi(&draw(&area(40), &theme, &frame));
        // The rendered band is exactly the width-spanning paint;
        // no sub-item glyphs appear.
        assert!(plain.contains("                                        "));
        assert!(!plain.contains("menu"));
        assert!(!plain.contains("up "));
    }

    #[test]
    fn left_section_renders_visible_items_in_order() {
        let theme = Theme::new();
        let plain = strip_ansi(&draw(&area(80), &theme, &full_frame()));
        // menu before preset before update
        let menu_pos = plain.find("menu").expect("menu present");
        let preset_pos = plain.find("← P all p →").expect("preset present");
        let rate_pos = plain.find("- 2000ms +").expect("rate present");
        assert!(menu_pos < preset_pos);
        assert!(preset_pos < rate_pos);
    }

    #[test]
    fn omitting_preset_omits_the_filter_star_too() {
        let theme = Theme::new();
        let mut frame = full_frame();
        frame.show_preset = false;
        frame.filter_active = true;
        let plain = strip_ansi(&draw(&area(80), &theme, &frame));
        assert!(!plain.contains("←"), "preset hidden: arrow must be absent");
        assert!(!plain.contains("*"), "no preset means no filter star");
    }

    #[test]
    fn filter_active_appends_star_when_show_preset_on() {
        let theme = Theme::new();
        let mut frame = full_frame();
        frame.filter_active = true;
        let plain = strip_ansi(&draw(&area(80), &theme, &frame));
        assert!(plain.contains("all*"), "expected 'all*' in: {plain}");
    }

    #[test]
    fn menu_keybind_letter_renders_in_hi_color() {
        // Regression: the `m` in `menu` is the keybind that opens
        // the main menu and must render in STATUSBAR_HI to
        // advertise the keypress. The remaining `enu` stays in
        // STATUSBAR_FG.
        let theme = Theme::new();
        let raw = draw(&area(80), &theme, &full_frame());
        let hi = theme.color(tc::STATUSBAR_HI);
        let fg = theme.color(tc::STATUSBAR_FG);
        // `<HI>m<FG>enu` is the contract.
        let expected = format!("{hi}m{fg}enu");
        assert!(
            raw.contains(&expected),
            "menu must render `m` in HI then `enu` in FG; got: {raw:?}",
        );
    }

    #[test]
    fn rate_keybind_glyphs_render_in_hi_color() {
        // Regression: both `-` and `+` are rate-down/rate-up
        // keybinds; both must render in STATUSBAR_HI. The
        // borderless statusbar uses literal ASCII `-` here, NOT
        // the U+2500 box-drawing horizontal that the earlier
        // CPU-widget border inset used.
        let theme = Theme::new();
        let raw = draw(&area(80), &theme, &full_frame());
        let hi = theme.color(tc::STATUSBAR_HI);
        let fg = theme.color(tc::STATUSBAR_FG);
        let expected_minus = format!("{hi}-{fg}");
        let expected_plus = format!("{hi}+{fg}");
        assert!(
            raw.contains(&expected_minus),
            "rate-down `-` must render in HI then return to FG; got: {raw:?}",
        );
        assert!(
            raw.contains(&expected_plus),
            "rate-up `+` must render in HI then return to FG; got: {raw:?}",
        );
    }

    #[test]
    fn bracket_glyphs_wrap_each_visible_item_in_sep_color() {
        // Each visible item is wrapped in `[ ]` brackets painted
        // in STATUSBAR_SEP. Pin the contract so a future change
        // can't silently drop the brackets or mis-colour them.
        // With 3 left items + 2 right items = 5 items, we expect
        // 5 `<bg><sep>[` opens and 5 `<bg><sep>]` closes.
        let theme = Theme::new();
        let mut frame = full_frame();
        frame.clock_format = "%T"; // forces a non-zero clock
        let raw = draw(&area(80), &theme, &frame);
        let sep = theme.color(tc::STATUSBAR_SEP);
        let open_count = raw.matches(&format!("{sep}[")).count();
        let close_count = raw.matches(&format!("{sep}]")).count();
        // 3 left items + 2 right items = 5 bracketed items.
        assert_eq!(
            open_count, 5,
            "expected 5 `[` brackets in SEP colour; got {open_count} in: {raw:?}",
        );
        assert_eq!(
            close_count, 5,
            "expected 5 `]` brackets in SEP colour; got {close_count} in: {raw:?}",
        );
    }

    #[test]
    fn bg_paints_statusbar_bg_when_theme_background_on() {
        // Default `theme_background = true` → the statusbar's
        // background colour is the absolute `STATUSBAR_BG` from
        // the theme palette. Pin the contract.
        let theme = Theme::new();
        let raw = draw(&area(80), &theme, &full_frame());
        let expected_bg = theme.background(tc::STATUSBAR_BG);
        assert!(
            raw.contains(&expected_bg),
            "expected STATUSBAR_BG escape {expected_bg:?} in: {raw:?}",
        );
        assert!(
            !raw.contains(TERMINAL_DEFAULT_BG),
            "must not emit terminal-default-bg escape when theme_background is on",
        );
    }

    #[test]
    fn bg_paints_terminal_default_when_theme_background_off() {
        // When the user has turned off `ui.theme_background` (the
        // global "let my terminal background show through" toggle),
        // the statusbar must emit `\x1b[49m` instead of its
        // STATUSBAR_BG so the bar honours the same transparency
        // semantics as the rest of the UI.
        let theme = Theme::new();
        let mut frame = full_frame();
        frame.theme_background = false;
        let raw = draw(&area(80), &theme, &frame);
        assert!(
            raw.contains(TERMINAL_DEFAULT_BG),
            "expected terminal-default-bg escape {TERMINAL_DEFAULT_BG:?} in: {raw:?}",
        );
        let themed_bg = theme.background(tc::STATUSBAR_BG);
        assert!(
            !raw.contains(&themed_bg),
            "must not emit STATUSBAR_BG when theme_background is off",
        );
    }

    #[test]
    fn fg_colours_unchanged_when_theme_background_off() {
        // Foreground colours stay themed when theme_background is
        // off — only the background source changes. Mirrors how
        // `MAIN_FG` always paints regardless of `MAIN_BG`'s
        // visibility.
        let theme = Theme::new();
        let mut frame = full_frame();
        frame.theme_background = false;
        let raw = draw(&area(80), &theme, &frame);
        let fg = theme.color(tc::STATUSBAR_FG);
        let hi = theme.color(tc::STATUSBAR_HI);
        assert!(
            raw.contains(fg),
            "STATUSBAR_FG escape {fg:?} must still appear in: {raw:?}",
        );
        // The keybind highlights (`m`, `P`, `p`, `-`, `+`) all
        // render in HI; assert HI still appears.
        assert!(
            raw.contains(hi),
            "STATUSBAR_HI escape {hi:?} must still appear in: {raw:?}",
        );
    }

    #[test]
    fn right_section_omits_clock_when_format_empty() {
        let theme = Theme::new();
        let frame = full_frame();
        let plain = strip_ansi(&draw(&area(80), &theme, &frame));
        // Empty clock_format → format_clock returns "", so no
        // clock string is concatenated into the right section.
        // The uptime label "up 1d00:00" contains a `:` so we must
        // not test by counting `:`. Instead, verify the trailing
        // glyphs are the uptime label, not a clock.
        assert!(plain.contains("up "), "uptime present");
        // The full clock format expansion would produce 8 chars
        // matching `\d\d:\d\d:\d\d`. With empty format the bar
        // stops at the uptime — nothing follows.
        let after_uptime = plain.rsplit("up ").next().expect("uptime tag is present");
        // After the uptime label we expect only its dhms digits
        // and trailing background spaces — no second `:`-bearing
        // group attached.
        let trailing_groups = after_uptime.matches(':').count();
        // sec_to_dhms with no_seconds=true produces `Xd00:00` →
        // exactly one `:` after the `d`. A clock would add 2 more
        // (`HH:MM:SS`), pushing the count to 3.
        assert_eq!(
            trailing_groups, 1,
            "no clock when format empty; trailing: {after_uptime:?}"
        );
    }

    #[test]
    fn right_section_renders_uptime_left_of_clock() {
        let theme = Theme::new();
        let mut frame = full_frame();
        frame.clock_format = "%T";
        // %T expands to %H:%M:%S — exact value depends on local
        // time; we assert structural ordering only.
        let plain = strip_ansi(&draw(&area(80), &theme, &frame));
        let up_pos = plain.find("up ").expect("uptime present");
        let clock_pos = plain.rfind(":").expect("clock present");
        assert!(up_pos < clock_pos);
    }

    #[test]
    fn right_section_anchors_one_cell_in_from_right_edge() {
        // The right section is positioned by the renderer via
        // `mv(right_x, row)` with
        // `right_x = start_x + area.width - visible_width - RIGHT_PAD`
        // (and `start_x = area.x + 1` to convert 0-based layout
        // coords to 1-based ANSI coords). RIGHT_PAD = 1 mirrors
        // LEFT_PAD so the rightmost glyph (the closing `]` of the
        // last item) doesn't sit flush against the bar's right
        // edge.
        let theme = Theme::new();
        let mut frame = full_frame();
        frame.clock_format = "%T"; // forces a non-zero clock
        let width = 80;
        let raw = draw(&area(width), &theme, &frame);
        // Right section width with brackets:
        //   [up 1d00:00]  = 10 + 2 = 12
        //   single space  =          1
        //   [HH:MM:SS]    =  8 + 2 = 10
        //   total                  = 23
        // start_x = area.x + 1 = 1. row = area.y + 1 = 1.
        // right_x = 1 + 80 - 23 - 1 = 57. So the escape is
        // `\x1b[1;57H` and the rightmost glyph (`]` of clock)
        // lands at column 79 (one cell in from column 80).
        let expected = "\x1b[1;57H";
        assert!(
            raw.contains(expected),
            "expected right-section move {expected:?}; got: {raw:?}",
        );
    }

    #[test]
    fn min_height_and_preferred_height_are_one() {
        let widget = StatusbarWidget;
        let hints = LayoutHints::default();
        assert_eq!(
            <StatusbarWidget as super::super::Widget>::min_height(&widget, &hints),
            1
        );
        assert_eq!(
            <StatusbarWidget as super::super::Widget>::preferred_height(&widget, &hints),
            1
        );
    }

    #[test]
    fn min_width_is_zero_when_every_item_off() {
        let widget = StatusbarWidget;
        let hints = LayoutHints::default();
        assert_eq!(
            <StatusbarWidget as super::super::Widget>::min_width(&widget, &hints),
            0
        );
    }

    #[test]
    fn min_width_sums_bracketed_items_with_section_separator_and_left_padding() {
        let widget = StatusbarWidget;
        let hints = full_hints();
        // Each item is wrapped in `[ ]` (BRACKET_WIDTH = 2) and
        // adjacent items are joined by a single space (ITEM_GAP = 1).
        // l_pad = 1 (LEFT_PAD)
        // left  = (4+2)[menu] + 1 + (18+2)[preset] + 1 + (9+2)[rate]
        //       = 6 + 1 + 20 + 1 + 11                            = 39
        // sep   = 1 (between left and right sections)
        // right = (11+2)[uptime] + 1 + (8+2)[clock]
        //       = 13 + 1 + 10                                    = 24
        // r_pad = 1 (RIGHT_PAD)
        // total = 1 + 39 + 1 + 24 + 1                            = 66
        assert_eq!(
            <StatusbarWidget as super::super::Widget>::min_width(&widget, &hints),
            66
        );
    }

    #[test]
    fn min_width_excludes_hidden_items_and_their_brackets() {
        let widget = StatusbarWidget;
        let mut hints = full_hints();
        hints.statusbar_show_preset = false;
        hints.statusbar_preset_label_width = 0;
        // l_pad = 1
        // left  = (4+2)[menu] + 1 + (9+2)[rate] = 6 + 1 + 11    = 18
        // sep   = 1
        // right = (11+2) + 1 + (8+2) = 13 + 1 + 10              = 24
        // r_pad = 1
        // total                                                  = 45
        assert_eq!(
            <StatusbarWidget as super::super::Widget>::min_width(&widget, &hints),
            45
        );
    }

    #[test]
    fn min_width_drops_section_separator_when_only_left_section_present() {
        let widget = StatusbarWidget;
        let mut hints = full_hints();
        hints.statusbar_show_uptime = false;
        hints.statusbar_show_clock = false;
        hints.statusbar_uptime_label_width = 0;
        hints.statusbar_clock_label_width = 0;
        // l_pad = 1
        // left  = (4+2) + 1 + (18+2) + 1 + (9+2) = 6+1+20+1+11   = 39
        // sep   = 0 (right is empty)
        // right = 0
        // r_pad = 0
        // total                                                   = 40
        assert_eq!(
            <StatusbarWidget as super::super::Widget>::min_width(&widget, &hints),
            40
        );
    }

    #[test]
    fn min_width_omits_left_padding_when_left_section_empty() {
        // No left items, only right items: there's no left edge
        // to pad, so the bar's `min_width` is just the right-side
        // content plus the right padding.
        let widget = StatusbarWidget;
        let mut hints = full_hints();
        hints.statusbar_show_menu = false;
        hints.statusbar_show_preset = false;
        hints.statusbar_show_update_interval = false;
        hints.statusbar_preset_label_width = 0;
        hints.statusbar_update_label_width = 0;
        // right = (11+2) + 1 + (8+2) = 13+1+10 = 24, r_pad = 1
        // total = 25
        assert_eq!(
            <StatusbarWidget as super::super::Widget>::min_width(&widget, &hints),
            25
        );
    }

    #[test]
    fn left_section_starts_one_cell_in_from_area_x() {
        // Regression: the layout engine's `area.x` is 0-based;
        // ANSI is 1-based. The renderer translates `area.x → area.x + 1`
        // (so `area.x = 0` lands at ANSI column 1, the first
        // visible cell) AND adds `LEFT_PAD = 1` so the leftmost
        // glyph (`menu`'s `m`) sits one cell in from the bar's
        // edge, never flush against it.
        //
        // Place the bar at area.x = 10 so the assertion can
        // distinguish `area.x` (which would reach ANSI col 11)
        // from `area.x + 1 + LEFT_PAD` (ANSI col 12).
        let theme = Theme::new();
        let frame = full_frame();
        let area = WidgetArea {
            x: 10,
            y: 0,
            width: 80,
            height: 1,
            rounded: true,
        };
        let raw = draw(&area, &theme, &frame);
        // start_x = area.x + 1 = 11. left section anchor =
        // start_x + LEFT_PAD = 12. The escape is `\x1b[1;12H`.
        assert!(
            raw.contains("\x1b[1;12H"),
            "left section must start at area.x + 1 + LEFT_PAD = 12; raw: {raw:?}",
        );
        // Background fill must paint from start_x = 11 (the bar's
        // leftmost visible cell). Without this the leftmost cell
        // is left blank with the terminal default colour.
        assert!(
            raw.contains("\x1b[1;11H"),
            "background fill must paint from start_x = 11; raw: {raw:?}",
        );
        // And there must be no paint at column 10 — that's outside
        // the bar (it's the column where the layout-engine `area.x`
        // sits before ANSI translation).
        assert!(
            !raw.contains("\x1b[1;10H"),
            "must not paint at column 10 (untranslated area.x); raw: {raw:?}",
        );
    }
}
