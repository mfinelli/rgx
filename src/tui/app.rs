/* rgx: command line regexp tester
 * Copyright 2026 Mario Finelli
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use ratatui_textarea::TextArea;

use crate::engine::{
    native::{
        RustEngine, render_invocation_fancy_regex,
        render_invocation_regex_crate,
    },
    types::{EngineError, EvalMode, EvalRequest, EvalResponse, Flags, Match},
};

#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    /// Keypresses go to the focused text field.
    Insert,
    /// Escape was pressed; bare-key shortcuts are active.
    Nav,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Focus {
    Pattern,
    Input,
    Flags,
    Results,
}

/// The four results pane layouts, cycled with `v` in nav mode.
#[derive(Debug, Clone, PartialEq)]
pub enum ResultsView {
    /// Input preview (top) + match breakdown (bottom).
    SplitVertical,
    /// Input preview (left) + match breakdown (right).
    SplitHorizontal,
    /// Input preview only, full pane.
    Preview,
    /// Match breakdown only, full pane.
    Matches,
}

impl ResultsView {
    /// Advance to the next view in the cycle.
    pub fn next(&self) -> Self {
        match self {
            Self::SplitVertical => Self::SplitHorizontal,
            Self::SplitHorizontal => Self::Preview,
            Self::Preview => Self::Matches,
            Self::Matches => Self::SplitVertical,
        }
    }

    /// Short label shown in the hint bar.
    pub fn label(&self) -> &'static str {
        match self {
            Self::SplitVertical => "split-v",
            Self::SplitHorizontal => "split-h",
            Self::Preview => "preview",
            Self::Matches => "matches",
        }
    }
}

/// Which sub-pane receives scroll input in split views.
#[derive(Debug, Clone, PartialEq)]
pub enum ResultsSubFocus {
    Preview,
    Matches,
}

/// Where the cursor sits within the flag row.
#[derive(Debug, Clone, PartialEq)]
pub enum FlagRowFocus {
    /// The variant selector at the left of the row.
    Variant,
    /// One of the flag toggles (0-indexed).
    Flag(usize),
}

pub struct App<'a> {
    pub mode: AppMode,
    pub focus: Focus,

    pub pattern: TextArea<'a>,
    pub input: TextArea<'a>,

    pub flags: Flags,
    /// Which item in the flag row is currently highlighted.
    pub flag_row_focus: FlagRowFocus,

    pub use_fancy: bool,

    pub results_view: ResultsView,
    /// Scroll offset for the match breakdown sub-pane.
    pub matches_scroll: usize,
    /// Scroll offset for the input preview sub-pane.
    pub preview_scroll: usize,

    pub eval_result: Option<Result<EvalResponse, EngineError>>,
    pub last_edit: Option<Instant>,
    pub debounce_ms: u64,

    /// Which sub-pane is active in split views (determines scroll target).
    pub results_sub_focus: ResultsSubFocus,

    /// Whether to show the keybind help overlay.
    pub show_help: bool,
}

impl<'a> App<'a> {
    /// Create a new `App` with default state, ready for the event loop.
    pub fn new() -> Self {
        let mut pattern = TextArea::default();
        pattern.set_block(
            Block::default()
                .title(" Pattern ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        );
        pattern.set_cursor_line_style(Style::default());

        let mut input = TextArea::default();
        input.set_block(
            Block::default().title(" Test Input ").borders(Borders::ALL),
        );
        input.set_cursor_line_style(Style::default());

        Self {
            mode: AppMode::Insert,
            focus: Focus::Pattern,
            pattern,
            input,
            flags: Flags {
                global: true,
                ..Default::default()
            },
            flag_row_focus: FlagRowFocus::Variant,
            use_fancy: false,
            results_view: ResultsView::SplitVertical,
            matches_scroll: 0,
            preview_scroll: 0,
            eval_result: None,
            last_edit: None,
            debounce_ms: 150,
            results_sub_focus: ResultsSubFocus::Matches,
            show_help: false,
        }
    }

    /// Called on every text edit — resets the debounce timer.
    pub fn mark_dirty(&mut self) {
        self.last_edit = Some(Instant::now());
    }

    /// Evaluate if the debounce period has elapsed since the last edit.
    pub fn maybe_evaluate(&mut self, engine: &RustEngine) {
        if let Some(last) = self.last_edit {
            if last.elapsed() >= Duration::from_millis(self.debounce_ms) {
                self.evaluate(engine);
                self.last_edit = None;
            }
        }
    }

    /// Run the engine against the current pattern and input, storing the result.
    /// Resets scroll offsets when results change.
    fn evaluate(&mut self, engine: &RustEngine) {
        let pattern = self.pattern.lines().join("\n");
        let input_text = self.input.lines().join("\n");

        if pattern.is_empty() {
            self.eval_result = None;
            return;
        }

        let req = EvalRequest {
            pattern,
            flags: self.flags.clone(),
            input: input_text,
            mode: EvalMode::Match,
            replacement: String::new(),
            use_fancy: self.use_fancy,
        };

        self.eval_result = Some(engine.evaluate(&req));
        // Reset scroll when results change.
        self.matches_scroll = 0;
        self.preview_scroll = 0;
    }

    /// Update border styles to reflect current focus.
    pub fn update_borders(&mut self) {
        let active = Style::default().fg(Color::Yellow);
        let inactive = Style::default().fg(Color::DarkGray);

        self.pattern.set_block(
            Block::default()
                .title(" Pattern ")
                .borders(Borders::ALL)
                .border_style(if self.focus == Focus::Pattern {
                    active
                } else {
                    inactive
                }),
        );
        self.input.set_block(
            Block::default()
                .title(" Test Input ")
                .borders(Borders::ALL)
                .border_style(if self.focus == Focus::Input {
                    active
                } else {
                    inactive
                }),
        );
    }

    /// Move focus and switch to Insert mode (for text fields).
    /// For non-text panes (Flags, Results) stays in Nav mode.
    pub fn jump_to(&mut self, focus: Focus) {
        let is_text = matches!(focus, Focus::Pattern | Focus::Input);
        self.focus = focus;
        if is_text {
            self.mode = AppMode::Insert;
        }
        self.update_borders();
    }

    /// Cycle focus forward: Pattern → Input → Flags → Results → Pattern
    ///
    /// Order matches the visual top-to-bottom layout.
    /// Does not enter Insert mode — Tab cycling should never capture input.
    /// Insert mode activates only when the user presses Enter or types a char.
    fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Pattern => Focus::Input,
            Focus::Input => Focus::Flags,
            Focus::Flags => Focus::Results,
            Focus::Results => Focus::Pattern,
        };
        self.update_borders();
    }

    /// Cycle focus backward: Pattern → Results → Flags → Input → Pattern
    fn cycle_focus_back(&mut self) {
        self.focus = match self.focus {
            Focus::Pattern => Focus::Results,
            Focus::Input => Focus::Pattern,
            Focus::Flags => Focus::Input,
            Focus::Results => Focus::Flags,
        };
        self.update_borders();
    }

    /// Cycle the engine variant (global shortcut and flag row Space/Enter).
    pub fn cycle_variant(&mut self) {
        self.use_fancy = !self.use_fancy;
        self.mark_dirty();
    }

    /// Toggle the flag currently highlighted in the flag row, or cycle the
    /// variant if the variant selector is focused.
    fn activate_flag_row(&mut self) {
        match self.flag_row_focus {
            FlagRowFocus::Variant => self.cycle_variant(),
            FlagRowFocus::Flag(i) => {
                match i {
                    0 => {
                        self.flags.case_insensitive =
                            !self.flags.case_insensitive
                    }
                    1 => self.flags.multiline = !self.flags.multiline,
                    2 => self.flags.dotall = !self.flags.dotall,
                    3 => self.flags.global = !self.flags.global,
                    _ => {}
                }
                self.mark_dirty();
            }
        }
    }

    /// Scroll the active results sub-pane up by one line.
    fn scroll_up(&mut self) {
        match (&self.results_view, &self.results_sub_focus) {
            (ResultsView::Preview, _)
            | (
                ResultsView::SplitVertical | ResultsView::SplitHorizontal,
                ResultsSubFocus::Preview,
            ) => {
                self.preview_scroll = self.preview_scroll.saturating_sub(1);
            }
            (ResultsView::Matches, _)
            | (
                ResultsView::SplitVertical | ResultsView::SplitHorizontal,
                ResultsSubFocus::Matches,
            ) => {
                self.matches_scroll = self.matches_scroll.saturating_sub(1);
            }
        }
    }

    /// Scroll the active results sub-pane down by one line.
    fn scroll_down(&mut self) {
        match (&self.results_view, &self.results_sub_focus) {
            (ResultsView::Preview, _)
            | (
                ResultsView::SplitVertical | ResultsView::SplitHorizontal,
                ResultsSubFocus::Preview,
            ) => {
                self.preview_scroll = self.preview_scroll.saturating_add(1);
            }
            (ResultsView::Matches, _)
            | (
                ResultsView::SplitVertical | ResultsView::SplitHorizontal,
                ResultsSubFocus::Matches,
            ) => {
                self.matches_scroll = self.matches_scroll.saturating_add(1);
            }
        }
    }

    /// Toggle which sub-pane is active in split views.
    fn toggle_sub_focus(&mut self) {
        self.results_sub_focus = match self.results_sub_focus {
            ResultsSubFocus::Preview => ResultsSubFocus::Matches,
            ResultsSubFocus::Matches => ResultsSubFocus::Preview,
        };
    }

    /// Build the rendered invocation string for the status line.
    /// Returns the fully rendered invocation shown in the status line.
    ///
    /// Delegates to the engine-specific renderer so each engine shows its
    /// own idiomatic syntax. As more engines are added this will dispatch
    /// to each engine's own implementation.
    pub fn status_invocation(&self) -> String {
        let pattern = self.pattern.lines().join("");
        if self.use_fancy {
            render_invocation_fancy_regex(&pattern, &self.flags)
        } else {
            render_invocation_regex_crate(&pattern, &self.flags)
        }
    }
}

/// Process one key event. Returns `true` if the application should quit.
/// Always-available ctrl shortcuts (work inside text fields)
pub fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use KeyCode::*;
    use KeyModifiers as KM;

    // ctrl+p — jump to pattern field
    if key.modifiers == KM::CONTROL && key.code == Char('p') {
        app.jump_to(Focus::Pattern);
        return false;
    }

    // ctrl+t — jump to test input field
    if key.modifiers == KM::CONTROL && key.code == Char('t') {
        app.jump_to(Focus::Input);
        return false;
    }

    match app.mode {
        AppMode::Insert => handle_insert(app, key),
        AppMode::Nav => handle_nav(app, key),
    }
}

/// Handle a key event while in Insert mode.
/// Returns `true` if the application should quit (always `false` here —
/// quitting is only triggered from Nav mode).
fn handle_insert(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use KeyCode::*;

    match key.code {
        Esc => {
            if app.show_help {
                app.show_help = false;
            } else {
                app.mode = AppMode::Nav;
                app.update_borders();
            }
        }
        _ => match app.focus {
            Focus::Pattern => {
                app.pattern.input(key);
                app.mark_dirty();
            }
            Focus::Input => {
                app.input.input(key);
                app.mark_dirty();
            }
            Focus::Flags => {
                if key.code == Char(' ') {
                    app.activate_flag_row();
                }
            }
            Focus::Results => {} // Results pane is never in insert mode
        },
    }
    false
}

/// Handle a key event while in Nav mode.
/// Returns `true` if the application should quit (e.g. user pressed `q`).
fn handle_nav(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use KeyCode::*;
    use KeyModifiers as KM;

    match (key.code, key.modifiers) {
        // Quit
        (Char('q'), KM::NONE) => return true,

        // Help overlay
        (Char('?'), KM::NONE) => app.show_help = !app.show_help,
        (Esc, KM::NONE) if app.show_help => app.show_help = false,

        // Cycle engine variant (f for fancy-regex roots, generic across engines)
        (Char('f'), KM::NONE) => app.cycle_variant(),

        // Cycle results view
        (Char('v'), KM::NONE) => {
            app.results_view = app.results_view.next();
        }

        // Tab — cycle focus forward
        (Tab, KM::NONE) => app.cycle_focus(),

        // Shift+Tab — cycle focus backward
        (BackTab, _) => app.cycle_focus_back(),

        // Arrow keys — scroll results when results pane is focused.
        // Up/Down on text panes are handled by the editing-intent arm below.
        (Up, KM::NONE) if app.focus == Focus::Results => app.scroll_up(),
        (Down, KM::NONE) if app.focus == Focus::Results => app.scroll_down(),
        // Left/Right in results: toggle active sub-pane in split views
        (Left, KM::NONE) | (Right, KM::NONE) if app.focus == Focus::Results => {
            if matches!(
                app.results_view,
                ResultsView::SplitVertical | ResultsView::SplitHorizontal
            ) {
                app.toggle_sub_focus();
            }
        }
        (Left, KM::NONE) if app.focus == Focus::Flags => {
            app.flag_row_focus = match &app.flag_row_focus {
                FlagRowFocus::Variant => FlagRowFocus::Variant,
                FlagRowFocus::Flag(0) => FlagRowFocus::Variant,
                FlagRowFocus::Flag(i) => FlagRowFocus::Flag(i - 1),
            };
        }
        (Right, KM::NONE) if app.focus == Focus::Flags => {
            app.flag_row_focus = match &app.flag_row_focus {
                FlagRowFocus::Variant => FlagRowFocus::Flag(0),
                FlagRowFocus::Flag(i) if *i < 3 => FlagRowFocus::Flag(i + 1),
                FlagRowFocus::Flag(i) => FlagRowFocus::Flag(*i),
            };
        }

        // Space/Enter — activate focused flag row item
        (Char(' '), KM::NONE) if app.focus == Focus::Flags => {
            app.activate_flag_row();
        }

        // Any editing-intent key re-enters insert mode on focused text panes
        // and forwards the keypress so it takes effect immediately.
        // Covers: printable chars, Enter, arrows, Backspace, Delete.
        (Enter, KM::NONE)
        | (Char(_), KM::NONE)
        | (Up, KM::NONE)
        | (Down, KM::NONE)
        | (Left, KM::NONE)
        | (Right, KM::NONE)
        | (Backspace, KM::NONE)
        | (Delete, KM::NONE)
            if matches!(app.focus, Focus::Pattern | Focus::Input) =>
        {
            app.mode = AppMode::Insert;
            app.update_borders();
            // Forward the key so it takes effect immediately — the cursor
            // moves or character is deleted without needing a second keypress.
            match app.focus {
                Focus::Pattern => {
                    app.pattern.input(key);
                    app.mark_dirty();
                }
                Focus::Input => {
                    app.input.input(key);
                    app.mark_dirty();
                }
                _ => {}
            }
        }

        _ => {}
    }
    false
}

/// Render the full application UI into `frame`.
/// Dispatches to per-pane render functions. Shows a size warning if the
/// terminal is too small to render the layout meaningfully.
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    if area.width < 40 || area.height < 12 {
        frame.render_widget(
            Paragraph::new("terminal too small — resize to continue")
                .style(Style::default().fg(Color::Red)),
            area,
        );
        return;
    }

    // Vertical layout:
    // [engine bar 1] [pattern 3] [input flex] [flags 1] [results flex] [status 1] [hint 1]
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // engine bar
            Constraint::Length(3), // pattern
            Constraint::Min(4),    // input
            Constraint::Length(1), // flags row
            Constraint::Min(4),    // results
            Constraint::Length(1), // status
            Constraint::Length(1), // key hint
        ])
        .split(area);

    render_engine_bar(app, frame, chunks[0]);
    render_pattern(app, frame, chunks[1]);
    render_input(app, frame, chunks[2]);
    render_flags(app, frame, chunks[3]);
    render_results(app, frame, chunks[4]);
    render_status(app, frame, chunks[5]);
    render_hint(app, frame, chunks[6]);

    if app.show_help {
        render_help_overlay(frame, area);
    }
}

/// Render the engine tab bar at the top of the layout.
/// Currently shows only the active Rust engine variant; will become a
/// full tab bar once multiple engines are implemented.
fn render_engine_bar(app: &App, frame: &mut Frame, area: Rect) {
    let engine_name = if app.use_fancy {
        " [ Rust · fancy-regex ] "
    } else {
        " [ Rust · regex ] "
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            engine_name,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        area,
    );
}

/// Render the pattern input field.
fn render_pattern(app: &App, frame: &mut Frame, area: Rect) {
    frame.render_widget(&app.pattern, area);
}

/// Render the variant selector and flag toggles row.
/// The variant selector sits at the left; flags follow to the right.
/// Both are navigable with arrow keys when the flags row has focus.
fn render_flags(app: &App, frame: &mut Frame, area: Rect) {
    let flags = &app.flags;
    let focus = &app.flag_row_focus;
    let row_focused = app.focus == Focus::Flags;
    let sep = Span::styled(" │ ", Style::default().fg(Color::DarkGray));

    // Variant selector at the left of the row.
    let variant_label = if app.use_fancy {
        "fancy-regex"
    } else {
        "regex"
    };
    let variant_focused = row_focused && *focus == FlagRowFocus::Variant;
    let variant_style = if variant_focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let variant_span =
        Span::styled(format!(" [{}] ", variant_label), variant_style);

    let flag_defs = [
        ("Case insensitive", flags.case_insensitive),
        ("Multiline", flags.multiline),
        ("Dotall", flags.dotall),
        ("Global", flags.global),
    ];

    let mut spans = vec![variant_span, sep.clone()];

    for (i, (label, on)) in flag_defs.iter().enumerate() {
        let indicator = if *on { "◉" } else { "○" };
        let is_cursor = row_focused && *focus == FlagRowFocus::Flag(i);
        let style = if is_cursor {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if *on {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(format!(" {} {} ", indicator, label), style));
        if i < flag_defs.len() - 1 {
            spans.push(sep.clone());
        }
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Render the test input field.
fn render_input(app: &App, frame: &mut Frame, area: Rect) {
    frame.render_widget(&app.input, area);
}

/// Render the results pane, dispatching to the appropriate layout based
/// on `app.results_view`. Shows a placeholder when there is no pattern,
/// an error message on parse failure, or match results on success.
fn render_results(app: &App, frame: &mut Frame, area: Rect) {
    let focused = app.focus == Focus::Results;
    let border_style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let view_label = app.results_view.label();
    let title = format!(" Results [{}] ", view_label);

    let block = Block::default()
        .title(title.as_str())
        .borders(Borders::ALL)
        .border_style(border_style);

    match &app.eval_result {
        None => {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "no pattern",
                    Style::default().fg(Color::DarkGray),
                ))
                .block(block),
                area,
            );
        }
        Some(Err(e)) => {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    e.to_string(),
                    Style::default().fg(Color::Red),
                ))
                .block(block)
                .wrap(Wrap { trim: false }),
                area,
            );
        }
        Some(Ok(resp)) => {
            render_match_results(app, resp, block, frame, area);
        }
    }
}

/// Render match results inside the results block, splitting the inner area
/// into preview and match-list sub-panes according to `app.results_view`.
fn render_match_results(
    app: &App,
    resp: &EvalResponse,
    block: Block,
    frame: &mut Frame,
    area: Rect,
) {
    let preview_active = app.results_sub_focus == ResultsSubFocus::Preview;
    let matches_active = app.results_sub_focus == ResultsSubFocus::Matches;
    let input_text = app.input.lines().join("\n");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if resp.matches.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "no match",
                Style::default().fg(Color::DarkGray),
            )),
            inner,
        );
        return;
    }

    match app.results_view {
        ResultsView::SplitVertical => {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(40),
                    Constraint::Percentage(60),
                ])
                .split(inner);
            render_preview(
                app,
                &input_text,
                &resp.matches,
                preview_active,
                frame,
                split[0],
            );
            render_match_list(app, resp, matches_active, frame, split[1]);
        }
        ResultsView::SplitHorizontal => {
            let split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(50),
                    Constraint::Percentage(50),
                ])
                .split(inner);
            render_preview(
                app,
                &input_text,
                &resp.matches,
                preview_active,
                frame,
                split[0],
            );
            render_match_list(app, resp, matches_active, frame, split[1]);
        }
        ResultsView::Preview => {
            render_preview(app, &input_text, &resp.matches, true, frame, inner);
        }
        ResultsView::Matches => {
            render_match_list(app, resp, true, frame, inner);
        }
    }
}

/// Renders the input text with match spans highlighted and a scrollbar.
///
/// Uses a per-byte style map so overlapping spans are handled correctly —
/// group highlights take precedence over full-match highlights where they
/// overlap, avoiding duplicate text from cursor rewinding.
fn render_preview(
    app: &App,
    input: &str,
    matches: &[Match],
    active: bool,
    frame: &mut Frame,
    area: Rect,
) {
    let match_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let group_style = Style::default().fg(Color::Black).bg(Color::Cyan);

    // Collect highlights in priority order: full matches first, groups after.
    // When written into the style map, groups overwrite match styles where
    // they overlap, giving groups visual precedence.
    let highlights: Vec<(usize, usize, Style)> = matches
        .iter()
        .map(|m| (m.span.0, m.span.1, match_style))
        .chain(matches.iter().flat_map(|m| {
            m.groups
                .iter()
                .filter(|g| g.matched)
                .filter_map(|g| g.span.map(|(s, e)| (s, e, group_style)))
        }))
        .collect();

    let mut all_lines: Vec<Line> = Vec::new();
    let mut byte_pos: usize = 0;

    for raw_line in input.split('\n') {
        let line_start = byte_pos;
        let line_end = byte_pos + raw_line.len();

        // Build a style map over the bytes of this line.
        // None = unstyled, Some(style) = highlighted.
        let mut style_map: Vec<Option<Style>> = vec![None; raw_line.len()];
        for &(hs, he, style) in &highlights {
            let hs =
                hs.max(line_start).min(line_end).saturating_sub(line_start);
            let he =
                he.max(line_start).min(line_end).saturating_sub(line_start);
            if hs >= he {
                continue;
            }
            for cell in style_map[hs..he].iter_mut() {
                *cell = Some(style);
            }
        }

        // Merge consecutive bytes with the same style into spans.
        // We walk char boundaries to avoid slicing mid-codepoint.
        let mut spans: Vec<Span> = Vec::new();
        let mut seg_start = 0;
        let mut chars = raw_line.char_indices().peekable();

        while let Some((i, _)) = chars.next() {
            let current_style = style_map[i];
            let next_style = chars.peek().map(|(j, _)| style_map[*j]);

            if next_style != Some(current_style) {
                // Find the end of this segment: start of next char or line end
                let seg_end =
                    chars.peek().map(|(j, _)| *j).unwrap_or(raw_line.len());
                let text = &raw_line[seg_start..seg_end];
                match current_style {
                    Some(s) => spans.push(Span::styled(text.to_string(), s)),
                    None => spans.push(Span::raw(text.to_string())),
                }
                seg_start = seg_end;
            }
        }

        if spans.is_empty() {
            spans.push(Span::raw(raw_line.to_string()));
        }

        all_lines.push(Line::from(spans));
        byte_pos = line_end + 1;
    }

    let total_lines = all_lines.len();
    let scroll = app.preview_scroll.min(total_lines.saturating_sub(1));

    // Reserve one column on the right for the scrollbar.
    let preview_area = Rect {
        width: area.width.saturating_sub(1),
        ..area
    };
    let scrollbar_area = Rect {
        x: area.x + area.width.saturating_sub(1),
        width: 1,
        ..area
    };

    let title_style = if active {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let para = Paragraph::new(all_lines)
        .block(
            Block::default()
                .title(Span::styled(" Preview ", title_style))
                .borders(Borders::TOP),
        )
        .scroll((scroll as u16, 0));
    frame.render_widget(para, preview_area);

    let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        scrollbar_area,
        &mut scrollbar_state,
    );
}

/// Renders the per-match breakdown list with a scrollbar.
fn render_match_list(
    app: &App,
    resp: &EvalResponse,
    active: bool,
    frame: &mut Frame,
    area: Rect,
) {
    let count = resp.matches.len();
    let header_style = Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let group_style = Style::default().fg(Color::Cyan);
    let unmatched_style = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::ITALIC);

    let mut items: Vec<ListItem> = Vec::new();

    // Header row counts as item 0 — offset must account for it.
    items.push(ListItem::new(Line::from(Span::styled(
        format!("{} match{}", count, if count == 1 { "" } else { "es" }),
        header_style,
    ))));

    for (i, m) in resp.matches.iter().enumerate() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                format!("  Match {} ", i + 1),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(format!("[{}..{}]", m.span.0, m.span.1), dim),
            Span::raw("  "),
            Span::styled(
                format!("\"{}\"", truncate(&m.full_match, 40)),
                Style::default().fg(Color::White),
            ),
        ])));

        for g in &m.groups {
            let label = match &g.name {
                Some(n) => format!("    group {} ({}) ", g.index, n),
                None => format!("    group {} ", g.index),
            };
            if g.matched {
                let span_str = g
                    .span
                    .map(|(s, e)| format!("[{}..{}]", s, e))
                    .unwrap_or_default();
                let val = g.value.as_deref().unwrap_or("");
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(label, group_style),
                    Span::styled(span_str, dim),
                    Span::raw("  "),
                    Span::styled(
                        format!("\"{}\"", truncate(val, 30)),
                        Style::default().fg(Color::White),
                    ),
                ])));
            } else {
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(label, unmatched_style),
                    Span::styled("(unmatched)", unmatched_style),
                ])));
            }
        }
    }

    let total_items = items.len();

    // Reserve one column on the right for the scrollbar.
    let list_area = Rect {
        width: area.width.saturating_sub(1),
        ..area
    };
    let scrollbar_area = Rect {
        x: area.x + area.width.saturating_sub(1),
        width: 1,
        ..area
    };

    // ListState.offset controls which item appears at the top of the visible
    // area — this is the idiomatic ratatui scroll mechanism for List widgets.
    let scroll = app.matches_scroll.min(total_items.saturating_sub(1));
    let mut state = ListState::default();
    *state.offset_mut() = scroll;

    let title_style = if active {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    frame.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(" Matches ", title_style))
                .borders(Borders::TOP),
        ),
        list_area,
        &mut state,
    );

    let mut scrollbar_state = ScrollbarState::new(total_items).position(scroll);
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        scrollbar_area,
        &mut scrollbar_state,
    );
}

/// Render the status line showing the rendered engine invocation on the
/// left and the match count on the right.
fn render_status(app: &App, frame: &mut Frame, area: Rect) {
    let invocation = app.status_invocation();
    let match_count = match &app.eval_result {
        Some(Ok(r)) => format!(
            "{} match{}",
            r.matches.len(),
            if r.matches.len() == 1 { "" } else { "es" }
        ),
        Some(Err(_)) => "error".to_string(),
        None => String::new(),
    };

    let left_str = format!(" {} ", invocation);
    let right_str = format!(" {} ", match_count);
    let padding =
        (area.width as usize).saturating_sub(left_str.len() + right_str.len());

    let line = Line::from(vec![
        Span::styled(left_str, Style::default().fg(Color::White)),
        Span::raw(" ".repeat(padding)),
        Span::styled(
            right_str,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(Color::DarkGray)),
        area,
    );
}

/// Render the hint bar at the bottom showing the current mode indicator
/// and a summary of the most common keybinds.
fn render_hint(app: &App, frame: &mut Frame, area: Rect) {
    let mode_indicator = match app.mode {
        AppMode::Insert => Span::styled(
            " INSERT ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        AppMode::Nav => Span::styled(
            " NAV ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let hints = Span::styled(
        "  Esc: nav  │  ctrl+p: pattern  │  ctrl+t: input  │  Tab: cycle  │  v: view  │  f: fancy  │  ?: help  │  q: quit",
        Style::default().fg(Color::DarkGray),
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![mode_indicator, hints])),
        area,
    );
}

/// Render the keybind reference overlay, centered over the full terminal area.
/// Shown when `app.show_help` is true; dismissed with `?` or `Esc`.
fn render_help_overlay(frame: &mut Frame, area: Rect) {
    use ratatui::widgets::Clear;

    let width = area.width.min(62);
    let height = 26u16;
    let x = area.width.saturating_sub(width) / 2;
    let y = area.height.saturating_sub(height) / 2;
    let popup_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(Span::styled(
            " Keybinds",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(Span::styled(
            " Always available:",
            Style::default().fg(Color::Yellow),
        )),
        Line::raw("   ctrl+p      Jump to pattern field"),
        Line::raw("   ctrl+t      Jump to test input field"),
        Line::raw(""),
        Line::from(Span::styled(
            " Nav layer (after Escape):",
            Style::default().fg(Color::Cyan),
        )),
        Line::raw("   q           Quit"),
        Line::raw("   ?           Toggle this help"),
        Line::raw("   Tab         Cycle focus forward"),
        Line::raw("   Shift+Tab   Cycle focus backward"),
        Line::raw("   f           Toggle fancy-regex mode"),
        Line::raw("   v           Cycle results view"),
        Line::raw("   Enter       Re-enter insert mode"),
        Line::raw(""),
        Line::from(Span::styled(
            " When Results pane is focused:",
            Style::default().fg(Color::Green),
        )),
        Line::raw("   ↑  ↓        Scroll active sub-pane"),
        Line::raw("   ←  →        Switch active sub-pane (split views)"),
        Line::raw(""),
        Line::from(Span::styled(
            " When Flags row is focused:",
            Style::default().fg(Color::Green),
        )),
        Line::raw("   ←  →        Move between variant selector and flags"),
        Line::raw("   Space       Toggle flag / cycle variant"),
        Line::raw(""),
        Line::from(Span::styled(
            " Results views (v to cycle):",
            Style::default().fg(Color::Magenta),
        )),
        Line::raw("   split-v  split-h  preview  matches"),
        Line::raw(""),
        Line::from(Span::styled(
            " Press ? to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    frame.render_widget(
        Paragraph::new(help_text).block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        ),
        popup_area,
    );
}

/// Truncate `s` to at most `max_chars` characters, appending `…` if truncated.
/// Counts Unicode scalar values (chars), not bytes.
fn truncate(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}…", collected)
    } else {
        collected
    }
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_length_unchanged() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_long_string_appends_ellipsis() {
        assert_eq!(truncate("hello world", 5), "hello…");
    }

    #[test]
    fn truncate_empty_string() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn truncate_max_zero() {
        assert_eq!(truncate("hello", 0), "…");
    }

    #[test]
    fn truncate_multibyte_chars_counted_as_one() {
        // "héllo" is 5 chars but 6 bytes — truncate should count chars not bytes
        assert_eq!(truncate("héllo", 5), "héllo");
        assert_eq!(truncate("héllo world", 5), "héllo…");
    }

    #[test]
    fn truncate_ellipsis_is_single_char() {
        // The appended … should not itself count toward max_chars
        let result = truncate("abcdef", 3);
        assert_eq!(result, "abc…");
        // Result has 4 chars (3 + ellipsis), not 3
        assert_eq!(result.chars().count(), 4);
    }
}
