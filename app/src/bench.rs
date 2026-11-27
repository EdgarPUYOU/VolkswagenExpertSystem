//! Step 1 spike: measure load time, memory and query latency of embedded Oxigraph.
use crate::{config::Config, kg::Kg, services::portfolio};
use anyhow::Result;
use std::time::Instant;

fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|n| n.parse::<f64>().ok()))
        })
        .map(|kb| kb / 1024.0)
        .unwrap_or(f64::NAN)
}

pub fn run(config: &Config, iterations: usize) -> Result<()> {
    let rss_before = rss_mb();
    let t = Instant::now();
    let (kg, stats) = Kg::load_dir(&config.ontology_dir)?;
    let load = t.elapsed();
    let t = Instant::now();
    let infer = kg.infer(&config.rules_dir)?;
    let infer_time = t.elapsed();
    let rss_after = rss_mb();

    println!("graphs:");
    for s in &stats {
        println!("  {:<28} {:>6} triples", s.file, s.triples);
    }
    println!("load time     : {:.1} ms", load.as_secs_f64() * 1000.0);
    println!(
        "rules         : {:.1} ms, {} passes, {} inferred triples",
        infer_time.as_secs_f64() * 1000.0,
        infer.passes,
        infer.inferred
    );
    println!("total triples : {}", kg.len()?);
    println!("RSS           : {rss_before:.1} MB before load, {rss_after:.1} MB after load and rules");

    let queries = [
        ("models", portfolio::MODELS_QUERY),
        ("contracts", portfolio::CONTRACTS_QUERY),
        ("contract->model path", portfolio::PATH_QUERY),
        ("subClassOf* walk", portfolio::SUBCLASS_QUERY),
    ];
    println!("\nquery latency over {iterations} runs (ms):");
    println!("  {:<22} {:>6} {:>8} {:>8} {:>8}", "query", "rows", "mean", "p95", "max");
    for (name, q) in queries {
        let mut times = Vec::with_capacity(iterations);
        let mut rows = 0;
        for _ in 0..iterations {
            let t = Instant::now();
            rows = portfolio::run(&kg, q)?.len();
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        let p95 = times[((times.len() as f64 * 0.95) as usize).min(times.len() - 1)];
        println!("  {name:<22} {rows:>6} {mean:>8.3} {p95:>8.3} {:>8.3}", times.last().unwrap());
    }
    println!("\nRSS after queries: {:.1} MB", rss_mb());
    Ok(())
}
