//! Pricing model and scenario runner.
//!
//! The knowledge graph says which contracts, clauses, models and profit centers exist and how they
//! connect. SQLite holds the monthly index values, FX rates, overhead and budget. A `Snapshot`
//! reads both once per request and everything else here is plain arithmetic on it.
//!
//! Effective EUR unit price of a contract in month `t`:
//!
//!   base_price * (1 + sum_i weight_i * pass_i * (I_i(t) / I_i(start) - 1)) * fx
//!
//! where `fx = per_eur(start) / per_eur(t)` for a contract invoiced in a foreign currency
//! (the contract's local-currency price is fixed, so a stronger foreign currency costs more EUR).

use super::portfolio::run;
use crate::kg::{Kg, Row};
use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const NS: &str = "http://example.org/volkswagen/";
pub const REPORTING_CURRENCY: &str = "http://example.org/volkswagen/EUR";
pub const DIRECT_MATERIAL: &str = "http://example.org/volkswagen/CE_DirectMaterial";
pub const DEFAULT_MARGIN_FLOOR: f64 = 0.15;

/// A request the caller got wrong (unknown IRI, bad number). Mapped to HTTP 400.
#[derive(Debug)]
pub struct BadRequest(pub String);
impl std::fmt::Display for BadRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for BadRequest {}

pub fn bad<T>(msg: impl Into<String>) -> Result<T> {
    Err(BadRequest(msg.into()).into())
}

pub fn short(iri: &str) -> String {
    iri.rsplit('/').next().unwrap_or(iri).to_string()
}

fn num(row: &Row, key: &str) -> Result<f64> {
    row.get(key)
        .with_context(|| format!("missing {key}"))?
        .parse::<f64>()
        .with_context(|| format!("{key} is not a number"))
}

fn opt_num(row: &Row, key: &str) -> Option<f64> {
    row.get(key).and_then(|v| v.parse().ok())
}

