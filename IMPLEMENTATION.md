# rgx — Implementation Order

> This document suggests a build sequence for `rgx`. Each step leaves the tool
> in a working (if incomplete) state. It is a guide, not a contract — a different
> implementer may choose a different order and that is fine.
>
> Read DESIGN.md first for full feature descriptions, rationale, and data structures.

---

## Phase 1 — Core evaluation loop

The goal of this phase is the simplest possible working tool: type a pattern, see
Rust matches. Everything else builds on this foundation.

**1. Project scaffold**
- Cargo workspace layout
- Module skeleton: `engine`, `tui`, `session`, `db`, `config`
- CI skeleton (GitHub Actions, matrix builds)
- `clap` for any CLI flags (e.g. `--config`, `--db`)

**2. Native Rust engine**
- `regex` crate evaluation
- Internal `EvalRequest` / `EvalResponse` types (mirrors the JSON contract but
  native Rust structs — no serialization yet)
- `fancy-regex` behind a feature flag or runtime toggle, off by default

**3. Basic TUI skeleton**
- `ratatui` setup, event loop, graceful exit
- Three panes: pattern field, test input field, results pane
- `tui-textarea` for both text fields
- Hardwired to the Rust engine — no tabs yet

**4. Live evaluation with debounce**
- Wire pattern + input changes to the Rust engine
- Debounce timer (~150ms)
- Results pane shows match count and per-match breakdown
- Match spans highlighted inline in the input pane (ANSI color)
- Clear no-match vs parse-error distinction

---

## Phase 2 — Flags, status, and navigation

**5. Flag row**
- Per-engine flag toggles (case insensitive, multiline, dotall, global)
- Bidirectional sync: toggling a flag updates evaluation; inline `(?i)` prefix
  detection updates the toggle
- Flags hidden (not greyed) when not meaningful for the current engine

**6. Status line**
- Always-visible rendered invocation for current engine + flags
- Live updates with every change
- Match count on the right

**7. Navigation model**
- Escape → nav layer, bare-key shortcuts active
- `ctrl+p` / `ctrl+t` / `ctrl+g` quick-jump keybinds (work inside text fields)
- `ctrl+z` / `ctrl+shift+z` undo/redo (wired to in-memory stack for now —
  SQLite comes later)
- Full keybind table from DESIGN.md implemented and conflict-checked
- `?` opens keybind reference panel
- Terminal-too-small guard

---

## Phase 3 — Script engine infrastructure

**8. Script engine infrastructure**
- `include_str!()` embedding for all engine scripts
- Temp dir management (`$TMPDIR/rgx/engines/`) — write on first use, rewrite if
  binary is newer
- Subprocess management: spawn, write JSON request to stdin, read JSON response
  from stdout, timeout handling
- JSON serialization of `EvalRequest` / `EvalResponse`
- Error handling: distinguish syntax errors, runtime errors, timeout

**9. Python engine script**
- `re` module, all flags, match + replace mode
- Named and indexed group support
- `matched: false` for non-participating optional groups

**10. Node engine script**
- All flags, match + replace mode
- Named groups (`(?<name>...)`)
- Handle Node version differences for `/v` flag

**11. Engine tab bar**
- Tab switching UI
- Greyed tabs for unavailable engines with install hint on focus
- Only Rust, Python, Node wired at this point — others added in Phase 6

---

## Phase 4 — Runtime probing

**12. Runtime probing**
- Probe logic per runtime: `which` + `--version` parsing
- Startup splash: real-time output as each probe completes, wait for Enter, `[r]`
  rescan
- `AvailableEngines` struct populated once at startup, consulted for tab
  availability
- Mid-session rescan via `r` in nav layer
- Install hints per runtime per platform (Homebrew, apt, canonical URL)

---

## Phase 5 — grep and sed tabs

**13. grep tab**
- Sub-flavor selector (only available flavors shown)
- BSD vs GNU vs ggrep detection (including `ggrep` on macOS)
- PCRE availability probed separately for GNU grep
- Flag-only UI (no pattern options field)
- Command preview line
- Replace toggle disabled with tooltip

**14. sed tab**
- BSD vs GNU vs `gsed` detection
- Sub-flavor selector (BRE/ERE)
- Command preview line
- Match mode nudge message
- Replace mode fully supported (see Phase 6 for replace mode UI)

---

## Phase 6 — Replace mode

