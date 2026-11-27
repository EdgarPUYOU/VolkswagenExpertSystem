//! Chat layer: the question services exposed as tools, and a rule-based planner that turns a
//! sentence into a tool call.
//!
//! A tool takes JSON arguments and returns a `ToolOutput`: a one-paragraph summary (what an LLM
//! would read back to the user), structured blocks (what the chat bubble draws) and the full data.
//! The planner is the only part that guesses; swap it for an LLM that calls the same tools and
//! nothing else changes. Tools never accept SPARQL or SQL.

use super::combined::{self, CombinedRequest};
use crate::kg::Kg;
use super::finance::*;
use super::graph::{self, ExplainRequest, PairReq};
use super::market::{self, MarketData};
use super::sales::{self, SalesData};
use super::questions;
use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ---------------------------------------------------------------- types

#[derive(Serialize, Debug)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

#[derive(Serialize, Debug, Clone)]
pub struct Kpi {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// "bad" renders in the warning colour
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct BarItem {
    pub label: String,
    pub value: f64,
}

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Kpis { items: Vec<Kpi> },
    Table { title: String, columns: Vec<String>, rows: Vec<Vec<String>> },
    Bars { title: String, unit: String, items: Vec<BarItem> },
    /// A link to a page of the app; `shocks` pre-fills the scenario lab.
    Link {
        label: String,
        to: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        shocks: Option<Vec<ShockSpec>>,
    },
}

#[derive(Serialize, Debug)]
pub struct ToolOutput {
    pub summary: String,
    pub blocks: Vec<Block>,
    pub data: Value,
}

#[derive(Serialize, Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub args: Value,
}

#[derive(Serialize, Debug)]
pub struct Reply {
    pub text: String,
    pub blocks: Vec<Block>,
    pub tool_calls: Vec<ToolCall>,
    pub planner: &'static str,
}

/// One earlier turn of the conversation, sent back by the client so the model keeps context.
#[derive(Deserialize, Debug, Clone)]
pub struct Turn {
    /// "user" or "assistant"
    pub role: String,
    pub content: String,
}

#[derive(Deserialize, Debug, Default)]
pub struct ChatRequest {
    pub message: String,
    /// Earlier turns for the language model; the rule-based planner ignores them.
    #[serde(default)]
    pub history: Vec<Turn>,
}

// ---------------------------------------------------------------- formatting

fn money(v: f64) -> String {
    let a = v.abs();
    let sign = if v < 0.0 { "-" } else { "" };
    if a >= 1e9 {
        format!("{sign}€{:.1}B", a / 1e9)
    } else if a >= 1e6 {
        format!("{sign}€{:.1}M", a / 1e6)
    } else if a >= 1e3 {
        format!("{sign}€{:.1}K", a / 1e3)
    } else {
        format!("{sign}€{:.0}", a)
    }
}

fn p1(v: f64) -> String {
    format!("{v:.1}%")
}

fn p2(v: f64) -> String {
    format!("{v:.2}%")
}

fn signed(v: f64, digits: usize) -> String {
    format!("{}{:.*}", if v > 0.0 { "+" } else { "" }, digits, v)
}

fn int(v: f64) -> String {
    let s = format!("{:.0}", v.abs());
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{}{}", if v < 0.0 { "-" } else { "" }, out.chars().rev().collect::<String>())
}

fn list_names(items: &[String], max: usize) -> String {
    match items.len() {
        0 => "none".into(),
        n if n <= max => items.join(", "),
        n => format!("{} and {} more", items[..max].join(", "), n - max),
    }
}

fn kpi(label: &str, value: String, hint: Option<String>, bad: bool) -> Kpi {
    Kpi { label: label.into(), value, hint, tone: bad.then(|| "bad".to_string()) }
}

fn to_value<T: Serialize>(v: &T) -> Result<Value> {
    Ok(serde_json::to_value(v)?)
}

fn parse<T: DeserializeOwned>(args: &Value) -> Result<T> {
    let args = if args.is_null() { json!({}) } else { args.clone() };
    serde_json::from_value(args).or_else(|e| bad(format!("invalid arguments: {e}")))
}

// ---------------------------------------------------------------- tool catalogue

pub fn tools() -> Vec<Tool> {
    let none = || json!({ "type": "object", "properties": {} });
    vec![
        Tool {
            name: "run_scenario",
            description: "What-if analysis. Apply percentage moves to commodity indices and/or currencies (against EUR) and report the extra annual material cost, margin before and after, profit centers, models and contracts affected. Use for questions like 'what if rubber rises 15%?'. Indices: NaturalRubber, Steel, Aluminium, Lithium, Copper, EnergyEU. Currencies: SEK, CZK, USD (positive = the currency gets stronger, so contracts invoiced in it cost more EUR).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "shocks": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "enum": ["commodity", "fx"] },
                                "target": { "type": "string", "description": "Index name such as NaturalRubber or Steel, or a currency code such as SEK" },
                                "percent": { "type": "number", "description": "+15 means up 15%, -10 means down 10%" }
                            },
                            "required": ["kind", "target", "percent"]
                        }
                    },
                    "margin_floor_percent": { "type": "number", "description": "Contribution margin floor to test models against. Default 15." }
                },
                "required": ["shocks"]
            }),
        },
        Tool {
            name: "currency_exposure",
            description: "How much annual material spend is invoiced in each currency, and the share in foreign (non-EUR) currencies.",
            input_schema: none(),
        },
        Tool {
            name: "commodity_sensitivity",
            description: "How sensitive each profit center (brand) is to commodity prices: extra annual cost for a given rise in every index, one index at a time and all together.",
            input_schema: json!({ "type": "object", "properties": { "bump_percent": { "type": "number", "description": "Size of the index rise. Default 10." } } }),
        },
        Tool {
            name: "discount_savings",
            description: "Contracts whose projected volume reaches a volume-discount tier, and the annual saving if the tier price is applied.",
            input_schema: none(),
        },
        Tool {
            name: "supplier_risk",
            description: "Which suppliers the business depends on most: revenue of the models that use their parts, lead time, single-sourced parts and alternative suppliers.",
            input_schema: json!({ "type": "object", "properties": { "limit": { "type": "integer", "description": "How many suppliers to show. Default 5." } } }),
        },
        Tool {
            name: "single_sourced_components",
            description: "Components that have a single supplier and no qualified alternative, ranked by the revenue that depends on them.",
            input_schema: json!({ "type": "object", "properties": { "limit": { "type": "integer", "description": "How many to show. Default 8." } } }),
        },
        Tool {
            name: "expiring_contracts",
            description: "Supply contracts ending within a number of months, flagged when projected volume exceeds the committed volume.",
            input_schema: json!({ "type": "object", "properties": { "months": { "type": "integer", "description": "Look-ahead window. Default 12." } } }),
        },
        Tool {
            name: "lead_time_impact",
            description: "What happens if a supplier's lead time is multiplied: models whose launch or deliveries slip, revenue delayed and high-intent customer leads affected.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "supplier": { "type": "string", "description": "Supplier IRI or short name, for example Supplier_BatterySystems. Use list_entities with kind 'suppliers' to find it." },
                    "factor": { "type": "number", "description": "Lead-time multiplier. 2 means doubled. Default 2." }
                },
                "required": ["supplier"]
            }),
        },
        Tool {
            name: "margin_trace",
            description: "Explain why a model's contribution margin changed between two months: which components, contracts, price clauses and indices drove it.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "model": { "type": "string", "description": "Model short name such as Golf, ID4, Enyaq, TGX. Use list_entities with kind 'models' to find it." },
                    "from": { "type": "string", "description": "Start month YYYY-MM. Default: six months before the latest month." },
                    "to": { "type": "string", "description": "End month YYYY-MM. Default: latest month." }
                },
                "required": ["model"]
            }),
        },
        Tool {
            name: "dashboard",
            description: "Headline position: planned revenue, contribution margin, material spend, exposure shares, contract counts and the biggest market movers.",
            input_schema: none(),
        },
        Tool {
            name: "subsidy_removal",
            description: "What if a market ends its EV purchase subsidy? Electric models sold there lose demand (elasticity times subsidy over price). Reports units, revenue and contribution lost per model and the high-intent leads in that market who want an affected model. Markets: Germany, France, Czech Republic, Sweden, United States.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "market": { "type": "string", "description": "Market name. Default Germany. Use list_entities with kind 'markets'." },
                    "elasticity": { "type": "number", "description": "Price elasticity of demand. Default 1.5." }
                }
            }),
        },
        Tool {
            name: "co2_fleet",
            description: "CO2 fleet target position by brand: average g/km of cars sold in the markets the target covers, penalty today and after the target is lowered. Also shows the result if all brands were pooled.",
            input_schema: json!({ "type": "object", "properties": { "tighten_percent": { "type": "number", "description": "How much the target is lowered, in percent. Default 10." } } }),
        },
        Tool {
            name: "competitor_price_cut",
            description: "What if a competitor cuts its prices? Our models are exposed when a rival of the same segment ends up priced below them; their volume falls by cross elasticity times the cut. Reports revenue and contribution at risk.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "competitor": { "type": "string", "description": "Competitor name such as Tesla, BYD, Mercedes, BMW, Toyota. Omit for all electric rivals." },
                    "cut_percent": { "type": "number", "description": "Size of the price cut. Default 10." },
                    "elasticity": { "type": "number", "description": "Cross elasticity of demand. Default 0.8." }
                }
            }),
        },
        Tool {
            name: "site_dependency",
            description: "Which brand group depends most on one production site: revenue (price times planned production) per group and site, with the share of the largest site.",
            input_schema: none(),
        },
        Tool {
            name: "powertrain_mix",
            description: "Contribution margin per powertrain (electric, petrol, diesel) with subsidy and CO2 exposure, and a what-if that moves a share of each brand's combustion cars to its electric models: contribution change and CO2 penalty change.",
            input_schema: json!({ "type": "object", "properties": { "shift_percent": { "type": "number", "description": "Share of combustion cars moved to electric. Default 10." } } }),
        },
        Tool {
            name: "lead_pressure",
            description: "Which high-intent customer leads want models whose margin is under pressure: margin within a buffer of the floor (or below it) or falling over a look-back window.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "floor_percent": { "type": "number", "description": "Margin floor. Default 15." },
                    "buffer_points": { "type": "number", "description": "How close to the floor counts as pressure. Default 1." },
                    "months": { "type": "integer", "description": "Look-back window for the margin change. Default 6." }
                }
            }),
        },
        Tool {
            name: "campaign_capacity",
            description: "Which incentive campaigns spend on models already at planned production capacity: extra cars the campaign sells beyond the model's headroom cannot be built, so that share of the spend is wasted.",
            input_schema: json!({ "type": "object", "properties": { "threshold_percent": { "type": "number", "description": "Utilisation that counts as at capacity. Default 95." } } }),
        },
        Tool {
            name: "financing_comparison",
            description: "Lease versus loan profitability for a model in a market: profit per contract after funding cost, default and residual risk and subvention. Only products defined in the knowledge graph are compared (Taycan in the Czech Republic).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "model": { "type": "string", "description": "Model short name. Default Taycan." },
                    "market": { "type": "string", "description": "Market name. Default Czech Republic (the Prague market)." }
                }
            }),
        },
        Tool {
            name: "combined_scenario",
            description: "A combined 'bad year': commodity and currency shocks together with the end of EV subsidies in one or more markets. Reports the profit change in total and per brand, the five models hit hardest and the contracts to renegotiate first. Use for questions like 'what does a bad year cost: rubber +15%, krona +5% and no German EV subsidy?'.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "shocks": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "kind": { "type": "string", "enum": ["commodity", "fx"] },
                                "target": { "type": "string", "description": "Index name such as NaturalRubber or a currency code such as SEK" },
                                "percent": { "type": "number" }
                            },
                            "required": ["kind", "target", "percent"]
                        }
                    },
                    "subsidy_markets": { "type": "array", "items": { "type": "string" }, "description": "Markets whose EV subsidy ends, for example Germany" },
                    "elasticity": { "type": "number", "description": "Price elasticity of demand for the subsidy loss. Default 1.5." }
                }
            }),
        },
        Tool {
            name: "explain_path",
            description: "Explain how two things are connected in the knowledge graph: the chain of facts between them (for example a commodity index and a model, a regulation and a model, a supplier and a model), with where each fact comes from (asserted in a data file, derived by a rule file, or an ERP number) and the key values of each entity. Use it when asked 'how does X affect Y', 'why is X exposed', or to back up another answer with its evidence.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Start entity: index (NaturalRubber), currency (SEK), supplier, regulation, model, ... Use list_entities to find names." },
                    "to": { "type": "string", "description": "End entity, for example a model such as Golf or a profit center." },
                    "max_paths": { "type": "integer", "description": "Alternative routes to show. Default 3." }
                },
                "required": ["from", "to"]
            }),
        },
        Tool {
            name: "list_entities",
            description: "List the names the other tools accept: models, suppliers, commodity indices or currencies.",
            input_schema: json!({
                "type": "object",
                "properties": { "kind": { "type": "string", "enum": ["models", "suppliers", "indices", "currencies", "markets", "competitors"] } },
                "required": ["kind"]
            }),
        },
    ]
}