#[derive(Clone, Debug, Serialize)]
pub struct Model {
    pub iri: String,
    pub label: String,
    pub brand_iri: String,
    pub brand_label: String,
    pub pc_iri: String,
    pub pc_label: String,
    pub price: f64,
    pub planned: f64,
    pub launch: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Clause {
    pub index: String,
    pub weight: f64,
    pub pass: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Contract {
    pub iri: String,
    pub label: String,
    pub component: String,
    pub supplier: String,
    pub currency: String,
    pub start: String,
    pub end: String,
    pub base_price: f64,
    pub discount_price: Option<f64>,
    pub tier: Option<f64>,
    pub committed: f64,
    pub projected: f64,
    pub sourcing: String,
    pub alternatives: Vec<String>,
    pub clauses: Vec<Clause>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Supplier {
    pub iri: String,
    pub label: String,
    pub country: String,
    pub lead_time_days: f64,
}

pub struct Snapshot {
    pub models: Vec<Model>,
    pub contracts: Vec<Contract>,
    pub model_components: HashMap<String, Vec<String>>,
    /// component IRI -> index into `contracts`
    pub contract_of: HashMap<String, usize>,
    pub component_labels: HashMap<String, String>,
    pub suppliers: HashMap<String, Supplier>,
    pub index_labels: BTreeMap<String, String>,
    pub currency_codes: BTreeMap<String, String>,
    pub critical_components: HashSet<String>,
    /// (customer label, model IRI) for high-intent leads
    pub leads: Vec<(String, String)>,
    index_values: HashMap<(String, String), f64>,
    fx: HashMap<(String, String), f64>,
    pub overhead: HashMap<String, f64>,
    /// pc IRI -> direct-material budget for the budget year
    pub material_budget: HashMap<String, f64>,
    /// Latest month with index values, 'YYYY-MM'
    pub asof: String,
}

#[derive(Default, Clone, Debug)]
pub struct Shocks {
    /// index IRI -> fractional move (0.15 = +15%)
    pub commodity: HashMap<String, f64>,
    /// currency IRI -> fractional move of that currency against EUR (+ = foreign currency stronger)
    pub fx: HashMap<String, f64>,
}

impl Snapshot {
    pub fn load(kg: &Kg, conn: &Connection) -> Result<Snapshot> {
        let rows = |q: &str| run(kg, q);

        let models = rows(
            "SELECT ?model ?label ?brand ?brandLabel ?pc ?pcLabel ?price ?planned ?launch WHERE {
               ?model a vw:Model ; vw:modelOf ?brand ; vw:price ?price ; vw:plannedAnnualProduction ?planned .
               ?brand rdfs:label ?brandLabel ; vw:brandProfitCenter ?pc .
               ?pc rdfs:label ?pcLabel .
               OPTIONAL { ?model rdfs:label ?label }
               OPTIONAL { ?model vw:plannedLaunchDate ?launch }
             } ORDER BY ?brandLabel ?model",
        )?
        .iter()
        .map(|r| {
            Ok(Model {
                iri: r["model"].clone(),
                label: r.get("label").cloned().unwrap_or_else(|| short(&r["model"])),
                brand_iri: r["brand"].clone(),
                brand_label: r["brandLabel"].clone(),
                pc_iri: r["pc"].clone(),
                pc_label: r["pcLabel"].clone(),
                price: num(r, "price")?,
                planned: num(r, "planned")?,
                launch: r.get("launch").cloned(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

        let mut model_components: HashMap<String, Vec<String>> = HashMap::new();
        for r in rows("SELECT ?model ?comp WHERE { ?model a vw:Model ; vw:hasComponent ?comp } ORDER BY ?model ?comp")? {
            model_components.entry(r["model"].clone()).or_default().push(r["comp"].clone());
        }

        let mut component_labels = HashMap::new();
        for r in rows("SELECT ?c ?label WHERE { ?c a vw:Component ; rdfs:label ?label }")? {
            component_labels.insert(r["c"].clone(), r["label"].clone());
        }

        let mut contracts = rows(
            "SELECT ?contract ?label ?comp ?supplier ?currency ?start ?end ?price ?committed ?projected
                    ?discount ?tier ?sourcing WHERE {
               ?contract a vw:SupplyContract ;
                         vw:contractCoversComponent ?comp ;
                         vw:contractSupplier ?supplier ;
                         vw:contractCurrency ?currency ;
                         vw:contractStartDate ?start ;
                         vw:contractEndDate ?end ;
                         vw:currentUnitPriceEUR ?price ;
                         vw:currentContractedAnnualVolume ?committed ;
                         vw:projectedAnnualComponentVolume ?projected .
               OPTIONAL { ?contract rdfs:label ?label }
               OPTIONAL { ?contract vw:discountUnitPriceEUR ?discount }
               OPTIONAL { ?contract vw:discountTierAnnualVolume ?tier }
               OPTIONAL { ?contract vw:sourcingType ?sourcing }
             } ORDER BY ?contract",
        )?
        .iter()
        .map(|r| {
            Ok(Contract {
                iri: r["contract"].clone(),
                label: r.get("label").cloned().unwrap_or_else(|| short(&r["contract"])),
                component: r["comp"].clone(),
                supplier: r["supplier"].clone(),
                currency: r["currency"].clone(),
                start: r["start"].clone(),
                end: r["end"].clone(),
                base_price: num(r, "price")?,
                discount_price: opt_num(r, "discount"),
                tier: opt_num(r, "tier"),
                committed: num(r, "committed")?,
                projected: num(r, "projected")?,
                sourcing: r.get("sourcing").map(|s| short(s)).unwrap_or_default(),
                alternatives: Vec::new(),
                clauses: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
        let by_iri: HashMap<String, usize> = contracts.iter().enumerate().map(|(i, c)| (c.iri.clone(), i)).collect();
        for r in rows("SELECT ?contract ?alt WHERE { ?contract vw:alternativeSupplier ?alt } ORDER BY ?contract ?alt")? {
            if let Some(&i) = by_iri.get(&r["contract"]) {
                contracts[i].alternatives.push(r["alt"].clone());
            }
        }
        for r in rows(
            "SELECT ?contract ?index ?w ?p WHERE {
               ?contract vw:hasIndexationClause ?cl .
               ?cl vw:indexedTo ?index ; vw:indexWeight ?w ; vw:passThroughRatio ?p
             } ORDER BY ?contract ?index",
        )? {
            if let Some(&i) = by_iri.get(&r["contract"]) {
                contracts[i].clauses.push(Clause { index: r["index"].clone(), weight: num(&r, "w")?, pass: num(&r, "p")? });
            }
        }
        let contract_of = contracts.iter().enumerate().map(|(i, c)| (c.component.clone(), i)).collect();

        let suppliers = rows(
            "SELECT ?s ?label ?country ?lead WHERE {
               ?s a vw:Supplier .
               OPTIONAL { ?s rdfs:label ?label } OPTIONAL { ?s vw:supplierCountry ?country }
               OPTIONAL { ?s vw:supplierLeadTimeDays ?lead }
             }",
        )?
        .iter()
        .map(|r| {
            (
                r["s"].clone(),
                Supplier {
                    iri: r["s"].clone(),
                    label: r.get("label").cloned().unwrap_or_else(|| short(&r["s"])),
                    country: r.get("country").cloned().unwrap_or_default(),
                    lead_time_days: opt_num(r, "lead").unwrap_or(0.0),
                },
            )
        })
        .collect();

        let index_labels = rows("SELECT ?i ?label WHERE { ?i a vw:CommodityIndex ; rdfs:label ?label }")?
            .iter()
            .map(|r| (r["i"].clone(), r["label"].clone()))
            .collect();
        let currency_codes = rows("SELECT ?c ?code WHERE { ?c a vw:Currency ; vw:currencyCode ?code }")?
            .iter()
            .map(|r| (r["c"].clone(), r["code"].clone()))
            .collect();
        let critical_components = rows("SELECT ?c WHERE { ?c a vw:SingleSourcedCriticalComponent }")?
            .iter()
            .map(|r| r["c"].clone())
            .collect();
        let leads = rows(
            "SELECT ?customer ?label ?model WHERE { ?customer vw:highIntentForModel ?model . OPTIONAL { ?customer rdfs:label ?label } } ORDER BY ?customer ?model",
        )?
        .iter()
        .map(|r| (r.get("label").cloned().unwrap_or_else(|| short(&r["customer"])), r["model"].clone()))
        .collect();

        let mut index_values = HashMap::new();
        let mut stmt = conn.prepare("SELECT index_iri, month, value FROM index_values")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?)))? {
            let (i, m, v) = row?;
            index_values.insert((i, m), v);
        }
        let mut fx = HashMap::new();
        let mut stmt = conn.prepare("SELECT currency_iri, month, per_eur FROM fx_rates")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?)))? {
            let (c, m, v) = row?;
            fx.insert((c, m), v);
        }
        let mut overhead: HashMap<String, f64> = HashMap::new();
        let mut stmt = conn.prepare("SELECT model_iri, SUM(eur_per_unit) FROM model_cost GROUP BY model_iri")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))? {
            let (m, v) = row?;
            overhead.insert(m, v);
        }
        let mut material_budget: HashMap<String, f64> = HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT profit_center_iri, SUM(amount_eur) FROM budget WHERE cost_element_iri = ?1 GROUP BY profit_center_iri",
        )?;
        for row in stmt.query_map([DIRECT_MATERIAL], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))? {
            let (pc, v) = row?;
            material_budget.insert(pc, v);
        }
        let asof = index_values.keys().map(|(_, m)| m.clone()).max().unwrap_or_default();