**15. Replace mode**
- `m` in nav layer toggles mode
- Replacement input field appears above results pane
- Normalized internal replacement syntax (`{1}`, `{name}`)
- Per-engine translation table (see DESIGN.md Replace Mode section)
- Named → indexed auto-conversion for sed, with warning and restoration on
  switch back
- `rg --replace` support in the grep tab
- Unsupported flag warnings when switching engines with active flags

---

## Phase 7 — Persistence and session model

**16. SQLite foundation**
- Schema creation and `rusqlite_migration` setup
- Schema version table, initial migration
- Session and `session_states` tables
- FTS5 virtual table and triggers

**17. Undo/redo via SQLite**
- Replace in-memory undo stack with SQLite-backed session states
- `undo_cursor` persisted on every change
- Crash recovery guaranteed by WAL mode + atomic writes

**18. Session list and history panel**
- Toggleable history panel (`h` in nav layer)
- Session list with current session highlighted
- Filter bar: All / Named / This language / Recent / Search
- Switch to session (saves + restores `undo_cursor` and language tab)
- `y` on highlighted entry opens copy menu without loading
- `n` to rename, `d` to delete

**19. Implicit fork on non-head edit**
- Detect edit while `undo_cursor` is not at head
- Create new session with `forked_from_id` + `forked_at_seq`
- Ancestry reconstruction for display
- Fork indicator in UI

---

## Phase 8 — State collapse and hygiene

**20. State collapse heuristics**
- Field-switch collapse: collapse same-field run when focus leaves
- Pause boundary collapse: collapse burst older than 5s idle
- Explicit retroactive collapse: user-initiated from history panel
- Bulk delete: unnamed sessions older than N days (configurable)
- Collapse session to first + last state (user-initiated)

---

## Phase 9 — Remaining engines

**21. Ruby engine script**
**22. PHP engine script**
**23. Go engine script**

Each follows the same pattern as Python/Node. Wire into the tab bar.

---

## Phase 10 — Sub-toggles

**24. `fancy-regex` sub-toggle on Rust tab**
- Toggle within the Rust tab, off by default
- Respects `fancy_regex_default` config key

**25. Python `regex` module sub-toggle**
- Toggle within the Python tab, off by default
- Greyed with `pip install regex` hint if module not available
- Probe separately from Python itself

---

## Phase 11 — Reference panels and copy

**26. Escape sequence reference panel**
- Toggleable via `w` in nav layer
- ~40-60 items, searchable
- Per-engine availability: greyed with engine label, never omitted

**27. Flag explainer and engine reference**
- `?` in nav layer opens help panel
- Per-engine tl;dr explainers (RE2 vs PCRE, Python re vs regex, grep flavors, etc.)

**28. Code preview panel**
- Toggleable via `p` in nav layer
- Full pasteable snippet per engine
- Syntax translation warnings (e.g. `\w` in BRE)

**29. Copy menu**
- `y` in nav layer: pattern / status line / code snippet
- Same menu accessible from history panel entries without loading

---

## Phase 12 — File input

**30. Load input from file**
- Path input line with tab completion
- Binary file confirmation prompt (default No)
- Large file confirmation prompt above threshold (default No, 1MB)
- `source_file` + `file_dirty` tracking in session states

**31. Reload and clear**
- Reload keybind (active only when `source_file` set)
- Dirty check before reload
- File-not-found handling
- Clear keybind with confirmation

---

## Phase 13 — Configuration

**32. Configuration file**
- Full `~/.config/rgx/config.toml` support
- All keys from DESIGN.md Configuration section
- Per-runtime path overrides and enable/disable
- Config loaded at startup, validated with helpful error messages for unknown keys

---

## Phase 14 — Polish

**33. `$NO_COLOR` support**
- Bracket markers instead of ANSI color for match spans
- Verify all color usage respects the flag

**34. Nerd fonts**
- `RGX_NERD_FONTS=1` env var and `nerd_fonts = true` config key
- Engine tab logos via Nerd Font codepoints

**35. Mouse support**
- Optional, off by default, `mouse = true` in config
- Click to focus pane

**36. Manpage**
- `roff` or `md2man` generated manpage
- Covers all keybinds, config keys, runtime requirements

**37. README**
- Install instructions (cargo install, prebuilt binaries)
- Quick start
- Engine availability matrix
- Config reference

---

*Last updated: initial design — pre-implementation*
