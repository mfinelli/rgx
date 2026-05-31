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

use super::types::*;
use regex::RegexBuilder;

/// The native Rust regex engine. Stateless (all configuration is carried
/// in `EvalRequest`) so the same instance can be shared across the app.
pub struct RustEngine;

impl RustEngine {
    /// Create a new `RustEngine` instance.
    pub fn new() -> Self {
        Self
    }

    /// Evaluate a regex request, dispatching to the `regex` crate or
    /// `fancy-regex` based on `req.use_fancy`.
    /// Returns an empty response for an empty pattern rather than an error.
    pub fn evaluate(
        &self,
        req: &EvalRequest,
    ) -> Result<EvalResponse, EngineError> {
        if req.pattern.is_empty() {
            return Ok(EvalResponse::default());
        }

        if req.use_fancy {
            self.eval_fancy(req)
        } else {
            self.eval_regex(req)
        }
    }

    /// Evaluate using the `regex` crate (RE2-style, linear time).
    /// Flags are applied via `RegexBuilder` methods.
    fn eval_regex(
        &self,
        req: &EvalRequest,
    ) -> Result<EvalResponse, EngineError> {
        let re = RegexBuilder::new(&req.pattern)
            .case_insensitive(req.flags.case_insensitive)
            .multi_line(req.flags.multiline)
            .dot_matches_new_line(req.flags.dotall)
            .ignore_whitespace(req.flags.extended)
            .build()
            .map_err(|e| EngineError {
                kind: ErrorKind::Syntax,
                message: e.to_string(),
                position: None,
            })?;

        let matches: Vec<Match> = if req.flags.global {
            re.captures_iter(&req.input)
                .map(|cap| captures_to_match(&cap, &re))
                .collect()
        } else {
            re.captures(&req.input)
                .map(|cap| captures_to_match(&cap, &re))
                .into_iter()
                .collect()
        };

        let replaced =
            if req.mode == EvalMode::Replace && !req.replacement.is_empty() {
                let native = normalized_to_rust_replacement(&req.replacement);
                Some(if req.flags.global {
                    re.replace_all(&req.input, native.as_str()).to_string()
                } else {
                    re.replace(&req.input, native.as_str()).to_string()
                })
            } else {
                None
            };

        Ok(EvalResponse { matches, replaced })
    }

    /// Evaluate using `fancy-regex` (PCRE-style, supports
    /// lookahead/lookbehind/backrefs). Flags are applied by prepending inline
    /// `(?flags)` syntax since `fancy-regex`'s `RegexBuilder` does not expose
    /// individual flag methods.
    fn eval_fancy(
        &self,
        req: &EvalRequest,
    ) -> Result<EvalResponse, EngineError> {
        let mut inline_flags = String::new();
        if req.flags.case_insensitive {
            inline_flags.push('i');
        }
        if req.flags.multiline {
            inline_flags.push('m');
        }
        if req.flags.dotall {
            inline_flags.push('s');
        }

        let pattern = if inline_flags.is_empty() {
            req.pattern.clone()
        } else {
            format!("(?{}){}", inline_flags, req.pattern)
        };

        let re =
            fancy_regex::Regex::new(&pattern).map_err(|e| EngineError {
                kind: ErrorKind::Syntax,
                message: e.to_string(),
                position: None,
            })?;

        let mut matches = Vec::new();

        if req.flags.global {
            let mut pos = 0;
            while pos <= req.input.len() {
                match re.captures_from_pos(&req.input, pos) {
                    Ok(Some(cap)) => {
                        let m = cap.get(0).unwrap();
                        // Advance past zero-width matches to avoid an infinite loop
                        let next = if m.start() == m.end() {
                            pos + 1
                        } else {
                            m.end()
                        };
                        matches.push(fancy_captures_to_match(&cap, &re));
                        pos = next;
                    }
                    Ok(None) => break,
                    Err(e) => {
                        return Err(EngineError {
                            kind: ErrorKind::RuntimeError,
                            message: e.to_string(),
                            position: None,
                        });
                    }
                }
            }
        } else {
            match re.captures(&req.input) {
                Ok(Some(cap)) => {
                    matches.push(fancy_captures_to_match(&cap, &re))
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(EngineError {
                        kind: ErrorKind::RuntimeError,
                        message: e.to_string(),
                        position: None,
                    });
                }
            }
        }

        // TODO phase 6: replace mode for fancy-regex
        let replaced: Option<String> = None;

        Ok(EvalResponse { matches, replaced })
    }
}

/// Convert a `regex::Captures` into our `Match` type, preserving group names,
/// spans, and unmatched optional groups.
fn captures_to_match(cap: &regex::Captures, re: &regex::Regex) -> Match {
    let full = cap.get(0).unwrap();
    let names: Vec<Option<&str>> = re.capture_names().collect();

    let groups = (1..cap.len())
        .map(|i| {
            let name = names.get(i).and_then(|n| *n).map(|s| s.to_string());
            match cap.get(i) {
                Some(m) => Group {
                    index: i,
                    name,
                    value: Some(m.as_str().to_string()),
                    span: Some((m.start(), m.end())),
                    matched: true,
                },
                None => Group {
                    index: i,
                    name,
                    value: None,
                    span: None,
                    matched: false,
                },
            }
        })
        .collect();

    Match {
        full_match: full.as_str().to_string(),
        span: (full.start(), full.end()),
        groups,
    }
}

