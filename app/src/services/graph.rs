//! Explain paths: the chain of knowledge-graph facts that connects two entities, with where each
//! fact came from. Built for explainable answers: every edge says whether it was asserted in a Turtle
//! file, derived by a rule file, or is an ERP number written from SQLite, and every node carries its
//! literal values for a side panel.
//!
//! Only "explanatory" predicates are walked (see `ALLOWED`). Derived shortcuts such as
//! `exposedToIndex` are left out on purpose: they would hide the real chain
//! index -> clause -> contract -> component -> model that the pricing arithmetic follows.

use super::finance::{bad, short, NS};
use super::portfolio::run;
use crate::kg::{Kg, INFERRED_GRAPH};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

const GRAPH_BASE: &str = "http://example.org/volkswagen/graph/";
const DEFAULT_DEPTH: usize = 7;
const MAX_DEPTH: usize = 12;
const DEFAULT_PATHS: usize = 3;
const MAX_PATHS: usize = 8;
const MAX_PAIRS: usize = 8;
const MAX_PROPERTIES: usize = 40;
/// Node groups a route may start or end at but never pass through: two entities that merely share a
/// currency or a supplier are not related by it.
const TERMINAL_ONLY: &[&str] = &["currency", "supplier"];
/// Safety valve for the route search on a dense graph.
const MAX_VISITS: usize = 200_000;

/// Predicates that explain something, most natural first. When two entities are linked by several
/// of them (an asserted fact and its inverse), the earlier one is drawn.
const ALLOWED: &[&str] = &[
    "indexedTo", "hasIndexationClause", "contractCoversComponent", "hasComponent", "contractSupplier", "componentSupplier",
    "contractCurrency", "modelOf", "hasModel", "profitCenterOfBrand", "brandProfitCenter",
    "hasBrand", "belongsToGroup", "hasProductionSite", "soldInMarket", "appliesInMarket", "appliesToPowertrain", "hasPowertrain",
    "competesWith", "competitorOf", "campaignAppliesTo", "financingFor", "offeredInMarket", "alternativeSupplier",
    "subsidisedBy", "co2TargetBreachedUnder", "highIntentForModel", "livesInRegion",
];

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Origin {
    /// "asserted", "inferred" or "erp_fact"
    pub kind: &'static str,
    /// Turtle file, rule file, or "SQLite"
    pub source: String,
}

impl Origin {
    fn from_graph(kg: &Kg, graph: &str) -> Origin {
        if let Some(rule) = graph.strip_prefix(&format!("{INFERRED_GRAPH}/")) {
            Origin { kind: "inferred", source: format!("{rule}.ru") }
        } else if graph.ends_with("/erp-facts") {
            Origin { kind: "erp_fact", source: "SQLite".into() }
        } else {
            let file = kg.graph_file(graph).map(String::from).unwrap_or_else(|| graph.trim_start_matches(GRAPH_BASE).to_string());
            Origin { kind: "asserted", source: file }
        }
    }

    fn rank(&self) -> u8 {
        match self.kind {
            "asserted" => 0,
            "erp_fact" => 1,
            _ => 2,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct PairReq {
    pub from: String,
    pub to: String,
}

#[derive(Deserialize, Debug)]
pub struct ExplainRequest {
    pub pairs: Vec<PairReq>,
    pub max_depth: Option<usize>,
    pub max_paths: Option<usize>,
}

#[derive(Serialize, Debug)]
pub struct GraphProperty {
    pub predicate: String,
    pub label: String,
    pub value: String,
    pub datatype: String,
    pub origin: Origin,
}

#[derive(Serialize, Debug)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub group: String,
    pub types: Vec<String>,
    pub properties: Vec<GraphProperty>,
}

#[derive(Serialize, Debug)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub predicate: String,
    pub label: String,
    pub origin: Origin,
    /// Every file or rule file that states this fact.
    pub sources: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct Route {
    pub nodes: Vec<String>,
    pub edges: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct ExplainPath {
    pub from: String,
    pub to: String,
    pub from_label: String,
    pub to_label: String,
    pub found: bool,
    pub routes: Vec<Route>,
}

#[derive(Serialize, Debug)]
pub struct ExplainGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub paths: Vec<ExplainPath>,
}

#[derive(Serialize, Debug)]
pub struct GraphEntity {
    pub iri: String,
    pub label: String,
    pub group: String,
}

/// "contractCoversComponent" -> "contract covers component"
pub fn humanize(local: &str) -> String {
    let mut out = String::new();
    for (i, c) in local.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
        }
        out.extend(c.to_lowercase());
    }
    out.replace('_', " ")
}

