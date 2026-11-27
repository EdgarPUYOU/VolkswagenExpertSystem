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
    #[arg(long, env = "ONTOLOGY_DIR", default_value = "../ontology")]
    pub ontology_dir: PathBuf,

    /// Directory holding the *.ru SPARQL update rule files, applied in name order until nothing new is derived.
    #[arg(long, env = "RULES_DIR", default_value = "../rules")]
    pub rules_dir: PathBuf,

    /// SQLite database file.
    #[arg(long, env = "SQLITE_PATH", default_value = "../data/erp.db")]
    pub sqlite_path: PathBuf,

    /// API key for the language model behind the chat (any OpenAI-compatible API). Leave unset to use the rule-based planner only.
    #[arg(long, env = "AI_API_KEY", hide_env_values = true)]
    pub llm_api_key: Option<Secret>,

    /// Base URL of the OpenAI-compatible API (no trailing slash needed).
    #[arg(long, env = "AI_BASE_URL", default_value = "https://ai.sumopod.com/v1")]
    pub llm_base_url: String,

    /// Model the chat asks for.
    #[arg(long, env = "AI_MODEL", default_value = "gpt-4o-mini")]
    pub llm_model: String,

    /// Sent as `reasoning_effort` when set (for example `none`). Some reasoning models refuse function tools unless it is `none`; leave it unset for models that do not take the parameter.
    #[arg(long, env = "AI_REASONING_EFFORT")]
    pub llm_reasoning_effort: Option<String>,
}

/// What the HTTP layer needs to talk to the model. The key stays on the server.
#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub api_key: Option<Secret>,
    pub base_url: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
}

/// A host-only URL (`https://host`) gets `/v1`, which is where OpenAI-compatible APIs serve `/chat/completions`.
/// Any URL that already has a path is kept as given.
fn normalise_base_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    if after_scheme.contains('/') { url.to_string() } else { format!("{url}/v1") }
}

impl LlmConfig {
    pub fn from_config(c: &Config) -> LlmConfig {
        LlmConfig {
            // an empty value in .env means "not set"
            api_key: c.llm_api_key.clone().filter(|k| !k.expose().is_empty()),
            base_url: normalise_base_url(&c.llm_base_url),
            model: c.llm_model.trim().to_string(),
            reasoning_effort: c.llm_reasoning_effort.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string),
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
            ontology_dir: "../ontology".into(),
            rules_dir: "../rules".into(),
            sqlite_path: "x.db".into(),
            llm_api_key: key.map(|k| k.parse().unwrap()),
            llm_base_url: url.into(),
            llm_model: " deepseek-chat ".into(),
            llm_reasoning_effort: Some(" none ".into()),
        }
    }

    #[test]
    fn empty_key_means_not_configured_and_url_is_normalised() {
        let l = LlmConfig::from_config(&config(Some("  "), "https://example.test/ "));
        assert!(!l.has_key());
        assert_eq!((l.base_url.as_str(), l.model.as_str()), ("https://example.test/v1", "deepseek-chat"));
        assert_eq!(l.reasoning_effort.as_deref(), Some("none"));
        assert_eq!(normalise_base_url("https://api.example.test/v1/"), "https://api.example.test/v1");
        assert_eq!(normalise_base_url("https://api.example.test/openai"), "https://api.example.test/openai");
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
