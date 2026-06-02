# rgx — Design Document

> A terminal UI regex tester with multi-engine support, session history, and snippet export.
> Binary name: `rgx` | Written in Rust

---

## Table of Contents

1. [Overview](#overview)
2. [Goals & Non-Goals](#goals--non-goals)
3. [TUI Layout & Navigation](#tui-layout--navigation)
4. [Engine Architecture](#engine-architecture)
5. [Supported Engines](#supported-engines)
6. [Evaluation Model](#evaluation-model)
7. [Input Handling](#input-handling)
8. [Flags & Options](#flags--options)
9. [Results Display](#results-display)
10. [Replace Mode](#replace-mode)
11. [Copy & Export](#copy--export)
12. [Session Model](#session-model)
13. [History & Library](#history--library)
14. [State Collapse & Hygiene](#state-collapse--hygiene)
15. [Reference Panels](#reference-panels)
16. [Runtime Probing](#runtime-probing)
17. [Configuration](#configuration)
18. [Database Schema](#database-schema)
19. [JSON Contract](#json-contract)

---

## Overview

`rgx` is a terminal UI tool for interactively developing and testing regular expressions. It is inspired by [rubular.com](https://rubular.com) but runs locally, supports multiple regex engines/runtimes, maintains a persistent named session history, and can export ready-to-paste code snippets.

The primary workflow is:
1. Type a pattern — see matches highlighted in real time
2. Adjust flags via toggles — see the rendered invocation in the status line
3. Switch engine tabs to verify behaviour across languages
4. Name the session to save it as a reusable library entry
5. Copy a code snippet or CLI command directly from the tool

---

## Goals & Non-Goals

### Goals
- Fast, local, offline-capable regex development
- Honest representation of per-engine differences (RE2 vs PCRE vs Oniguruma etc.)
- Persistent personal regex library via named sessions
- Single self-contained binary (scripts embedded at compile time)
- Educational — help users understand *why* engines differ

### Non-Goals
- Processing large files as primary input (the tool is for developing patterns, not running them at scale)
- Replacing `grep`/`sed` for production use
- Collaborative or shared libraries (copy/paste covers the sharing use case)
- A GUI or web interface

---

## TUI Layout & Navigation

### Pane Structure

```
┌─────────────────────────────────────────────────────────────────────┐
│ [ Rust ] [ Python ] [ JS ] [ Ruby ] [ PHP ] [ Go ] [ grep ] [ sed ] │  tab bar
├─────────────────────────────────────────────────────────────────────┤
│ Pattern: hello\s+(\w+)                                              │  pattern field
├─────────────────────────────────────────────────────────────────────┤
│ ◉ Case insensitive  ○ Multiline  ○ Dotall  ◉ Global  ○ ...         │  flag row
├─────────────────────────────────────────────────────────────────────┤
│ Test input                                                          │  input field
│ Hello world                                                         │
│ hello mario                                                         │
├─────────────────────────────────────────────────────────────────────┤
│ 2 matches                                                           │  results pane
│ Match 1: "Hello world"   group 1: "world"  [0..11]                 │
│ Match 2: "hello mario"   group 1: "mario"  [0..11]                 │
├─────────────────────────────────────────────────────────────────────┤
│ re.compile(r"hello\s+(\w+)", re.I)                     2 matches   │  status line
└─────────────────────────────────────────────────────────────────────┘
  [?] help  [h] history  [w] ref  [p] preview  [y] copy  [m] mode
```

### Navigation Model

**Hybrid approach** — always-insert in text fields, Escape to navigation layer:

- While focused in a text field (pattern, input, replacement): all keypresses are text input
- `Escape` drops focus → navigation layer, bare letter shortcuts become active
- In navigation layer: `Tab`/`Shift-Tab` cycles panes, arrow keys move within panes, `Enter` or typing re-enters a text field
- A small set of `ctrl+` shortcuts work everywhere including inside text fields for actions needed without leaving a field
- Mouse support optional (config toggle), clicking a pane focuses it

### Keybinds

**Always-available — work inside text fields:**

| Action | Keybind |
|--------|---------|
| Jump to pattern field | `ctrl+p` |
| Jump to test input field | `ctrl+t` |
| Jump to replacement field | `ctrl+g` |
| Undo | `ctrl+z` |
| Redo | `ctrl+shift+z` |

**Nav layer — active after `Escape`:**

| Action | Key |
|--------|-----|
| Yank / copy menu | `y` |
| Toggle history panel | `h` |
| Toggle help / keybind reference | `?` |
| Toggle escape sequence reference | `w` |
| Toggle code preview panel | `p` |
| Toggle replace mode | `m` |
| Cycle results view | `v` |
| Rescan runtimes | `r` |
| Quit | `q` |
| Cycle panes | `Tab` / `Shift+Tab` |
| Move within pane | arrows |
| Re-enter focused field | `Enter` or type |

**History panel — while history panel is focused:**

| Action | Key |
|--------|-----|
| Load / switch to session | `Enter` |
| Yank from entry (without loading) | `y` |
| Rename session | `n` |
| Delete session | `d` |

### Terminal Size

Below a minimum usable size, show a single-line message: `terminal too small — resize to continue`. No broken layouts.

### Nerd Fonts

Enabled via config key `nerd_fonts = true`. When enabled, engine tabs show language logos as Nerd Font codepoints. Default off.

### NO_COLOR

Respects `$NO_COLOR` environment variable per the [no-color.org](https://no-color.org) spec. Match spans are indicated with bracket markers `[` `]` instead of ANSI color when set. The tool remains functional but less visually rich.

---

## Engine Architecture

### Core Abstraction

All engines implement a common Rust trait:

```rust
trait RegexEngine {
    fn name(&self) -> &str;
    fn language(&self) -> Language;
    fn flavor(&self) -> Option<&str>;
    fn is_available(&self) -> bool;
    fn evaluate(&self, req: &EvalRequest) -> Result<EvalResponse, EngineError>;
}
```

### Two Engine Types

**Native engine (Rust)** — compiled directly into the binary. Uses the `regex` crate by default with an optional toggle to `fancy-regex`. No subprocess, no temp files.

**Script engines (all others)** — a small script per language is embedded in the binary at compile time via `include_str!()`. At runtime, scripts are written to a temp directory and invoked via subprocess. Scripts read a JSON request on stdin and write a JSON response on stdout.

This keeps `rgx` a single self-contained binary with no separate installation of scripts. If the underlying runtime (Python, Node, etc.) is not available, the tab is shown greyed out with an install hint.

### Script Embedding

```rust
const PYTHON_SCRIPT: &str = include_str!("../engines/python/engine.py");
const NODE_SCRIPT: &str  = include_str!("../engines/node/engine.js");
const RUBY_SCRIPT: &str  = include_str!("../engines/ruby/engine.rb");
// etc.
```

Scripts are written to `$TMPDIR/rgx/engines/` on first use and reused for the session. If the binary is newer than the cached script, the script is rewritten.

### Debounce

All evaluation (native and script) is debounced. Default 150ms, configurable. On every keystroke the debounce timer resets. Evaluation fires only after the timer expires. This prevents excessive subprocess spawning for script engines.

---

## Supported Engines

### Rust

- **Default**: `regex` crate — RE2-style, linear time guarantee, no backtracking catastrophe, no lookahead/lookbehind, no backreferences
- **Sub-toggle**: `fancy-regex` — adds lookahead, lookbehind, backreferences; loses the linear time guarantee
- Toggle visible within the Rust tab
- **Rationale for `regex` as default**: honest about Rust's actual regex story; the limitation is educational

### Python

- **Default**: `re` stdlib module — always available, backtracking NFA
- **Sub-toggle**: `regex` third-party module — possessive quantifiers `a++`, atomic groups `(?>...)`, better Unicode, overlapping matches
- Probe for `regex` module availability separately from Python itself
- When `regex` module unavailable: show greyed toggle with `pip install regex` hint

### JavaScript

- Runtime: Node.js
- Single flavor — V8 engine, same as browser at the regex level
- Document Node version during probe; `/v` flag (set notation) requires Node 20+
- **TypeScript note**: TS regex behaviour is identical to JS at runtime — no separate tab needed

### Ruby

- Runtime: `ruby`
- Oniguruma engine
- Strong named group support

### PHP

- Runtime: `php`
- PCRE2 via `preg_*` functions
- `preg_match` (first match) vs `preg_match_all` (global) controlled by the global toggle
- Replace mode uses `preg_replace`

### Go

- Runtime: `go run` with a small inline program
- `regexp` package — RE2-based like Rust's `regex` crate
- Useful for comparing RE2 dialect differences between Go and Rust

### grep

Lives under a single "grep" tab with a sub-flavor selector showing only *available* flavors:

| Flavor | Availability | Notes |
|--------|-------------|-------|
| BSD grep (BRE) | macOS default | No `-P`, limited syntax |
| BSD grep (ERE) | macOS default | `-E` flag |
| GNU grep (BRE) | Linux default, `ggrep` on macOS | |
| GNU grep (ERE) | | `-E` flag |
| GNU grep (PCRE) | GNU only, not all builds | `-P` flag, probed separately |
| ripgrep | if `rg` in PATH | RE2-based, `--pcre2` available |

- No replace mode — tooltip: "grep does not support replacement — use the sed tab"
- Command preview line shows the exact `grep`/`rg` invocation
- On macOS: probe `grep` (BSD), `ggrep` (GNU if installed), `rg` separately
- User can specify explicit path in config (e.g. for a custom grep build)

### sed

- Own tab, distinct from grep
- Sub-flavor selector: BSD sed (BRE/ERE), GNU sed / `gsed` (BRE/ERE) — same probing logic as grep
- On macOS: probe `sed` (BSD) and `gsed` (GNU via Homebrew) separately; user can specify path in config
- **Primary mode**: replace (`s/pattern/replacement/flags`) — fully supported
- **Match mode**: supported but shows a nudge:
  > `ℹ  sed -n '/pattern/p'` is equivalent to grep — consider using the grep tab if that's all you need
- Default mode is match mode (consistent with all other engines — user switches to replace via `m`)
- Command preview line shows the full `sed` expression
- Replacement syntax uses `\1` not `$1` — handled by internal normalization (see Replace Mode)
- No named group replacement in sed — auto-converts to indexed on switch, warns user, restores named form on switch back

---

## Evaluation Model

### Request / Response

All engines share the same logical request/response model:

```
pattern:     String       — the regex pattern (no inline flags injected by the tool)
flags:       Set<Flag>    — normalized flag set
input:       String       — the full test input text
mode:        match|replace
replacement: String?      — normalized replacement (only in replace mode)
global:      bool         — find all matches or just first
multiline:   bool         — ^ and $ match line boundaries vs string boundaries
```

### Global Toggle

- Default: **on** (find all matches)
- When off: first match only
- Maps to: `re.search` vs `re.findall` (Python), `match` vs `matchAll` (JS), `preg_match` vs `preg_match_all` (PHP), no `g` flag (JS), `-m 1` (grep)

### Multiline Toggle

- Default: **off** (string mode — `^`/`$` match start/end of entire input)
- When on: `^`/`$` match start/end of each line
- Each engine has a per-engine explainer note on what this means specifically
- grep is naturally line-by-line; the toggle for grep controls whether the entire input is treated as one subject vs line-by-line

### Inline Flag Detection

Not implemented. Inline flags (e.g. `(?i)`) typed directly in the pattern field are passed to the engine as-is and not synced to the flag row toggles. This is intentional — silently stripping and rewriting the pattern would be surprising behaviour. Experts who use inline flag syntax know what they're doing; everyone else uses the flag row.

---

## Input Handling

### Pattern Field

Single-line text input via `tui-textarea`. Accepts the regex pattern without flags (flags are managed separately via the flag row).

### Test Input Field

Multi-line text input via `tui-textarea`. The entire buffer is treated as one string subject (multiline toggle controls `^`/`$` semantics). Not line-by-line unless using grep.

**Loading from file**:
- `ctrl+g` (when in nav layer or via a dedicated keybind) opens a path input line with tab completion — no graphical file picker
- File contents imported once as a snapshot — no live watch
- Binary file check: if null bytes detected, prompt: `⚠ File appears to be binary. Load anyway? [y/N]`
- Large file check: if above threshold (default 1MB, configurable), prompt: `⚠ File is 2.4MB. Load anyway? [y/N]`
- Default No for both prompts — user can override freely
- `source_file` path stored as nullable metadata on the session state
- `file_dirty` flag set when input is edited after loading from file

**Reload from file**:
- Keybind active only when current session state has a non-null `source_file`
- Re-reads the file from disk, creates a new session state
- If `file_dirty`: prompts `discard changes to [filename] and reload? [y/N]`
- If file no longer exists: `⚠ source file "x" no longer exists. Input preserved as-is.`

**Clear input**:
- Dedicated keybind
- If a file is loaded: prompts `clear input? [y/N]`
- Creates a new session state with empty input and null `source_file`

---

## Flags & Options

### Flag Row

Always visible between the pattern field and test input. Shows flags appropriate to the current engine. Each flag is a toggle (◉/○). Toggling a flag:
1. Updates the normalized internal flag set
2. Updates the status line immediately
3. Triggers re-evaluation (debounced)

Flags are never injected into the pattern field — the pattern stays clean. The engine receives flags as separate parameters. Flags not meaningful for the current engine are hidden (not greyed — simply absent).

### Per-Engine Flags

| Flag | Rust | Python | JS | Ruby | PHP | Go | grep | sed |
|------|------|--------|----|------|-----|----|------|-----|
| Case insensitive | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `-i` | `I` (GNU) |
| Multiline | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | n/a | `m` (GNU) |
| Dotall (`.` matches `\n`) | ✓ | ✓ | ✓ | ✓ | ✓ | — | — | — |
| Global (all matches) | toggle | toggle | `g` | toggle | toggle | toggle | default | `g` |
| Extended / verbose | ✓ | `re.X` | — | `x` | `x` | — | — | — |
| Unicode | default | default | `/u` `/v` | default | `u` | default | — | — |

### Unsupported Flag Warnings

When switching engine tabs or loading a history entry, the tool diffs the active flags against the target engine's supported set. Non-blocking warning shown in the results pane:

```
⚠ 1 option not supported on GNU grep (BRE):
  · dotall — not available in BRE or ERE, only in grep -P
  Suggested: switch to grep -P flavor
```

Auto-suggest flavor upgrade (BRE → ERE → PCRE) where applicable. Never silently drop a flag.

---

## Results Display

### Results Pane Views

The results pane has four views, cycled with `v` in the nav layer:

- `split_vertical` — input preview (top) + match breakdown (bottom)
- `split_horizontal` — input preview (left) + match breakdown (right)
- `preview` — input preview only, full pane
- `matches` — match breakdown only, full pane

Default view is configurable (`default_results_view` in config). Both sub-panes are independently scrollable when focused — arrow keys scroll when the results pane has focus.

### Match Mode

- Summary line: `N matches` or `no match` or `error: ...`
- Per-match:
  - Full match text and byte span `[start..end]`
  - Each capture group: index, name (if named), value, span
  - Optional/unmatched groups shown explicitly as `group 2: (unmatched)` — not omitted
- Input preview: matched spans highlighted inline with ANSI color (or bracket markers under `$NO_COLOR`)
- Error state: parse error shown inline below pattern field with position indicator if available

### No-Match vs Error

Visually distinct:
- No match: neutral indicator, `0 matches`
- Error: prominent indicator, error message with position

### Status Line

Always-visible single line at the bottom showing the fully rendered invocation for the current engine and flags:

```
# Python:   re.compile(r"hello\s+(\w+)", re.IGNORECASE)
# JS:       /hello\s+(\w+)/i
# Rust:     Regex::new(r"(?i)hello\s+(\w+)").unwrap()
# grep:     grep -Ei 'hello\s+(\w+)'
# sed:      sed -E 's/hello\s+(\w+)/hi \1/gi'
```

Updates live with every change. Right side of status line shows match count.

---

## Replace Mode

### Activation

`m` in the nav layer toggles between match and replace mode. In replace mode a replacement input field appears between the pattern and test input fields — the layout reads top-to-bottom as a transformation pipeline: pattern → replacement → input → output. Default is always match mode regardless of engine.

### sed and grep

- **sed**: replace mode fully supported. Match mode shows a nudge toward the grep tab for non-substitution use.
- **grep**: replace mode toggle is disabled. Tooltip: "grep does not support replacement — use the sed tab." ripgrep (`rg --replace`) does support replacement and has it enabled.

### Normalized Replacement Syntax

Internally replacements are stored in a normalized form: `{1}` for indexed groups, `{name}` for named groups. Each engine receives the replacement pre-translated to its native syntax:

| Internal | Python | JS / PHP | Ruby | Rust | sed |
|----------|--------|----------|------|------|-----|
| `{1}` | `\1` | `$1` | `\1` | `$1` | `\1` |
| `{name}` | `\g<name>` | `$<name>` | `\k<name>` | `$name` | n/a |

**Rationale**: normalizing means switching between languages works without manually rewriting the replacement string. The status line always shows the native form so the user sees what the engine actually receives.

### Named Groups on sed

sed has no named group replacement syntax. When switching to the sed tab with named groups in the replacement:
```
⚠ sed does not support named group replacements.
  {word} → \1, {name} → \2
  Replacement field updated. Switch back to restore named form.
```

The named form is preserved internally. Switching back to Python/JS/etc restores `{name}` automatically. No information is lost.

---

## Copy & Export

### Copy Menu

Accessible via `y` in the nav layer. Opens a one-line prompt:

```
Yank:  [p] pattern   [s] status line   [c] code snippet   [Esc] cancel
```

### Copy Targets

**Pattern only** — the raw pattern string, no flags

**Status line** — the compact rendered invocation (one line, as shown in the status bar)

**Code snippet** — a complete pasteable block with imports and usage, per engine:

```python
# Python
import re
pattern = re.compile(r"hello\s+(\w+)", re.IGNORECASE)
matches = list(pattern.finditer(text))
```

```javascript
// JavaScript
const pattern = /hello\s+(\w+)/gi;
const matches = [...text.matchAll(pattern)];
```

```rust
// Rust
let re = regex::Regex::new(r"(?i)hello\s+(\w+)").unwrap();
let matches: Vec<_> = re.captures_iter(text).collect();
```

```bash
# grep
grep -Ei 'hello\s+(\w+)' file.txt
# sed
sed -E 's/hello\s+(\w+)/hi \1/gi' file.txt
```

Snippet warns if the pattern uses syntax that doesn't translate cleanly (e.g. `\w` in BRE):
```
⚠ \w is not valid in BRE — consider [[:alpha:]_0-9] or switching to grep -E
```

### Copy from History

In the history panel, `y` on a highlighted entry opens the same copy menu without loading or switching to that session.

---

## Session Model

### Core Concept

A **session** is a linear sequence of states (the undo/redo stack) plus a cursor pointing to the current position. Sessions are stored in SQLite and persist across launches.

### Language Is Not a History Action

**Decision**: switching engine tabs does not push a new state onto the undo stack.

**Rationale**: switching tabs is a view/evaluation decision, not an edit to the pattern or input. You can develop a pattern on the Python tab, switch to Ruby to verify behaviour, switch back — the undo/redo stack is identical on both tabs. The active language is stored on the session row (not in individual states) and restored on resume.

### State Contents

Each state snapshot contains:
- `pattern` — the regex pattern
- `options` — normalized flag set (JSON array)
- `input` — the full test input text
- `replacement` — normalized replacement string
- `mode` — `match` or `replace`
- `source_file` — nullable path to loaded file
- `file_dirty` — whether input has been edited since file load

### Undo / Redo

- `ctrl+z` moves `undo_cursor` back one `seq`
- `ctrl+shift+z` moves `undo_cursor` forward one `seq`
- `undo_cursor` is persisted to the `sessions` table immediately on every change — crash recovery is automatic since SQLite writes are atomic

### Implicit Fork on Non-Head Edit

When the user edits while `undo_cursor` is not at the head of the stack (i.e. after undoing without redoing to head):

1. A new session row is created with `forked_from_id` and `forked_at_seq` pointing to the branch point
2. The new session's history is the parent's states up to the fork point (reconstructed via ancestry walk — not duplicated in DB)
3. New states are appended to the new session
4. The original session is **untouched** — its full state history is preserved, redo still works there
5. A small UI indicator notes the fork: `forked from [session name] at state 7`

**Rationale**: implicit fork is non-destructive. No information is ever lost. The user never has to think about whether to fork before editing.

### On Open Behaviour

Configurable via `on_open` in config:
- `continue` — always resume the last active session at its saved `undo_cursor`
- `new` — always start a fresh session
- `ask` — one-line prompt: `[C]ontinue last session  [N]ew session`

### Session List Navigation

The history panel shows a list of sessions. The current session is highlighted. Navigating to another session saves the current `undo_cursor`, then restores the selected session at its saved `undo_cursor` and language tab. Switching is non-destructive in both directions — like switching buffers in an editor.

---

## History & Library

### Named vs Unnamed Sessions

- **Unnamed** — auto-created, ephemeral feel, visually de-emphasised in the history panel
- **Named** — user-assigned via `n` in the history panel, shown prominently, treated as personal library entries
- Naming sets the `name` column — no structural difference, just visibility and emphasis

### History Panel

Toggleable via `h` in the nav layer. Filter bar at top:

```
[ All ] [ Named ] [ This language ] [ Recent ] [ Search... ]
```

Search uses FTS5 full-text search over session name and pattern.

### Cleanup

- Bulk delete: unnamed sessions older than N days (configurable, default 30)
- Collapse session: reduces to first + last state only — user-initiated per session
- Retroactive collapse: collapses all intermediate states to major checkpoints — user-initiated, destructive but explicit

No hard cap on states per session.

### use_count

Incremented each time a session is loaded or resumed. Available as a sort option in the history panel.

---

## State Collapse & Hygiene

### Problem

Without any management, the `session_states` table accumulates many low-value intermediate states from keystroke-by-keystroke editing.

### Strategies

**1. Debounce** — evaluation (and state writes) fire after ~150ms idle, not per keystroke. Reduces state count by ~10x compared to per-keystroke recording.

**2. Field-switch collapse** — when focus moves from one field to another, any consecutive run of states within the same field since the last boundary is collapsed into a single state (the last one in the run — what you had when you switched away).

**3. Pause boundary collapse** — a configurable idle period (default 5 seconds) marks a semantic boundary. States within a burst before the pause are collapsed to one. The 5-second default reflects "stopped to read results."

**4. Explicit retroactive collapse** — user-initiated per session from the history panel. Collapses all intermediate states to major checkpoints. Destructive and explicit.

### What Is Not Done

- No hard cap on states per session
- No word-boundary or semantic heuristics
- No automatic retroactive collapse — always user-initiated

---

## Reference Panels

### Escape Sequence Reference

Toggleable via `w` in the nav layer. Searchable. ~40-60 items across categories:

- Character classes: `\w \W \d \D \s \S`
- Anchors: `^ $ \b \B \A \Z`
- Quantifiers: `* + ? *? +? {n} {n,m}`
- Groups: `(...)` `(?:...)` `(?=...)` `(?!...)` `(?<=...)` `(?<!...)`
- Named groups: `(?P<name>...)` Python · `(?<name>...)` JS/Ruby/Rust · `(?'name'...)` PCRE
- Backreferences: `\1` `\k<name>`
- POSIX classes: `[:alpha:] [:digit:] [:space:] [:alnum:]` (grep/sed)
- Unicode properties: `\p{L} \p{N}` (Rust/Python `regex`/JS `/u`)

**Per-engine availability**: items unavailable on the current tab are shown greyed with a label indicating where they *are* supported. Items are never omitted — seeing that `(?<=...)` lookbehind is greyed on the Rust tab with "available in Python, JS, PHP, Ruby" is more useful than not seeing it at all.

### Flag Reference

Per-engine flag explainer accessible via `?`. Short tl;dr format. Examples:

> **Rust `regex` crate** — RE2-style engine. Guarantees linear time — no catastrophic backtracking. Lookahead, lookbehind, and backreferences are not supported. Enable `fancy-regex` mode for those (loses the time guarantee).

> **Python `re` vs `regex` module** — Built-in `re` uses a backtracking NFA. Third-party `regex` adds possessive quantifiers, atomic groups, and better Unicode. This tool uses `re` by default.

> **GNU grep -P** — Uses the PCRE library. Supports lookahead, lookbehind, backreferences. Not available on BSD grep or minimal GNU builds.

### Keybind Reference

`?` in the nav layer shows all keybinds. Full keybind table always available, never requires memorisation.

---

## Runtime Probing

### Startup Splash

Shown on every launch (skippable via `show_probe_splash = false` in config). Probes run sequentially, output appears in real time:

```
rgx v0.1.0 — runtime probe

  ✓ Rust        built-in
  ✓ Python      3.12.2   /usr/bin/python3
    Python `regex` module: not installed  →  pip install regex
  ✓ Node        v22.1.0  /opt/homebrew/bin/node
  ✗ Ruby        not found  →  brew install ruby
  ✓ PHP         8.3.0    /usr/bin/php
  ✓ Go          1.22.0   /usr/local/go/bin/go
  ✓ grep        BSD grep 2.6 — PCRE unavailable (-P not supported)
  ✓ ggrep       GNU grep 3.11 — PCRE available
  ✓ rg          14.1.0   /opt/homebrew/bin/rg
  ✗ sed (GNU)   not found  →  brew install gnu-sed
  ✓ sed         BSD sed  /usr/bin/sed

Press Enter to continue   [r] rescan
```

No auto-timeout — waits for user to press Enter. `[r]` rescans all runtimes in place.

### Mid-Session Rescan

`r` in the nav layer re-probes all runtimes and updates tab availability. Useful when a runtime is installed in another terminal window during a session.

### Install Hints

Per-runtime, per-platform. At minimum: Homebrew (macOS), apt (Debian/Ubuntu), canonical install URL. Shown on the probe splash and on greyed tabs when focused.

### Config Overrides

```toml
[runtimes.python]
enabled = true
path = "/usr/bin/python3"

[runtimes.ruby]
enabled = false   # skip probing entirely

[runtimes.grep]
prefer = "ggrep"
path = "/opt/homebrew/bin/ggrep"
```

---

## Configuration

File location: `~/.config/rgx/config.toml` (XDG config dir).

```toml
# UI
nerd_fonts = false
default_language = "rust"    # which tab opens by default
mouse = false                # enable mouse support
default_results_view = "split_vertical"  # "split_vertical" | "split_horizontal" | "preview" | "matches"

# Evaluation
debounce_ms = 150

# Session behaviour
on_open = "ask"              # "continue" | "new" | "ask"

# State collapse
pause_boundary_secs = 5

# History
history_cleanup_days = 30    # bulk-delete unnamed sessions older than this

# Startup
show_probe_splash = true

# File loading
large_file_warn_mb = 1.0

# Rust engine
fancy_regex_default = false  # use fancy-regex as default on Rust tab (off by default, toggle available per-session)

# Per-runtime overrides
[runtimes.grep]
prefer = "auto"              # "auto" | "gnu" | "bsd" | "ggrep" | "rg"
```

---

## Database Schema

Location: `~/.local/share/rgx/history.db` (XDG data dir).

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Schema version for migrations (rusqlite_migration crate)
CREATE TABLE schema_version (
    version INTEGER NOT NULL
);

CREATE TABLE sessions (
    id              INTEGER PRIMARY KEY,
    name            TEXT,                        -- null = unnamed
    language        TEXT NOT NULL,               -- last active engine tab
    flavor          TEXT,                        -- last active sub-flavor
    created_at      TEXT NOT NULL,               -- ISO 8601
    updated_at      TEXT NOT NULL,
    use_count       INTEGER NOT NULL DEFAULT 1,
    undo_cursor     INTEGER NOT NULL DEFAULT 0,  -- current seq position
    forked_from_id  INTEGER REFERENCES sessions(id),
    forked_at_seq   INTEGER                      -- seq in parent at fork point
);

CREATE TABLE session_states (
    id           INTEGER PRIMARY KEY,
    session_id   INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,
    pattern      TEXT NOT NULL,
    options      TEXT NOT NULL DEFAULT '',       -- normalized flags, JSON array
    input        TEXT NOT NULL DEFAULT '',
    replacement  TEXT NOT NULL DEFAULT '',
    mode         TEXT NOT NULL DEFAULT 'match',  -- "match" | "replace"
    source_file  TEXT,                           -- nullable, path of loaded file
    file_dirty   INTEGER NOT NULL DEFAULT 0,     -- 1 if edited after file load
    UNIQUE(session_id, seq)
);

-- Full-text search over session name and pattern
CREATE VIRTUAL TABLE sessions_fts USING fts5(
    name,
    pattern,
    content=sessions,
    content_rowid=id
);
```

**Migrations**: `rusqlite_migration` crate. Migrations run automatically on startup before any other DB access.

**Ancestry reconstruction**: to reconstruct the full state history of a forked session, walk `forked_from_id` recursively, collecting states up to `forked_at_seq` from each ancestor, then append the session's own states. States are never duplicated in the DB.

**FTS scope**: indexes `name` and `pattern` from the `sessions` table. Search finds sessions, not individual historical states.

---

## JSON Contract

Used between the binary and external engine scripts (binary writes request to script's stdin; script writes response to stdout).

**Request**:
```json
{
  "pattern": "hello\\s+(\\w+)",
  "flags": ["case_insensitive", "global"],
  "input": "Hello world\nhello mario",
  "mode": "match",
  "replacement": "hi {1}"
}
```

Flags are string identifiers from a fixed vocabulary: `case_insensitive`, `multiline`, `dotall`, `global`, `extended`, `unicode`. Each script maps these to its engine's native mechanism.

Replacement uses the normalized internal form (`{1}`, `{name}`). Each script translates to its native backreference syntax before invoking the engine.

**Response**:
```json
{
  "matches": [
    {
      "full_match": "Hello world",
      "span": [0, 11],
      "groups": [
        {
          "index": 1,
          "name": null,
          "value": "world",
          "span": [6, 11],
          "matched": true
        }
      ]
    }
  ],
  "replaced": null,
  "error": null
}
```

In replace mode, `replaced` contains the full transformed input string. `matches` may still be populated in replace mode for highlighting purposes.

**Error response**:
```json
{
  "matches": [],
  "replaced": null,
  "error": {
    "kind": "syntax",
    "message": "missing closing parenthesis",
    "position": 10
  }
}
```

`error.kind` is one of: `syntax`, `timeout`, `unsupported_flag`, `runtime_error`.
`error.position` is nullable — not all engines report error position.

**No version field** — scripts are embedded in the binary at compile time and always in sync with the binary. Version skew is impossible.

---

*Last updated: initial design — pre-implementation*