// ---------------------------------------------------------------- tool arguments

#[derive(Deserialize)]
struct ScenarioArgs {
    shocks: Vec<ShockSpec>,
    margin_floor_percent: Option<f64>,
}
#[derive(Deserialize, Default)]
struct BumpArgs {
    bump_percent: Option<f64>,
}
#[derive(Deserialize, Default)]
struct LimitArgs {
    limit: Option<usize>,
}
#[derive(Deserialize, Default)]
struct ExpiringArgs {
    months: Option<u32>,
}
#[derive(Deserialize)]
struct LeadArgs {
    supplier: String,
    factor: Option<f64>,
}
#[derive(Deserialize)]
struct TraceArgs {
    model: String,
    from: Option<String>,
    to: Option<String>,
}
#[derive(Deserialize, Default)]
struct SubsidyArgs {
    market: Option<String>,
    elasticity: Option<f64>,
}
#[derive(Deserialize, Default)]
struct Co2Args {
    tighten_percent: Option<f64>,
}
#[derive(Deserialize, Default)]
struct CompetitorArgs {
    competitor: Option<String>,
    cut_percent: Option<f64>,
    elasticity: Option<f64>,
}
#[derive(Deserialize, Default)]
struct MixArgs {
    shift_percent: Option<f64>,
}
#[derive(Deserialize, Default)]
struct PressureArgs {
    floor_percent: Option<f64>,
    buffer_points: Option<f64>,
    months: Option<u32>,
}
#[derive(Deserialize, Default)]
struct CampaignArgs {
    threshold_percent: Option<f64>,
}
#[derive(Deserialize, Default)]
struct FinancingArgs {
    model: Option<String>,
    market: Option<String>,
}
#[derive(Deserialize)]
struct ExplainArgs {
    from: String,
    to: String,
    max_paths: Option<usize>,
}
#[derive(Deserialize)]
struct ListArgs {
    kind: String,
}

fn link(label: &str, to: &str) -> Block {
    Block::Link { label: label.into(), to: to.into(), shocks: None }
}

// ---------------------------------------------------------------- tool execution

/// Run a tool. Analyses also get a "show why" link to the reasoning graph; `explain_path` is the graph itself.
pub fn call_tool(kg: &Kg, snap: &Snapshot, md: &MarketData, sd: &SalesData, name: &str, args: &Value, today: &str) -> Result<ToolOutput> {
    if name == "explain_path" {
        return explain_tool(kg, args);
    }
    let mut out = call_tool_inner(snap, md, sd, name, args, today)?;
    let pairs = why_pairs(snap, md, name, &out.data);
    if !pairs.is_empty() {
        out.blocks.push(Block::Link { label: "Show why: the facts behind this".into(), to: graph::explain_link(&pairs), shocks: None });
    }
    Ok(out)
}

fn explain_tool(kg: &Kg, args: &Value) -> Result<ToolOutput> {
    let a: ExplainArgs = parse(args)?;
    let g = graph::explain(kg, &ExplainRequest { pairs: vec![PairReq { from: a.from, to: a.to }], max_depth: None, max_paths: a.max_paths })?;
    let p = &g.paths[0];
    let node = |id: &str| g.nodes.iter().find(|n| n.id == id);
    let mut summary = graph::describe(&g);
    if p.found {
        if let Some(n) = p.routes.first().and_then(|r| r.nodes.last()).and_then(|id| node(id)) {
            let facts: Vec<String> = n.properties.iter().take(6).map(|x| format!("{} {}", x.label, x.value)).collect();
            if !facts.is_empty() {
                summary += &format!("\nKey values of {}: {}.", n.label, facts.join(", "));
            }
        }
        summary += "\nLinks marked 'derived' come from rules; everything else is stated in the data files or comes from the ERP database.";
    }
    let mut blocks = Vec::new();
    if let Some(r) = p.routes.first() {
        let rows: Vec<Vec<String>> = r
            .edges
            .iter()
            .enumerate()
            .filter_map(|(i, eid)| {
                let e = g.edges.iter().find(|e| &e.id == eid)?;
                let (from, to) = (node(&r.nodes[i])?.label.clone(), node(&r.nodes[i + 1])?.label.clone());
                let link = if e.source == r.nodes[i] { e.label.clone() } else { format!("{} (reverse)", e.label) };
                let src = match e.origin.kind {
                    "inferred" => format!("derived by {}", e.origin.source),
                    "erp_fact" => "ERP number (SQLite)".to_string(),
                    _ => format!("asserted in {}", e.origin.source),
                };
                Some(vec![from, link, to, src])
            })
            .collect();
        if !rows.is_empty() {
            blocks.push(Block::Table { title: format!("{} to {}", p.from_label, p.to_label), columns: ["From", "Link", "To", "Where it comes from"].map(String::from).to_vec(), rows });
        }
    }
    blocks.push(Block::Link { label: "Open the reasoning graph".into(), to: graph::explain_link(&[(p.from.clone(), p.to.clone())]), shocks: None });
    Ok(ToolOutput { summary, blocks, data: to_value(&g)? })
}

/// Does a price move in `source` (an index or currency IRI) reach this model through its contracts?
fn reaches(snap: &Snapshot, model_iri: &str, kind: &str, source: &str) -> bool {
    snap.model_components.get(model_iri).into_iter().flatten().filter_map(|c| snap.contract_for(c)).any(|c| match kind {
        "fx" => c.currency == source,
        _ => c.clauses.iter().any(|cl| cl.index == source),
    })
}

fn pc_of(snap: &Snapshot, model_iri: &str) -> Option<String> {
    snap.models.iter().find(|m| m.iri == model_iri).map(|m| m.pc_iri.clone())
}

fn push_unique(pairs: &mut Vec<(String, String)>, a: String, b: String) {
    if !pairs.iter().any(|(x, y)| *x == a && *y == b) {
        pairs.push((a, b));
    }
}

/// The (start, end) pairs whose reasoning graph backs a tool's answer, read from the tool's JSON result.
fn why_pairs(snap: &Snapshot, md: &MarketData, name: &str, data: &Value) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let shock_pairs = |pairs: &mut Vec<(String, String)>, models: Vec<(String, String)>| {
        for sh in data["shocks"].as_array().into_iter().flatten().take(3) {
            let (kind, target) = (s(&sh["kind"]), s(&sh["target"]));
            let source = if kind == "fx" { snap.resolve_currency(&target).ok() } else { snap.resolve_index(&target).ok() };
            let Some(source) = source else { continue };
            if let Some((model, pc)) = models.iter().find(|(m, _)| reaches(snap, m, &kind, &source)) {
                push_unique(pairs, source, model.clone());
                push_unique(pairs, model.clone(), pc.clone());
            }
        }
    };
    match name {
        "run_scenario" => {
            let models = data["by_model"].as_array().into_iter().flatten().filter(|m| m["extra_cost"].as_f64().unwrap_or(0.0) > 0.0).filter_map(|m| Some((s(&m["iri"]), pc_of(snap, &s(&m["iri"]))?))).collect();
            shock_pairs(&mut pairs, models);
        }
        "combined_scenario" => {
            let top: Vec<&Value> = data["top_models"].as_array().into_iter().flatten().collect();
            let models = top.iter().filter_map(|m| Some((s(&m["iri"]), pc_of(snap, &s(&m["iri"]))?))).collect();
            shock_pairs(&mut pairs, models);
            let markets: Vec<String> = data["subsidy_markets"].as_array().into_iter().flatten().map(&s).collect();
            if let Some(m) = top.iter().find(|m| m["lost_units"].as_f64().unwrap_or(0.0) > 0.0) {
                for sub in md.subsidies.iter().filter(|x| md.markets.get(&x.market).is_some_and(|mk| markets.contains(&mk.label))) {
                    push_unique(&mut pairs, sub.iri.clone(), s(&m["iri"]));
                }
            }
        }
        "margin_trace" => {
            let model = s(&data["model"]);
            let mut drivers: Vec<(String, f64)> = data["components"].as_array().into_iter().flatten().flat_map(|c| c["drivers"].as_array().into_iter().flatten()).map(|d| (s(&d["index"]), d["delta_eur_per_unit"].as_f64().unwrap_or(0.0).abs())).collect();
            drivers.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (label, _) in drivers.iter().take(3) {
                if let Some(iri) = snap.index_labels.iter().find(|(_, l)| *l == label).map(|(i, _)| i.clone()) {
                    push_unique(&mut pairs, iri, model.clone());
                }
            }
            if let Some(pc) = pc_of(snap, &model) {
                push_unique(&mut pairs, model, pc);
            }
        }
        "lead_time_impact" => {
            for m in data["models"].as_array().into_iter().flatten().take(2) {
                push_unique(&mut pairs, s(&data["supplier"]), s(&m["iri"]));
            }
        }
        "supplier_risk" => {
            if let Some(first) = data.as_array().and_then(|a| a.first()) {
                for label in first["models"].as_array().into_iter().flatten().take(2) {
                    if let Some(m) = snap.models.iter().find(|m| m.label == s(label)) {
                        push_unique(&mut pairs, s(&first["iri"]), m.iri.clone());
                    }
                }
            }
        }
        "subsidy_removal" => {
            let label = s(&data["market"]);
            for sub in md.subsidies.iter().filter(|x| md.markets.get(&x.market).is_some_and(|mk| mk.label == label)) {
                for m in data["models"].as_array().into_iter().flatten().take(2) {
                    push_unique(&mut pairs, sub.iri.clone(), s(&m["iri"]));
                }
            }
        }
        "competitor_price_cut" => {
            for h in data["hits"].as_array().into_iter().flatten().filter(|h| h["undercut"].as_bool().unwrap_or(false)).take(2) {
                let model = snap.models.iter().find(|m| m.label == s(&h["model"]));
                let rival = md.rivals.values().find(|r| r.label == s(&h["rival"]));
                if let (Some(m), Some(r)) = (model, rival) {
                    push_unique(&mut pairs, m.iri.clone(), r.iri.clone());
                }
            }
        }
        _ => {}
    }
    pairs
}

