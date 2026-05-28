# rgx

A terminal UI regex tester with multi-engine support.

## Building

Requires Rust 1.80 or newer. Install via [rustup](https://rustup.rs):

```bash
rustup update stable
cargo build --release
```

The binary is at `target/release/rgx`.

## Phase 1 — What's implemented

- Rust engine (`regex` crate, RE2-style) with `fancy-regex` toggle (`f` key)
- Live evaluation with 150ms debounce
- Pattern field and multi-line test input field
- Match count, per-match span and capture group breakdown
- Highlighted input preview in results pane (yellow = full match, cyan = group)
- Flag toggles: case insensitive, multiline, dotall, global
- Status line showing rendered `Regex::new(...)` invocation
- Navigation: Insert mode / Nav mode (Escape), `ctrl+p`/`ctrl+t` quick jump
- `?` keybind for help overlay

## What's coming

See [DESIGN.md](DESIGN.md) for the full feature spec and
[IMPLEMENTATION.md](IMPLEMENTATION.md) for the build sequence.

Phases 2–14 add: multi-engine tabs (Python, Node, Ruby, PHP, Go, grep, sed),
replace mode, session history with undo/redo, named sessions, reference panels,
snippet export, file input, and full configuration.