fn group_of(types: &[String]) -> &'static str {
    const ORDER: &[(&str, &str)] = &[
        ("CommodityIndex", "index"), ("Currency", "currency"), ("IndexationClause", "clause"), ("SupplyContract", "contract"),
        ("Supplier", "supplier"), ("Component", "component"), ("Model", "model"), ("CompetitorModel", "competitor"),
        ("Competitor", "competitor"), ("ProfitCenter", "profit_center"), ("Brand", "brand"), ("BrandGroup", "group"),
        ("ProductionSite", "site"), ("Market", "market"), ("Regulation", "regulation"), ("IncentiveCampaign", "campaign"),
        ("FinancingProduct", "financing"), ("Client", "customer"), ("Customer", "customer"), ("Powertrain", "powertrain"),
        ("MarketRegion", "region"),
    ];
    for (t, g) in ORDER {
        if types.iter().any(|x| x == t) {
            return g;
        }
    }
    "other"
}

/// Edge key: subject, predicate local name, object.
type Key = (String, String, String);

struct EdgeInfo {
    origin: Origin,
    sources: BTreeSet<String>,
}

/// A read-only view of the graph built for one request: labels, types and the allowed edges.
pub struct GraphIndex<'a> {
    kg: &'a Kg,
    labels: HashMap<String, String>,
    types: HashMap<String, Vec<String>>,
    edges: BTreeMap<Key, EdgeInfo>,
    /// node -> (neighbour, edge key) over the best edge of every connected pair
    adjacency: HashMap<String, Vec<(String, Key)>>,
}