        Ok(Snapshot {
            models,
            contracts,
            model_components,
            contract_of,
            component_labels,
            suppliers,
            index_labels,
            currency_codes,
            critical_components,
            leads,
            index_values,
            fx,
            overhead,
            material_budget,
            asof,
        })
    }

    /// Monthly points for an index or FX series, oldest first.
    pub fn series_of(&self, iri: &str) -> Vec<(String, f64)> {
        let mut points: Vec<(String, f64)> = self
            .index_values
            .iter()
            .chain(self.fx.iter())
            .filter(|((i, _), _)| i == iri)
            .map(|((_, m), v)| (m.clone(), *v))
            .collect();
        points.sort_by(|a, b| a.0.cmp(&b.0));
        points
    }

    /// Insert series that are not in SQLite yet (used by the seed, which prices before saving).
    pub fn with_series(mut self, index_values: HashMap<(String, String), f64>, fx: HashMap<(String, String), f64>) -> Snapshot {
        self.asof = index_values.keys().map(|(_, m)| m.clone()).max().unwrap_or_default();
        self.index_values = index_values;
        self.fx = fx;
        self
    }

    pub fn index_value(&self, index: &str, month: &str) -> Result<f64> {
        self.index_values
            .get(&(index.to_string(), month.to_string()))
            .copied()
            .with_context(|| format!("no value for index {} in {month}", short(index)))
    }

    pub fn fx_rate(&self, currency: &str, month: &str) -> Result<f64> {
        self.fx
            .get(&(currency.to_string(), month.to_string()))
            .copied()
            .with_context(|| format!("no FX rate for {} in {month}", short(currency)))
    }

    /// Effective EUR unit price of a contract in `month`, with optional shocks applied to that month.
    pub fn price_at(&self, c: &Contract, month: &str, shocks: &Shocks) -> Result<f64> {
        let base_month = &c.start[..7];
        let mut adjust = 1.0;
        for cl in &c.clauses {
            let i0 = self.index_value(&cl.index, base_month)?;
            let shock = 1.0 + shocks.commodity.get(&cl.index).copied().unwrap_or(0.0);
            let it = self.index_value(&cl.index, month)? * shock;
            adjust += cl.weight * cl.pass * (it / i0 - 1.0);
        }
        let fx = if c.currency == REPORTING_CURRENCY {
            1.0
        } else {
            let r0 = self.fx_rate(&c.currency, base_month)?;
            let shock = 1.0 + shocks.fx.get(&c.currency).copied().unwrap_or(0.0);
            r0 / (self.fx_rate(&c.currency, month)? / shock)
        };
        Ok(c.base_price * adjust * fx)
    }

    pub fn contract_for(&self, component: &str) -> Option<&Contract> {
        self.contract_of.get(component).map(|&i| &self.contracts[i])
    }

    /// Direct material cost of one vehicle.
    pub fn material_cost(&self, model: &Model, month: &str, shocks: &Shocks) -> Result<f64> {
        let mut total = 0.0;
        for comp in self.model_components.get(&model.iri).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(c) = self.contract_for(comp) {
                total += self.price_at(c, month, shocks)?;
            }
        }
        Ok(total)
    }

    pub fn overhead_of(&self, model: &Model) -> f64 {
        self.overhead.get(&model.iri).copied().unwrap_or(0.0)
    }

    pub fn models_using(&self, component: &str) -> Vec<&Model> {
        self.models
            .iter()
            .filter(|m| self.model_components.get(&m.iri).is_some_and(|cs| cs.iter().any(|c| c == component)))
            .collect()
    }

    pub fn label_of_component(&self, iri: &str) -> String {
        self.component_labels.get(iri).cloned().unwrap_or_else(|| short(iri))
    }

    pub fn supplier_label(&self, iri: &str) -> String {
        self.suppliers.get(iri).map(|s| s.label.clone()).unwrap_or_else(|| short(iri))
    }

    pub fn index_label(&self, iri: &str) -> String {
        self.index_labels.get(iri).cloned().unwrap_or_else(|| short(iri))
    }

    pub fn resolve_index(&self, name: &str) -> Result<String> {
        for candidate in [name.to_string(), format!("{NS}{name}"), format!("{NS}Index_{name}")] {
            if self.index_labels.contains_key(&candidate) {
                return Ok(candidate);
            }
        }
        bad(format!("unknown commodity index '{name}'"))
    }

    pub fn resolve_currency(&self, name: &str) -> Result<String> {
        for candidate in [name.to_string(), format!("{NS}{name}")] {
            if self.currency_codes.contains_key(&candidate) {
                return Ok(candidate);
            }
        }
        bad(format!("unknown currency '{name}'"))
    }

    pub fn resolve_supplier(&self, name: &str) -> Result<String> {
        for candidate in [name.to_string(), format!("{NS}{name}")] {
            if self.suppliers.contains_key(&candidate) {
                return Ok(candidate);
            }
        }
        bad(format!("unknown supplier '{name}'"))
    }

    pub fn resolve_model(&self, name: &str) -> Result<&Model> {
        self.models
            .iter()
            .find(|m| m.iri == name || m.iri == format!("{NS}{name}"))
            .map_or_else(|| bad(format!("unknown model '{name}'")), Ok)
    }
}