/// Convert a `fancy_regex::Captures` into our `Match` type, preserving group
/// names, spans, and unmatched optional groups.
fn fancy_captures_to_match(
    cap: &fancy_regex::Captures,
    re: &fancy_regex::Regex,
) -> Match {
    let full = cap.get(0).unwrap();
    let names: Vec<Option<&str>> = re.capture_names().collect();

    let groups = (1..cap.len())
        .map(|i| {
            let name = names.get(i).and_then(|n| *n).map(|s| s.to_string());
            match cap.get(i) {
                Some(m) => Group {
                    index: i,
                    name,
                    value: Some(m.as_str().to_string()),
                    span: Some((m.start(), m.end())),
                    matched: true,
                },
                None => Group {
                    index: i,
                    name,
                    value: None,
                    span: None,
                    matched: false,
                },
            }
        })
        .collect();

    Match {
        full_match: full.as_str().to_string(),
        span: (full.start(), full.end()),
        groups,
    }
}

/// Translate normalized replacement syntax `{1}`/`{name}` to Rust's
/// `$1`/`${name}`.
fn normalized_to_rust_replacement(s: &str) -> String {
    let re = regex::Regex::new(r"\{(\w+)\}").unwrap();
    re.replace_all(s, |caps: &regex::Captures| {
        let key = &caps[1];
        if key.chars().all(|c| c.is_ascii_digit()) {
            format!("${}", key)
        } else {
            format!("${{{}}}", key)
        }
    })
    .to_string()
}

// Each renderer produces the idiomatic invocation string for its engine,
// shown in the status line. As more engines are added they will each have
// their own renderer function here or in their own module.

/// Renders the `regex` crate invocation using `RegexBuilder` for flags,
/// which is the idiomatic API for that crate.
///
/// Example: `RegexBuilder::new(r"hello").case_insensitive(true).build()`
pub fn render_invocation_regex_crate(pattern: &str, flags: &Flags) -> String {
    if pattern.is_empty() {
        return "regex · RE2-style, linear time, no lookahead".to_string();
    }

    let mut builder_calls = String::new();
    if flags.case_insensitive {
        builder_calls.push_str(".case_insensitive(true)");
    }
    if flags.multiline {
        builder_calls.push_str(".multi_line(true)");
    }
    if flags.dotall {
        builder_calls.push_str(".dot_matches_new_line(true)");
    }
    if flags.extended {
        builder_calls.push_str(".ignore_whitespace(true)");
    }

    if builder_calls.is_empty() {
        format!("Regex::new(r\"{}\")", pattern)
    } else {
        format!(
            "RegexBuilder::new(r\"{}\"){}.build()",
            pattern, builder_calls
        )
    }
}