impl<'a> GraphIndex<'a> {
    pub fn build(kg: &'a Kg) -> Result<GraphIndex<'a>> {
        let mut labels = HashMap::new();
        for r in run(kg, "SELECT ?x ?l WHERE { ?x rdfs:label ?l }")? {
            labels.entry(r["x"].clone()).or_insert_with(|| r["l"].clone());
        }
        let mut types: HashMap<String, Vec<String>> = HashMap::new();
        for r in run(kg, &format!("SELECT DISTINCT ?x ?t WHERE {{ ?x a ?t FILTER(STRSTARTS(STR(?t), \"{NS}\")) }}"))? {
            types.entry(r["x"].clone()).or_default().push(short(&r["t"]));
        }
        let mut edges: BTreeMap<Key, EdgeInfo> = BTreeMap::new();
        let rows = run(
            kg,
            &format!(
                "SELECT ?s ?p ?o ?g WHERE {{ GRAPH ?g {{ ?s ?p ?o }}
                   FILTER(isIRI(?o) && STRSTARTS(STR(?s), \"{NS}\") && STRSTARTS(STR(?o), \"{NS}\") && STRSTARTS(STR(?p), \"{NS}\")) }}"
            ),
        )?;
        for r in rows {
            let p = short(&r["p"]);
            if !ALLOWED.contains(&p.as_str()) {
                continue;
            }
            let origin = Origin::from_graph(kg, &r["g"]);
            let e = edges.entry((r["s"].clone(), p, r["o"].clone())).or_insert_with(|| EdgeInfo { origin: origin.clone(), sources: BTreeSet::new() });
            if origin.rank() < e.origin.rank() {
                e.origin = origin.clone();
            }
            e.sources.insert(origin.source);
        }

        // one edge per connected pair: asserted before inferred, then the predicate order of ALLOWED
        let rank = |k: &Key, e: &EdgeInfo| (e.origin.rank(), ALLOWED.iter().position(|p| *p == k.1).unwrap_or(usize::MAX));
        let mut best: HashMap<(String, String), &Key> = HashMap::new();
        for (k, e) in &edges {
            let pair = if k.0 <= k.2 { (k.0.clone(), k.2.clone()) } else { (k.2.clone(), k.0.clone()) };
            match best.get(&pair) {
                Some(cur) if rank(cur, &edges[*cur]) <= rank(k, e) => {}
                _ => {
                    best.insert(pair, k);
                }
            }
        }
        let mut adjacency: HashMap<String, Vec<(String, Key)>> = HashMap::new();
        for k in best.values() {
            adjacency.entry(k.0.clone()).or_default().push((k.2.clone(), (*k).clone()));
            adjacency.entry(k.2.clone()).or_default().push((k.0.clone(), (*k).clone()));
        }
        for v in adjacency.values_mut() {
            v.sort_by(|a, b| a.0.cmp(&b.0));
        }
        Ok(GraphIndex { kg, labels, types, edges, adjacency })
    }

    pub fn label(&self, iri: &str) -> String {
        self.labels.get(iri).cloned().unwrap_or_else(|| short(iri))
    }

    fn group(&self, iri: &str) -> &'static str {
        group_of(self.types.get(iri).map(Vec::as_slice).unwrap_or(&[]))
    }

    fn known(&self, iri: &str) -> bool {
        self.adjacency.contains_key(iri) || self.labels.contains_key(iri)
    }

    /// Turn a full IRI, a short name ("ID4", "Index_NaturalRubber") or a label ("Natural rubber") into an IRI.
    pub fn resolve(&self, name: &str) -> Result<String> {
        let name = name.trim();
        if self.known(name) {
            return Ok(name.to_string());
        }
        let lower = name.to_lowercase();
        let mut candidates: Vec<String> = self.labels.keys().chain(self.adjacency.keys()).filter(|i| i.starts_with(NS)).cloned().collect();
        candidates.sort();
        candidates.dedup();
        let exact = [format!("{NS}{name}"), format!("{NS}Index_{name}"), format!("{NS}Market_{name}")];
        if let Some(hit) = exact.iter().find(|i| self.known(i)) {
            return Ok(hit.clone());
        }
        if let Some(hit) = candidates.iter().find(|i| short(i).to_lowercase() == lower) {
            return Ok(hit.clone());
        }
        if let Some(hit) = candidates.iter().find(|i| self.label(i).to_lowercase() == lower) {
            return Ok(hit.clone());
        }
        let contains: Vec<&String> = candidates.iter().filter(|i| self.label(i).to_lowercase().contains(&lower)).collect();
        match contains.as_slice() {
            [one] => Ok((*one).clone()),
            [] => bad(format!("unknown entity '{name}'")),
            many => bad(format!("'{name}' is ambiguous: {}", many.iter().take(5).map(|i| self.label(i)).collect::<Vec<_>>().join(", "))),
        }
    }

    /// Shortest hop count from `start` to every node it reaches within `limit` hops.
    fn distances(&self, start: &str, limit: usize) -> HashMap<String, usize> {
        let mut dist = HashMap::from([(start.to_string(), 0usize)]);
        let mut queue = VecDeque::from([start.to_string()]);
        while let Some(n) = queue.pop_front() {
            let d = dist[&n];
            if d >= limit {
                continue;
            }
            for (m, _) in self.adjacency.get(&n).into_iter().flatten() {
                if !dist.contains_key(m) {
                    dist.insert(m.clone(), d + 1);
                    queue.push_back(m.clone());
                }
            }
        }
        dist
    }

    /// Simple routes from `from` to `to`, shortest first, at most one hop longer than the shortest.
    fn routes(&self, from: &str, to: &str, max_depth: usize, max_paths: usize) -> Vec<Vec<(String, Key)>> {
        let to_target = self.distances(to, max_depth);
        let Some(&shortest) = to_target.get(from) else { return vec![] };
        let limit = (shortest + 1).min(max_depth);
        let mut found: Vec<Vec<(String, Key)>> = Vec::new();
        let mut visits = 0usize;
        // iterative deepening keeps the shortest routes first
        for len in shortest..=limit {
            let mut stack: Vec<(String, Vec<(String, Key)>)> = vec![(from.to_string(), vec![])];
            while let Some((node, path)) = stack.pop() {
                visits += 1;
                if visits > MAX_VISITS || found.len() >= max_paths {
                    return found;
                }
                if node == to {
                    if path.len() == len {
                        found.push(path);
                    }
                    continue;
                }
                if path.len() >= len {
                    continue;
                }
                let on_path: BTreeSet<&String> = path.iter().map(|(n, _)| n).chain(std::iter::once(&node)).collect();
                for (next, key) in self.adjacency.get(&node).into_iter().flatten().rev() {
                    let remaining = len - path.len() - 1;
                    if on_path.contains(next) || to_target.get(next).is_none_or(|d| *d > remaining) {
                        continue;
                    }
                    if next != to && TERMINAL_ONLY.contains(&self.group(next)) {
                        continue;
                    }
                    let mut p = path.clone();
                    p.push((node.clone(), key.clone()));
                    stack.push((next.clone(), p));
                }
            }
        }
        found
    }

    fn properties(&self, iri: &str) -> Result<Vec<GraphProperty>> {
        let rows = run(
            self.kg,
            &format!(
                "SELECT ?p ?o ?dt ?g WHERE {{ GRAPH ?g {{ <{iri}> ?p ?o }} FILTER(isLiteral(?o) && ?p != rdfs:label) BIND(DATATYPE(?o) AS ?dt) }}"
            ),
        )?;
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for r in rows {
            let p = short(&r["p"]);
            if !seen.insert((p.clone(), r["o"].clone())) {
                continue;
            }
            out.push(GraphProperty {
                label: humanize(&p),
                predicate: p,
                value: r["o"].clone(),
                datatype: r.get("dt").map(|d| d.rsplit('#').next().unwrap_or(d).to_string()).unwrap_or_default(),
                origin: Origin::from_graph(self.kg, &r["g"]),
            });
        }
        out.sort_by(|a, b| a.label.cmp(&b.label));
        out.truncate(MAX_PROPERTIES);
        Ok(out)
    }

    pub fn entities(&self, query: &str, limit: usize) -> Vec<GraphEntity> {
        const PRIORITY: &[&str] = &["index", "currency", "model", "brand", "profit_center", "supplier", "contract", "component", "regulation", "market", "competitor", "group", "site", "campaign", "financing", "customer", "clause"];
        let q = query.trim().to_lowercase();
        let mut nodes: BTreeSet<&String> = self.adjacency.keys().collect();
        nodes.extend(self.labels.keys().filter(|i| self.group(i) != "other" && i.starts_with(NS)));
        let mut hits: Vec<GraphEntity> = nodes
            .into_iter()
            .filter(|i| q.is_empty() || self.label(i).to_lowercase().contains(&q) || short(i).to_lowercase().contains(&q))
            .map(|i| GraphEntity { iri: i.clone(), label: self.label(i), group: self.group(i).to_string() })
            .collect();
        let prio = |g: &str| PRIORITY.iter().position(|p| *p == g).unwrap_or(PRIORITY.len());
        hits.sort_by(|a, b| prio(&a.group).cmp(&prio(&b.group)).then(a.label.cmp(&b.label)));
        if q.is_empty() {
            // a spread of groups rather than 20 customers or clauses
            let mut per: HashMap<String, usize> = HashMap::new();
            hits.retain(|h| {
                let n = per.entry(h.group.clone()).or_default();
                *n += 1;
                *n <= 3
            });
        }
        hits.truncate(limit.clamp(1, 100));
        hits
    }
}

pub fn explain(kg: &Kg, req: &ExplainRequest) -> Result<ExplainGraph> {
    if req.pairs.is_empty() || req.pairs.len() > MAX_PAIRS {
        return bad(format!("give between 1 and {MAX_PAIRS} pairs"));
    }
    let depth = req.max_depth.unwrap_or(DEFAULT_DEPTH);
    let paths_wanted = req.max_paths.unwrap_or(DEFAULT_PATHS);
    if !(1..=MAX_DEPTH).contains(&depth) || !(1..=MAX_PATHS).contains(&paths_wanted) {
        return bad(format!("max_depth must be 1 to {MAX_DEPTH} and max_paths 1 to {MAX_PATHS}"));
    }
    let index = GraphIndex::build(kg)?;
    let mut node_ids: BTreeSet<String> = BTreeSet::new();
    let mut edge_keys: BTreeMap<Key, String> = BTreeMap::new();
    let mut paths = Vec::new();
    for pair in &req.pairs {
        let (from, to) = (index.resolve(&pair.from)?, index.resolve(&pair.to)?);
        let routes = index.routes(&from, &to, depth, paths_wanted);
        let mut out_routes = Vec::new();
        for route in &routes {
            let mut nodes = vec![from.clone()];
            let mut edges = Vec::new();
            let mut at = from.clone();
            for (_, key) in route {
                let next = if key.0 == at { key.2.clone() } else { key.0.clone() };
                let next_id = edge_keys.len();
                let id = edge_keys.entry(key.clone()).or_insert_with(|| format!("e{next_id}")).clone();
                edges.push(id);
                nodes.push(next.clone());
                at = next;
            }
            node_ids.extend(nodes.iter().cloned());
            out_routes.push(Route { nodes, edges });
        }
        // a route from a node to itself is a single node
        if from == to {
            node_ids.insert(from.clone());
            out_routes.push(Route { nodes: vec![from.clone()], edges: vec![] });
        }
        paths.push(ExplainPath { found: !out_routes.is_empty(), routes: out_routes, from_label: index.label(&from), to_label: index.label(&to), from, to });
    }
    let mut edges: Vec<GraphEdge> = edge_keys
        .iter()
        .map(|(k, id)| {
            let info = &index.edges[k];
            GraphEdge {
                id: id.clone(),
                source: k.0.clone(),
                target: k.2.clone(),
                label: humanize(&k.1),
                predicate: k.1.clone(),
                origin: info.origin.clone(),
                sources: info.sources.iter().cloned().collect(),
            }
        })
        .collect();
    edges.sort_by_key(|e| e.id[1..].parse::<usize>().unwrap_or(0));
    let mut nodes = Vec::new();
    for id in &node_ids {
        nodes.push(GraphNode { id: id.clone(), label: index.label(id), group: index.group(id).to_string(), types: index.types.get(id).cloned().unwrap_or_default(), properties: index.properties(id)? });
    }
    Ok(ExplainGraph { nodes, edges, paths })
}

/// One line per route: "Natural rubber -indexed to-> Clause ... <-contract covers component- ...", with the
/// source of every derived link. This is the text a language model reads back.
pub fn describe(g: &ExplainGraph) -> String {
    let label: HashMap<&str, &str> = g.nodes.iter().map(|n| (n.id.as_str(), n.label.as_str())).collect();
    let edge: HashMap<&str, &GraphEdge> = g.edges.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut lines = Vec::new();
    for p in &g.paths {
        if !p.found {
            lines.push(format!("{} and {} are not connected by any explanatory chain.", p.from_label, p.to_label));
            continue;
        }
        for (i, r) in p.routes.iter().enumerate() {
            let mut s = label[r.nodes[0].as_str()].to_string();
            let mut derived = Vec::new();
            for (k, eid) in r.edges.iter().enumerate() {
                let e = edge[eid.as_str()];
                let forward = e.source == r.nodes[k];
                let next = label[r.nodes[k + 1].as_str()];
                if forward {
                    s += &format!(" -[{}]-> {next}", e.label);
                } else {
                    s += &format!(" <-[{}]- {next}", e.label);
                }
                if e.origin.kind == "inferred" {
                    derived.push(format!("'{}' derived by {}", e.label, e.origin.source));
                }
            }
            if !derived.is_empty() {
                s += &format!(" ({})", derived.join("; "));
            }
            lines.push(format!("Route {} for {} to {}: {s}", i + 1, p.from_label, p.to_label));
        }
    }
    lines.join("\n")
}

/// `/graph?pairs=<from>|<to>,...` with each IRI percent-encoded, as the Explain graph page reads it.
pub fn explain_link(pairs: &[(String, String)]) -> String {
    fn enc(s: &str) -> String {
        s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
    }
    let joined: Vec<String> = pairs.iter().map(|(a, b)| format!("{}|{}", enc(a), enc(b))).collect();
    format!("/graph?pairs={}", joined.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::finance::BadRequest;

    fn kg() -> Kg {
        crate::kg::load_for_test().0
    }

    fn req(from: &str, to: &str) -> ExplainRequest {
        ExplainRequest { pairs: vec![PairReq { from: from.into(), to: to.into() }], max_depth: None, max_paths: None }
    }

    fn labels(g: &ExplainGraph, r: &Route) -> Vec<String> {
        r.nodes.iter().map(|n| g.nodes.iter().find(|x| &x.id == n).unwrap().label.clone()).collect()
    }

    #[test]
    fn rubber_reaches_the_golf_through_clause_contract_and_component() {
        let g = explain(&kg(), &req("NaturalRubber", "Golf")).unwrap();
        let p = &g.paths[0];
        assert!(p.found && p.from_label == "Natural rubber" && p.to_label == "Golf");
        let route = &p.routes[0];
        assert_eq!(route.edges.len(), 4, "index, clause, contract, component, model: {:?}", labels(&g, route));
        let preds: Vec<&str> = route.edges.iter().map(|e| g.edges.iter().find(|x| &x.id == e).unwrap().predicate.as_str()).collect();
        assert_eq!(preds, ["indexedTo", "hasIndexationClause", "contractCoversComponent", "hasComponent"]);
        assert!(route.edges.iter().all(|e| g.edges.iter().find(|x| &x.id == e).unwrap().origin.kind == "asserted"));
        assert!(labels(&g, route).last().unwrap() == "Golf");
    }

    #[test]
    fn suppliers_and_currencies_are_start_or_end_points_never_stepping_stones() {
        let kg = kg();
        for (from, to) in [("NaturalRubber", "Golf"), ("SEK", "RSeries"), ("Index_Steel", "TGX")] {
            let g = explain(&kg, &ExplainRequest { max_paths: Some(8), ..req(from, to) }).unwrap();
            for r in &g.paths[0].routes {
                for mid in &r.nodes[1..r.nodes.len() - 1] {
                    let group = &g.nodes.iter().find(|n| &n.id == mid).unwrap().group;
                    assert!(group != "supplier" && group != "currency", "{from} to {to} passes through a {group}: {:?}", labels(&g, r));
                }
            }
        }
        let g = explain(&kg, &req("Supplier_BatterySystems", "ID4")).unwrap();
        assert!(g.paths[0].found, "a supplier can still be the start of a route");
    }

    #[test]
    fn nodes_carry_literals_with_their_origin_for_the_side_panel() {
        let g = explain(&kg(), &req("NaturalRubber", "Golf")).unwrap();
        let golf = g.nodes.iter().find(|n| n.label == "Golf").unwrap();
        assert_eq!(golf.group, "model");
        let price = golf.properties.iter().find(|p| p.predicate == "price").unwrap();
        assert_eq!((price.value.parse::<f64>().unwrap(), price.origin.kind, price.origin.source.as_str()), (30000.0, "erp_fact", "SQLite"));
        assert_eq!(price.datatype, "decimal");
        assert!(golf.properties.iter().any(|p| p.predicate == "customerAppreciationRating" && p.origin.kind == "asserted"));
        let contract = g.nodes.iter().find(|n| n.group == "contract").unwrap();
        assert!(contract.properties.iter().any(|p| p.predicate == "currentUnitPriceEUR"));
        assert!(g.nodes.iter().all(|n| !n.properties.iter().any(|p| p.predicate == "label")));
    }

    #[test]
    fn inferred_links_name_the_rule_file_that_derived_them() {
        let kg = kg();
        let g = explain(&kg, &req("Reg_DE_EVSubsidy", "ID4")).unwrap();
        let inferred: Vec<&GraphEdge> = g.edges.iter().filter(|e| e.origin.kind == "inferred").collect();
        assert_eq!(inferred.len(), 1, "{:?}", g.edges.iter().map(|e| (&e.predicate, &e.origin)).collect::<Vec<_>>());
        assert_eq!((inferred[0].predicate.as_str(), inferred[0].origin.source.as_str()), ("subsidisedBy", "50_market.ru"));
        assert!(g.paths[0].routes.len() >= 2, "the asserted market and powertrain chains are shown as well");
        let graphs = run(&kg, "SELECT ?g WHERE { GRAPH ?g { ?c a vw:CommodityExposedContract } } LIMIT 1").unwrap();
        assert!(graphs[0]["g"].ends_with("/graph/inferred/40_exposure"), "{graphs:?}");
    }

    #[test]
    fn brand_to_profit_center_uses_asserted_facts_only() {
        let g = explain(&kg(), &req("Golf", "ProfitCenter_Volkswagen")).unwrap();
        assert_eq!(g.paths[0].routes[0].edges.len(), 2);
        assert!(g.edges.iter().take(2).all(|e| e.origin.kind == "asserted" && e.origin.source.ends_with(".ttl")));
        // an asserted fact and its inferred inverse are drawn once
        let ids: BTreeSet<&String> = g.paths[0].routes[0].edges.iter().collect();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn unconnected_pairs_and_depth_limits_are_reported_not_errors() {
        let g = explain(&kg(), &ExplainRequest { pairs: vec![PairReq { from: "Index_Lithium".into(), to: "Huracan".into() }], max_depth: Some(3), max_paths: None }).unwrap();
        assert!(!g.paths[0].found && g.paths[0].routes.is_empty());
        assert!(describe(&g).contains("not connected"));
        let same = explain(&kg(), &req("Golf", "Golf")).unwrap();
        assert!(same.paths[0].found && same.paths[0].routes[0].edges.is_empty());
    }

    #[test]
    fn names_resolve_from_iris_short_names_and_labels() {
        let kg = kg();
        let idx = GraphIndex::build(&kg).unwrap();
        let want = format!("{NS}Index_NaturalRubber");
        for name in [want.as_str(), "Index_NaturalRubber", "NaturalRubber", "Natural rubber", "natural rubber"] {
            assert_eq!(idx.resolve(name).unwrap(), want, "{name}");
        }
        assert_eq!(idx.resolve("ID4").unwrap(), format!("{NS}ID4"));
        for bad in ["Atlantis", "", "profit"] {
            let e = idx.resolve(bad).unwrap_err();
            assert!(e.downcast_ref::<BadRequest>().is_some(), "{bad}: {e:#}");
        }
    }

    #[test]
    fn requests_are_validated() {
        let kg = kg();
        let bad_req = |r: ExplainRequest| explain(&kg, &r).unwrap_err().downcast_ref::<BadRequest>().is_some();
        assert!(bad_req(ExplainRequest { pairs: vec![], max_depth: None, max_paths: None }));
        assert!(bad_req(ExplainRequest { pairs: vec![PairReq { from: "Golf".into(), to: "ID4".into() }; 9], max_depth: None, max_paths: None }));
        assert!(bad_req(ExplainRequest { max_depth: Some(0), ..req("Golf", "ID4") }));
        assert!(bad_req(ExplainRequest { max_paths: Some(99), ..req("Golf", "ID4") }));
        assert!(bad_req(req("Golf", "Atlantis")));
    }

    #[test]
    fn several_pairs_share_one_graph_and_routes_are_shortest_first() {
        let g = explain(&kg(), &ExplainRequest { pairs: vec![PairReq { from: "NaturalRubber".into(), to: "Golf".into() }, PairReq { from: "NaturalRubber".into(), to: "Octavia".into() }], max_depth: None, max_paths: Some(4) }).unwrap();
        assert_eq!(g.paths.len(), 2);
        for p in &g.paths {
            assert!(p.found && p.routes.windows(2).all(|w| w[0].edges.len() <= w[1].edges.len()));
        }
        let ids: BTreeSet<&String> = g.nodes.iter().map(|n| &n.id).collect();
        assert_eq!(ids.len(), g.nodes.len(), "a node appears once even when two pairs pass through it");
        assert!(g.edges.iter().all(|e| ids.contains(&e.source) && ids.contains(&e.target)));
    }

    #[test]
    fn description_reads_as_a_chain_and_flags_derived_links() {
        let kg = kg();
        let text = describe(&explain(&kg, &req("NaturalRubber", "Golf")).unwrap());
        assert!(text.starts_with("Route 1 for Natural rubber to Golf: Natural rubber <-[indexed to]-"), "{text}");
        assert!(text.ends_with("Golf") || text.contains("-[has component]->"), "{text}");
        let derived = describe(&explain(&kg, &req("Reg_DE_EVSubsidy", "ID4")).unwrap());
        assert!(derived.contains("derived by 50_market.ru"), "{derived}");
    }

    #[test]
    fn entity_search_and_link_encoding() {
        let kg = kg();
        let idx = GraphIndex::build(&kg).unwrap();
        let hits = idx.entities("rubber", 10);
        assert!(hits.iter().any(|h| h.group == "index" && h.label == "Natural rubber"));
        let spread = idx.entities("", 20);
        assert!(spread.len() <= 20 && spread.iter().map(|h| h.group.as_str()).collect::<BTreeSet<_>>().len() >= 5);
        let link = explain_link(&[(format!("{NS}Index_Steel"), format!("{NS}Golf"))]);
        assert_eq!(link, "/graph?pairs=http%3A%2F%2Fexample.org%2Fvolkswagen%2FIndex_Steel|http%3A%2F%2Fexample.org%2Fvolkswagen%2FGolf");
        assert_eq!(humanize("contractCoversComponent"), "contract covers component");
    }
}
