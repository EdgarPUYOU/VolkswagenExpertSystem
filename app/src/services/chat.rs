//! Chat layer: the question services exposed as tools, and a rule-based planner that turns a
//! sentence into a tool call.
//!
//! A tool takes JSON arguments and returns a `ToolOutput`: a one-paragraph summary (what an LLM
//! would read back to the user), structured blocks (what the chat bubble draws) and the full data.
//! The planner is the only part that guesses; swap it for an LLM that calls the same tools and
//! nothing else changes. Tools never accept SPARQL or SQL.

use super::finance::*;
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

#[derive(Deserialize, Debug, Default)]
pub struct ChatRequest {
    pub message: String,
    /// Earlier turns, kept for an LLM planner; the rule-based planner ignores them.
    #[serde(default)]
    #[allow(dead_code)]
    pub history: Vec<Value>,
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
            name: "list_entities",
            description: "List the names the other tools accept: models, suppliers, commodity indices or currencies.",
            input_schema: json!({
                "type": "object",
                "properties": { "kind": { "type": "string", "enum": ["models", "suppliers", "indices", "currencies"] } },
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
#[derive(Deserialize)]
struct ListArgs {
    kind: String,
}

fn link(label: &str, to: &str) -> Block {
    Block::Link { label: label.into(), to: to.into(), shocks: None }
}

// ---------------------------------------------------------------- tool execution

pub fn call_tool(snap: &Snapshot, name: &str, args: &Value, today: &str) -> Result<ToolOutput> {
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
                other => return bad(format!("unknown kind '{other}', expected models, suppliers, indices or currencies")),
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

pub const SUGGESTIONS: &[&str] = &[
    "What if rubber goes up 15%?",
    "Which models fall below 15% margin if steel rises 10% and aluminium 8%?",
    "Why did the Golf margin drop?",
    "Which contracts expire in the next 12 months?",
    "What if the battery supplier's lead time doubles?",
    "Which parts have a single source?",
    "How much of our spend is in foreign currency?",
    "Which suppliers do we depend on most?",
];

fn help_text() -> String {
    format!(
        "I can answer questions about cost, margin, contracts and suppliers from the knowledge graph and the finance data. Try:\n- {}",
        SUGGESTIONS.join("\n- ")
    )
}

fn call(snap: &Snapshot, name: &str, args: Value, today: &str, prefix: Option<String>) -> Result<Reply> {
    let out = call_tool(snap, name, &args, today)?;
    let text = match prefix {
        Some(p) => format!("{p} {}", out.summary),
        None => out.summary,
    };
    Ok(Reply { text, blocks: out.blocks, tool_calls: vec![ToolCall { name: name.into(), args }], planner: "rules" })
}

/// Map a sentence to one tool call. Returns help text when nothing matches.
pub fn plan(snap: &Snapshot, message: &str, today: &str) -> Result<Reply> {
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
            return call(snap, "lead_time_impact", json!({ "supplier": supplier, "factor": factor }), today, None);
        }
        return Ok(Reply { text: "Which supplier? For example: battery, brakes, tyres, transmission, infotainment, electric drive, powertrain.".into(), blocks: vec![], tool_calls: vec![], planner: "rules" });
    }
    if has_any(lower, &["expire", "expiring", "expiry", "renewal", "renew", "ending", "end soon"]) {
        let months = find_number_before(lower, &["month", "months"])
            .map(|m| m as u32)
            .or_else(|| find_number_before(lower, &["year", "years"]).map(|y| y as u32 * 12))
            .unwrap_or(12);
        return call(snap, "expiring_contracts", json!({ "months": months }), today, None);
    }
    if has_any(lower, &["single source", "single-source", "single sourced", "single-sourced", "sole source", "no alternative", "second source", "only one supplier"]) {
        return call(snap, "single_sourced_components", json!({}), today, None);
    }
    if has_any(lower, &["saving", "savings", "discount", "volume tier"]) {
        return call(snap, "discount_savings", json!({}), today, None);
    }
    let wants_explanation = has_any(lower, &["why", "explain", "margin drop", "margin fall", "margin change", "margin of", "margin for"]);
    if let (true, Some(model)) = (wants_explanation, find_model(snap, message)) {
        return call(snap, "margin_trace", json!({ "model": model }), today, None);
    }

    let moves = has_any(lower, &["what if", "what happens", "if "]) || words(lower).iter().any(|w| UP.contains(&w.as_str()) || DOWN.contains(&w.as_str()));
    let has_percent = !find_percents(lower).is_empty();
    let mentions = find_mentions(lower);
    if !mentions.is_empty() && (has_percent || moves) && !has_any(lower, &["exposure", "exposed", "share of", "how much of"]) {
        let (shocks, assumed) = plan_shocks(lower);
        let prefix = assumed.then(|| "You did not give a size for every move, so I assumed 10%.".to_string());
        return call(snap, "run_scenario", json!({ "shocks": shocks }), today, prefix);
    }

    if has_any(lower, &["supplier", "suppliers", "depend", "vendor"]) {
        return call(snap, "supplier_risk", json!({}), today, None);
    }
    if has_any(lower, &["currency", "currencies", "fx", "exchange", "foreign", "krona", "koruna", "sek", "czk"]) {
        return call(snap, "currency_exposure", json!({}), today, None);
    }
    if has_any(lower, &["sensitiv", "commodity", "commodities", "raw material", "exposed"]) {
        return call(snap, "commodity_sensitivity", json!({}), today, None);
    }
    if has_any(lower, &["dashboard", "overview", "summary", "kpi", "how are we doing", "headline", "status"]) {
        return call(snap, "dashboard", json!({}), today, None);
    }
    if let Some(model) = find_model(snap, message) {
        return call(snap, "margin_trace", json!({ "model": model }), today, None);
    }
    Ok(Reply { text: format!("I did not understand that. {}", help_text()), blocks: vec![], tool_calls: vec![], planner: "rules" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kg::Kg;
    use std::path::Path;

    const TODAY: &str = "2026-11-27";

    fn snap() -> Snapshot {
        let (kg, _, _) = Kg::load(Path::new(".."), Path::new("../rules")).unwrap();
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        crate::seed::seed(&kg, &mut conn).unwrap();
        Snapshot::load(&kg, &conn).unwrap()
    }

    fn ask(snap: &Snapshot, message: &str) -> Reply {
        plan(snap, message, TODAY).unwrap()
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
            ("list_entities", json!({ "kind": "models" })),
        ];
        let covered: std::collections::BTreeSet<_> = samples.iter().map(|s| s.0).collect();
        assert_eq!(covered, tools().iter().map(|t| t.name).collect(), "a sample for every tool");
        for (name, args) in samples {
            let out = call_tool(&snap, name, &args, TODAY).unwrap_or_else(|e| panic!("{name}: {e:#}"));
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
        ] {
            let err = call_tool(&snap, name, &args, TODAY).unwrap_err();
            assert!(err.downcast_ref::<BadRequest>().is_some(), "{name} {args}: {err:#}");
        }
    }

    #[test]
    fn scenario_summary_states_the_numbers() {
        let snap = snap();
        let out = call_tool(&snap, "run_scenario", &json!({ "shocks": [{ "kind": "commodity", "target": "NaturalRubber", "percent": 15 }] }), TODAY).unwrap();
        assert!(out.summary.contains("Natural rubber +15%"), "{}", out.summary);
        assert!(out.summary.contains("€100.") || out.summary.contains("€101."), "{}", out.summary);
        assert!(out.blocks.iter().any(|b| matches!(b, Block::Link { shocks: Some(s), .. } if s.len() == 1)));
    }

    #[test]
    fn a_shock_nothing_is_priced_on_says_so() {
        let snap = snap();
        let out = call_tool(&snap, "run_scenario", &json!({ "shocks": [{ "kind": "fx", "target": "USD", "percent": 10 }] }), TODAY).unwrap();
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