fn call_tool_inner(snap: &Snapshot, md: &MarketData, sd: &SalesData, name: &str, args: &Value, today: &str) -> Result<ToolOutput> {
    match name {
        "run_scenario" => {
            let a: ScenarioArgs = parse(args)?;
            let req = ScenarioRequest { shocks: a.shocks, margin_floor: a.margin_floor_percent.map(|p| p / 100.0), save_as: None };
            let r = run_scenario(snap, &req)?;
            let t = &r.totals;
            let what = r.shocks.iter().map(|s| shock_text(snap, s)).collect::<Vec<_>>().join(" and ");
            let crossing: Vec<String> = r.by_model.iter().filter(|m| m.crossed_floor).map(|m| m.label.clone()).collect();
            let mut summary;
            if r.by_contract.is_empty() {
                summary = format!("No supply contract is priced off {what}, so nothing changes.");
            } else {
                summary = format!(
                    "{what} adds {} a year in material cost (prices as of {}). Group contribution margin goes from {} to {} ({} points).",
                    money(t.extra_cost), r.asof, p2(t.margin_before_pct), p2(t.margin_after_pct), signed(t.margin_after_pct - t.margin_before_pct, 2)
                );
                if crossing.is_empty() {
                    summary += &format!(" No model falls below the {:.0}% margin floor.", r.margin_floor_pct);
                } else {
                    summary += &format!(" {} model(s) fall below the {:.0}% margin floor: {}.", crossing.len(), r.margin_floor_pct, list_names(&crossing, 6));
                }
                if let Some(pc) = r.by_profit_center.first() {
                    summary += &format!(" Largest hit: {} ({}).", pc.label, money(pc.extra_cost));
                }
            }
            let mut blocks = vec![Block::Kpis {
                items: vec![
                    kpi("Extra material cost / year", money(t.extra_cost), Some(format!("on {} planned revenue", money(t.revenue))), t.extra_cost > 0.0),
                    kpi("Group margin", format!("{} to {}", p2(t.margin_before_pct), p2(t.margin_after_pct)), Some(format!("{} points", signed(t.margin_after_pct - t.margin_before_pct, 2))), false),
                    kpi(&format!("Models below {:.0}% margin", r.margin_floor_pct), t.models_below_floor.to_string(), Some(format!("{} because of this", t.models_crossing_floor)), t.models_crossing_floor > 0),
                ],
            }];
            if !r.by_contract.is_empty() {
                blocks.push(Block::Bars {
                    title: "Extra cost by profit center".into(),
                    unit: "€M".into(),
                    items: r.by_profit_center.iter().map(|p| BarItem { label: p.label.replace(" profit center", ""), value: (p.extra_cost / 1e4).round() / 100.0 }).collect(),
                });
                blocks.push(Block::Table {
                    title: "Models with the largest extra cost".into(),
                    columns: ["Model", "Extra cost / year", "Margin before", "Margin after"].map(String::from).to_vec(),
                    rows: r.by_model.iter().take(5).map(|m| vec![m.label.clone(), money(m.extra_cost), p2(m.margin_before_pct), p2(m.margin_after_pct)]).collect(),
                });
            }
            blocks.push(Block::Link { label: "Open in Scenario lab".into(), to: "/scenarios".into(), shocks: Some(r.shocks.clone()) });
            Ok(ToolOutput { summary, blocks, data: to_value(&r)? })
        }
        "currency_exposure" => {
            let e = questions::currency_exposure(snap)?;
            let foreign: Vec<String> = e.by_currency.iter().filter(|c| c.foreign).map(|c| c.code.clone()).collect();
            let summary = format!(
                "{} of annual material spend ({} of {}) is invoiced in {}. The rest is in EUR.",
                p1(e.foreign_share_pct),
                money(e.total_spend_eur * e.foreign_share_pct / 100.0),
                money(e.total_spend_eur),
                foreign.join(" and ")
            );
            let blocks = vec![
                Block::Kpis { items: vec![kpi("Foreign-currency share", p1(e.foreign_share_pct), Some(foreign.join(", ")), true), kpi("Annual material spend", money(e.total_spend_eur), None, false)] },
                Block::Table {
                    title: "Spend by invoice currency".into(),
                    columns: ["Currency", "Annual spend", "Share", "Contracts"].map(String::from).to_vec(),
                    rows: e.by_currency.iter().map(|c| vec![c.code.clone(), money(c.spend_eur), p1(c.share_pct), c.contracts.to_string()]).collect(),
                },
                link("Open Commodities and FX", "/markets"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&e)? })
        }
        "commodity_sensitivity" => {
            let a: BumpArgs = parse(args)?;
            let bump = a.bump_percent.unwrap_or(10.0);
            let s = questions::commodity_sensitivity(snap, bump)?;
            let (first, last) = (s.rows.first().unwrap(), s.rows.last().unwrap());
            let summary = format!(
                "If every commodity index rose {bump:.0}%, {} would be hit hardest ({} a year, {} of revenue) and {} least ({}).",
                first.label, money(first.all_indices_extra_cost), p2(first.all_indices_pct_of_revenue), last.label, p2(last.all_indices_pct_of_revenue)
            );
            let blocks = vec![
                Block::Table {
                    title: format!("Extra annual cost for +{bump:.0}% on every index"),
                    columns: ["Profit center", "Material on an index", "Extra cost", "% of revenue"].map(String::from).to_vec(),
                    rows: s.rows.iter().map(|r| vec![r.label.replace(" profit center", ""), format!("{:.0}%", r.linked_material_share_pct), money(r.all_indices_extra_cost), p2(r.all_indices_pct_of_revenue)]).collect(),
                },
                link("Open Commodities and FX", "/markets"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&s)? })
        }
        "discount_savings" => {
            let s = questions::discount_savings(snap);
            let summary = format!(
                "{} contracts have reached a volume-discount tier. Applying the tier prices would save {} a year (assuming the tier price covers every unit).",
                s.rows.len(), money(s.total_annual_saving)
            );
            let blocks = vec![
                Block::Kpis { items: vec![kpi("Annual saving", money(s.total_annual_saving), Some(format!("{} contracts", s.rows.len())), false)] },
                Block::Table {
                    title: "Largest savings".into(),
                    columns: ["Component", "Supplier", "Projected volume", "Saving / year"].map(String::from).to_vec(),
                    rows: s.rows.iter().take(6).map(|r| vec![r.component.clone(), r.supplier.clone(), int(r.projected), money(r.annual_saving)]).collect(),
                },
                link("Open Contracts", "/contracts"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&s)? })
        }
        "supplier_risk" => {
            let a: LimitArgs = parse(args)?;
            let all = questions::supplier_risk(snap)?;
            let top = &all[..a.limit.unwrap_or(5).clamp(1, all.len())];
            let first = &top[0];
            let summary = format!(
                "{} carries the most revenue: {} ({} of planned revenue) across {} models. {}.",
                first.label, money(first.revenue_at_risk), p1(first.revenue_share_pct), first.models.len(),
                if first.alternatives.is_empty() { "It has no qualified alternative supplier".to_string() } else { format!("Alternatives: {}", first.alternatives.join(", ")) }
            );
            let blocks = vec![
                Block::Table {
                    title: "Suppliers ranked by revenue that depends on them".into(),
                    columns: ["Supplier", "Revenue", "Share", "Models", "Single-sourced parts", "Lead time"].map(String::from).to_vec(),
                    rows: top.iter().map(|r| vec![r.label.clone(), money(r.revenue_at_risk), p1(r.revenue_share_pct), r.models.len().to_string(), r.single_sourced_components.to_string(), format!("{} d", r.lead_time_days)]).collect(),
                },
                link("Open Suppliers", "/suppliers"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&all)? })
        }
        "single_sourced_components" => {
            let a: LimitArgs = parse(args)?;
            let all = questions::single_sourced(snap);
            let limit = a.limit.unwrap_or(8);
            let summary = match all.first() {
                Some(f) => format!(
                    "{} components have a single supplier and no qualified alternative. The largest dependency is {} from {}, which carries {} of revenue ({}).",
                    all.len(), f.label, f.supplier, money(f.revenue_dependent), list_names(&f.models, 4)
                ),
                None => "Every component has an alternative supplier.".into(),
            };
            let blocks = vec![
                Block::Kpis { items: vec![kpi("Single-sourced components", all.len().to_string(), Some("no qualified alternative".into()), !all.is_empty())] },
                Block::Table {
                    title: "Largest single-source dependencies".into(),
                    columns: ["Component", "Only supplier", "Revenue", "Lead time", "Contract ends"].map(String::from).to_vec(),
                    rows: all.iter().take(limit).map(|r| vec![r.label.clone(), r.supplier.clone(), money(r.revenue_dependent), format!("{} d", r.lead_time_days), r.contract_end.clone()]).collect(),
                },
                link("Open Suppliers", "/suppliers"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&all)? })
        }
        "expiring_contracts" => {
            let a: ExpiringArgs = parse(args)?;
            let months = a.months.unwrap_or(12);
            let e = questions::expiring(snap, today, months)?;
            let under = e.contracts.iter().filter(|c| c.under_committed).count();
            let summary = if e.contracts.is_empty() {
                format!("No contract ends within {months} months (by {}).", e.cutoff)
            } else {
                format!("{} contract(s) end within {months} months (by {}). {under} of them are also projected above their committed volume, so volume needs renegotiating.", e.contracts.len(), e.cutoff)
            };
            let blocks = vec![
                Block::Kpis { items: vec![kpi("Expiring", e.contracts.len().to_string(), Some(format!("by {}", e.cutoff)), false), kpi("Also under-committed", under.to_string(), None, under > 0)] },
                Block::Table {
                    title: "Contracts by end date".into(),
                    columns: ["Component", "Supplier", "Ends", "Days left", "Annual spend", "Volume"].map(String::from).to_vec(),
                    rows: e.contracts.iter().map(|c| vec![c.component.clone(), c.supplier.clone(), c.end.clone(), c.days_left.to_string(), money(c.annual_spend), if c.under_committed { "over commitment".into() } else { "ok".into() }]).collect(),
                },
                link("Open Contracts", "/contracts"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&e)? })
        }
        "lead_time_impact" => {
            let a: LeadArgs = parse(args)?;
            let factor = a.factor.unwrap_or(2.0);
            let r = questions::lead_time_impact(snap, &a.supplier, factor)?;
            let launches: Vec<String> = r.models.iter().filter_map(|m| Some(format!("{} {} to {}", m.label, m.planned_launch.as_ref()?, m.new_launch.as_ref()?))).collect();
            let summary = format!(
                "Multiplying {}'s lead time by {factor} ({} to {} days) delays {} model(s) by {} days, putting up to {} of revenue back. {} high-intent lead(s) are affected.{}",
                r.label, r.lead_time_days, r.new_lead_time_days, r.models.len(), r.delay_days, money(r.total_revenue_at_risk), r.leads_affected,
                if launches.is_empty() { String::new() } else { format!(" Launches move: {}.", launches.join("; ")) }
            );
            let blocks = vec![
                Block::Kpis { items: vec![
                    kpi("Delay", format!("{} days", r.delay_days), Some(format!("{} to {} days lead time", r.lead_time_days, r.new_lead_time_days)), true),
                    kpi("Revenue delayed (upper bound)", money(r.total_revenue_at_risk), None, true),
                    kpi("High-intent leads affected", r.leads_affected.to_string(), None, false),
                ] },
                Block::Table {
                    title: "Models affected".into(),
                    columns: ["Model", "Planned launch", "New date", "Revenue delayed", "Leads"].map(String::from).to_vec(),
                    rows: r.models.iter().map(|m| vec![m.label.clone(), m.planned_launch.clone().unwrap_or_else(|| "none planned".into()), m.new_launch.clone().unwrap_or_default(), money(m.revenue_at_risk), m.high_intent_leads.len().to_string()]).collect(),
                },
                link("Open Suppliers", "/suppliers"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&r)? })
        }
        "margin_trace" => {
            let a: TraceArgs = parse(args)?;
            let t = questions::margin_trace(snap, &a.model, a.from.as_deref(), a.to.as_deref())?;
            let top = t.components.first();
            let summary = format!(
                "{}'s contribution margin moved from {} in {} to {} in {} ({} points), because material cost per car changed by {} EUR on a price of {}.{}",
                t.label, p2(t.margin_from_pct), t.from, p2(t.margin_to_pct), t.to, signed(t.change_pp, 2), signed(t.material_change_eur_per_unit, 1), money(t.price),
                match top {
                    Some(c) => {
                        let d = c.drivers.first().filter(|d| d.delta_eur_per_unit.abs() >= 0.05);
                        format!(" Biggest change: {} ({} EUR){}.", c.component, signed(c.delta_eur_per_unit, 1), d.map(|d| format!(", mostly {} ({} EUR)", d.index, signed(d.delta_eur_per_unit, 1))).unwrap_or_default())
                    }
                    None => String::new(),
                }
            );
            let blocks = vec![
                Block::Kpis { items: vec![
                    kpi(&format!("Margin {}", t.from), p2(t.margin_from_pct), None, false),
                    kpi(&format!("Margin {}", t.to), p2(t.margin_to_pct), Some(format!("{} points", signed(t.change_pp, 2))), t.change_pp < 0.0),
                ] },
                Block::Bars {
                    title: "Cost change per car by component".into(),
                    unit: "EUR".into(),
                    items: t.components.iter().take(6).map(|c| BarItem { label: c.component.split(" (").next().unwrap_or(&c.component).to_string(), value: (c.delta_eur_per_unit * 10.0).round() / 10.0 }).collect(),
                },
                Block::Table {
                    title: "What drove it".into(),
                    columns: ["Component", "Change per car", "Main drivers"].map(String::from).to_vec(),
                    rows: t.components.iter().take(5).map(|c| {
                        let mut parts: Vec<String> = c.drivers.iter().filter(|d| d.delta_eur_per_unit.abs() >= 0.05).take(2).map(|d| format!("{} {}", d.index, signed(d.delta_eur_per_unit, 1))).collect();
                        if c.fx_effect_eur_per_unit.abs() >= 0.05 {
                            parts.push(format!("{} rate {}", c.currency, signed(c.fx_effect_eur_per_unit, 1)));
                        }
                        vec![c.component.clone(), format!("{} EUR", signed(c.delta_eur_per_unit, 1)), parts.join(", ")]
                    }).collect(),
                },
                link("Open in Explain a margin", &format!("/explain/{}", short(&t.model))),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&t)? })
        }
        "dashboard" => {
            let d = questions::dashboard(snap, today)?;
            let mover = d.movers.first();
            let summary = format!(
                "As of {}: planned revenue {}, contribution margin {}, material spend {} ({} on a commodity index, {} in foreign currency). {} contracts, {} expiring within a year, {} single-sourced components.{}",
                d.asof, money(d.planned_revenue), p1(d.contribution_margin_pct), money(d.material_spend), p1(d.commodity_linked_share_pct), p1(d.foreign_currency_share_pct),
                d.contracts, d.contracts_expiring_12m, d.single_sourced_components,
                mover.map(|m| format!(" Biggest 6-month mover: {} ({}%).", m.label, signed(m.change_6m_pct, 1))).unwrap_or_default()
            );
            let blocks = vec![
                Block::Kpis { items: vec![
                    kpi("Planned revenue", money(d.planned_revenue), None, false),
                    kpi("Contribution margin", p1(d.contribution_margin_pct), None, false),
                    kpi("Material spend", money(d.material_spend), Some(format!("{} on an index", p1(d.commodity_linked_share_pct))), false),
                    kpi("Foreign-currency share", p1(d.foreign_currency_share_pct), None, false),
                ] },
                Block::Bars { title: "Contribution margin by profit center".into(), unit: "%".into(), items: d.margin_by_profit_center.iter().map(|(l, v)| BarItem { label: l.replace(" profit center", ""), value: (v * 10.0).round() / 10.0 }).collect() },
                link("Open Dashboard", "/"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&d)? })
        }
        "subsidy_removal" => {
            let a: SubsidyArgs = parse(args)?;
            let r = market::subsidy_removal(snap, md, a.market.as_deref().unwrap_or("Germany"), a.elasticity.unwrap_or(market::DEFAULT_SUBSIDY_ELASTICITY))?;
            let summary = match r.models.first() {
                None => format!("No EV subsidy applies in {}, so nothing changes.", r.market),
                Some(top) => {
                    let leads: Vec<String> = r.leads.iter().map(|l| format!("{} ({})", l.customer.replace("Synthetic customer ", ""), l.model)).collect();
                    format!(
                        "Ending the subsidy in {} (elasticity {}) cuts demand for {} model(s): about {} cars, {} revenue and {} contribution a year. Biggest hit: {} ({}). {} high-intent lead(s) affected{}.",
                        r.market, r.elasticity, r.models.len(), int(r.lost_units), money(r.lost_revenue), money(r.lost_contribution),
                        top.label, money(top.lost_revenue), r.leads.len(), if leads.is_empty() { String::new() } else { format!(": {}", leads.join("; ")) }
                    )
                }
            };
            let mut blocks = vec![Block::Kpis { items: vec![
                kpi("Cars lost / year", int(r.lost_units), Some(format!("in {}", r.market)), r.lost_units > 0.0),
                kpi("Revenue lost", money(r.lost_revenue), None, r.lost_revenue > 0.0),
                kpi("Contribution lost", money(r.lost_contribution), None, r.lost_contribution > 0.0),
            ] }];
            if !r.models.is_empty() {
                blocks.push(Block::Bars { title: "Revenue lost by model".into(), unit: "€M".into(), items: r.models.iter().map(|m| BarItem { label: m.label.clone(), value: (m.lost_revenue / 1e4).round() / 100.0 }).collect() });
                blocks.push(Block::Table {
                    title: "Affected models".into(),
                    columns: ["Model", "Subsidy", "Demand lost", "Cars lost", "Revenue lost"].map(String::from).to_vec(),
                    rows: r.models.iter().map(|m| vec![m.label.clone(), money(m.subsidy_eur), p1(m.demand_loss_pct), int(m.lost_units), money(m.lost_revenue)]).collect(),
                });
            }
            blocks.push(link("Open Markets and regulation", "/strategy"));
            Ok(ToolOutput { summary, blocks, data: to_value(&r)? })
        }
        "co2_fleet" => {
            let a: Co2Args = parse(args)?;
            let tighten = a.tighten_percent.unwrap_or(10.0);
            let r = market::co2_fleet(snap, md, tighten)?;
            let today_breach: Vec<String> = r.brands.iter().filter(|b| b.penalty_now > 0.0).map(|b| b.label.clone()).collect();
            let new_breach: Vec<String> = r.brands.iter().filter(|b| b.breaches_new).map(|b| b.label.clone()).collect();
            let total_new: f64 = r.brands.iter().map(|b| b.penalty_new).sum();
            let summary = format!(
                "Lowering the CO2 target {:.0}% (from {:.0} to {:.1} g/km) leaves {} brand(s) in breach: {}. Brand penalties total {} a year (EUR {} per g/km per car); today's breaches: {}. Pooled as one fleet the group's penalty would be {}.",
                tighten, r.target_now, r.target_new, new_breach.len(), list_names(&new_breach, 6), money(total_new), r.penalty_per_gram, list_names(&today_breach, 6), money(r.pooled.penalty_new)
            );
            let blocks = vec![
                Block::Kpis { items: vec![
                    kpi("Brands in breach today", today_breach.len().to_string(), Some(format!("target {:.0} g/km", r.target_now)), !today_breach.is_empty()),
                    kpi("Brands in breach at new target", new_breach.len().to_string(), Some(format!("target {:.1} g/km", r.target_new)), !new_breach.is_empty()),
                    kpi("Penalty at new target", money(total_new), Some(format!("pooled: {}", money(r.pooled.penalty_new))), total_new > 0.0),
                ] },
                Block::Table {
                    title: "CO2 fleet by brand".into(),
                    columns: ["Brand", "Fleet g/km", "Penalty today", "Penalty at new target"].map(String::from).to_vec(),
                    rows: r.brands.iter().map(|b| vec![b.label.clone(), format!("{:.1}", b.avg_g_per_km), money(b.penalty_now), money(b.penalty_new)]).collect(),
                },
                link("Open Markets and regulation", "/strategy"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&r)? })
        }
        "competitor_price_cut" => {
            let a: CompetitorArgs = parse(args)?;
            let cut = a.cut_percent.unwrap_or(10.0);
            let r = market::competitor_cut(snap, md, a.competitor.as_deref(), cut, a.elasticity.unwrap_or(market::DEFAULT_CROSS_ELASTICITY))?;
            let mut hit: Vec<String> = r.hits.iter().filter(|h| h.undercut).map(|h| h.model.clone()).collect();
            hit.sort();
            hit.dedup();
            let summary = if hit.is_empty() {
                format!("If {} cut prices by {}%, none of our models ends up priced above a rival, so nothing is at risk.", r.competitor, cut)
            } else {
                format!(
                    "If {} cut prices by {}%, {} of our models would be undercut: {}. That puts {} of revenue and {} of contribution at risk (cross elasticity {}).",
                    r.competitor, cut, hit.len(), list_names(&hit, 6), money(r.revenue_at_risk), money(r.contribution_at_risk), r.elasticity
                )
            };
            let blocks = vec![
                Block::Kpis { items: vec![
                    kpi("Revenue exposed", money(r.revenue_exposed), Some(format!("{} model(s) undercut", hit.len())), r.revenue_exposed > 0.0),
                    kpi("Revenue at risk", money(r.revenue_at_risk), None, r.revenue_at_risk > 0.0),
                    kpi("Contribution at risk", money(r.contribution_at_risk), None, r.contribution_at_risk > 0.0),
                ] },
                Block::Table {
                    title: "Our models against the rivals".into(),
                    columns: ["Model", "Rival", "Our price", "Rival after cut", "Undercut", "Revenue at risk"].map(String::from).to_vec(),
                    rows: r.hits.iter().map(|h| vec![h.model.clone(), h.rival.clone(), money(h.our_price), money(h.rival_price_after), if h.undercut { "yes".into() } else { "no".into() }, money(h.revenue_at_risk)]).collect(),
                },
                link("Open Markets and regulation", "/strategy"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&r)? })
        }
        "site_dependency" => {
            let groups = market::site_dependency(snap, md);
            let summary = match groups.first() {
                Some(g) => format!(
                    "{} depends most on one site: {} of its {} revenue is built at {}.{}",
                    g.group, p1(g.top_share_pct), money(g.revenue), g.top_site,
                    groups.get(1).map(|n| format!(" Next: {} at {} ({}).", n.group, n.top_site, p1(n.top_share_pct))).unwrap_or_default()
                ),
                None => "No brand group has production sites in the knowledge graph.".into(),
            };
            let blocks = vec![
                Block::Bars { title: "Share of group revenue at its largest site".into(), unit: "%".into(), items: groups.iter().map(|g| BarItem { label: g.group.clone(), value: (g.top_share_pct * 10.0).round() / 10.0 }).collect() },
                Block::Table {
                    title: "Revenue by group and site".into(),
                    columns: ["Group", "Site", "Brand", "Revenue", "Share"].map(String::from).to_vec(),
                    rows: groups.iter().flat_map(|g| g.sites.iter().map(move |x| vec![g.group.clone(), x.site.clone(), x.brand.clone(), money(x.revenue), p1(x.share_pct)])).collect(),
                },
                link("Open Markets and regulation", "/strategy"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&groups)? })
        }
        "powertrain_mix" => {
            let a: MixArgs = parse(args)?;
            let shift = a.shift_percent.unwrap_or(10.0);
            let r = market::mix_analysis(snap, md, shift)?;
            let margins = r.powertrains.iter().map(|p| format!("{} {}", p.powertrain, p1(p.margin_pct))).collect::<Vec<_>>().join(", ");
            let change: f64 = r.shifts.iter().map(|x| x.contribution_change).sum();
            let net: f64 = r.shifts.iter().map(|x| x.net_change).sum();
            let best = r.shifts.first().map(|x| format!(" Best for {} ({}).", x.brand, money(x.net_change))).unwrap_or_default();
            let summary = format!(
                "Contribution margin by powertrain: {margins}. Moving {}% of combustion cars to electric changes contribution by {} and, after CO2 penalties, nets {} across {} brand(s).{best}",
                shift, money(change), money(net), r.shifts.len()
            );
            let blocks = vec![
                Block::Kpis { items: vec![
                    kpi("Contribution change", money(change), Some(format!("{shift}% of combustion cars moved")), change < 0.0),
                    kpi("Net of CO2 penalties", money(net), None, net < 0.0),
                ] },
                Block::Table {
                    title: "Margin by powertrain".into(),
                    columns: ["Powertrain", "Models", "Units", "Margin", "Per unit"].map(String::from).to_vec(),
                    rows: r.powertrains.iter().map(|p| vec![p.powertrain.clone(), p.models.to_string(), int(p.units), p1(p.margin_pct), money(p.contribution_per_unit)]).collect(),
                },
                Block::Table {
                    title: "Shift by brand".into(),
                    columns: ["Brand", "Cars moved", "Contribution", "CO2 penalty change", "Net"].map(String::from).to_vec(),
                    rows: r.shifts.iter().map(|x| vec![x.brand.clone(), int(x.moved_units), money(x.contribution_change), money(x.co2_penalty_after - x.co2_penalty_before), money(x.net_change)]).collect(),
                },
                link("Open Markets and regulation", "/strategy"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&r)? })
        }
        "lead_pressure" => {
            let a: PressureArgs = parse(args)?;
            let r = sales::lead_pressure(snap, md, a.floor_percent.unwrap_or(15.0), a.buffer_points.unwrap_or(1.0), a.months.unwrap_or(6))?;
            let hot: Vec<String> = r.leads.iter().filter(|l| l.pressure).map(|l| format!("{} wants {} (margin {})", l.customer.replace("Synthetic customer ", ""), l.model, p1(l.margin_pct))).collect();
            let summary = if r.leads.is_empty() {
                "There are no high-intent leads.".to_string()
            } else if hot.is_empty() {
                format!("{} high-intent lead(s), none on a model under margin pressure (floor {:.0}%, buffer {} points).", r.leads.len(), r.floor_pct, r.buffer_pp)
            } else {
                format!("{} of {} high-intent lead(s) want a model under margin pressure (floor {:.0}%, buffer {} points, {} months to {}): {}.", r.pressured_count, r.leads.len(), r.floor_pct, r.buffer_pp, r.months, r.asof, hot.join("; "))
            };
            let blocks = vec![
                Block::Kpis { items: vec![kpi("High-intent leads", r.leads.len().to_string(), None, false), kpi("On pressured models", r.pressured_count.to_string(), Some(format!("floor {:.0}%", r.floor_pct)), r.pressured_count > 0)] },
                Block::Table {
                    title: "Leads and the margin of the model they want".into(),
                    columns: ["Customer", "Model", "Margin", "Change", "Pressure"].map(String::from).to_vec(),
                    rows: r.leads.iter().map(|l| vec![l.customer.replace("Synthetic customer ", ""), l.model.clone(), p1(l.margin_pct), format!("{} pts", signed(l.margin_change_pp, 2)), if l.pressure { "yes".into() } else { "no".into() }]).collect(),
                },
                link("Open Sales and leads", "/sales"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&r)? })
        }
        "campaign_capacity" => {
            let a: CampaignArgs = parse(args)?;
            let r = sales::campaign_capacity(snap, sd, a.threshold_percent.unwrap_or(95.0))?;
            let full: Vec<String> = r.campaigns.iter().filter(|c| c.at_capacity).map(|c| c.label.clone()).collect();
            let share = if r.total_spend > 0.0 { r.wasted_spend / r.total_spend * 100.0 } else { 0.0 };
            let summary = format!(
                "{} of {} campaign spend ({}) goes to extra cars that cannot be built. {} campaign(s) target models at or above {:.0}% utilisation: {}.",
                money(r.wasted_spend), money(r.total_spend), p1(share), full.len(), r.threshold_pct, list_names(&full, 6)
            );
            let blocks = vec![
                Block::Kpis { items: vec![kpi("Campaign spend", money(r.total_spend), None, false), kpi("Wasted on full models", money(r.wasted_spend), Some(p1(share)), r.wasted_spend > 0.0)] },
                Block::Bars { title: "Wasted spend by campaign".into(), unit: "€M".into(), items: r.campaigns.iter().filter(|c| c.wasted_spend > 0.0).map(|c| BarItem { label: c.label.clone(), value: (c.wasted_spend / 1e4).round() / 100.0 }).collect() },
                Block::Table {
                    title: "Campaigns against capacity".into(),
                    columns: ["Campaign", "Spend", "Utilisation", "Headroom", "At capacity", "Wasted"].map(String::from).to_vec(),
                    rows: r.campaigns.iter().map(|c| vec![c.label.clone(), money(c.spend), p1(c.utilisation_pct), int(c.headroom_units), if c.at_capacity { "yes".into() } else { "no".into() }, money(c.wasted_spend)]).collect(),
                },
                link("Open Sales and leads", "/sales"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&r)? })
        }
        "financing_comparison" => {
            let a: FinancingArgs = parse(args)?;
            let r = sales::financing_comparison(snap, md, sd, a.model.as_deref().unwrap_or("Taycan"), a.market.as_deref().unwrap_or("Czech Republic"))?;
            let top = &r.products[0];
            let other = if r.best_kind == "Loan" { "lease" } else { "loan" };
            let summary = format!(
                "For the {} in {}, {} earns the most: {} per contract ({} of the {} price), {} more than the best {other}.",
                r.model, r.market, r.best, money(top.profit), p1(top.profit_pct_of_price), money(r.price), money(r.kind_gap)
            );
            let blocks = vec![
                Block::Kpis { items: vec![kpi("Best product", r.best.clone(), Some(format!("{} per contract", money(top.profit))), false), kpi(&format!("Lead over best {other}"), money(r.kind_gap), None, false)] },
                Block::Table {
                    title: "Profit per contract".into(),
                    columns: ["Product", "Rate", "Income", "Funding", "Risk", "Profit"].map(String::from).to_vec(),
                    rows: r.products.iter().map(|p| vec![p.label.clone(), p1(p.rate_pct), money(p.income), money(p.funding_cost), money(p.risk_cost), money(p.profit)]).collect(),
                },
                link("Open Sales and leads", "/sales"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&r)? })
        }
        "combined_scenario" => {
            let req: CombinedRequest = parse(args)?;
            let r = combined::run_combined(snap, md, &req)?;
            let what: Vec<String> = r.shocks.iter().map(|s| shock_text(snap, s)).chain(r.subsidy_markets.iter().map(|m| format!("no EV subsidy in {m}"))).collect();
            let t = &r.totals;
            let worst = r.by_brand.first().map(|b| format!(" Hardest hit brand: {} ({}).", b.label, money(b.profit_change))).unwrap_or_default();
            let models: Vec<String> = r.top_models.iter().map(|m| format!("{} ({})", m.label, money(m.profit_change))).collect();
            let first = r.contracts.first().map(|c| format!(" Renegotiate first: {} ({} a year, ends {}).", c.label.replace("Illustrative supply contract for ", ""), money(c.extra_cost), c.end)).unwrap_or_default();
            let summary = format!(
                "{}: profit changes by {} ({} of contribution): {} extra material cost and {} contribution lost on {} cars.{worst} Top models: {}.{first}",
                what.join(", "), money(t.profit_change), p1(t.profit_change_pct), money(t.extra_material_cost), money(t.lost_contribution), int(t.lost_units), list_names(&models, 5)
            );
            let blocks = vec![
                Block::Kpis { items: vec![
                    kpi("Profit change", money(t.profit_change), Some(format!("{} of contribution", p1(t.profit_change_pct))), t.profit_change < 0.0),
                    kpi("Extra material cost", money(t.extra_material_cost), None, t.extra_material_cost > 0.0),
                    kpi("Contribution lost to demand", money(t.lost_contribution), Some(format!("{} cars", int(t.lost_units))), t.lost_contribution > 0.0),
                ] },
                Block::Bars { title: "Profit lost by brand".into(), unit: "€M".into(), items: r.by_brand.iter().map(|b| BarItem { label: b.label.clone(), value: (-b.profit_change / 1e4).round() / 100.0 }).collect() },
                Block::Table {
                    title: "Top 5 models hit".into(),
                    columns: ["Model", "Extra cost", "Cars lost", "Profit change", "Margin after"].map(String::from).to_vec(),
                    rows: r.top_models.iter().map(|m| vec![m.label.clone(), money(m.extra_material_cost), int(m.lost_units), money(m.profit_change), p1(m.margin_after_pct)]).collect(),
                },
                Block::Table {
                    title: "Contracts to renegotiate first".into(),
                    columns: ["#", "Contract", "Supplier", "Extra cost / year", "Ends"].map(String::from).to_vec(),
                    rows: r.contracts.iter().take(5).map(|c| vec![c.priority.to_string(), c.label.replace("Illustrative supply contract for ", ""), c.supplier.replace("Illustrative ", ""), money(c.extra_cost), c.end.clone()]).collect(),
                },
                link("Open Bad year", "/bad-year"),
            ];
            Ok(ToolOutput { summary, blocks, data: to_value(&r)? })
        }
        "list_entities" => {
            let a: ListArgs = parse(args)?;
            let (title, columns, rows): (&str, Vec<&str>, Vec<Vec<String>>) = match a.kind.as_str() {
                "models" => ("Models", vec!["Name for tools", "Model", "Brand"], snap.models.iter().map(|m| vec![short(&m.iri), m.label.clone(), m.brand_label.clone()]).collect()),
                "suppliers" => {
                    let mut v: Vec<_> = snap.suppliers.values().filter(|s| snap.contracts.iter().any(|c| c.supplier == s.iri)).collect();
                    v.sort_by(|a, b| a.label.cmp(&b.label));
                    ("Suppliers", vec!["Name for tools", "Supplier", "Country"], v.iter().map(|s| vec![short(&s.iri), s.label.clone(), s.country.clone()]).collect())
                }
                "indices" => ("Commodity indices", vec!["Name for tools", "Index"], snap.index_labels.iter().map(|(i, l)| vec![short(i), l.clone()]).collect()),
                "currencies" => ("Currencies", vec!["Name for tools", "Currency"], snap.currency_codes.iter().map(|(i, c)| vec![short(i), c.clone()]).collect()),
                "markets" => ("Markets", vec!["Name for tools", "Market"], md.markets.values().map(|m| vec![m.country.clone(), m.label.clone()]).collect()),
                "competitors" => {
                    let names: std::collections::BTreeSet<_> = md.rivals.values().map(|r| r.competitor.clone()).collect();
                    ("Competitors", vec!["Name for tools", "Competitor"], names.into_iter().map(|n| vec![n.clone(), n]).collect())
                }
                other => return bad(format!("unknown kind '{other}', expected models, suppliers, indices, currencies, markets or competitors")),
            };
            let summary = format!("{} {} are known: {}.", rows.len(), a.kind, list_names(&rows.iter().map(|r| r[0].clone()).collect::<Vec<_>>(), 20));
            let blocks = vec![Block::Table { title: title.into(), columns: columns.into_iter().map(String::from).collect(), rows: rows.clone() }];
            Ok(ToolOutput { summary, blocks, data: json!(rows) })
        }
        other => bad(format!("unknown tool '{other}'")),
    }
}

