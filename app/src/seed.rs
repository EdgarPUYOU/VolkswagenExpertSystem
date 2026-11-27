//! Deterministic synthetic data for the finance tables. Everything here is illustrative.
//!
//! Index and FX series are piecewise-linear paths through hand-picked anchor points plus a small
//! repeatable wobble, so the story is controlled: rubber, copper and energy have risen, lithium has
//! fallen, the Swedish krona has strengthened. Overhead per model is solved so that each model sits
//! at a chosen contribution margin at the latest month, which makes margin questions meaningful.

use crate::kg::Kg;
use crate::services::finance::{short, Shocks, Snapshot, NS};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection};
use std::collections::HashMap;

const MONTHS: usize = 35; // 2024-01 ..= 2026-11

/// (index name, anchors as (month number, level), wobble)
type Anchors = &'static [(usize, f64)];

const INDICES: &[(&str, Anchors, f64)] = &[
    ("NaturalRubber", &[(0, 100.0), (12, 104.0), (18, 108.0), (28, 113.0), (34, 132.0)], 0.008),
    ("Steel", &[(0, 100.0), (12, 96.0), (24, 98.0), (34, 106.0)], 0.008),
    ("Aluminium", &[(0, 100.0), (12, 102.0), (28, 108.0), (34, 109.0)], 0.008),
    ("Lithium", &[(0, 100.0), (12, 70.0), (24, 62.0), (34, 68.0)], 0.012),
    ("Copper", &[(0, 100.0), (12, 104.0), (28, 112.0), (34, 118.0)], 0.008),
    ("EnergyEU", &[(0, 100.0), (12, 95.0), (28, 100.0), (34, 112.0)], 0.01),
];

/// (currency, anchors as (month number, units per EUR), wobble)
const FX: &[(&str, Anchors, f64)] = &[
    ("SEK", &[(0, 11.5), (12, 11.2), (28, 10.9), (34, 10.7)], 0.003),
    ("CZK", &[(0, 24.7), (12, 25.2), (28, 24.9), (34, 24.4)], 0.003),
    ("USD", &[(0, 1.09), (12, 1.04), (28, 1.12), (34, 1.15)], 0.003),
];

/// Target contribution margin per model at the latest month, before any shock.
const TARGET_MARGIN: &[(&str, f64)] = &[
    ("Golf", 0.172), ("Passat", 0.168), ("Octavia", 0.170), ("ID4", 0.153), ("Enyaq", 0.152),
    ("A4", 0.210), ("Q5", 0.225), ("e-tron", 0.154), ("911", 0.310), ("Cayenne", 0.270),
    ("Taycan", 0.165), ("Huracan", 0.360), ("Urus", 0.330), ("RSeries", 0.155), ("TGX", 0.152),
];

/// How non-material cost per vehicle splits over cost elements.
const OVERHEAD_SPLIT: &[(&str, f64)] =
    &[("CE_Labour", 0.48), ("CE_Logistics", 0.12), ("CE_Warranty", 0.14), ("CE_Marketing", 0.26)];

const BUDGET_YEAR: i32 = 2027;
const ACTUAL_QUARTERS: &[(&str, &str)] = &[("2026-Q1", "2026-02"), ("2026-Q2", "2026-05"), ("2026-Q3", "2026-08")];

/// Relative weight of each market when a model's planned units are split over the markets it is sold in.
const MARKET_WEIGHT: &[(&str, f64)] =
    &[("Market_Germany", 0.38), ("Market_CzechRepublic", 0.12), ("Market_France", 0.20), ("Market_Sweden", 0.10), ("Market_USA", 0.20)];

/// Competitor list prices in EUR (illustrative).
const COMPETITOR_PRICES: &[(&str, f64)] = &[
    ("Tesla_ModelY", 47000.0), ("BYD_Atto3", 38000.0), ("Mercedes_EQE_SUV", 72000.0), ("Tesla_ModelS", 95000.0),
    ("Mercedes_CClass", 46000.0), ("BMW_3Series", 44000.0), ("Toyota_Corolla", 27000.0), ("BMW_X3", 56000.0),
];

