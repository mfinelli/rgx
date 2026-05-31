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

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::Deserialize;

use crate::tui::app::ResultsView;

/// Application configuration, loaded from `~/.config/rgx/config.toml`.
///
/// All fields have defaults so a missing or empty config file is valid.
/// Unknown keys are ignored to allow forward compatibility.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Show Nerd Font icons in the engine tab bar.
    pub nerd_fonts: bool,

    /// Default results pane layout on startup.
    pub default_results_view: ResultsViewConfig,

    /// Evaluation debounce in milliseconds.
    pub debounce_ms: u64,

    /// Use fancy-regex as the default variant on the Rust tab.
    pub fancy_regex_default: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            nerd_fonts: false,
            default_results_view: ResultsViewConfig::default(),
            debounce_ms: 150,
            fancy_regex_default: false,
        }
    }
}

/// The `default_results_view` config value.
///
/// A separate type is needed because `ResultsView` lives in the TUI layer and
/// we want clean (de)serialisation without pulling serde into `app.rs`.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResultsViewConfig {
    #[default]
    SplitVertical,
    SplitHorizontal,
    Preview,
    Matches,
}

impl ResultsViewConfig {
    /// Convert to the TUI `ResultsView` type.
    pub fn to_results_view(&self) -> ResultsView {
        match self {
            Self::SplitVertical => ResultsView::SplitVertical,
            Self::SplitHorizontal => ResultsView::SplitHorizontal,
            Self::Preview => ResultsView::Preview,
            Self::Matches => ResultsView::Matches,
        }
    }
}

impl Config {
    /// Load config from the given path, falling back to the XDG default.
    ///
    /// A missing file is not an error (the default `Config` is returned).
    /// A file that exists but cannot be parsed is an error.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let resolved = match path {
            Some(p) => p.to_path_buf(),
            None => default_config_path(),
        };

        if !resolved.exists() {
            return Ok(Config::default());
        }

        let contents =
            std::fs::read_to_string(&resolved).with_context(|| {
                format!("failed to read config file: {}", resolved.display())
            })?;

        toml::from_str(&contents).with_context(|| {
            format!("failed to parse config file: {}", resolved.display())
        })
    }
}

/// Returns `~/.config/rgx/config.toml`, or a fallback if `$HOME` is unset.
fn default_config_path() -> PathBuf {
    dirs_path().join("config.toml")
}

/// Returns the XDG config directory for rgx: `~/.config/rgx`.
fn dirs_path() -> PathBuf {
    // Respect XDG_CONFIG_HOME if set, otherwise fall back to ~/.config
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("rgx")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config").join("rgx")
    } else {
        PathBuf::from(".config").join("rgx")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = Config::default();
        assert!(!config.nerd_fonts);
        assert_eq!(config.debounce_ms, 150);
        assert!(!config.fancy_regex_default);
    }

    #[test]
    fn missing_file_returns_defaults() {
        let result =
            Config::load(Some(Path::new("/nonexistent/path/config.toml")));
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.debounce_ms, 150);
    }

    #[test]
    fn parses_valid_toml() {
        let dir = std::env::temp_dir();
        let path = dir.join("rgx_test_config.toml");
        std::fs::write(
            &path,
            r#"
            nerd_fonts = true
            debounce_ms = 200
            fancy_regex_default = true
            default_results_view = "split_horizontal"
        "#,
        )
        .unwrap();

        let config = Config::load(Some(&path)).unwrap();
        assert!(config.nerd_fonts);
        assert_eq!(config.debounce_ms, 200);
        assert!(config.fancy_regex_default);
        assert!(matches!(
            config.default_results_view,
            ResultsViewConfig::SplitHorizontal
        ));

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_toml_returns_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("rgx_test_bad_config.toml");
        std::fs::write(&path, "not = [valid toml").unwrap();

        let result = Config::load(Some(&path));
        assert!(result.is_err());

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let dir = std::env::temp_dir();
        let path = dir.join("rgx_test_unknown_config.toml");
        std::fs::write(&path, "unknown_key = true").unwrap();

        let result = Config::load(Some(&path));
        assert!(result.is_err());

        std::fs::remove_file(path).unwrap();
    }
}