fn shock_text(snap: &Snapshot, s: &ShockSpec) -> String {
    let target = if s.kind == "fx" {
        snap.resolve_currency(&s.target).ok().and_then(|i| snap.currency_codes.get(&i).cloned()).unwrap_or_else(|| s.target.clone())
    } else {
        snap.resolve_index(&s.target).map(|i| snap.index_label(&i)).unwrap_or_else(|_| s.target.clone())
    };
    format!("{} {}%", target, signed(s.percent, 0).trim_end_matches(".0"))
}

// ---------------------------------------------------------------- rule-based planner

const UP: &[&str] = &["up", "rise", "rises", "rising", "risen", "increase", "increases", "increased", "jump", "jumps", "hike", "surge", "surges", "higher", "strengthen", "strengthens", "stronger", "climb", "climbs", "spike", "spikes"];
const DOWN: &[&str] = &["down", "fall", "falls", "falling", "drop", "drops", "decrease", "decreases", "decline", "declines", "lower", "cheaper", "weaken", "weakens", "weaker", "crash", "crashes", "dip", "dips"];

const INDEX_WORDS: &[(&str, &str)] = &[
    ("rubber", "NaturalRubber"), ("steel", "Steel"), ("aluminium", "Aluminium"), ("aluminum", "Aluminium"),
    ("lithium", "Lithium"), ("copper", "Copper"), ("energy", "EnergyEU"), ("electricity", "EnergyEU"),
];
const CURRENCY_WORDS: &[(&str, &str)] = &[
    ("krona", "SEK"), ("kronor", "SEK"), ("sek", "SEK"), ("swedish", "SEK"),
    ("koruna", "CZK"), ("czk", "CZK"), ("czech", "CZK"), ("dollar", "USD"), ("usd", "USD"),
];
/// keyword in a supplier label -> unique part of its name
const SUPPLIER_WORDS: &[(&str, &str)] = &[
    ("battery", "Battery"), ("brake", "Brake"), ("tyre", "Tire"), ("tire", "Tire"), ("transmission", "Transmission"),
    ("infotainment", "Infotainment"), ("electric drive", "Electric Drive"), ("powertrain", "Powertrain"),
    ("heavy vehicle", "Heavy Vehicle"), ("northstar", "Northstar"), ("apex", "Apex"), ("roadstead", "Roadstead"),
    ("performance components", "Performance Components"),
];

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// Lowercase and drop dots and dashes so "ID.4", "e-tron" and "R-series" become single tokens.
fn normalise(text: &str) -> String {
    text.to_lowercase().chars().filter(|c| *c != '.' && *c != '-').collect()
}