/// Campaign spend and expected extra cars (illustrative).
const CAMPAIGNS: &[(&str, f64, f64)] = &[
    ("Campaign_ID4_Conquest", 25e6, 8000.0), ("Campaign_Enyaq_Family", 12e6, 6000.0), ("Campaign_Golf_Loyalty", 20e6, 12000.0),
    ("Campaign_Taycan_TestDrive", 6e6, 1500.0), ("Campaign_Q5_Winter", 9e6, 5000.0), ("Campaign_Octavia_Fleet", 11e6, 7000.0),
];

/// Annual capacity as a multiple of planned production. Models not listed get `DEFAULT_CAPACITY_FACTOR`.
const CAPACITY_FACTOR: &[(&str, f64)] = &[("ID4", 1.0), ("Enyaq", 1.03), ("Taycan", 1.0), ("e-tron", 1.01), ("Golf", 1.12), ("Octavia", 1.15), ("Q5", 1.08)];
const DEFAULT_CAPACITY_FACTOR: f64 = 1.1;

/// (product, term months, rate, funding cost, default loss, residual, residual risk, subvention, down payment), all percent.
#[allow(clippy::type_complexity)]
const FINANCING: &[(&str, i64, f64, f64, f64, f64, f64, f64, f64)] = &[
    ("Fin_Taycan_Lease36", 36, 6.9, 3.6, 0.6, 58.0, 6.0, 0.5, 0.0),
    ("Fin_Taycan_Lease48", 48, 6.9, 3.7, 0.7, 50.0, 7.0, 0.5, 0.0),
    ("Fin_Taycan_Loan48", 48, 6.9, 3.9, 1.4, 0.0, 0.0, 0.0, 20.0),
    ("Fin_Taycan_Loan60", 60, 7.4, 4.1, 1.9, 0.0, 0.0, 0.0, 20.0),
];

pub fn month_label(i: usize) -> String {
    format!("{}-{:02}", 2024 + i / 12, i % 12 + 1)
}

struct Lcg(u64);
impl Lcg {
    fn new(seed: &str) -> Lcg {
        let mut h = 1469598103934665603u64;
        for b in seed.bytes() {
            h = (h ^ b as u64).wrapping_mul(1099511628211);
        }
        Lcg(h)
    }
    /// Uniform in [-1, 1).
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0
    }
}

fn path(name: &str, anchors: &[(usize, f64)], wobble: f64) -> Vec<f64> {
    let mut rng = Lcg::new(name);
    (0..MONTHS)
        .map(|i| {
            let w = anchors.windows(2).find(|w| i >= w[0].0 && i <= w[1].0).unwrap();
            let t = (i - w[0].0) as f64 / (w[1].0 - w[0].0) as f64;
            let level = w[0].1 + (w[1].1 - w[0].1) * t;
            // keep the first and last month exact so the story's end points hold
            let noise = if i == 0 || i == MONTHS - 1 { 0.0 } else { wobble * rng.next() };
            level * (1.0 + noise)
        })
        .collect()
}

#[derive(Debug)]
#[allow(dead_code)] // fields are reported through Debug
pub struct SeedStats {
    pub index_rows: usize,
    pub fx_rows: usize,
    pub model_cost_rows: usize,
    pub budget_rows: usize,
    pub actual_rows: usize,
}

