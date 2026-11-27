use std::path::PathBuf;
use std::str::FromStr;

/// A value that must never reach logs: Debug and Display print a placeholder.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl FromStr for Secret {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Secret(s.trim().to_string()))
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// Runtime configuration, shared by every subcommand. Each field can be set
/// through a flag or an environment variable.
#[derive(clap::Args, Debug, Clone)]
pub struct Config {
    /// Directory holding the *.ttl files. Each file is loaded into its own named graph.
    #[arg(long, env = "ONTOLOGY_DIR", default_value = "..")]
    pub ontology_dir: PathBuf,

    /// Directory holding the *.ru SPARQL update rule files, applied in name order until nothing new is derived.
    #[arg(long, env = "RULES_DIR", default_value = "../rules")]
    pub rules_dir: PathBuf,

    /// SQLite database file.
    #[arg(long, env = "SQLITE_PATH", default_value = "../data/erp.db")]
    pub sqlite_path: PathBuf,

    /// API key for the language model behind the chat. Leave unset to use the rule-based planner only.
    #[arg(long, env = "ANTHROPIC_API_KEY", hide_env_values = true)]
    pub anthropic_api_key: Option<Secret>,

    /// Base URL of the model API (no trailing slash needed).
    #[arg(long, env = "ANTHROPIC_BASE_URL", default_value = "https://api.anthropic.com")]
    pub anthropic_base_url: String,

    /// Model the chat asks for.
    #[arg(long, env = "ANTHROPIC_MODEL", default_value = "claude-sonnet-5-5")]
    pub anthropic_model: String,
}

/// What the HTTP layer needs to talk to the model. The key stays on the server.
#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub api_key: Option<Secret>,
    pub base_url: String,
    pub model: String,
}

impl LlmConfig {
    pub fn from_config(c: &Config) -> LlmConfig {
        LlmConfig {
            // an empty value in .env means "not set"
            api_key: c.anthropic_api_key.clone().filter(|k| !k.expose().is_empty()),
            base_url: c.anthropic_base_url.trim().trim_end_matches('/').to_string(),
            model: c.anthropic_model.trim().to_string(),
        }
    }

    pub fn has_key(&self) -> bool {
        self.api_key.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(key: Option<&str>, url: &str) -> Config {
        Config {
            ontology_dir: "..".into(),
            rules_dir: "../rules".into(),
            sqlite_path: "x.db".into(),
            anthropic_api_key: key.map(|k| k.parse().unwrap()),
            anthropic_base_url: url.into(),
            anthropic_model: " claude-sonnet-5-5 ".into(),
        }
    }

    #[test]
    fn empty_key_means_not_configured_and_url_is_normalised() {
        let l = LlmConfig::from_config(&config(Some("  "), "https://example.test/ "));
        assert!(!l.has_key());
        assert_eq!((l.base_url.as_str(), l.model.as_str()), ("https://example.test", "claude-sonnet-5-5"));
        assert!(LlmConfig::from_config(&config(Some("sk-abc"), "https://x")).has_key());
        assert!(!LlmConfig::from_config(&config(None, "https://x")).has_key());
    }

    #[test]
    fn the_key_never_appears_in_debug_output() {
        let l = LlmConfig::from_config(&config(Some("sk-very-secret"), "https://x"));
        let shown = format!("{l:?} {:?}", config(Some("sk-very-secret"), "https://x"));
        assert!(!shown.contains("sk-very-secret"), "{shown}");
        assert!(shown.contains("Secret(***)"));
    }
}
