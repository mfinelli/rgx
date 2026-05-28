use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use tui_textarea::TextArea;

use crate::engine::{
    types::{EngineError, EvalMode, EvalRequest, EvalResponse, Flags, Match},
    RustEngine,
};

// ─── Mode & Focus ────────────────────────────────────────────────────────────

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
}

// ─── App State ───────────────────────────────────────────────────────────────

pub struct App<'a> {
    pub mode: AppMode,
    pub focus: Focus,

    pub pattern: TextArea<'a>,
    pub input: TextArea<'a>,

    pub flags: Flags,
    /// Index of the currently highlighted flag in the flag row (for nav).
    pub flag_cursor: usize,

    pub use_fancy: bool,

    pub eval_result: Option<Result<EvalResponse, EngineError>>,
    pub last_edit: Option<Instant>,
    pub debounce_ms: u64,

    /// Whether to show the keybind help overlay.
    pub show_help: bool,
}

impl<'a> App<'a> {
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
            Block::default()
                .title(" Test Input ")
                .borders(Borders::ALL),
        );
        input.set_cursor_line_style(Style::default());

        Self {
            mode: AppMode::Insert,
            focus: Focus::Pattern,
            pattern,
            input,
            flags: Flags {
                global: true, // default on as per design
                ..Default::default()
            },
            flag_cursor: 0,
            use_fancy: false,
            eval_result: None,
            last_edit: None,
            debounce_ms: 150,
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
        };

        self.eval_result = Some(engine.evaluate(&req));
    }

    /// Update border styles to reflect current focus and mode.
    pub fn update_borders(&mut self) {
        let active = Style::default().fg(Color::Yellow);
        let inactive = Style::default().fg(Color::DarkGray);

        self.pattern.set_block(
            Block::default()
                .title(" Pattern ")
                .borders(Borders::ALL)
                .border_style(if self.focus == Focus::Pattern { active } else { inactive }),
        );
        self.input.set_block(
            Block::default()
                .title(" Test Input ")
                .borders(Borders::ALL)
                .border_style(if self.focus == Focus::Input { active } else { inactive }),
        );
    }

    /// Move focus and switch to Insert mode.
    fn jump_to(&mut self, focus: Focus) {
        self.focus = focus;
        self.mode = AppMode::Insert;
        self.update_borders();
    }

    /// Cycle focus forward through the panes.
    fn cycle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Pattern => Focus::Flags,
            Focus::Flags => Focus::Input,
            Focus::Input => Focus::Pattern,
        };
    }

    /// Toggle the flag at `flag_cursor`.
    fn toggle_flag(&mut self) {
        match self.flag_cursor {
            0 => self.flags.case_insensitive = !self.flags.case_insensitive,
            1 => self.flags.multiline = !self.flags.multiline,
            2 => self.flags.dotall = !self.flags.dotall,
            3 => self.flags.global = !self.flags.global,
            _ => {}
        }
        self.mark_dirty();
    }

    /// Build the rendered invocation string for the status line.
    pub fn status_invocation(&self) -> String {
        let pattern = self.pattern.lines().join("");
        if pattern.is_empty() {
            return String::from("Rust · regex crate (RE2-style, linear time)");
        }

        let mut flags = String::new();
        if self.flags.case_insensitive { flags.push('i'); }
        if self.flags.multiline { flags.push('m'); }
        if self.flags.dotall { flags.push('s'); }

        let engine_note = if self.use_fancy { "fancy-regex" } else { "regex" };

        if flags.is_empty() {
            format!("Rust ({}) · Regex::new(r\"{}\")", engine_note, pattern)
        } else {
            format!(
                "Rust ({}) · RegexBuilder::new(r\"{}\").flags(\"{}\").build()",
                engine_note, pattern, flags
            )
        }
    }
}

// ─── Event Handling ───────────────────────────────────────────────────────────

/// Process one key event. Returns true if the application should quit.
pub fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use KeyCode::*;
    use KeyModifiers as KM;

    // ctrl+z / ctrl+shift+z: undo/redo within the active text field.
    // (Session-level undo comes in a later phase.)
    if key.modifiers == KM::CONTROL && key.code == Char('z') {
        match app.focus {
            Focus::Pattern => { app.pattern.undo(); app.mark_dirty(); }
            Focus::Input => { app.input.undo(); app.mark_dirty(); }
            Focus::Flags => {}
        }
        return false;
    }

    // ctrl+p — jump to pattern field (works from anywhere)
    if key.modifiers == KM::CONTROL && key.code == Char('p') {
        app.jump_to(Focus::Pattern);
        return false;
    }

    // ctrl+t — jump to test input field (works from anywhere)
    if key.modifiers == KM::CONTROL && key.code == Char('t') {
        app.jump_to(Focus::Input);
        return false;
    }

    match app.mode {
        AppMode::Insert => handle_insert(app, key),
        AppMode::Nav => handle_nav(app, key),
    }
}

