//! Knowledge graph adapter. Oxigraph runs in memory and is rebuilt from the
//! TTL files at startup, so the files stay the single source of truth.

use anyhow::{Context, Result};
use oxigraph::io::{RdfFormat, RdfParser};
use oxigraph::model::{GraphNameRef, Literal, NamedNode, Quad, Term};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub const GRAPH_BASE: &str = "http://example.org/volkswagen/graph/";
/// Named graph that receives every triple derived by the rules.
/// Named graph that receives the numeric facts written from SQLite.
pub const FACTS_GRAPH: &str = "http://example.org/volkswagen/graph/erp-facts";
pub const INFERRED_GRAPH: &str = "http://example.org/volkswagen/graph/inferred";
/// Upper bound on fixpoint passes; hitting it means a rule keeps producing new triples.
const MAX_PASSES: usize = 25;

pub type Row = BTreeMap<String, String>;

/// One numeric fact to write into the graph: `subject vw:predicate "value"^^datatype`.
#[derive(Debug, Clone)]
pub struct Fact {
    pub subject: String,
    /// Local name in the `vw:` namespace
    pub predicate: String,
    pub value: String,
    pub datatype: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphStat {
    pub graph: String,
    pub file: String,
    pub triples: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct InferStats {
    /// Passes over all rule files, including the final pass that derived nothing.
    pub passes: usize,
    /// Triples in the inferred graph.
    pub inferred: usize,
}

pub struct Kg {
    store: Store,
}

impl Kg {
    /// Load every `*.ttl` file in `dir` (sorted by name) into a named graph
    /// derived from the file stem.
    pub fn load_dir(dir: &Path) -> Result<(Kg, Vec<GraphStat>)> {
        let store = Store::new()?;
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .with_context(|| format!("reading ontology dir {}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "ttl"))
            .collect();
        files.sort();

        let mut stats = Vec::new();
        for path in files {
            let stem = path.file_stem().unwrap().to_string_lossy().to_lowercase();
            let graph = NamedNode::new(format!("{GRAPH_BASE}{stem}"))?;
            let reader = BufReader::new(
                File::open(&path).with_context(|| format!("opening {}", path.display()))?,
            );
            let parser = RdfParser::from_format(RdfFormat::Turtle)
                .with_default_graph(graph.clone());
            store
                .load_from_reader(parser, reader)
                .with_context(|| format!("parsing {}", path.display()))?;
            let triples = store
                .quads_for_pattern(None, None, None, Some(GraphNameRef::NamedNode(graph.as_ref())))
                .count();
            stats.push(GraphStat {
                graph: graph.into_string(),
                file: path.file_name().unwrap().to_string_lossy().into_owned(),
                triples,
            });
        }
        Ok((Kg { store }, stats))
    }

    /// Write numeric facts (from SQLite) into their own named graph.
    pub fn add_facts(&self, facts: &[Fact]) -> Result<GraphStat> {
        let graph = NamedNode::new(FACTS_GRAPH)?;
        for f in facts {
            let quad = Quad::new(
                NamedNode::new(&f.subject)?,
                NamedNode::new(format!("http://example.org/volkswagen/{}", f.predicate))?,
                Literal::new_typed_literal(&f.value, NamedNode::new(f.datatype)?),
                graph.clone(),
            );
            self.store.insert(&quad)?;
        }
        let triples = self.store.quads_for_pattern(None, None, None, Some(GraphNameRef::NamedNode(graph.as_ref()))).count();
        Ok(GraphStat { graph: FACTS_GRAPH.to_string(), file: "(SQLite facts)".to_string(), triples })
    }

    /// Load the TTL files, add the numeric facts from SQLite, then run the rules.
    /// The inferred graph is appended to the stats.
    pub fn load(ontology_dir: &Path, rules_dir: &Path, conn: &rusqlite::Connection) -> Result<(Kg, Vec<GraphStat>, InferStats)> {
        let (kg, mut stats) = Kg::load_dir(ontology_dir)?;
        stats.push(kg.add_facts(&crate::facts::from_db(&kg, conn)?)?);
        let infer = kg.infer(rules_dir)?;
        stats.push(GraphStat {
            graph: INFERRED_GRAPH.to_string(),
            file: "(rules)".to_string(),
            triples: infer.inferred,
        });
        Ok((kg, stats, infer))
    }

    /// Apply every `*.ru` file in `dir` (sorted by name) repeatedly until a full pass adds nothing.
    /// Rules insert into the inferred graph and read the union of all graphs, so rules can build on
    /// each other regardless of file order.
    pub fn infer(&self, dir: &Path) -> Result<InferStats> {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .with_context(|| format!("reading rules dir {}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "ru"))
            .collect();
        files.sort();
        let rules: Vec<(String, String)> = files
            .iter()
            .map(|p| {
                let text = std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
                Ok((p.file_name().unwrap().to_string_lossy().into_owned(), text))
            })
            .collect::<Result<_>>()?;

        let mut passes = 0;
        loop {
            let before = self.store.len()?;
            for (name, text) in &rules {
                let mut prepared = SparqlEvaluator::new()
                    .parse_update(text)
                    .with_context(|| format!("parsing {name}"))?;
                for spec in prepared.using_datasets_mut() {
                    spec.set_default_graph_as_union();
                }
                prepared
                    .on_store(&self.store)
                    .execute()
                    .with_context(|| format!("running {name}"))?;
            }
            passes += 1;
            if self.store.len()? == before {
                break;
            }
            anyhow::ensure!(passes < MAX_PASSES, "rules did not reach a fixpoint after {MAX_PASSES} passes");
        }
        let graph = NamedNode::new(INFERRED_GRAPH)?;
        let inferred = self
            .store
            .quads_for_pattern(None, None, None, Some(GraphNameRef::NamedNode(graph.as_ref())))
            .count();
        Ok(InferStats { passes, inferred })
    }

    pub fn len(&self) -> Result<usize> {
        Ok(self.store.len()?)
    }

    /// Run a SELECT query and return each solution as variable -> lexical value.
    pub fn select(&self, query: &str) -> Result<Vec<Row>> {
        // Each TTL file lives in its own named graph; queries see their union.
        let mut prepared = SparqlEvaluator::new().parse_query(query)?;
        prepared.dataset_mut().set_default_graph_as_union();
        let results = prepared.on_store(&self.store).execute()?;
        let QueryResults::Solutions(solutions) = results else {
            anyhow::bail!("not a SELECT query");
        };
        let mut rows = Vec::new();
        for solution in solutions {
            let solution = solution?;
            let mut row = Row::new();
            for (var, term) in solution.iter() {
                row.insert(var.as_str().to_string(), term_value(term));
            }
            rows.push(row);
        }
        Ok(rows)
    }
}

fn term_value(term: &Term) -> String {
    match term {
        Term::NamedNode(n) => n.as_str().to_string(),
        Term::BlankNode(b) => format!("_:{}", b.as_str()),
        Term::Literal(l) => l.value().to_string(),
    }
}

/// Test helper: an in-memory database with all migrations applied and a graph built from it.
#[cfg(test)]
pub fn load_for_test() -> (Kg, rusqlite::Connection) {
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::db::migrate(&mut conn).unwrap();
    let (kg, _, _) = Kg::load(Path::new("../ontology"), Path::new("../rules"), &conn).unwrap();
    (kg, conn)
}
