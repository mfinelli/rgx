# rgx

A terminal UI regex tester with multi-engine support, session history, and
snippet export. Currently implements the Rust engine with the full navigation
model; additional engines are coming in later phases.

## Building

Requires Rust 1.88 or newer (ratatui MSRV). Install via [rustup](https://rustup.rs):

```bash
rustup update stable
cargo build --release
```

The binary is at `target/release/rgx`.

## Usage

```
rgx [OPTIONS]

Options:
  --config <FILE>  Path to config file (default: ~/.config/rgx/config.toml)
  -h, --help       Print help
  -V, --version    Print version
```

Shell completions (hidden subcommand for packagers):

```bash
rgx completions bash >> ~/.bash_completion
rgx completions zsh  >  ~/.zfunc/_rgx
rgx completions fish >  ~/.config/fish/completions/rgx.fish
```

## Keybinds

### Always available

| Key | Action |
|-----|--------|
| `ctrl+p` | Jump to pattern field |
| `ctrl+t` | Jump to test input field |
| `ctrl+g` | Jump to replacement field (replace mode only) |

### Nav layer (press `Escape` to enter)

| Key | Action |
|-----|--------|
| `q` | Quit |
| `?` | Toggle keybind reference |
| `Tab` / `Shift+Tab` | Cycle focus forward / backward |
| `↑ ↓` | Scroll results (when results pane focused) |
| `← →` | Switch active sub-pane in split views / navigate flags |
| `f` | Cycle engine variant (regex ↔ fancy-regex) |
| `v` | Cycle results view |
| `m` | Toggle replace mode |
| `h` | Toggle history panel *(coming in sessions phase)* |
| `y` | Copy menu *(coming in copy/export phase)* |
| `r` | Rescan runtimes *(coming in runtime probe phase)* |
| `p` | Code preview panel *(coming in copy/export phase)* |
| `w` | Escape sequence reference *(coming in reference panels phase)* |

### Flag row (Tab to focus)

| Key | Action |
|-----|--------|
| `← →` | Move between variant selector and flags |
| `Space` | Toggle focused flag / cycle variant |

### Results pane (Tab to focus)

| Key | Action |
|-----|--------|
| `↑ ↓` | Scroll active sub-pane |
| `← →` | Switch active sub-pane (split views only) |
| `v` | Cycle view: split-vertical → split-horizontal → preview → matches |

## Results views

Cycle with `v` when the results pane is focused or from the nav layer:

| View | Description |
|------|-------------|
| `split-v` | Input preview (top) + match breakdown (bottom) — default |
| `split-h` | Input preview (left) + match breakdown (right) |
| `preview` | Input preview only, full pane |
| `matches` | Match breakdown only, full pane |

In split views, `←`/`→` switches which sub-pane scrolls.

## Replace mode

Press `m` in the nav layer to toggle replace mode. A replacement field appears
between the pattern and test input, making the layout read as a pipeline:

```
Pattern      — the regex
Replacement  — what to replace matches with
Test Input   — the text to operate on
Output       — the transformed result
```

Use `ctrl+g` to jump directly to the replacement field, or `Tab` through the
panes. The output pane shows the replaced text with a replacement count.

Replacement syntax uses the normalized form — `{1}` for indexed groups,
`{name}` for named groups. The status line shows the actual Rust call:

```
# global replace
let re = Regex::new(r"(\w+)")?; re.replace_all(input, "$1")

# first match only (global flag off)
let re = Regex::new(r"(\w+)")?; re.replace(input, "$1")
```

## Engine variants

The Rust tab has two variants toggled with `f` or via the flag row:

| Variant | Engine | Notes |
|---------|--------|-------|
| `regex` | `regex` crate | RE2-style, linear time, no lookahead/lookbehind/backrefs |
| `fancy-regex` | `fancy-regex` crate | PCRE-style, adds lookahead/lookbehind/backrefs, no time guarantee |

The status line shows the idiomatic invocation for the active variant and flags:

```
# regex crate, no flags
Regex::new(r"(\w+)")

# regex crate, with flags
RegexBuilder::new(r"(\w+)").case_insensitive(true).multi_line(true).build()

# fancy-regex, with flags
fancy_regex::Regex::new(r"(?im)(\w+)")
```

## Configuration

`rgx` reads `~/.config/rgx/config.toml` on startup (XDG config dir). A missing
file is not an error — all options have defaults. Specify a custom path with
`--config`.

```toml
# UI
nerd_fonts = false               # show Nerd Font icons in the engine tab bar
default_results_view = "split_vertical"  # split_vertical | split_horizontal | preview | matches

# Evaluation
debounce_ms = 150                # ms to wait after last keystroke before evaluating

# Sessions
on_open = "ask"                  # "ask" | "continue" | "new"
db_path = "/custom/path.db"      # default: ~/.local/share/rgx/history.db

# Rust engine
fancy_regex_default = false      # start on fancy-regex variant instead of regex
```

Unknown keys are rejected with an error message pointing to the offending line.

## Sessions and undo/redo

`rgx` automatically saves your work to `~/.local/share/rgx/history.db`. Every
time the engine evaluates (after the debounce), the current pattern, input,
flags, and replacement are persisted.

**On startup** (when `on_open = "ask"`, the default), a splash screen shows
available runtimes and asks whether to continue the last session or start fresh:

```
  rgx v0.1.0

  ✓ Rust    built-in

  Last session: unnamed session

  [C]ontinue   [N]ew session   [q]uit
```

**Undo/redo** operates at the session level — each evaluation is a checkpoint:

| Key | Action |
|-----|--------|
| `ctrl+z` | Undo to previous evaluation state |
| `ctrl+shift+z` | Redo to next evaluation state |

The history panel (`h` in nav mode) for browsing, naming, and switching between
sessions is coming in the next phase.

## What's coming

See [DESIGN.md](DESIGN.md) for the full feature specification and
[IMPLEMENTATION.md](IMPLEMENTATION.md) for the planned build sequence.

Upcoming phases add: multi-engine tabs (Python, Node, Ruby, PHP, Go, grep,
sed), history panel UI with named sessions, reference panels, snippet export,
file input, and runtime probe splash screen.