struct Mention {
    pos: usize,
    kind: &'static str,
    target: String,
}

fn find_mentions(lower: &str) -> Vec<Mention> {
    let mut out = Vec::new();
    for (word, target) in INDEX_WORDS {
        if let Some(pos) = lower.find(word).filter(|_| !out.iter().any(|m: &Mention| m.target == *target)) {
            out.push(Mention { pos, kind: "commodity", target: target.to_string() });
        }
    }
    for (word, target) in CURRENCY_WORDS {
        let hit = if word.len() <= 3 { words(lower).iter().any(|w| w == word).then(|| lower.find(word)).flatten() } else { lower.find(word) };
        if let Some(pos) = hit.filter(|_| !out.iter().any(|m: &Mention| m.target == *target)) {
            out.push(Mention { pos, kind: "fx", target: target.to_string() });
        }
    }
    out.sort_by_key(|m| m.pos);
    out
}

/// (position, magnitude) of every "12%" or "12 percent" in the text.
fn find_percents(lower: &str) -> Vec<(usize, f64)> {
    let bytes: Vec<char> = lower.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == '.') {
                i += 1;
            }
            let number: String = bytes[start..i].iter().collect();
            let rest: String = bytes[i..].iter().take(9).collect();
            let rest = rest.trim_start();
            let is_percent = rest.starts_with('%') || rest.starts_with("percent") || rest.starts_with("per cent");
            if let (true, Ok(v)) = (is_percent, number.trim_end_matches('.').parse::<f64>()) {
                out.push((start, v));
            }
        } else {
            i += 1;
        }
    }
    out
}