fn handle_insert(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use KeyCode::*;

    match key.code {
        Esc => {
            app.mode = AppMode::Nav;
            app.update_borders();
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
                // Flags row doesn't accept text input; Esc already handled above.
                // Space toggles the current flag.
                if key.code == KeyCode::Char(' ') {
                    app.toggle_flag();
                }
            }
        },
    }
    false
}

fn handle_nav(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use KeyCode::*;
    use KeyModifiers as KM;

    match (key.code, key.modifiers) {
        // Quit
        (Char('q'), KM::NONE) => return true,

        // Help overlay
        (Char('?'), KM::NONE) => app.show_help = !app.show_help,

        // Fancy-regex toggle
        (Char('f'), KM::NONE) => {
            app.use_fancy = !app.use_fancy;
            app.mark_dirty();
        }

        // Tab — cycle focus
        (Tab, KM::NONE) => {
            app.cycle_focus();
            app.update_borders();
        }
        (BackTab, _) => {
            // Reverse cycle
            app.focus = match app.focus {
                Focus::Pattern => Focus::Input,
                Focus::Flags => Focus::Pattern,
                Focus::Input => Focus::Flags,
            };
            app.update_borders();
        }

        // Arrow left/right — move flag cursor when focused on flags
        (Left, KM::NONE) if app.focus == Focus::Flags => {
            if app.flag_cursor > 0 { app.flag_cursor -= 1; }
        }
        (Right, KM::NONE) if app.focus == Focus::Flags => {
            if app.flag_cursor < 3 { app.flag_cursor += 1; }
        }

        // Space — toggle flag when focused on flags
        (Char(' '), KM::NONE) if app.focus == Focus::Flags => {
            app.toggle_flag();
        }

        // Enter / any printable char — re-enter insert mode on the focused pane
        (Enter, KM::NONE) | (Char(_), KM::NONE) => {
            if app.focus != Focus::Flags {
                app.mode = AppMode::Insert;
                app.update_borders();
                // Forward the character if it was a printable key
                if let Char(_) = key.code {
                    match app.focus {
                        Focus::Pattern => { app.pattern.input(key); app.mark_dirty(); }
                        Focus::Input => { app.input.input(key); app.mark_dirty(); }
                        Focus::Flags => {}
                    }
                }
            }
        }

        _ => {}
    }
    false
}

// ─── Rendering ────────────────────────────────────────────────────────────────

pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Guard: minimum terminal size
    if area.width < 40 || area.height < 12 {
        let msg = Paragraph::new("terminal too small — resize to continue")
            .style(Style::default().fg(Color::Red));
        frame.render_widget(msg, area);
        return;
    }

    // Vertical layout:
    // [engine bar 1] [pattern 3] [flags 1] [input flex] [results flex] [status 1] [hint 1]
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // engine bar
            Constraint::Length(3), // pattern
            Constraint::Length(1), // flags row
            Constraint::Min(4),    // input
            Constraint::Min(4),    // results
            Constraint::Length(1), // status
            Constraint::Length(1), // key hint
        ])
        .split(area);

    render_engine_bar(app, frame, chunks[0]);
    render_pattern(app, frame, chunks[1]);
    render_flags(app, frame, chunks[2]);
    render_input(app, frame, chunks[3]);
    render_results(app, frame, chunks[4]);
    render_status(app, frame, chunks[5]);
    render_hint(app, frame, chunks[6]);

    if app.show_help {
        render_help_overlay(frame, area);
    }
}

