use super::types::*;
use regex::RegexBuilder;

pub struct RustEngine {
    /// When true, uses fancy-regex which adds lookahead/lookbehind/backreferences
    /// at the cost of the linear-time guarantee. Off by default.
    pub use_fancy: bool,
}

impl RustEngine {
    pub fn new() -> Self {
        Self { use_fancy: false }
    }

    pub fn evaluate(
        &self,
        req: &EvalRequest,
    ) -> Result<EvalResponse, EngineError> {
        if req.pattern.is_empty() {
            return Ok(EvalResponse::default());
        }

        if self.use_fancy {
            self.eval_fancy(req)
        } else {
            self.eval_regex(req)
        }
    }

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

    fn eval_fancy(
        &self,
        req: &EvalRequest,
    ) -> Result<EvalResponse, EngineError> {
        // fancy-regex's RegexBuilder does not expose individual flag methods —
        // flags are applied by prepending inline (?flags) syntax to the pattern.
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
                        // Advance past zero-width matches to avoid infinite loop
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

        // TODO phase 2: replace mode for fancy-regex
        let replaced: Option<String> = None;

        Ok(EvalResponse { matches, replaced })
    }
}

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

/// Translate normalized replacement syntax {1}/{name} to Rust's $1/${name}.
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
// ─── Status line invocation renderers ────────────────────────────────────────
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
/// Example: `Regex::new(r"(?im)hello")`
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