fn direction(window: &str) -> f64 {
    let ws = words(window);
    // the word closest to the end of the window decides
    for w in ws.iter().rev() {
        if DOWN.contains(&w.as_str()) {
            return -1.0;
        }
        if UP.contains(&w.as_str()) {
            return 1.0;
        }
    }
    1.0
}

fn plan_shocks(lower: &str) -> (Vec<ShockSpec>, bool) {
    let mentions = find_mentions(lower);
    let percents = find_percents(lower);
    let mut assumed = false;
    let mut shocks = Vec::new();
    for m in &mentions {
        // nearest percentage, preferring one after the mention
        let chosen = percents.iter().min_by_key(|(p, _)| if *p >= m.pos { *p - m.pos } else { (m.pos - *p) * 3 + 40 });
        let (magnitude, at) = match chosen {
            Some((p, v)) => (*v, *p),
            None => {
                assumed = true;
                (10.0, m.pos)
            }
        };
        let from = at.min(m.pos).saturating_sub(40);
        let to = (at.max(m.pos) + 3).min(lower.len());
        let window: String = lower.chars().skip(from).take(to.saturating_sub(from)).collect();
        let minus = lower.chars().nth(at.saturating_sub(1)) == Some('-');
        let sign = if minus { -1.0 } else { direction(&window) };
        shocks.push(ShockSpec { kind: m.kind.into(), target: m.target.clone(), percent: sign * magnitude });
    }
    (shocks, assumed)
}

fn has_any(lower: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| lower.contains(n))
}

fn find_model(snap: &Snapshot, message: &str) -> Option<String> {
    let tokens: Vec<String> = words(&normalise(message));
    snap.models
        .iter()
        .find(|m| {
            let keys = [normalise(&short(&m.iri)), normalise(&m.label), normalise(&m.label.replace(&m.brand_label, "")).trim().to_string()];
            keys.iter().any(|k| !k.is_empty() && tokens.iter().any(|t| t == k))
        })
        .map(|m| short(&m.iri))
}

fn find_supplier(snap: &Snapshot, lower: &str) -> Option<String> {
    for (word, part) in SUPPLIER_WORDS {
        if lower.contains(word) {
            let hit = snap.suppliers.values().filter(|s| snap.contracts.iter().any(|c| c.supplier == s.iri)).find(|s| s.label.contains(part));
            if let Some(s) = hit {
                return Some(short(&s.iri));
            }
        }
    }
    None
}

fn find_number_before(lower: &str, unit_words: &[&str]) -> Option<f64> {
    let ws = words(lower);
    ws.iter()
        .enumerate()
        .filter(|(i, w)| *i > 0 && unit_words.contains(&w.as_str()))
        .find_map(|(i, _)| ws[i - 1].parse::<f64>().ok())
}

const MARKET_WORDS: &[(&str, &str)] = &[
    ("german", "Germany"), ("france", "France"), ("french", "France"), ("czech", "Czech Republic"),
    ("sweden", "Sweden"), ("swedish", "Sweden"), ("prague", "Czech Republic"), ("united states", "United States"), ("usa", "United States"),
];
const COMPETITOR_WORDS: &[(&str, &str)] = &[("tesla", "Tesla"), ("byd", "BYD"), ("mercedes", "Mercedes"), ("bmw", "BMW"), ("toyota", "Toyota")];

fn first_percent(lower: &str) -> Option<f64> {
    find_percents(lower).first().map(|p| p.1.abs())
}

/// "how does natural rubber affect the Golf?" -> ("natural rubber", "golf"). Needs a linking phrase and two sides.
fn extract_pair(lower: &str) -> Option<(String, String)> {
    const LINKS: &[&str] = &[" connected to ", " connected with ", " linked to ", " related to ", " affects ", " affect ", " reaches ", " reach ", " impacts ", " impact ", " hits ", " drives ", " to "];
    const OPENERS: &[&str] = &["how does", "how do", "how is", "how are", "why does", "why do", "why is", "explain how", "explain why", "show how", "show the path from", "path from", "the path from", "connection between", "link between"];
    const FILLER: &[&str] = &["the", "a", "an", "our", "its", "of", "and", "between", "from"];
    let mut text = format!(" {} ", lower.trim().trim_end_matches('?'));
    let opener = OPENERS.iter().find(|o| text.trim_start().starts_with(**o));
    opener?;
    text = text.trim_start().strip_prefix(*opener?)?.to_string();
    let text = format!(" {} ", text.trim());
    let (idx, link) = LINKS.iter().filter_map(|l| text.find(l).map(|i| (i, *l))).min_by_key(|(i, _)| *i)?;
    let clean = |t: &str| t.split_whitespace().filter(|w| !FILLER.contains(w)).collect::<Vec<_>>().join(" ");
    let (a, b) = (clean(&text[..idx]), clean(&text[idx + link.len()..]));
    (!a.is_empty() && !b.is_empty()).then_some((a, b))
}

pub const SUGGESTIONS: &[&str] = &[
    "What if rubber goes up 15%?",
    "Which models fall below 15% margin if steel rises 10% and aluminium 8%?",
    "Why did the Golf margin drop?",
    "Which contracts expire in the next 12 months?",
    "What if the battery supplier's lead time doubles?",
    "Which parts have a single source?",
    "How much of our spend is in foreign currency?",
    "Which suppliers do we depend on most?",
    "What if Germany ends the EV subsidy?",
    "What if the EU tightens the CO2 target by 10%?",
    "What if Tesla cuts prices 10%?",
    "Which brand group depends most on one production site?",
    "Should we shift production toward EVs?",
    "Which high-intent leads match models under margin pressure?",
    "Which incentive campaigns spend on models already at capacity?",
    "Is lease or loan more profitable for Taycan buyers in Prague?",
    "What does a bad year cost: rubber up 15%, krona up 5% and no German EV subsidy?",
    "How does natural rubber affect the Golf?",
];

fn help_text() -> String {
    format!(
        "I can answer questions about cost, margin, contracts and suppliers from the knowledge graph and the finance data. Try:\n- {}",
        SUGGESTIONS.join("\n- ")
    )
}

/// Planner helper: run one tool and wrap its output as a reply.
#[allow(clippy::too_many_arguments)]
fn call(kg: &Kg, snap: &Snapshot, md: &MarketData, sd: &SalesData, name: &str, args: Value, today: &str, prefix: Option<String>) -> Result<Reply> {
    let out = call_tool(kg, snap, md, sd, name, &args, today)?;
    let text = match prefix {
        Some(p) => format!("{p} {}", out.summary),
        None => out.summary,
    };
    Ok(Reply { text, blocks: out.blocks, tool_calls: vec![ToolCall { name: name.into(), args }], planner: "rules" })
}

