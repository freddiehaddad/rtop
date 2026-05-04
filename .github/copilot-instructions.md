# Copilot Instructions for rtop

rtop is a terminal-based system monitor for **Windows only**, written in
Rust 2024 edition (MSRV 1.95) using native Win32 APIs (`windows` crate),
`crossterm` for terminal I/O, and `tracing` for logging. The UI is
inspired by btop. The codebase is organised as
`collect → domain → ui → terminal` with per-box dirty rendering and
per-collector threads. There are no Linux/macOS code paths and none
should be added.

## Behavioral Contract — read first

These rules govern *how you work*, not *what the code looks like*. They
override any conflicting instinct from your training distribution. If
you cannot satisfy a rule, stop and tell the user — do not silently
relax it.

### Never act outside the user's explicit instruction

- **Never run `git commit`, `git push`, `git rebase`, `git reset --hard`,
  `git checkout -- .`, `git clean`, or any other history-altering or
  remote-mutating command** unless the user has given an explicit
  instruction in the *current turn* ("commit this", "push it", "amend
  the last commit"). Staging files (`git add`), running `git status`,
  and showing a diff are not approval. When in doubt, stop and ask.
- **Never run `cargo release`, `cargo publish`, or any release tooling.**
  Releases are a human action.
- **Never modify files outside the scope the user requested.** If you
  notice a pre-existing bug adjacent to your change, surface it — do
  not silently fix it.

### Never assume, never guess

- If the spec is silent on an edge case, two valid approaches exist, or
  a change crosses a module boundary in a way without precedent in the
  codebase, **stop and ask** using a clarifying question.
- If you do not know how a Win32 API, vendor SDK call, or `crossterm`
  function behaves in a specific case, **read the documentation or the
  existing call sites — do not speculate**. The codebase already
  exercises most of the surface area you will need; find the precedent
  before inventing one.
- "I think this probably works" is not acceptable reasoning. Either you
  can point to evidence (a doc, a precedent, a passing test), or you
  ask.

### No shortcuts, no placeholders, no "works for now"

- Every change you propose must be **production-grade and ready to
  ship**. No `// TODO`, no `// FIXME`, no `unimplemented!()`, no
  `println!("debug: …")`, no commented-out code, no scaffolding for
  features that are not in the current request.
- If you find a quick fix that papers over a deeper problem, **surface
  the deeper problem and ask** before papering. Do not introduce a
  silent patch.
- Partial solutions are not acceptable. If the request decomposes into
  parts you cannot all complete, deliver the parts you can and
  explicitly enumerate what is missing and why.

Concrete proxies that make this checkable:

- **No placeholder return values** (`Ok(Default::default())`,
  `vec![]`, `None`) used to satisfy the type checker for an
  unfinished branch.
- **No string-typed state** when the set of values is known —
  introduce or extend an enum.
- **No new allocation or dynamic dispatch** without a one-line
  justification for why borrowing or static dispatch was not viable.
- **No new compatibility-parsing** (accepting both an old and new
  shape for the same input). Pick one shape; migrate the other.
- **No "leave it for later"** — if the change requires a follow-up,
  state the follow-up to the user and let them decide whether to
  expand the current scope or open an issue. Do not check in a
  comment that promises the follow-up.

### Definition of Done

A change is not complete until **all** of the following pass with zero
warnings on Windows:

```powershell
cargo build --release
cargo test
cargo clippy --release --all-targets -- -D warnings
cargo fmt -- --check
```

In addition, before declaring work complete, verify:

- No new lint-suppression attribute was added at any scope. This
  includes `#[allow(...)]`, `#[expect(...)]`, `#[cfg_attr(_, allow(_))]`,
  `#[cfg_attr(_, expect(_))]`, and any inner-attribute (`#![...]`)
  variant of the same.
- No new `.unwrap()`, `todo!`, or `unimplemented!` outside test code.
- No new `panic!` or `unreachable!` outside test code unless the call
  guards a logically-unreachable branch and is paired with a comment
  or message naming the proven invariant (see *`.unwrap()` and panics
  outside tests* below for the full rule).
- No new `let _ = …` on a fallible call without justification (see
  Logging conventions).
- No dead code, unused imports, unused fields, or unreferenced
  parameters were introduced.
- Every `unsafe` block has a `// SAFETY:` comment immediately above it
  stating the invariant being upheld.
- If your change makes existing `README.md` content inaccurate, or
  changes a CLI flag, config key, keybind, or other user-facing
  surface that the README documents, update the README in the same
  change. Otherwise leave the README alone.

"Test code" in this repository means anything inside a `#[cfg(test)]`
module. If integration tests under `tests/`, benches under `benches/`,
or examples under `examples/` are added later, the same relaxed rules
apply to those targets.

If any of those fail, the work is not done. Fix it or tell the user
why you cannot.

## Forbidden Patterns

### Suppressing compiler or clippy warnings — zero tolerance

- **No lint-suppression attribute of any kind.** This bans
  `#[allow(dead_code)]`, `#[allow(unused_*)]`, `#[allow(clippy::*)]`,
  `#[allow(warnings)]`, `#![allow(...)]`, `#[expect(...)]` and its
  inner form, and any `#[cfg_attr(_, allow(_))]` or
  `#[cfg_attr(_, expect(_))]` wrapper around the same. Fix the root
  cause: if clippy says "too many arguments," extract a struct; if it
  flags a pattern, refactor.
- **No `_` prefix to hide unused variables** — `_foo` is not a fix.
  Either use the variable or remove it.

If code exists, it must be reachable and used. Dead feature scaffolding
(config fields, enum variants, UI options for unimplemented features)
must not be checked in — add it when the feature is built, not before.

### `.unwrap()` and panics outside tests

- **`.unwrap()`, `todo!`, and `unimplemented!` are forbidden outside
  test code.** For `.unwrap()`: either return a `Result` and propagate
  with `?`, handle the `None`/`Err` branch explicitly, or use
  `.expect("<invariant>")` if the call is infallible by construction.
- **`.expect("<invariant>")` is allowed in non-test code only when the
  message names the proven invariant** — e.g.
  `.expect("BUNDLED_THEMES is non-empty by construction")`,
  `.expect("config.proc_sorting must always be a known ProcSort variant")`.
  A bare `.expect("oops")` or `.expect("should work")` is not
  acceptable.
- **`panic!` and `unreachable!` in non-test code are allowed only to
  guard a logically-unreachable branch**, and the call must be paired
  either with a `panic!`/`unreachable!` message or an immediately-
  preceding comment that names the proven invariant. Do not use
  `panic!` or `unreachable!` for recoverable failures.

```rust
// BAD — outside tests
let theme = BUNDLED_THEMES.first().unwrap();

// GOOD — invariant is documented
let theme = BUNDLED_THEMES
    .first()
    .expect("BUNDLED_THEMES is non-empty by construction");

// ALSO GOOD — propagate the failure
let theme = BUNDLED_THEMES
    .first()
    .ok_or(ThemeError::NoBundledThemes)?;
```

### Backwards-compatibility layers and shims

- **No internal backwards-compatibility layers.** No `legacy_*`
  modules, no `old_*` aliases, no transitional types that exist
  solely to preserve a prior shape of the code. When you change an
  internal API, update every caller in the same change.
- **No internal shim modules** that exist only to bridge two of our
  own APIs. The single carve-out is **vendor SDK shims** in
  `collect/gpu/` (NvAPI, ADL, IGCL dynamic-load wrappers) and the
  PawnIO IOCTL layer in `collect/pawnio/`, because those bridge to
  external APIs we do not own. New shims of that kind require explicit
  user approval.
- **No deprecated-but-kept code paths.** If a code path is no longer
  used, delete it.

### Magic strings, hardcoded ANSI, untyped maps

These are covered in detail in *Conventions* below; the short form
here is:

- No string literals for config or theme keys — use the constants in
  `config_keys.rs` and `theme_keys.rs`.
- No raw `\x1b[...]` ANSI escapes in UI code — use `AnsiBuffer` and
  the `theme.c(...)` palette.
- No `HashMap<String, T>` for structured data that has a known shape —
  define a typed struct in `domain/`.

### Silent error swallowing

`let _ = fallible_call();` is forbidden unless the discarded result is
genuinely uninteresting (RAII drops in shutdown paths, first-call
sizing probes, send-on-shutdown channel sends). In every other case,
attach a `tracing::warn!` with `subsystem`, the operation name, and
the error. See Logging below for the full rule.

## Required Patterns

### Idiomatic Rust

- **Prefer iterators over index loops.** Use `.iter()`, `.iter_mut()`,
  `.into_iter()`, `.enumerate()`, `.zip()`, `.filter()`, `.map()`,
  `.fold()`, `.collect()` rather than `for i in 0..vec.len()`.
- **Prefer `?` over `match` on `Result`** when the only purpose of the
  match is to propagate.
- **Prefer `Option`/`Result` combinators** (`map`, `and_then`,
  `unwrap_or`, `unwrap_or_else`, `ok_or`, `ok_or_else`) over `if let`
  chains when the operation is a single transformation.
- **Prefer borrowing over cloning.** Reach for `&str` over `String`,
  `&[T]` over `Vec<T>`, and `&Config` over `Config` in function
  signatures unless ownership is genuinely needed.

```rust
// BAD
let mut totals = Vec::new();
for i in 0..cores.len() {
    totals.push(cores[i].user + cores[i].system);
}

// GOOD
let totals: Vec<_> = cores.iter().map(|c| c.user + c.system).collect();
```

### Zero-cost abstractions

- **Prefer generics + monomorphisation over `Box<dyn Trait>`** unless
  heterogeneous storage or dynamic dispatch is genuinely required.
- **Do not allocate when you do not need to.** No `String` when `&str`
  suffices. No `Vec<T>` when `&[T]` or an iterator suffices.
- **Do not `.collect()` only to immediately re-iterate.** Chain the
  iterator instead.
- **Avoid `.clone()` reflexively.** If you find yourself cloning to
  satisfy the borrow checker, restructure the code first.

### Traits when there are real implementers

- Define a trait when **two or more types share a common shape and
  callers want to be polymorphic** over it. The canonical example is
  `Collector` in `collect/`.
- **Do not define a single-implementer trait speculatively** to "leave
  room" for future implementers. Add the trait when the second
  implementer arrives.
- A single-implementer trait *is* acceptable when it isolates an
  external/unsafe boundary (vendor SDK function table, FFI seam) or
  is required by an existing generic caller or test seam. Justify it
  in a doc comment on the trait.

### Enums for closed sets of variants

- **Use enums for state with a known set of values.** `MenuState`,
  `GraphMode`, `CollectStatus`, `ProcSort`, `TempScale`, `GraphSymbol`,
  `CpuGraphSource` are precedents.
- **Never compare strings with `==` to model state.** Parse the string
  into an enum at the boundary and use the enum thereafter.
- **Never use `bool` for tri-state values** ("on / off / auto"). Use a
  three-variant enum.

```rust
// BAD
if config.get_string("graph_symbol") == "braille" { ... }

// GOOD
match config.graph_symbol {
    GraphSymbol::Braille => ...,
    GraphSymbol::Block   => ...,
    GraphSymbol::Ascii   => ...,
}
```

### RAII for Win32 resources

Every Win32 handle, library, registry key, PDH query, and vendor SDK
context must be wrapped in a Drop-implementing type that releases the
resource. `OwnedHandle`, `OwnedLibrary`, `OwnedRegKey` are the
established patterns in `collect/win.rs`. Do not call `CloseHandle`,
`FreeLibrary`, `RegCloseKey`, or `PdhCloseQuery` directly from
business logic — let `Drop` do it.

### `// SAFETY:` on every `unsafe` block

Every `unsafe { ... }` block must have an immediately-preceding
comment of the form `// SAFETY: <invariant being upheld>`. Match the
existing style in `collect/gpu/amd.rs` and `collect/win.rs`.

### Win32 FFI and vendor SDK boundaries

This repository's highest-risk generation area. Treat `unsafe`, raw
handles, raw pointers, vendor function tables, and Win32 return codes
as a **boundary** — not a vocabulary the rest of the code speaks.

- **Convert raw values to typed Rust at the boundary.** Wrap `HANDLE`
  in `OwnedHandle`, `HMODULE` in `OwnedLibrary`, `HKEY` in
  `OwnedRegKey`. Convert `WIN32_ERROR` / `NTSTATUS` / vendor return
  codes into `Result<T, ConcreteError>` immediately after the call.
  Do not let raw codes propagate up into business logic.
- **Look up symbols once, in the loader.** Vendor SDK function
  pointers (`GetProcAddress` results) are resolved into a typed
  function-pointer struct at load time (see `AdlFunctions`,
  `NvApiFunctions`). Business logic calls typed methods on that
  struct; it never re-resolves symbols or re-`transmute`s.
- **Absence of a vendor DLL or device is a normal degraded case**, not
  a fatal error. Log at `debug` (expected on systems without that
  vendor) and return `None` / an empty list. Never panic.
- **Keep `unsafe` blocks small.** Each `unsafe` block should contain
  exactly the FFI call(s) the SAFETY comment justifies, and nothing
  more. Do not wrap whole functions in `unsafe`.

```rust
// BAD — raw HMODULE escapes; symbol lookup scattered through caller
let hmod = unsafe { LoadLibraryW(name) }.unwrap();
let proc = unsafe { GetProcAddress(hmod, c"Foo".as_ptr().cast()) };
let f: extern "C" fn() -> i32 = unsafe { std::mem::transmute(proc) };

// GOOD — typed wrapper, lookup-once, degraded path returns None
let lib = OwnedLibrary::load("vendor.dll")?;
let funcs = VendorFunctions::resolve(&lib)?;   // returns Option
let result = funcs.foo();                       // safe typed call
```

### Render and collection boundaries

- **Set the minimum `Dirty` flags required.** A keybind that only
  affects the process box sets `PROC_BOX`, not `ALL_BOXES`. Forcing
  unnecessary redraws breaks the per-box rendering invariant.
- **Collectors never run from inside renderers.** The `collect →
  domain → ui` arrow is one-way. UI reads from public domain struct
  fields; it does not call `Collector::collect()` and does not block
  on collector channels.
- **Renderers are pure functions of (domain data, settings, theme).**
  No I/O, no `tracing` calls inside the hot render path (use `debug`
  only outside the path), no mutation of domain data.

### Loose coupling

- **UI renderers depend on per-widget `Settings` structs, not on
  `Config`.** `CpuBoxSettings`, `NetBoxSettings`, `GpuBoxSettings` are
  the precedents. Config reads happen in `app.rs`, not in renderers.
- **Collectors expose data via public domain struct fields**, not via
  callbacks or trait objects passed in by the UI.
- **New cross-module dependencies must go through `domain/`.**
  `collect/` may depend on `domain/`. `ui/` may depend on `domain/`
  and `draw/`. `ui/` may **not** depend on `collect/`. `domain/` may
  not depend on either.

## When to ask vs. when to proceed

**Ask** (use a clarifying question, do not proceed) if any of these
hold:

- The user's request is silent on an edge case that materially affects
  the implementation (e.g. "what should happen if no GPU is detected").
- Two valid approaches exist and the choice has user-visible
  consequences (e.g. defaults, ordering, error surfaces).
- Your change crosses a module boundary in a way without precedent in
  the codebase.
- You discover a pre-existing bug or violation adjacent to your change.

**Proceed** (no need to ask) if all of these hold:

- The convention is established in the codebase (you can point to ≥1
  precedent).
- The change is fully local to the scope the user named.
- There is exactly one reasonable implementation given the existing
  patterns.

## Build, Test, Lint

```powershell
cargo build --release          # Build optimized binary
cargo test                     # Run all tests
cargo test proc_box            # Run tests matching a name
cargo clippy --release --all-targets -- -D warnings   # Lint — zero warnings required
cargo fmt -- --check           # Format check
```

All four must pass with zero warnings before declaring work complete.
The AI does not run `cargo release`, `cargo publish`, or any other
command that mutates remote state.

## Logging

Logging is always installed; the level filter is the only knob, and
defaults to `warn`. Disable by setting `log_level = "off"` in
`rtop.toml` — that suppresses log file creation entirely.

A startup diagnostic banner is emitted at `info` from
`log::startup_banner` (rtop version, profile, target, Windows version,
host, user, config path, log path). To collect it for a bug report,
set `log_level = "info"` in `rtop.toml` and reproduce.

### Conventions

- Every `tracing::*!` call must include
  `subsystem = %log::Subsystem::Foo` as a structured field.
- Vendor and Win32 return codes use `code = %log::Hex(ret)`. The `Hex`
  newtype standardises the format as `0x` prefixed, uppercase, 8-wide.
- Other typed values (pid, device index, theme name, option key) use
  structured fields, not message interpolation.
- The message string is a stable, present-tense identifier of the
  operation: `"PdhCollectQueryData failed"`, `"option toggled"`. Not
  a sentence.
- State-changing user actions are logged at `info` with
  `subsystem = %log::Subsystem::Input` and an `action` field
  (preset switch, option toggle, theme cycle, log-level change,
  process kill, filter commit, config save). Read-only navigation
  (arrow keys, menu open/close) is `debug` or unlogged.
- **Never silently swallow errors with `let _ = …`** on a fallible
  operation the user might care about. Either attach a `warn!` with
  the error and the operation name, or document why the result is
  intentionally discarded (RAII drops, first-call sizing probes,
  shutdown-path channel sends).
- No throttling. If a per-cycle log would spam, choose the right
  level (`debug` for expected-on-some-systems, `warn` for a real
  degraded state) and accept the volume.

### Do not log

- Per-frame render and dirty-flag updates
- Per-keystroke navigation (arrow keys, j/k, Page Up/Down, Home/End,
  mouse wheel)
- Filter-text character appends/backspaces (log once on Enter when
  the filter actually commits)
- Sub-threshold resize bursts (log only when the size actually
  changes)
- Routine Win32 success returns (the absence of a `warn!` is the
  success signal)
- Shutdown-path RAII drop errors (`CloseHandle`, `RegCloseKey`,
  `FreeLibrary`, `PdhCloseQuery`, vendor SDK unloads)

### Level rubric

- `error` — unrecoverable failure that requires the process to exit.
  Reserved for the panic hook and terminal-init failure in `main`.
- `warn` — recoverable failure that degrades observable behavior; the
  user might notice missing data and want to know why.
- `info` — significant lifecycle event the user may want to confirm
  (subscriber installed, level changed, config reloaded, GPU vendor
  detected, theme loaded, user took a state-changing action).
- `debug` — per-cycle or per-resource diagnostics for bug reports;
  includes vendor-init failures expected on systems without that
  vendor.
- `trace` — reserved.

## Architecture

### Data flow: Collection → Domain → Rendering

```
collect/*.rs  →  domain/*.rs  →  ui/*_box.rs  →  terminal
(Win32 APIs)     (typed structs)  (AnsiBuffer)    (crossterm)
```

- **`collect/`**: One collector per subsystem (CPU, memory, disk,
  network, GPU, process). Collectors implement a `collect()` method
  (typically via a `Collector` trait — see existing collectors for the
  current shape). Data collected via Windows APIs, stored in public
  domain struct fields.
- **`domain/`**: Pure data types with typed structs (no
  `HashMap<String, T>` for structured data). E.g.
  `CpuPercent { total, user, system, idle }`, not
  `HashMap<String, VecDeque<i64>>`.
- **`ui/`**: One renderer per widget. Each takes domain data + a
  settings struct + theme, returns ANSI string via `AnsiBuffer`.
- **`draw/`**: Shared rendering primitives — `AnsiBuffer`, `Graph`,
  `Meter`, box drawing, layout engine.

### Event loop (`app.rs`)

Three-phase loop with `Dirty` bitflags:

1. **Detect dirty** — timer tick, resize, keybinds set flags.
2. **Execute** — collect data (`COLLECT`), rebuild proc list
   (`PROC_LIST`), recalculate layout (`LAYOUT`).
3. **Render** — only redraw boxes whose dirty flag is set.

Input handling is split into per-`MenuState` handler functions, each
taking `&mut InputContext`. See `handlers/` for the current set.

### Key abstractions

- **`Dirty` bitflags** — Per-box dirty tracking. Keybinds set the
  minimum flags needed (e.g. `PROC_BOX` for arrow keys,
  `LAYOUT | ALL_BOXES` for box toggles).
- **`AnsiBuffer`** — Fluent builder for ANSI output. All UI rendering
  uses `.mv()`, `.color()`, `.text()`, `.finish()`. No raw `\x1b[`
  escapes in UI code.
- **`render_all()`** — Extracted render function callable from both
  the main loop and menu transitions.
- **Settings structs** — `CpuBoxSettings`, `NetBoxSettings`,
  `GpuBoxSettings` decouple renderers from `Config`. Config reads
  happen in `app.rs`, not in renderers.

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

Title insets on box borders use shared functions in
`draw/box_drawing.rs`:

```rust
box_drawing::title_inset(text, border_color, text_color, bottom)
box_drawing::keybind_inset(text, border_color, hi_color, text_color, bottom)
box_drawing::section_divider(section, width, border_color, text_color)
box_drawing::inset_width(text)           // visible width of an inset
box_drawing::right_inset_x(x, w, vis)    // X position for right-edge placement
```

No UI file should directly reference `title_syms::TITLE_LEFT` etc.

### Box content positioning

Content inside a box starts at `x + 3` (border, space, content) and
ends at `x + width - 2` (content, space, border). Usable content width
is `width - 4`.

### Widget heights are data-driven

Layout engine (`draw/layout.rs`) receives `disk_count`, `has_swap`,
`core_count` etc. in `LayoutConfig` and sizes widgets to fit their
content. Net box gets remaining space.

### Themes

All bundled themes are loaded via `include_str!`. All colors derive
from theme — no hardcoded ANSI escapes in production code. The default
theme is built-in as a fallback. Theme files can define `gpu_box`,
`disk_box`, `help_box`, `options_box`, `proc_tree_fg` and other
widget-specific colors.

### Process display pipeline

Raw processes (`procs`) are never modified in place. `rebuild_display()`
clones raw → sorts → filters → tree-builds into `display_procs`. In
tree mode, sort applies within each parent group, then
`sort_by_key(tree_index)` flattens for display order.

## When the user asks you to commit

**This section does not grant permission to commit.** It applies only
when the user has given an explicit commit instruction in the current
turn (see *Behavioral Contract* above). If you have not received that
instruction, do not commit — even if you believe the work is
well-formed and the message would be obvious.

When the user does ask for a commit, write a Conventional Commit
subject that matches the existing history:

```text
feat: add disk performance metrics
fix: rename GPU MHz label to Clock
refactor: decouple handlers from terminal via HandleResult
release: v0.3.0
```

- Keep the subject concise, imperative, and under 72 characters.
- Use a blank line between the subject, body, and trailers.
- Wrap commit body lines at 72 characters.
- Use the body to explain *what* changed and *why* when the subject is
  not enough.
- Do not wrap Git trailers.
- Include the Copilot co-author trailer:

```text
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

## Releases

`cargo release patch|minor|major --execute` bumps version, commits,
tags, pushes. The GitHub Action (`windows-release.yml`) builds on
`windows-latest`, runs tests, creates a GitHub Release with zipped exe.
**This is a human action.** The AI never runs `cargo release` or any
release tooling.
