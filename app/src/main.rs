mod bench;
mod config;
mod db;
mod facts;
mod kg;
mod routers;
mod seed;
mod services;
mod web;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "vw-erp", about = "Volkswagen mini-ERP: SQLite + embedded knowledge graph")]
struct Cli {
    #[command(flatten)]
    config: Config,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP server (applies migrations and loads the KG first).
    Serve {
        #[arg(long, env = "BIND", default_value = "0.0.0.0:8000")]
        bind: String,
    },
    /// Load the TTL files into a fresh in-memory store and print per-graph counts.
    LoadKg,
    /// Apply pending SQLite migrations.
    Migrate,
    /// Regenerate the synthetic finance data (index values, FX, overhead, budget, actuals) in SQLite.
    Seed,
    /// Run one SPARQL SELECT against the loaded KG (rules applied) and print a tab-separated table.
    Query {
        /// Query text; the vw:, rdfs:, rdf:, owl: and xsd: prefixes are added for you.
        sparql: String,
    },
    /// Probe GET /health on localhost (for container healthchecks; the runtime image has no curl).
    Healthcheck {
        #[arg(long, env = "BIND", default_value = "0.0.0.0:8000")]
        bind: String,
    },
    /// Measure load time, memory and query latency of the embedded store.
    Bench {
        #[arg(long, default_value_t = 200)]
        iterations: usize,
    },
}

/// Open the SQLite file and apply migrations; the graph needs its numeric facts.
fn ready_db(config: &Config) -> Result<rusqlite::Connection> {
    let mut conn = db::open(&config.sqlite_path)?;
    db::migrate(&mut conn)?;
    Ok(conn)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    // .env in the working directory or a parent (the repo root when run from app/); real environment variables win
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    match cli.command {
        Command::Migrate => {
            let mut conn = db::open(&cli.config.sqlite_path)?;
            let n = db::migrate(&mut conn)?;
            println!("applied {n} migration(s)");
        }
        Command::LoadKg => {
            let conn = ready_db(&cli.config)?;
            let (kg, stats, infer) = kg::Kg::load(&cli.config.ontology_dir, &cli.config.rules_dir, &conn)?;
            for s in &stats {
                println!("{:<28} {:>6} triples  ->  {}", s.file, s.triples, s.graph);
            }
            println!("total: {} triples ({} passes to reach the rule fixpoint)", kg.len()?, infer.passes);
        }
        Command::Seed => {
            let mut conn = ready_db(&cli.config)?;
            let (kg, _, _) = kg::Kg::load(&cli.config.ontology_dir, &cli.config.rules_dir, &conn)?;
            println!("{:?}", seed::seed(&kg, &mut conn)?);
        }
        Command::Query { sparql } => {
            let conn = ready_db(&cli.config)?;
            let (kg, _, _) = kg::Kg::load(&cli.config.ontology_dir, &cli.config.rules_dir, &conn)?;
            let rows = services::portfolio::run(&kg, &sparql)?;
            let cols: Vec<String> = rows.first().map(|r| r.keys().cloned().collect()).unwrap_or_default();
            println!("{}", cols.join("\t"));
            for r in &rows {
                let cells: Vec<String> =
                    cols.iter().map(|c| r.get(c).map(|v| v.replace("http://example.org/volkswagen/", "vw:")).unwrap_or_default()).collect();
                println!("{}", cells.join("\t"));
            }
            eprintln!("{} rows", rows.len());
        }
        Command::Healthcheck { bind } => {
            use std::io::{Read, Write};
            let port = bind.rsplit(':').next().unwrap_or("8000");
            let mut s = std::net::TcpStream::connect(format!("127.0.0.1:{port}"))?;
            s.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
            s.write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
            let mut out = String::new();
            s.read_to_string(&mut out)?;
            if !out.starts_with("HTTP/1.0 200") && !out.starts_with("HTTP/1.1 200") {
                anyhow::bail!("unhealthy: {}", out.lines().next().unwrap_or(""));
            }
        }
        Command::Bench { iterations } => bench::run(&cli.config, iterations)?,
        Command::Serve { bind } => {
            let mut conn = db::open(&cli.config.sqlite_path)?;
            let applied = db::migrate(&mut conn)?;
            let (kg, graphs, infer) = kg::Kg::load(&cli.config.ontology_dir, &cli.config.rules_dir, &conn)?;
            if seed::needs_seed(&conn)? {
                tracing::info!(stats = ?seed::seed(&kg, &mut conn)?, "seeded empty finance tables");
            }
            tracing::info!(
                applied,
                triples = kg.len()?,
                graphs = graphs.len(),
                inferred = infer.inferred,
                passes = infer.passes,
                llm_key_set = config::LlmConfig::from_config(&cli.config).has_key(),
                "startup complete"
            );
            let llm_cfg = config::LlmConfig::from_config(&cli.config);
            let state = routers::AppState {
                kg: Arc::new(kg),
                db: Arc::new(Mutex::new(conn)),
                graphs: Arc::new(graphs),
                llm: Arc::new(llm_cfg.clone()),
                backend: services::llm::SdkBackend::new(&llm_cfg).map(Arc::new),
            };
            let listener = tokio::net::TcpListener::bind(&bind).await?;
            tracing::info!(%bind, "listening");
            axum::serve(listener, routers::router(state))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
    }
    Ok(())
}
