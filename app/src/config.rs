use std::path::PathBuf;

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
}