/// Renders the `fancy-regex` crate invocation using inline flag syntax,
/// which is how fancy-regex applies flags.
///
/// Example: `fancy_regex::Regex::new(r"(?im)hello")`
pub fn render_invocation_fancy_regex(pattern: &str, flags: &Flags) -> String {
    if pattern.is_empty() {
        return "fancy-regex · PCRE-style, lookahead/lookbehind/backreferences"
            .to_string();
    }

    let mut flag_chars = String::new();
    if flags.case_insensitive {
        flag_chars.push('i');
    }
    if flags.multiline {
        flag_chars.push('m');
    }
    if flags.dotall {
        flag_chars.push('s');
    }
    if flags.extended {
        flag_chars.push('x');
    }

    let rendered = if flag_chars.is_empty() {
        pattern.to_string()
    } else {
        format!("(?{}){}", flag_chars, pattern)
    };

    format!("fancy_regex::Regex::new(r\"{}\")", rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(pattern: &str) -> EvalRequest {
        EvalRequest {
            pattern: pattern.to_string(),
            flags: Flags {
                global: true,
                ..Default::default()
            },
            input: String::new(),
            mode: EvalMode::Match,
            replacement: String::new(),
            use_fancy: false,
        }
    }

    fn req_with_input(pattern: &str, input: &str) -> EvalRequest {
        EvalRequest {
            pattern: pattern.to_string(),
            input: input.to_string(),
            ..req(pattern)
        }
    }

    #[test]
    fn empty_pattern_returns_empty_response() {
        let engine = RustEngine::new();
        let result = engine.evaluate(&req("")).unwrap();
        assert!(result.matches.is_empty());
    }

    #[test]
    fn syntax_error_returns_error_kind() {
        let engine = RustEngine::new();
        let err = engine.evaluate(&req("[unclosed")).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Syntax);
    }

    #[test]
    fn global_on_finds_all_matches() {
        let engine = RustEngine::new();
        let mut r = req_with_input(r"\d+", "123 456 789");
        r.flags.global = true;
        let result = engine.evaluate(&r).unwrap();
        assert_eq!(result.matches.len(), 3);
    }

    #[test]
    fn global_off_finds_only_first_match() {
        let engine = RustEngine::new();
        let mut r = req_with_input(r"\d+", "123 456 789");
        r.flags.global = false;
        let result = engine.evaluate(&r).unwrap();
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].full_match, "123");
    }

    #[test]
    fn unmatched_optional_group_is_preserved() {
        let engine = RustEngine::new();
        // Group 2 is optional and won't match
        let r = req_with_input(r"(\d+)( hello)?", "123");
        let result = engine.evaluate(&r).unwrap();
        assert_eq!(result.matches.len(), 1);
        let groups = &result.matches[0].groups;
        assert_eq!(groups.len(), 2);
        assert!(groups[0].matched);
        assert!(!groups[1].matched);
    }

    #[test]
    fn named_group_is_captured() {
        let engine = RustEngine::new();
        let r = req_with_input(r"(?P<word>\w+)", "hello");
        let result = engine.evaluate(&r).unwrap();
        assert_eq!(result.matches[0].groups[0].name.as_deref(), Some("word"));
        assert_eq!(result.matches[0].groups[0].value.as_deref(), Some("hello"));
    }

    #[test]
    fn case_insensitive_flag_matches() {
        let engine = RustEngine::new();
        let mut r = req_with_input("hello", "HELLO");
        r.flags.case_insensitive = true;
        let result = engine.evaluate(&r).unwrap();
        assert_eq!(result.matches.len(), 1);
    }

    #[test]
    fn fancy_regex_lookahead_works() {
        let engine = RustEngine::new();
        let mut r = req_with_input(r"\d+(?= dollars)", "100 dollars");
        r.use_fancy = true;
        let result = engine.evaluate(&r).unwrap();
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].full_match, "100");
    }

    #[test]
    fn fancy_regex_zero_width_match_does_not_loop() {
        let engine = RustEngine::new();
        let mut r = req_with_input(r"a*", "bbb");
        r.use_fancy = true;
        // Should terminate and not hang; zero-width matches at each position
        let result = engine.evaluate(&r).unwrap();
        assert!(!result.matches.is_empty());
    }

    #[test]
    fn indexed_replacement_becomes_dollar() {
        assert_eq!(normalized_to_rust_replacement("{1}"), "$1");
        assert_eq!(normalized_to_rust_replacement("{2}"), "$2");
    }

    #[test]
    fn named_replacement_becomes_dollar_braces() {
        assert_eq!(normalized_to_rust_replacement("{word}"), "${word}");
    }

    #[test]
    fn replacement_with_literal_text_preserved() {
        assert_eq!(
            normalized_to_rust_replacement("hello {1} world"),
            "hello $1 world"
        );
    }

    #[test]
    fn empty_replacement_unchanged() {
        assert_eq!(normalized_to_rust_replacement(""), "");
    }

    #[test]
    fn invocation_no_flags() {
        let flags = Flags::default();
        assert_eq!(
            render_invocation_regex_crate("hello", &flags),
            r#"Regex::new(r"hello")"#
        );
    }

    #[test]
    fn invocation_single_flag() {
        let flags = Flags {
            case_insensitive: true,
            ..Default::default()
        };
        assert_eq!(
            render_invocation_regex_crate("hello", &flags),
            r#"RegexBuilder::new(r"hello").case_insensitive(true).build()"#
        );
    }

    #[test]
    fn invocation_multiple_flags() {
        let flags = Flags {
            case_insensitive: true,
            multiline: true,
            ..Default::default()
        };
        let result = render_invocation_regex_crate("hello", &flags);
        assert!(result.contains(".case_insensitive(true)"));
        assert!(result.contains(".multi_line(true)"));
    }

    #[test]
    fn invocation_empty_pattern_returns_description() {
        let flags = Flags::default();
        let result = render_invocation_regex_crate("", &flags);
        assert!(result.contains("RE2"));
    }

    #[test]
    fn fancy_invocation_no_flags() {
        let flags = Flags::default();
        assert_eq!(
            render_invocation_fancy_regex("hello", &flags),
            r#"fancy_regex::Regex::new(r"hello")"#
        );
    }

    #[test]
    fn fancy_invocation_with_flags() {
        let flags = Flags {
            case_insensitive: true,
            multiline: true,
            ..Default::default()
        };
        assert_eq!(
            render_invocation_fancy_regex("hello", &flags),
            r#"fancy_regex::Regex::new(r"(?im)hello")"#
        );
    }

    #[test]
    fn fancy_invocation_empty_pattern_returns_description() {
        let flags = Flags::default();
        let result = render_invocation_fancy_regex("", &flags);
        assert!(result.contains("PCRE"));
    }
}