// ---------------------------------------------------------------- scenarios

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ShockSpec {
    /// "commodity" or "fx"
    pub kind: String,
    /// Index or currency, as an IRI or short name ("Steel", "Index_Steel", "SEK")
    pub target: String,
    /// +15 means +15%
    pub percent: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ScenarioRequest {
    pub shocks: Vec<ShockSpec>,
    pub margin_floor: Option<f64>,
    pub save_as: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct ModelImpact {
    pub iri: String,
    pub label: String,
    pub brand: String,
    pub profit_center: String,
    pub price: f64,
    pub planned: f64,
    pub material_before: f64,
    pub material_after: f64,
    pub overhead: f64,
    pub margin_before_pct: f64,
    pub margin_after_pct: f64,
    pub extra_cost: f64,
    pub below_floor: bool,
    pub crossed_floor: bool,
}

#[derive(Serialize, Debug)]
pub struct ProfitCenterImpact {
    pub iri: String,
    pub label: String,
    pub revenue: f64,
    pub extra_cost: f64,
    pub extra_cost_pct_of_revenue: f64,
    pub material_budget: f64,
    pub extra_cost_pct_of_budget: f64,
    pub margin_before_pct: f64,
    pub margin_after_pct: f64,
}

#[derive(Serialize, Debug)]
pub struct Driver {
    pub shock: String,
    pub extra_cost: f64,
}

#[derive(Serialize, Debug)]
pub struct ContractImpact {
    pub iri: String,
    pub label: String,
    pub supplier: String,
    pub price_before: f64,
    pub price_after: f64,
    pub annual_units: f64,
    pub extra_cost: f64,
    pub sourcing: String,
    pub end: String,
    pub drivers: Vec<Driver>,
}

#[derive(Serialize, Debug)]
pub struct ScenarioTotals {
    pub revenue: f64,
    pub extra_cost: f64,
    pub margin_before_pct: f64,
    pub margin_after_pct: f64,
    pub models_below_floor: usize,
    pub models_crossing_floor: usize,
}

#[derive(Serialize, Debug)]
pub struct ScenarioResult {
    pub asof: String,
    pub margin_floor_pct: f64,
    pub shocks: Vec<ShockSpec>,
    pub totals: ScenarioTotals,
    pub by_profit_center: Vec<ProfitCenterImpact>,
    pub by_model: Vec<ModelImpact>,
    pub by_contract: Vec<ContractImpact>,
}

fn pct(x: f64) -> f64 {
    x * 100.0
}

fn describe(snap: &Snapshot, s: &ShockSpec) -> String {
    let target = if s.kind == "fx" {
        snap.currency_codes.get(&format!("{NS}{}", s.target)).cloned().unwrap_or_else(|| s.target.clone())
    } else {
        snap.resolve_index(&s.target).map(|i| snap.index_label(&i)).unwrap_or_else(|_| s.target.clone())
    };
    format!("{target} {:+}%", s.percent)
}

pub fn build_shocks(snap: &Snapshot, specs: &[ShockSpec]) -> Result<Shocks> {
    let mut shocks = Shocks::default();
    for s in specs {
        if !s.percent.is_finite() || s.percent <= -100.0 {
            return bad("percent must be a number greater than -100");
        }
        match s.kind.as_str() {
            "commodity" => {
                *shocks.commodity.entry(snap.resolve_index(&s.target)?).or_insert(0.0) += s.percent / 100.0;
            }
            "fx" => {
                *shocks.fx.entry(snap.resolve_currency(&s.target)?).or_insert(0.0) += s.percent / 100.0;
            }
            other => return bad(format!("unknown shock kind '{other}', expected 'commodity' or 'fx'")),
        }
    }
    Ok(shocks)
}

pub fn run_scenario(snap: &Snapshot, req: &ScenarioRequest) -> Result<ScenarioResult> {
    if req.shocks.is_empty() {
        return bad("at least one shock is required");
    }
    let floor = req.margin_floor.unwrap_or(DEFAULT_MARGIN_FLOOR);
    let shocks = build_shocks(snap, &req.shocks)?;
    let none = Shocks::default();
    let asof = snap.asof.as_str();

    let mut by_model = Vec::new();
    for m in &snap.models {
        let before = snap.material_cost(m, asof, &none)?;
        let after = snap.material_cost(m, asof, &shocks)?;
        let oh = snap.overhead_of(m);
        let mb = (m.price - before - oh) / m.price;
        let ma = (m.price - after - oh) / m.price;
        by_model.push(ModelImpact {
            iri: m.iri.clone(),
            label: m.label.clone(),
            brand: m.brand_label.clone(),
            profit_center: m.pc_label.clone(),
            price: m.price,
            planned: m.planned,
            material_before: before,
            material_after: after,
            overhead: oh,
            margin_before_pct: pct(mb),
            margin_after_pct: pct(ma),
            extra_cost: (after - before) * m.planned,
            below_floor: ma < floor,
            crossed_floor: mb >= floor && ma < floor,
        });
    }
    by_model.sort_by(|a, b| b.extra_cost.total_cmp(&a.extra_cost));

    let mut by_pc: BTreeMap<String, ProfitCenterImpact> = BTreeMap::new();
    let mut profit_before: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for (m, i) in snap.models.iter().zip(snap.models.iter().map(|m| by_model.iter().find(|x| x.iri == m.iri).unwrap())) {
        let e = by_pc.entry(m.pc_iri.clone()).or_insert_with(|| ProfitCenterImpact {
            iri: m.pc_iri.clone(),
            label: m.pc_label.clone(),
            revenue: 0.0,
            extra_cost: 0.0,
            extra_cost_pct_of_revenue: 0.0,
            material_budget: snap.material_budget.get(&m.pc_iri).copied().unwrap_or(0.0),
            extra_cost_pct_of_budget: 0.0,
            margin_before_pct: 0.0,
            margin_after_pct: 0.0,
        });
        e.revenue += m.price * m.planned;
        e.extra_cost += i.extra_cost;
        let p = profit_before.entry(m.pc_iri.clone()).or_default();
        p.0 += (m.price - i.material_before - i.overhead) * m.planned;
        p.1 += (m.price - i.material_after - i.overhead) * m.planned;
    }
    for (iri, e) in by_pc.iter_mut() {
        let (pb, pa) = profit_before[iri];
        e.extra_cost_pct_of_revenue = pct(e.extra_cost / e.revenue);
        e.extra_cost_pct_of_budget = if e.material_budget > 0.0 { pct(e.extra_cost / e.material_budget) } else { 0.0 };
        e.margin_before_pct = pct(pb / e.revenue);
        e.margin_after_pct = pct(pa / e.revenue);
    }
    let mut by_profit_center: Vec<_> = by_pc.into_values().collect();
    by_profit_center.sort_by(|a, b| b.extra_cost.total_cmp(&a.extra_cost));

    let mut by_contract = Vec::new();
    for c in &snap.contracts {
        let users = snap.models_using(&c.component);
        let units: f64 = users.iter().map(|m| m.planned).sum();
        let before = snap.price_at(c, asof, &none)?;
        let after = snap.price_at(c, asof, &shocks)?;
        if (after - before).abs() < 1e-9 {
            continue;
        }
        let mut drivers = Vec::new();
        for spec in &req.shocks {
            let single = build_shocks(snap, std::slice::from_ref(spec))?;
            let d = (snap.price_at(c, asof, &single)? - before) * units;
            if d.abs() > 1e-9 {
                drivers.push(Driver { shock: describe(snap, spec), extra_cost: d });
            }
        }
        by_contract.push(ContractImpact {
            iri: c.iri.clone(),
            label: c.label.clone(),
            supplier: snap.supplier_label(&c.supplier),
            price_before: before,
            price_after: after,
            annual_units: units,
            extra_cost: (after - before) * units,
            sourcing: c.sourcing.clone(),
            end: c.end.clone(),
            drivers,
        });
    }
    by_contract.sort_by(|a, b| b.extra_cost.total_cmp(&a.extra_cost));

    let revenue: f64 = by_profit_center.iter().map(|p| p.revenue).sum();
    let (pb, pa) = profit_before.values().fold((0.0, 0.0), |a, v| (a.0 + v.0, a.1 + v.1));
    let totals = ScenarioTotals {
        revenue,
        extra_cost: by_model.iter().map(|m| m.extra_cost).sum(),
        margin_before_pct: pct(pb / revenue),
        margin_after_pct: pct(pa / revenue),
        models_below_floor: by_model.iter().filter(|m| m.below_floor).count(),
        models_crossing_floor: by_model.iter().filter(|m| m.crossed_floor).count(),
    };

    Ok(ScenarioResult {
        asof: snap.asof.clone(),
        margin_floor_pct: pct(floor),
        shocks: req.shocks.clone(),
        totals,
        by_profit_center,
        by_model,
        by_contract,
    })
}

// ---------------------------------------------------------------- market series

#[derive(Serialize)]
pub struct SeriesOut {
    pub iri: String,
    pub label: String,
    /// "index" or "fx"
    pub kind: String,
    pub unit: String,
    pub latest: f64,
    pub change_6m_pct: f64,
    pub change_since_start_pct: f64,
    pub points: Vec<(String, f64)>,
}

pub fn series_overview(snap: &Snapshot) -> Vec<SeriesOut> {
    let make = |iri: &String, label: String, kind: &str, unit: String| {
        let points = snap.series_of(iri);
        let latest = points.last().map_or(0.0, |p| p.1);
        let six = points.len().checked_sub(7).and_then(|i| points.get(i)).map_or(latest, |p| p.1);
        let first = points.first().map_or(latest, |p| p.1);
        SeriesOut {
            iri: iri.clone(),
            label,
            kind: kind.into(),
            unit,
            latest,
            change_6m_pct: (latest / six - 1.0) * 100.0,
            change_since_start_pct: (latest / first - 1.0) * 100.0,
            points,
        }
    };
    let mut out: Vec<SeriesOut> = snap
        .index_labels
        .iter()
        .map(|(iri, label)| make(iri, label.clone(), "index", "index, Jan 2024 = 100".into()))
        .collect();
    out.extend(
        snap.currency_codes
            .iter()
            .filter(|(iri, _)| iri.as_str() != REPORTING_CURRENCY)
            .map(|(iri, code)| make(iri, format!("EUR/{code}"), "fx", format!("{code} per EUR"))),
    );
    out
}