/// Map a sentence to one tool call. Returns help text when nothing matches.
pub fn plan(kg: &Kg, snap: &Snapshot, md: &MarketData, sd: &SalesData, message: &str, today: &str) -> Result<Reply> {
    let lower = message.to_lowercase();
    let lower = lower.trim();

    if lower.is_empty() || has_any(lower, &["help", "what can you", "what can i ask", "how do i use"]) {
        return Ok(Reply { text: help_text(), blocks: vec![], tool_calls: vec![], planner: "rules" });
    }
    if has_any(lower, &["lead time", "lead-time", "leadtime"]) {
        if let Some(supplier) = find_supplier(snap, lower) {
            let factor = if has_any(lower, &["double", "twice", "2x"]) {
                2.0
            } else if lower.contains("triple") {
                3.0
            } else {
                find_number_before(lower, &["x", "times"]).unwrap_or(2.0)
            };
            return call(kg, snap, md, sd, "lead_time_impact", json!({ "supplier": supplier, "factor": factor }), today, None);
        }
        return Ok(Reply { text: "Which supplier? For example: battery, brakes, tyres, transmission, infotainment, electric drive, powertrain.".into(), blocks: vec![], tool_calls: vec![], planner: "rules" });
    }
    if let Some((from, to)) = extract_pair(lower) {
        return call(kg, snap, md, sd, "explain_path", json!({ "from": from, "to": to }), today, None);
    }
    if lower.contains("bad year") || (lower.contains("subsid") && !find_mentions(lower).is_empty()) {
        let (mut shocks, _) = plan_shocks(lower);
        if shocks.is_empty() {
            shocks = vec![
                ShockSpec { kind: "commodity".into(), target: "NaturalRubber".into(), percent: 15.0 },
                ShockSpec { kind: "fx".into(), target: "SEK".into(), percent: 5.0 },
            ];
        }
        let market = MARKET_WORDS.iter().find(|(w, _)| lower.contains(w)).map_or("Germany", |m| m.1);
        return call(kg, snap, md, sd, "combined_scenario", json!({ "shocks": shocks, "subsidy_markets": [market] }), today, None);
    }
    if has_any(lower, &["subsid"]) {
        let market = MARKET_WORDS.iter().find(|(w, _)| lower.contains(w)).map_or("Germany", |m| m.1);
        return call(kg, snap, md, sd, "subsidy_removal", json!({ "market": market }), today, None);
    }
    if has_any(lower, &["co2", "emission", "fleet target"]) {
        let tighten = first_percent(lower).unwrap_or(10.0);
        return call(kg, snap, md, sd, "co2_fleet", json!({ "tighten_percent": tighten }), today, None);
    }
    let rival = COMPETITOR_WORDS.iter().find(|(w, _)| lower.contains(w)).map(|m| m.1);
    if rival.is_some() || has_any(lower, &["competitor", "rival"]) {
        let mut args = json!({ "cut_percent": first_percent(lower).unwrap_or(10.0) });
        if let Some(r) = rival {
            args["competitor"] = json!(r);
        }
        return call(kg, snap, md, sd, "competitor_price_cut", args, today, None);
    }
    if has_any(lower, &["production site", "one site", "single site", "which plant", "site depend", "depends most on"]) {
        return call(kg, snap, md, sd, "site_dependency", json!({}), today, None);
    }
    if has_any(lower, &["powertrain", "production mix", "shift production", "shift toward", "shift towards", "mix toward", "mix shift"]) {
        return call(kg, snap, md, sd, "powertrain_mix", json!({ "shift_percent": first_percent(lower).unwrap_or(10.0) }), today, None);
    }
    if has_any(lower, &["high-intent", "high intent", "hot lead", "leads", "prospect"]) {
        return call(kg, snap, md, sd, "lead_pressure", json!({}), today, None);
    }
    if has_any(lower, &["campaign", "incentive"]) {
        return call(kg, snap, md, sd, "campaign_capacity", json!({}), today, None);
    }
    if has_any(lower, &["lease", "leasing", "loan", "financing"]) {
        let market = MARKET_WORDS.iter().find(|(w, _)| lower.contains(w)).map_or("Czech Republic", |m| m.1);
        let model = find_model(snap, message).unwrap_or_else(|| "Taycan".into());
        return call(kg, snap, md, sd, "financing_comparison", json!({ "model": model, "market": market }), today, None);
    }
    if has_any(lower, &["expire", "expiring", "expiry", "renewal", "renew", "ending", "end soon"]) {
        let months = find_number_before(lower, &["month", "months"])
            .map(|m| m as u32)
            .or_else(|| find_number_before(lower, &["year", "years"]).map(|y| y as u32 * 12))
            .unwrap_or(12);
        return call(kg, snap, md, sd, "expiring_contracts", json!({ "months": months }), today, None);
    }
    if has_any(lower, &["single source", "single-source", "single sourced", "single-sourced", "sole source", "no alternative", "second source", "only one supplier"]) {
        return call(kg, snap, md, sd, "single_sourced_components", json!({}), today, None);
    }
    if has_any(lower, &["saving", "savings", "discount", "volume tier"]) {
        return call(kg, snap, md, sd, "discount_savings", json!({}), today, None);
    }
    let wants_explanation = has_any(lower, &["why", "explain", "margin drop", "margin fall", "margin change", "margin of", "margin for"]);
    if let (true, Some(model)) = (wants_explanation, find_model(snap, message)) {
        return call(kg, snap, md, sd, "margin_trace", json!({ "model": model }), today, None);
    }

    let moves = has_any(lower, &["what if", "what happens", "if "]) || words(lower).iter().any(|w| UP.contains(&w.as_str()) || DOWN.contains(&w.as_str()));
    let has_percent = !find_percents(lower).is_empty();
    let mentions = find_mentions(lower);
    if !mentions.is_empty() && (has_percent || moves) && !has_any(lower, &["exposure", "exposed", "share of", "how much of"]) {
        let (shocks, assumed) = plan_shocks(lower);
        let prefix = assumed.then(|| "You did not give a size for every move, so I assumed 10%.".to_string());
        return call(kg, snap, md, sd, "run_scenario", json!({ "shocks": shocks }), today, prefix);
    }

    if has_any(lower, &["supplier", "suppliers", "depend", "vendor"]) {
        return call(kg, snap, md, sd, "supplier_risk", json!({}), today, None);
    }
    if has_any(lower, &["currency", "currencies", "fx", "exchange", "foreign", "krona", "koruna", "sek", "czk"]) {
        return call(kg, snap, md, sd, "currency_exposure", json!({}), today, None);
    }
    if has_any(lower, &["sensitiv", "commodity", "commodities", "raw material", "exposed"]) {
        return call(kg, snap, md, sd, "commodity_sensitivity", json!({}), today, None);
    }
    if has_any(lower, &["dashboard", "overview", "summary", "kpi", "how are we doing", "headline", "status"]) {
        return call(kg, snap, md, sd, "dashboard", json!({}), today, None);
    }
    if let Some(model) = find_model(snap, message) {
        return call(kg, snap, md, sd, "margin_trace", json!({ "model": model }), today, None);
    }
    Ok(Reply { text: format!("I did not understand that. {}", help_text()), blocks: vec![], tool_calls: vec![], planner: "rules" })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-11-27";

    type World = (Snapshot, MarketData, SalesData, Kg);

    fn snap() -> World {
        let (kg, mut conn) = crate::kg::load_for_test();
        crate::seed::seed(&kg, &mut conn).unwrap();
        (Snapshot::load(&kg, &conn).unwrap(), MarketData::load(&kg, &conn).unwrap(), SalesData::load(&kg, &conn).unwrap(), kg)
    }

    fn ask(w: &World, message: &str) -> Reply {
        plan(&w.3, &w.0, &w.1, &w.2, message, TODAY).unwrap()
    }

    fn tool_of(r: &Reply) -> &str {
        r.tool_calls.first().map_or("none", |c| c.name.as_str())
    }

    fn shocks_of(r: &Reply) -> Vec<(String, String, f64)> {
        r.tool_calls[0].args["shocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["kind"].as_str().unwrap().into(), s["target"].as_str().unwrap().into(), s["percent"].as_f64().unwrap()))
            .collect()
    }

    #[test]
    fn catalogue_is_well_formed() {
        let tools = tools();
        let names: std::collections::BTreeSet<_> = tools.iter().map(|t| t.name).collect();
        assert_eq!(names.len(), tools.len(), "tool names are unique");
        for t in &tools {
            assert!(!t.description.is_empty());
            assert_eq!(t.input_schema["type"], "object", "{}", t.name);
            // every required property is declared
            for req in t.input_schema["required"].as_array().into_iter().flatten() {
                assert!(t.input_schema["properties"].get(req.as_str().unwrap()).is_some(), "{} requires an undeclared property", t.name);
            }
        }
    }

    #[test]
    fn every_tool_runs_with_sample_arguments_and_draws_something() {
        let snap = snap();
        let samples = [
            ("run_scenario", json!({ "shocks": [{ "kind": "commodity", "target": "NaturalRubber", "percent": 15 }] })),
            ("currency_exposure", json!({})),
            ("commodity_sensitivity", json!({ "bump_percent": 5 })),
            ("discount_savings", json!({})),
            ("supplier_risk", json!({ "limit": 3 })),
            ("single_sourced_components", json!({ "limit": 3 })),
            ("expiring_contracts", json!({ "months": 6 })),
            ("lead_time_impact", json!({ "supplier": "Supplier_BatterySystems" })),
            ("margin_trace", json!({ "model": "Golf" })),
            ("dashboard", json!({})),
            ("subsidy_removal", json!({ "market": "Germany" })),
            ("co2_fleet", json!({ "tighten_percent": 10 })),
            ("competitor_price_cut", json!({ "competitor": "Tesla", "cut_percent": 10 })),
            ("site_dependency", json!({})),
            ("powertrain_mix", json!({ "shift_percent": 10 })),
            ("lead_pressure", json!({ "months": 3 })),
            ("campaign_capacity", json!({ "threshold_percent": 90 })),
            ("financing_comparison", json!({ "model": "Taycan" })),
            ("combined_scenario", json!({ "shocks": [{ "kind": "commodity", "target": "NaturalRubber", "percent": 15 }], "subsidy_markets": ["Germany"] })),
            ("explain_path", json!({ "from": "NaturalRubber", "to": "Golf" })),
            ("list_entities", json!({ "kind": "models" })),
            ("list_entities", json!({ "kind": "markets" })),
            ("list_entities", json!({ "kind": "competitors" })),
        ];
        let covered: std::collections::BTreeSet<_> = samples.iter().map(|s| s.0).collect();
        assert_eq!(covered, tools().iter().map(|t| t.name).collect(), "a sample for every tool");
        for (name, args) in samples {
            let out = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, name, &args, TODAY).unwrap_or_else(|e| panic!("{name}: {e:#}"));
            assert!(!out.summary.is_empty() && !out.blocks.is_empty(), "{name}");
            serde_json::to_string(&out.blocks).unwrap();
        }
    }

    #[test]
    fn bad_tool_input_is_a_bad_request() {
        let snap = snap();
        for (name, args) in [
            ("nope", json!({})),
            ("run_scenario", json!({})),
            ("run_scenario", json!({ "shocks": [{ "kind": "commodity", "target": "Steel" }] })),
            ("run_scenario", json!({ "shocks": [{ "kind": "commodity", "target": "Unobtainium", "percent": 5 }] })),
            ("lead_time_impact", json!({ "supplier": "Nobody" })),
            ("margin_trace", json!({ "model": "Nope" })),
            ("list_entities", json!({ "kind": "planets" })),
            ("expiring_contracts", json!({ "months": "soon" })),
            ("subsidy_removal", json!({ "market": "Atlantis" })),
            ("co2_fleet", json!({ "tighten_percent": 150 })),
            ("competitor_price_cut", json!({ "competitor": "Ford" })),
            ("powertrain_mix", json!({ "shift_percent": -5 })),
            ("lead_pressure", json!({ "months": 0 })),
            ("campaign_capacity", json!({ "threshold_percent": -1 })),
            ("financing_comparison", json!({ "model": "Golf" })),
            ("financing_comparison", json!({ "market": "Atlantis" })),
            ("combined_scenario", json!({})),
            ("explain_path", json!({ "from": "Atlantis", "to": "Golf" })),
            ("explain_path", json!({ "from": "Golf" })),
            ("combined_scenario", json!({ "subsidy_markets": ["Atlantis"] })),
        ] {
            let err = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, name, &args, TODAY).unwrap_err();
            assert!(err.downcast_ref::<BadRequest>().is_some(), "{name} {args}: {err:#}");
        }
    }

    #[test]
    fn scenario_summary_states_the_numbers() {
        let snap = snap();
        let out = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, "run_scenario", &json!({ "shocks": [{ "kind": "commodity", "target": "NaturalRubber", "percent": 15 }] }), TODAY).unwrap();
        assert!(out.summary.contains("Natural rubber +15%"), "{}", out.summary);
        assert!(out.summary.contains("€100.") || out.summary.contains("€101."), "{}", out.summary);
        assert!(out.blocks.iter().any(|b| matches!(b, Block::Link { shocks: Some(s), .. } if s.len() == 1)));
    }

    #[test]
    fn a_shock_nothing_is_priced_on_says_so() {
        let snap = snap();
        let out = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, "run_scenario", &json!({ "shocks": [{ "kind": "fx", "target": "USD", "percent": 10 }] }), TODAY).unwrap();
        assert!(out.summary.starts_with("No supply contract is priced off"), "{}", out.summary);
    }

    #[test]
    fn plans_a_single_commodity_move() {
        let snap = snap();
        for q in ["What if rubber goes up 15%?", "rubber +15%", "what happens when natural rubber rises by 15 percent", "Rubber prices increase 15 %"] {
            let r = ask(&snap, q);
            assert_eq!(tool_of(&r), "run_scenario", "{q}");
            assert_eq!(shocks_of(&r), vec![("commodity".to_string(), "NaturalRubber".to_string(), 15.0)], "{q}");
        }
    }

    #[test]
    fn plans_two_moves_with_their_own_sizes() {
        let r = ask(&snap(), "Which models fall below 15% margin if steel rises 10% and aluminium 8%?");
        assert_eq!(tool_of(&r), "run_scenario");
        assert_eq!(
            shocks_of(&r),
            vec![("commodity".to_string(), "Steel".to_string(), 10.0), ("commodity".to_string(), "Aluminium".to_string(), 8.0)]
        );
    }

    #[test]
    fn plans_falls_and_currency_moves() {
        let snap = snap();
        assert_eq!(shocks_of(&ask(&snap, "what if copper drops 12%")), vec![("commodity".to_string(), "Copper".to_string(), -12.0)]);
        assert_eq!(shocks_of(&ask(&snap, "copper -12%")), vec![("commodity".to_string(), "Copper".to_string(), -12.0)]);
        assert_eq!(shocks_of(&ask(&snap, "what if the krona strengthens 5%?")), vec![("fx".to_string(), "SEK".to_string(), 5.0)]);
        assert_eq!(shocks_of(&ask(&snap, "what if the koruna weakens by 4%")), vec![("fx".to_string(), "CZK".to_string(), -4.0)]);
        let mixed = shocks_of(&ask(&snap, "rubber up 15% but steel down 5%"));
        assert_eq!(mixed, vec![("commodity".to_string(), "NaturalRubber".to_string(), 15.0), ("commodity".to_string(), "Steel".to_string(), -5.0)]);
    }

    #[test]
    fn a_move_without_a_size_assumes_ten_percent_and_says_so() {
        let r = ask(&snap(), "what if lithium goes up?");
        assert_eq!(shocks_of(&r), vec![("commodity".to_string(), "Lithium".to_string(), 10.0)]);
        assert!(r.text.contains("assumed 10%"), "{}", r.text);
    }

    #[test]
    fn plans_the_other_questions() {
        let snap = snap();
        let cases = [
            ("Which contracts expire in the next 6 months?", "expiring_contracts"),
            ("anything up for renewal?", "expiring_contracts"),
            ("Which parts have a single source?", "single_sourced_components"),
            ("what has no alternative supplier", "single_sourced_components"),
            ("how much could we save with volume discounts", "discount_savings"),
            ("Why did the Golf margin drop?", "margin_trace"),
            ("explain the ID.4 margin", "margin_trace"),
            ("margin of the e-tron", "margin_trace"),
            ("Which suppliers do we depend on most?", "supplier_risk"),
            ("How much of our spend is in foreign currency?", "currency_exposure"),
            ("how exposed are we to the krona", "currency_exposure"),
            ("which brand is most sensitive to commodity prices", "commodity_sensitivity"),
            ("give me an overview", "dashboard"),
            ("What if Germany ends the EV subsidy?", "subsidy_removal"),
            ("what if France stops the EV bonus subsidy", "subsidy_removal"),
            ("What if the EU tightens the CO2 target by 10%?", "co2_fleet"),
            ("which brand breaches the fleet emissions target", "co2_fleet"),
            ("What if Tesla cuts prices 10%?", "competitor_price_cut"),
            ("a competitor cuts its EV price by 10%", "competitor_price_cut"),
            ("Which brand group depends most on one production site?", "site_dependency"),
            ("Should we shift production toward EVs?", "powertrain_mix"),
            ("margin per powertrain", "powertrain_mix"),
            ("Which high-intent leads match models under margin pressure?", "lead_pressure"),
            ("Which incentive campaigns spend on models already at capacity?", "campaign_capacity"),
            ("Is lease or loan more profitable for Taycan buyers in Prague?", "financing_comparison"),
            ("What does a bad year cost: rubber up 15%, krona up 5% and no German EV subsidy?", "combined_scenario"),
            ("what if a bad year hits", "combined_scenario"),
            ("How does natural rubber affect the Golf?", "explain_path"),
            ("how is the Reg_DE_EVSubsidy connected to the ID.4", "explain_path"),
            ("show the path from steel to the Scania R-series", "explain_path"),
        ];
        for (q, tool) in cases {
            assert_eq!(tool_of(&ask(&snap, q)), tool, "{q}");
        }
    }

    #[test]
    fn parameters_are_extracted_for_the_other_tools() {
        let snap = snap();
        assert_eq!(ask(&snap, "Which contracts expire in the next 6 months?").tool_calls[0].args["months"], 6);
        assert_eq!(ask(&snap, "Which contracts expire in the next 12 months?").tool_calls[0].args["months"], 12);
        assert_eq!(ask(&snap, "which contracts expire within 2 years").tool_calls[0].args["months"], 24);
        assert_eq!(ask(&snap, "what expires this year").tool_calls[0].args["months"], 12);
        let r = ask(&snap, "What if the battery supplier's lead time doubles?");
        assert_eq!(tool_of(&r), "lead_time_impact");
        assert_eq!(r.tool_calls[0].args["supplier"], "Supplier_BatterySystems");
        assert_eq!(r.tool_calls[0].args["factor"], 2.0);
        assert_eq!(ask(&snap, "what if brake lead time triples").tool_calls[0].args["factor"], 3.0);
        assert_eq!(ask(&snap, "why did the Golf margin drop").tool_calls[0].args["model"], "Golf");
        assert_eq!(ask(&snap, "explain the ID.4 margin").tool_calls[0].args["model"], "ID4");
        assert_eq!(ask(&snap, "margin of the e-tron").tool_calls[0].args["model"], "e-tron");
        assert_eq!(ask(&snap, "why did the TGX margin change").tool_calls[0].args["model"], "TGX");
    }

    #[test]
    fn market_questions_extract_their_parameters() {
        let snap = snap();
        assert_eq!(ask(&snap, "What if Germany ends the EV subsidy?").tool_calls[0].args["market"], "Germany");
        assert_eq!(ask(&snap, "what if France ends its EV subsidy").tool_calls[0].args["market"], "France");
        assert_eq!(ask(&snap, "What if the EU tightens the CO2 target by 15%?").tool_calls[0].args["tighten_percent"], 15.0);
        let r = ask(&snap, "What if Tesla cuts prices 12%?");
        assert_eq!((r.tool_calls[0].args["competitor"].as_str(), r.tool_calls[0].args["cut_percent"].as_f64()), (Some("Tesla"), Some(12.0)));
        assert!(ask(&snap, "a competitor cuts its price").tool_calls[0].args.get("competitor").is_none());
        assert_eq!(ask(&snap, "shift production toward EVs by 20%").tool_calls[0].args["shift_percent"], 20.0);
    }

    #[test]
    fn market_summaries_state_the_numbers() {
        let snap = snap();
        let out = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, "subsidy_removal", &json!({ "market": "Germany" }), TODAY).unwrap();
        assert!(out.summary.contains("Enyaq") && out.summary.contains("2 high-intent lead"), "{}", out.summary);
        let none = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, "subsidy_removal", &json!({ "market": "Sweden" }), TODAY).unwrap();
        assert!(none.summary.starts_with("No EV subsidy applies in Sweden"), "{}", none.summary);
        let co2 = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, "co2_fleet", &json!({ "tighten_percent": 10 }), TODAY).unwrap();
        assert!(co2.summary.contains("Audi") && co2.summary.contains("3 brand"), "{}", co2.summary);
        let site = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, "site_dependency", &json!({}), TODAY).unwrap();
        assert!(site.summary.contains("Wolfsburg"), "{}", site.summary);
        let comp = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, "competitor_price_cut", &json!({ "competitor": "Tesla" }), TODAY).unwrap();
        assert!(comp.summary.contains("ID.4") && !comp.summary.contains("Enyaq"), "{}", comp.summary);
    }

    #[test]
    fn sales_summaries_state_the_numbers() {
        let snap = snap();
        let leads = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, "lead_pressure", &json!({}), TODAY).unwrap();
        assert!(leads.summary.starts_with("2 of 2 high-intent") && leads.summary.contains("Enyaq"), "{}", leads.summary);
        let camp = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, "campaign_capacity", &json!({}), TODAY).unwrap();
        assert!(camp.summary.contains("ID.4 conquest offer") && camp.summary.contains("€35.5M"), "{}", camp.summary);
        let fin = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, "financing_comparison", &json!({}), TODAY).unwrap();
        assert!(fin.summary.contains("loan, 60 months") && fin.summary.contains("Czech"), "{}", fin.summary);
        let r = ask(&snap, "is leasing a Taycan in Prague better than a loan?");
        assert_eq!(r.tool_calls[0].args["model"], "Taycan");
        assert_eq!(r.tool_calls[0].args["market"], "Czech Republic");
    }

    #[test]
    fn bad_year_question_is_planned_with_its_shocks_and_summarised() {
        let snap = snap();
        let r = ask(&snap, "What does a bad year cost: rubber up 15%, krona up 5% and no German EV subsidy?");
        assert_eq!(tool_of(&r), "combined_scenario");
        let args = &r.tool_calls[0].args;
        assert_eq!(args["subsidy_markets"][0], "Germany");
        assert_eq!(args["shocks"].as_array().unwrap().len(), 2);
        assert!(r.text.contains("Natural rubber +15%") && r.text.contains("no EV subsidy in Germany") && r.text.contains("Renegotiate first"), "{}", r.text);
        let preset = ask(&snap, "what if a bad year hits");
        assert_eq!(preset.tool_calls[0].args["shocks"].as_array().unwrap().len(), 2, "preset shocks when none are named");
        // a plain commodity question is still a scenario run, and a subsidy question without shocks is unchanged
        assert_eq!(tool_of(&ask(&snap, "what if rubber goes up 15%?")), "run_scenario");
        assert_eq!(tool_of(&ask(&snap, "What if Germany ends the EV subsidy?")), "subsidy_removal");
    }

    #[test]
    fn explain_path_gives_a_grounded_chain_with_provenance() {
        let snap = snap();
        let out = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, "explain_path", &json!({ "from": "NaturalRubber", "to": "Golf" }), TODAY).unwrap();
        assert!(out.summary.starts_with("Route 1 for Natural rubber to Golf:"), "{}", out.summary);
        assert!(out.summary.contains("Key values of Golf") && out.summary.contains("price"), "{}", out.summary);
        assert!(out.blocks.iter().any(|b| matches!(b, Block::Table { rows, .. } if rows.len() == 4 && rows[0][3].starts_with("asserted in VW_Supply_Abox.ttl"))));
        let r = ask(&snap, "How does natural rubber affect the Golf?");
        assert_eq!(r.tool_calls[0].args["from"], "natural rubber");
        assert_eq!(r.tool_calls[0].args["to"], "golf");
        // questions that are not about a link between two things keep their old tools
        assert_eq!(tool_of(&ask(&snap, "Why did the Golf margin drop?")), "margin_trace");
        assert_eq!(tool_of(&ask(&snap, "explain the ID.4 margin")), "margin_trace");
        assert_eq!(tool_of(&ask(&snap, "What if rubber goes up 15%?")), "run_scenario");
    }

    #[test]
    fn analyses_link_to_the_reasoning_graph_behind_them() {
        let snap = snap();
        let link_of = |name: &str, args: Value| {
            let out = call_tool(&snap.3, &snap.0, &snap.1, &snap.2, name, &args, TODAY).unwrap();
            out.blocks.iter().find_map(|b| match b {
                Block::Link { to, .. } if to.starts_with("/graph?pairs=") => Some(to.clone()),
                _ => None,
            })
        };
        let rubber = link_of("run_scenario", json!({ "shocks": [{ "kind": "commodity", "target": "NaturalRubber", "percent": 15 }] })).expect("scenario links to its graph");
        assert!(rubber.contains("Index_NaturalRubber|"), "{rubber}");
        let bad_year = link_of("combined_scenario", json!({ "shocks": [{ "kind": "fx", "target": "SEK", "percent": 5 }], "subsidy_markets": ["Germany"] })).unwrap();
        assert!(bad_year.contains("SEK|") && bad_year.contains("Reg_DE_EVSubsidy"), "{bad_year}");
        assert!(link_of("margin_trace", json!({ "model": "Golf" })).unwrap().contains("Index_"));
        assert!(link_of("lead_time_impact", json!({ "supplier": "Supplier_BatterySystems" })).unwrap().contains("Supplier_BatterySystems"));
        assert!(link_of("subsidy_removal", json!({})).unwrap().contains("Reg_DE_EVSubsidy"));
        assert!(link_of("competitor_price_cut", json!({ "competitor": "Tesla" })).is_some());
        assert!(link_of("supplier_risk", json!({})).is_some());
        assert!(link_of("dashboard", json!({})).is_none(), "no graph behind a headline view");
    }

    #[test]
    fn lead_time_without_a_supplier_asks_which() {
        let r = ask(&snap(), "what if lead time goes up");
        assert!(r.tool_calls.is_empty() && r.text.contains("Which supplier"), "{}", r.text);
    }

    #[test]
    fn help_and_unknown_input_list_what_to_ask() {
        let snap = snap();
        for q in ["help", "what can you do?", ""] {
            let r = ask(&snap, q);
            assert!(r.tool_calls.is_empty() && r.text.contains("What if rubber goes up 15%?"), "{q}");
        }
        let r = ask(&snap, "tell me a joke about cars");
        assert!(r.tool_calls.is_empty() && r.text.starts_with("I did not understand"), "{}", r.text);
    }

    #[test]
    fn every_suggestion_is_answered_by_a_tool() {
        let snap = snap();
        for q in SUGGESTIONS {
            let r = ask(&snap, q);
            assert!(!r.tool_calls.is_empty() && !r.blocks.is_empty(), "{q}: {}", r.text);
        }
    }
}
