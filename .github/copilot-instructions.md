# Copilot Instructions for rtop

## Build, Test, Lint

```powershell
cargo build --release          # Build optimized binary
cargo test                     # Run all 214+ tests
cargo test proc_box            # Run tests matching a name
cargo clippy --release         # Lint — zero warnings required
cargo fmt -- --check           # Format check
```

All four must pass with zero warnings before committing.

## Warning Policy — Zero Tolerance

Never suppress compiler or clippy warnings. Specifically:

- **No `#[allow(dead_code)]`** — if code is unused, delete it. If it's
  test-only, gate it with `#[cfg(test)]`.
- **No `#[allow(unused_*)]`** — no unused variables, imports, fields,
  or parameters. Remove them or use them.
- **No `#[allow(clippy::*)]`** — fix the root cause. If clippy says
  too many arguments, extract a struct. If it flags a pattern, refactor.
- **No `_` prefix to hide unused variables** — `_foo` is not a fix.
  Either use the variable or remove it.
- **No `#[allow(warnings)]` or `#![allow(...)]`** at any scope.

If code exists, it must be reachable and used. Dead feature scaffolding
(config fields, enum variants, UI options for unimplemented features)
must not be checked in — add it when the feature is built, not before.

## Commit Style

Use Conventional Commit subjects that match the existing history:

```text
feat: add disk performance metrics
fix: rename GPU MHz label to Clock
refactor: decouple handlers from terminal via HandleResult
release: v0.3.0
```

- Keep the subject concise, imperative, and under 72 characters.
- Use a blank line between the subject, body, and trailers.
- Wrap commit body lines at 72 characters.
- Use the body to explain what changed and why when the subject is not
  enough.
- Do not wrap Git trailers.
- Include the Copilot co-author trailer when Copilot creates the commit:

```text
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

## Architecture

### Data flow: Collection → Domain → Rendering

```
collect/*.rs  →  domain/*.rs  →  ui/*_box.rs  →  terminal
(Win32 APIs)     (typed structs)  (AnsiBuffer)    (crossterm)
```

- **`collect/`**: One collector per subsystem (CPU, memory, disk, network, GPU, process). All implement `trait Collector { fn collect(&mut self); }`. Data collected via Windows APIs, stored in public domain struct fields.
- **`domain/`**: Pure data types with typed structs (no `HashMap<String, T>` for structured data). E.g., `CpuPercent { total, user, system, idle }` not `HashMap<String, VecDeque<i64>>`.
- **`ui/`**: One renderer per widget. Each takes domain data + settings struct + theme, returns ANSI string via `AnsiBuffer`.
- **`draw/`**: Shared rendering primitives — `AnsiBuffer`, `Graph`, `Meter`, box drawing, layout engine.

### Event loop (`app.rs`)

Three-phase loop with `Dirty` bitflags:

1. **Detect dirty** — timer tick, resize, keybinds set flags
2. **Execute** — collect data (`COLLECT`), rebuild proc list (`PROC_LIST`), recalculate layout (`LAYOUT`)
3. **Render** — only redraw boxes whose dirty flag is set

Input handling is split into 5 handler functions per `MenuState`, each taking `&mut InputContext`.

### Key abstractions

- **`Dirty` bitflags** — Per-box dirty tracking. Keybinds set the minimum flags needed (e.g., `PROC_BOX` for arrow keys, `LAYOUT | ALL_BOXES` for box toggles).
- **`AnsiBuffer`** — Fluent builder for ANSI output. All UI rendering uses `.mv()`, `.color()`, `.text()`, `.finish()`. No raw `\x1b[` escapes in UI code.
- **`render_all()`** — Extracted render function callable from both the main loop and menu transitions.
- **Settings structs** — `CpuBoxSettings`, `NetBoxSettings`, `GpuBoxSettings` decouple renderers from `Config`. Config reads happen in `app.rs`, not in renderers.

## Conventions

### Typed key constants — no magic strings

Config keys use `config_keys.rs` constants:
```rust
use crate::config_keys::{bool_keys as bk, str_keys as sk, int_keys as ik};
config.get_bool(bk::SHOW_SWAP)   // not config.get_bool("show_swap")
```

Theme color keys use `theme_keys.rs` constants:
```rust
use crate::theme_keys as tc;
theme.c(tc::MAIN_FG)   // not theme.c("main_fg")
```

### Border insets — use helpers, never manual assembly

Title insets on box borders use shared functions in `draw/box_drawing.rs`:
```rust
box_drawing::title_inset(text, border_color, text_color, bottom)
box_drawing::keybind_inset(text, border_color, hi_color, text_color, bottom)
box_drawing::section_divider(section, width, border_color, text_color)
box_drawing::inset_width(text)           // visible width of an inset
box_drawing::right_inset_x(x, w, vis)   // X position for right-edge placement
```

No UI file should directly reference `title_syms::TITLE_LEFT` etc.

### Box content positioning

Content inside a box starts at `x + 3` (border, space, content) and ends at `x + width - 2` (content, space, border). Usable content width is `width - 4`.

### Widget heights are data-driven

Layout engine (`draw/layout.rs`) receives `disk_count`, `has_swap`, `core_count` etc. in `LayoutConfig` and sizes widgets to fit their content. Net box gets remaining space.

### Themes

40 bundled themes loaded via `include_str!`. All colors derive from theme — no hardcoded ANSI escapes in production code. The default theme is built-in as a fallback. Theme files can define `gpu_box`, `disk_box`, `help_box`, `options_box`, `proc_tree_fg` and other widget-specific colors.

### Process display pipeline

Raw processes (`procs`) are never modified in place. `rebuild_display()` clones raw → sorts → filters → tree-builds into `display_procs`. In tree mode, sort applies within each parent group, then `sort_by_key(tree_index)` flattens for display order.

### Releases

`cargo release patch|minor|major --execute` bumps version, commits, tags, pushes. GitHub Action (`windows-release.yml`) builds on `windows-latest`, runs tests, creates GitHub Release with zipped exe.
