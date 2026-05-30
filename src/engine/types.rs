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

/// Normalized flag set shared across all engines. Each engine script is
/// responsible for translating these to its native mechanism.
#[derive(Debug, Clone, Default)]
pub struct Flags {
    pub case_insensitive: bool,
    pub multiline: bool,
    pub dotall: bool,
    pub global: bool,
    pub extended: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum EvalMode {
    #[default]
    Match,
    Replace,
}

/// Request sent to any engine (native or script).
#[derive(Debug, Clone)]
pub struct EvalRequest {
    pub pattern: String,
    pub flags: Flags,
    pub input: String,
    pub mode: EvalMode,
    /// Normalized replacement string using {1}, {name} syntax.
    /// Each engine translates to its native backreference syntax.
    pub replacement: String,
}

/// A single capture group within a match.
#[derive(Debug, Clone)]
pub struct Group {
    pub index: usize,
    pub name: Option<String>,
    /// None when the group did not participate in the match (optional group).
    pub value: Option<String>,
    pub span: Option<(usize, usize)>,
    /// False for optional groups that didn't match — shown explicitly in UI.
    pub matched: bool,
}

/// A single match result.
#[derive(Debug, Clone)]
pub struct Match {
    pub full_match: String,
    /// Byte offsets into the input string.
    pub span: (usize, usize),
    pub groups: Vec<Group>,
}

/// Successful evaluation response.
#[derive(Debug, Clone, Default)]
pub struct EvalResponse {
    pub matches: Vec<Match>,
    /// Populated in Replace mode — the full transformed input string.
    pub replaced: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    Syntax,
    Timeout,
    UnsupportedFlag,
    RuntimeError,
}

#[derive(Debug, Clone)]
pub struct EngineError {
    pub kind: ErrorKind,
    pub message: String,
    /// Byte offset into the pattern where the error occurred, if known.
    pub position: Option<usize>,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(pos) = self.position {
            write!(f, "{} (at position {})", self.message, pos)
        } else {
            write!(f, "{}", self.message)
        }
    }
}