fn render_engine_bar(app: &App, frame: &mut Frame, area: Rect) {
    let engine_name = if app.use_fancy {
        " [ Rust · fancy-regex ] "
    } else {
        " [ Rust · regex ] "
    };
    let bar = Paragraph::new(Line::from(vec![
        Span::styled(engine_name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(
            " (press f to toggle fancy-regex, ? for help)",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    frame.render_widget(bar, area);
}

fn render_pattern(app: &App, frame: &mut Frame, area: Rect) {
    frame.render_widget(app.pattern.widget(), area);
}

fn render_flags(app: &App, frame: &mut Frame, area: Rect) {
    let flags = &app.flags;
    let cursor = app.flag_cursor;
    let focused = app.focus == Focus::Flags;

    let flag_defs = [
        ("Case insensitive", flags.case_insensitive),
        ("Multiline", flags.multiline),
        ("Dotall", flags.dotall),
        ("Global", flags.global),
    ];

    let spans: Vec<Span> = flag_defs
        .iter()
        .enumerate()
        .flat_map(|(i, (label, on))| {
            let indicator = if *on { "◉" } else { "○" };
            let is_cursor = focused && i == cursor;

            let style = if is_cursor {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if *on {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let text = format!(" {} {} ", indicator, label);
            let sep = if i < flag_defs.len() - 1 {
                Span::styled(" │", Style::default().fg(Color::DarkGray))
            } else {
                Span::raw("")
            };

            vec![Span::styled(text, style), sep]
        })
        .collect();

    let para = Paragraph::new(Line::from(spans));
    frame.render_widget(para, area);
}

fn render_input(app: &App, frame: &mut Frame, area: Rect) {
    frame.render_widget(app.input.widget(), area);
}

fn render_results(app: &App, frame: &mut Frame, area: Rect) {
    let block = Block::default()
        .title(" Results ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    match &app.eval_result {
        None => {
            let para = Paragraph::new(Span::styled(
                "no pattern",
                Style::default().fg(Color::DarkGray),
            ))
            .block(block);
            frame.render_widget(para, area);
        }
        Some(Err(e)) => {
            let msg = format!("error: {}", e);
            let para = Paragraph::new(Span::styled(msg, Style::default().fg(Color::Red)))
                .block(block)
                .wrap(Wrap { trim: false });
            frame.render_widget(para, area);
        }
        Some(Ok(resp)) => {
            render_match_results(app, resp, block, frame, area);
        }
    }
}

fn render_match_results(
    app: &App,
    resp: &EvalResponse,
    block: Block,
    frame: &mut Frame,
    area: Rect,
) {
    let input_text = app.input.lines().join("\n");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if resp.matches.is_empty() {
        let para = Paragraph::new(Span::styled(
            "no match",
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(para, inner);
        return;
    }

    // Split inner area: top portion for highlighted input preview,
    // bottom portion for match breakdown.
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(inner);

    // Highlighted input preview
    render_highlighted_input(&input_text, &resp.matches, frame, split[0]);

    // Match breakdown list
    render_match_list(resp, frame, split[1]);
}

/// Renders the input text with match spans highlighted.
/// Byte spans from the engine are mapped back to per-line character ranges.
fn render_highlighted_input(input: &str, matches: &[Match], frame: &mut Frame, area: Rect) {
    let match_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let group_style = Style::default().fg(Color::Black).bg(Color::Cyan);

    // Build a flat list of (start, end, style) sorted by start, groups after full matches
    // so that group highlighting overlays the match highlight.
    let mut highlights: Vec<(usize, usize, Style)> = matches
        .iter()
        .map(|m| (m.span.0, m.span.1, match_style))
        .chain(matches.iter().flat_map(|m| {
            m.groups.iter().filter(|g| g.matched).filter_map(|g| {
                g.span.map(|(s, e)| (s, e, group_style))
            })
        }))
        .collect();
    highlights.sort_by_key(|&(s, _, _)| s);

    let mut lines: Vec<Line> = Vec::new();
    let mut byte_pos: usize = 0;

    for raw_line in input.split('\n') {
        let line_start = byte_pos;
        let line_end = byte_pos + raw_line.len();
        let mut spans: Vec<Span> = Vec::new();
        let mut cursor = line_start;

        for &(hs, he, style) in &highlights {
            let hs = hs.max(line_start).min(line_end);
            let he = he.max(line_start).min(line_end);
            if hs >= he { continue; }
            if hs > cursor {
                spans.push(Span::raw(input[cursor..hs].to_string()));
            }
            spans.push(Span::styled(input[hs..he].to_string(), style));
            cursor = he;
        }
        if cursor < line_end {
            spans.push(Span::raw(input[cursor..line_end].to_string()));
        }
        if spans.is_empty() {
            spans.push(Span::raw(raw_line.to_string()));
        }

        lines.push(Line::from(spans));
        byte_pos = line_end + 1; // +1 for the '\n'
    }

    let para = Paragraph::new(lines)
        .block(Block::default().title(" Input preview ").borders(Borders::TOP))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

/// Renders the per-match breakdown list.


fn render_match_list(resp: &EvalResponse, frame: &mut Frame, area: Rect) {
    let count = resp.matches.len();
    let header_style = Style::default().fg(Color::Green).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let group_style = Style::default().fg(Color::Cyan);
    let unmatched_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);

    let mut items: Vec<ListItem> = Vec::new();

    // Header
    items.push(ListItem::new(Line::from(Span::styled(
        format!("{} match{}", count, if count == 1 { "" } else { "es" }),
        header_style,
    ))));

    for (i, m) in resp.matches.iter().enumerate() {
        // Match line
        let match_line = Line::from(vec![
            Span::styled(format!("  Match {} ", i + 1), Style::default().fg(Color::Yellow)),
            Span::styled(
                format!("[{}..{}]", m.span.0, m.span.1),
                dim,
            ),
            Span::raw("  "),
            Span::styled(
                format!("\"{}\"", truncate(&m.full_match, 40)),
                Style::default().fg(Color::White),
            ),
        ]);
        items.push(ListItem::new(match_line));

        // Group lines
        for g in &m.groups {
            let label = match &g.name {
                Some(n) => format!("    group {} ({}) ", g.index, n),
                None => format!("    group {} ", g.index),
            };
            if g.matched {
                let span_str = g.span.map(|(s, e)| format!("[{}..{}]", s, e)).unwrap_or_default();
                let val = g.value.as_deref().unwrap_or("");
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(label, group_style),
                    Span::styled(span_str, dim),
                    Span::raw("  "),
                    Span::styled(format!("\"{}\"", truncate(val, 30)), Style::default().fg(Color::White)),
                ])));
            } else {
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(label, unmatched_style),
                    Span::styled("(unmatched)", unmatched_style),
                ])));
            }
        }
    }

    let list = List::new(items)
        .block(Block::default().title(" Matches ").borders(Borders::TOP));
    frame.render_widget(list, area);
}

fn render_status(app: &App, frame: &mut Frame, area: Rect) {
    let invocation = app.status_invocation();
    let match_count = match &app.eval_result {
        Some(Ok(r)) => format!("{} match{}", r.matches.len(), if r.matches.len() == 1 { "" } else { "es" }),
        Some(Err(_)) => "error".to_string(),
        None => String::new(),
    };

    // Left: invocation  Right: match count
    let left = Span::styled(
        format!(" {} ", invocation),
        Style::default().fg(Color::DarkGray),
    );
    let right = Span::styled(
        format!(" {} ", match_count),
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
    );

    let available = area.width as usize;
    let right_len = match_count.len() + 3;
    let left_str = format!(" {} ", invocation);
    let padding = available.saturating_sub(left_str.len() + right_len);

    let line = Line::from(vec![
        left,
        Span::raw(" ".repeat(padding)),
        right,
    ]);

    let para = Paragraph::new(line)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(para, area);
}

fn render_hint(app: &App, frame: &mut Frame, area: Rect) {
    let mode_indicator = match app.mode {
        AppMode::Insert => Span::styled(" INSERT ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        AppMode::Nav => Span::styled(" NAV ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    };

    let hints = Span::styled(
        "  Esc: nav mode  │  ctrl+p: pattern  │  ctrl+t: input  │  Tab: cycle  │  f: toggle fancy  │  ?: help  │  q: quit",
        Style::default().fg(Color::DarkGray),
    );

    let line = Line::from(vec![mode_indicator, hints]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    use ratatui::widgets::Clear;

    let width = (area.width).min(60);
    let height = 22u16;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(Span::styled(" Keybinds", Style::default().add_modifier(Modifier::BOLD))),
        Line::raw(""),
        Line::from(Span::styled(" Always available:", Style::default().fg(Color::Yellow))),
        Line::raw("   ctrl+p      Jump to pattern field"),
        Line::raw("   ctrl+t      Jump to test input field"),
        Line::raw("   ctrl+z      Undo (within field)"),
        Line::raw(""),
        Line::from(Span::styled(" Nav layer (after Escape):", Style::default().fg(Color::Cyan))),
        Line::raw("   q           Quit"),
        Line::raw("   ?           Toggle this help"),
        Line::raw("   Tab         Cycle focus"),
        Line::raw("   Shift+Tab   Cycle focus (reverse)"),
        Line::raw("   f           Toggle fancy-regex mode"),
        Line::raw("   Enter       Re-enter insert mode"),
        Line::raw(""),
        Line::from(Span::styled(" When Flags row is focused:", Style::default().fg(Color::Green))),
        Line::raw("   ←  →        Move between flags"),
        Line::raw("   Space       Toggle flag"),
        Line::raw(""),
        Line::from(Span::styled(" Press ? to close", Style::default().fg(Color::DarkGray))),
    ];

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let para = Paragraph::new(help_text).block(block);
    frame.render_widget(para, popup_area);
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn truncate(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let collected: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}…", collected)
    } else {
        collected
    }
}