pub fn seed(kg: &Kg, conn: &mut Connection) -> Result<SeedStats> {
    let mut index_values = HashMap::new();
    for (name, anchors, wobble) in INDICES {
        let iri = format!("{NS}Index_{name}");
        for (i, v) in path(name, anchors, *wobble).into_iter().enumerate() {
            index_values.insert((iri.clone(), month_label(i)), v);
        }
    }
    let mut fx = HashMap::new();
    for (name, anchors, wobble) in FX {
        let iri = format!("{NS}{name}");
        for (i, v) in path(name, anchors, *wobble).into_iter().enumerate() {
            fx.insert((iri.clone(), month_label(i)), v);
        }
    }

    let snap = Snapshot::load(kg, conn)?.with_series(index_values.clone(), fx.clone());
    let asof = snap.asof.clone();
    let none = Shocks::default();
    let targets: HashMap<&str, f64> = TARGET_MARGIN.iter().copied().collect();

    // overhead per vehicle: solve for the target margin at the latest month
    let mut model_cost: Vec<(String, String, f64)> = Vec::new();
    let mut overhead: HashMap<String, f64> = HashMap::new();
    for m in &snap.models {
        let target = *targets.get(short(&m.iri).as_str()).with_context(|| format!("no target margin for {}", m.label))?;
        let oh = m.price * (1.0 - target) - snap.material_cost(m, &asof, &none)?;
        ensure!(oh > 0.0, "model {} has material cost above its target cost", m.label);
        overhead.insert(m.iri.clone(), oh);
        for (el, share) in OVERHEAD_SPLIT {
            model_cost.push((m.iri.clone(), format!("{NS}{el}"), oh * share));
        }
    }

    // budget (next year, contract base prices) and actuals (this year so far, effective prices)
    let mut budget: Vec<(String, String, String, f64)> = Vec::new();
    let mut actuals: Vec<(String, String, String, f64)> = Vec::new();
    for q in 1..=4 {
        let period = format!("{BUDGET_YEAR}-Q{q}");
        for pc in snap.models.iter().map(|m| m.pc_iri.clone()).collect::<std::collections::BTreeSet<_>>() {
            let (mut material, mut other) = (0.0, 0.0);
            for m in snap.models.iter().filter(|m| m.pc_iri == pc) {
                let base: f64 = snap
                    .model_components
                    .get(&m.iri)
                    .map(|cs| cs.iter().filter_map(|c| snap.contract_for(c)).map(|c| c.base_price).sum())
                    .unwrap_or(0.0);
                material += base * m.planned / 4.0;
                other += overhead[&m.iri] * m.planned / 4.0;
            }
            budget.push((pc.clone(), period.clone(), format!("{NS}CE_DirectMaterial"), material));
            for (el, share) in OVERHEAD_SPLIT {
                budget.push((pc.clone(), period.clone(), format!("{NS}{el}"), other * share));
            }
        }
    }
    for (period, month) in ACTUAL_QUARTERS {
        for pc in snap.models.iter().map(|m| m.pc_iri.clone()).collect::<std::collections::BTreeSet<_>>() {
            let (mut material, mut other) = (0.0, 0.0);
            for m in snap.models.iter().filter(|m| m.pc_iri == pc) {
                material += snap.material_cost(m, month, &none)? * m.planned / 4.0;
                other += overhead[&m.iri] * m.planned / 4.0;
            }
            actuals.push((pc.clone(), period.to_string(), format!("{NS}CE_DirectMaterial"), material));
            for (el, share) in OVERHEAD_SPLIT {
                let mut rng = Lcg::new(&format!("{pc}{period}{el}"));
                actuals.push((pc.clone(), period.to_string(), format!("{NS}{el}"), other * share * (1.0 + 0.03 * rng.next())));
            }
        }
    }

    // planned units per model and market: the model's planned production split over the markets the KG says it is sold in
    let mut sold: HashMap<String, Vec<String>> = HashMap::new();
    for r in kg.select("PREFIX vw: <http://example.org/volkswagen/> SELECT ?m ?mk WHERE { ?m vw:soldInMarket ?mk } ORDER BY ?m ?mk")? {
        sold.entry(r["m"].clone()).or_default().push(r["mk"].clone());
    }
    let weight = |mk: &str| MARKET_WEIGHT.iter().find(|(n, _)| format!("{NS}{n}") == mk).map_or(0.1, |(_, w)| *w);
    let mut sales_plan: Vec<(String, String, f64)> = Vec::new();
    for m in &snap.models {
        let markets = sold.get(&m.iri).with_context(|| format!("model {} is sold in no market", m.label))?;
        let total: f64 = markets.iter().map(|mk| weight(mk)).sum();
        for mk in markets {
            sales_plan.push((m.iri.clone(), mk.clone(), (m.planned * weight(mk) / total).round()));
        }
    }

    let tx = conn.transaction()?;
    for table in ["index_values", "fx_rates", "model_cost", "budget", "actuals", "sales_plan", "competitor_prices", "campaign_spend", "model_capacity", "financing_terms"] {
        tx.execute(&format!("DELETE FROM {table}"), [])?;
    }
    for ((iri, month), v) in &index_values {
        tx.execute("INSERT INTO index_values (index_iri, month, value) VALUES (?1, ?2, ?3)", params![iri, month, v])?;
    }
    for ((iri, month), v) in &fx {
        tx.execute("INSERT INTO fx_rates (currency_iri, month, per_eur) VALUES (?1, ?2, ?3)", params![iri, month, v])?;
    }
    for (m, el, v) in &model_cost {
        tx.execute("INSERT INTO model_cost (model_iri, cost_element_iri, eur_per_unit) VALUES (?1, ?2, ?3)", params![m, el, v])?;
    }
    for (pc, period, el, v) in &budget {
        tx.execute("INSERT INTO budget (profit_center_iri, period, cost_element_iri, amount_eur) VALUES (?1, ?2, ?3, ?4)", params![pc, period, el, v])?;
    }
    for (pc, period, el, v) in &actuals {
        tx.execute("INSERT INTO actuals (profit_center_iri, period, cost_element_iri, amount_eur) VALUES (?1, ?2, ?3, ?4)", params![pc, period, el, v])?;
    }
    for (m, mk, u) in &sales_plan {
        tx.execute("INSERT INTO sales_plan (model_iri, market_iri, units) VALUES (?1, ?2, ?3)", params![m, mk, u])?;
    }
    for (name, price) in COMPETITOR_PRICES {
        tx.execute("INSERT INTO competitor_prices (competitor_model_iri, price_eur) VALUES (?1, ?2)", params![format!("{NS}{name}"), price])?;
    }
    for (name, spend, units) in CAMPAIGNS {
        tx.execute("INSERT INTO campaign_spend (campaign_iri, spend_eur, expected_incremental_units) VALUES (?1, ?2, ?3)", params![format!("{NS}{name}"), spend, units])?;
    }
    for m in &snap.models {
        let factor = CAPACITY_FACTOR.iter().find(|(n, _)| *n == short(&m.iri)).map_or(DEFAULT_CAPACITY_FACTOR, |(_, f)| *f);
        tx.execute("INSERT INTO model_capacity (model_iri, annual_capacity) VALUES (?1, ?2)", params![m.iri, (m.planned * factor).round()])?;
    }
    for (name, term, rate, funding, default, residual, residual_risk, subvention, down) in FINANCING {
        tx.execute(
            "INSERT INTO financing_terms (product_iri, term_months, rate_pct, funding_cost_pct, default_loss_pct, residual_pct, residual_risk_pct, subvention_pct, down_payment_pct)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![format!("{NS}{name}"), term, rate, funding, default, residual, residual_risk, subvention, down],
        )?;
    }
    tx.commit()?;

    Ok(SeedStats {
        index_rows: index_values.len(),
        fx_rows: fx.len(),
        model_cost_rows: model_cost.len(),
        budget_rows: budget.len(),
        actual_rows: actuals.len(),
    })
}

pub fn needs_seed(conn: &Connection) -> Result<bool> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM index_values", [], |r| r.get(0))?;
    let m: i64 = conn.query_row("SELECT COUNT(*) FROM sales_plan", [], |r| r.get(0))?;
    let c: i64 = conn.query_row("SELECT COUNT(*) FROM campaign_spend", [], |r| r.get(0))?;
    Ok(n == 0 || m == 0 || c == 0)
}
