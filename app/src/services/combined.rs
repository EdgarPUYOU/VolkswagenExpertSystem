//! Q18: a combined "bad year" scenario. Commodity and FX shocks (priced through the contracts) and
//! the end of EV subsidies (demand loss) in one run, with the profit change per brand, the models hit
//! hardest and the contracts to renegotiate first.
//!
//! Profit change per model = -(extra material cost on the cars still sold + contribution lost on the
//! cars no longer sold). Extra material cost is scaled by the share of cars that are still sold, so
//! the two effects are not double counted.

use super::finance::*;
use super::market::{self, MarketData};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const TOP_MODELS: usize = 5;
const TOP_CONTRACTS: usize = 10;

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct CombinedRequest {
    #[serde(default)]
    pub shocks: Vec<ShockSpec>,
    #[serde(default)]
    pub subsidy_markets: Vec<String>,
    pub elasticity: Option<f64>,
    /// Fraction, 0.15 for 15%
    pub margin_floor: Option<f64>,
    pub save_as: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct CombinedTotals {
    pub revenue_before: f64,
    pub contribution_before: f64,
    pub extra_material_cost: f64,
    pub lost_contribution: f64,
    pub profit_change: f64,
    pub profit_change_pct: f64,
    pub lost_units: f64,
    pub lost_revenue: f64,
}

#[derive(Serialize, Debug)]
pub struct BrandImpact {
    pub label: String,
    pub contribution_before: f64,
    pub extra_material_cost: f64,
    pub lost_contribution: f64,
    pub profit_change: f64,
    pub profit_change_pct: f64,
}

#[derive(Serialize, Debug)]
pub struct HitModel {
    pub iri: String,
    pub label: String,
    pub brand: String,
    pub extra_material_cost: f64,
    pub lost_units: f64,
    pub lost_contribution: f64,
    pub profit_change: f64,
    pub margin_before_pct: f64,
    pub margin_after_pct: f64,
    pub below_floor: bool,
}

#[derive(Serialize, Debug)]
pub struct RenegotiationContract {
    pub priority: usize,
    pub iri: String,
    pub label: String,
    pub supplier: String,
    pub extra_cost: f64,
    pub sourcing: String,
    pub end: String,
    pub months_to_end: i32,
    pub drivers: Vec<Driver>,
}

#[derive(Serialize, Debug)]
pub struct CombinedResult {
    pub asof: String,
    pub margin_floor_pct: f64,
    pub shocks: Vec<ShockSpec>,
    pub subsidy_markets: Vec<String>,
    pub elasticity: f64,
    pub totals: CombinedTotals,
    pub by_brand: Vec<BrandImpact>,
    pub top_models: Vec<HitModel>,
    pub contracts: Vec<RenegotiationContract>,
}

fn month_index(s: &str) -> Option<i32> {
    let (y, rest) = s.split_once('-')?;
    let m = rest.get(..2)?;
    Some(y.parse::<i32>().ok()? * 12 + m.parse::<i32>().ok()?)
}

pub fn run_combined(snap: &Snapshot, md: &MarketData, req: &CombinedRequest) -> Result<CombinedResult> {
    if req.shocks.is_empty() && req.subsidy_markets.is_empty() {
        return bad("give at least one commodity or currency shock or one market whose subsidy ends");
    }
    let elasticity = req.elasticity.unwrap_or(market::DEFAULT_SUBSIDY_ELASTICITY);

    // an empty shock list still needs a scenario run for margins; a 0% move changes nothing
    let probe = [ShockSpec { kind: "commodity".into(), target: "NaturalRubber".into(), percent: 0.0 }];
    let scenario = run_scenario(
        snap,
        &ScenarioRequest { shocks: if req.shocks.is_empty() { probe.to_vec() } else { req.shocks.clone() }, margin_floor: req.margin_floor, save_as: None },
    )?;

    // demand loss per model from every subsidy that ends
    let mut markets = Vec::new();
    let mut lost: BTreeMap<String, (f64, f64, f64)> = BTreeMap::new(); // units, revenue, contribution
    for name in &req.subsidy_markets {
        let r = market::subsidy_removal(snap, md, name, elasticity)?;
        markets.push(r.market.clone());
        for m in r.models {
            let e = lost.entry(m.iri).or_default();
            e.0 += m.lost_units;
            e.1 += m.lost_revenue;
            e.2 += m.lost_contribution;
        }
    }

    let mut hit = Vec::new();
    let mut by_brand: BTreeMap<String, BrandImpact> = BTreeMap::new();
    let mut totals = CombinedTotals { revenue_before: 0.0, contribution_before: 0.0, extra_material_cost: 0.0, lost_contribution: 0.0, profit_change: 0.0, profit_change_pct: 0.0, lost_units: 0.0, lost_revenue: 0.0 };
    for mi in &scenario.by_model {
        let (units, revenue, contribution) = lost.get(&mi.iri).copied().unwrap_or_default();
        let sold_share = if mi.planned > 0.0 { (1.0 - units / mi.planned).max(0.0) } else { 1.0 };
        let extra = mi.extra_cost * sold_share;
        let change = -(extra + contribution);
        let before = (mi.price - mi.material_before - mi.overhead) * mi.planned;
        totals.revenue_before += mi.price * mi.planned;
        totals.contribution_before += before;
        totals.extra_material_cost += extra;
        totals.lost_contribution += contribution;
        totals.lost_units += units;
        totals.lost_revenue += revenue;
        let b = by_brand.entry(mi.brand.clone()).or_insert_with(|| BrandImpact { label: mi.brand.clone(), contribution_before: 0.0, extra_material_cost: 0.0, lost_contribution: 0.0, profit_change: 0.0, profit_change_pct: 0.0 });
        b.contribution_before += before;
        b.extra_material_cost += extra;
        b.lost_contribution += contribution;
        b.profit_change += change;
        hit.push(HitModel {
            iri: mi.iri.clone(),
            label: mi.label.clone(),
            brand: mi.brand.clone(),
            extra_material_cost: extra,
            lost_units: units,
            lost_contribution: contribution,
            profit_change: change,
            margin_before_pct: mi.margin_before_pct,
            margin_after_pct: mi.margin_after_pct,
            below_floor: mi.below_floor,
        });
    }
    totals.profit_change = -(totals.extra_material_cost + totals.lost_contribution);
    totals.profit_change_pct = if totals.contribution_before > 0.0 { totals.profit_change / totals.contribution_before * 100.0 } else { 0.0 };

    let mut by_brand: Vec<BrandImpact> = by_brand.into_values().collect();
    for b in &mut by_brand {
        b.profit_change_pct = if b.contribution_before > 0.0 { b.profit_change / b.contribution_before * 100.0 } else { 0.0 };
    }
    by_brand.sort_by(|a, b| a.profit_change.total_cmp(&b.profit_change));
    hit.sort_by(|a, b| a.profit_change.total_cmp(&b.profit_change));
    hit.truncate(TOP_MODELS);

    let now = month_index(&snap.asof).unwrap_or(0);
    let contracts = scenario
        .by_contract
        .into_iter()
        .take(TOP_CONTRACTS)
        .enumerate()
        .map(|(i, c)| RenegotiationContract {
            priority: i + 1,
            months_to_end: month_index(&c.end).map_or(0, |e| e - now),
            iri: c.iri,
            label: c.label,
            supplier: c.supplier,
            extra_cost: c.extra_cost,
            sourcing: c.sourcing,
            end: c.end,
            drivers: c.drivers,
        })
        .collect();

    Ok(CombinedResult {
        asof: scenario.asof,
        margin_floor_pct: scenario.margin_floor_pct,
        shocks: req.shocks.clone(),
        subsidy_markets: markets,
        elasticity,
        totals,
        by_brand,
        top_models: hit,
        contracts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kg::Kg;
    use rusqlite::Connection;
    use std::path::Path;

    fn load() -> (Snapshot, MarketData) {
        let (kg, _, _) = Kg::load(Path::new(".."), Path::new("../rules")).unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        crate::seed::seed(&kg, &mut conn).unwrap();
        (Snapshot::load(&kg, &conn).unwrap(), MarketData::load(&kg, &conn).unwrap())
    }

    fn shock(kind: &str, target: &str, percent: f64) -> ShockSpec {
        ShockSpec { kind: kind.into(), target: target.into(), percent }
    }

    fn bad_year() -> CombinedRequest {
        CombinedRequest {
            shocks: vec![shock("commodity", "NaturalRubber", 15.0), shock("fx", "SEK", 5.0)],
            subsidy_markets: vec!["Germany".into()],
            ..Default::default()
        }
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6 * b.abs().max(1.0)
    }

    #[test]
    fn q18_bad_year_adds_cost_and_lost_demand() {
        let (s, md) = load();
        let r = run_combined(&s, &md, &bad_year()).unwrap();
        assert_eq!(r.subsidy_markets, vec!["Germany"]);
        assert!(near(r.totals.profit_change, -(r.totals.extra_material_cost + r.totals.lost_contribution)));
        assert!(r.totals.profit_change < -4.0e8 && r.totals.profit_change > -6.0e8, "{}", r.totals.profit_change);
        assert!(r.totals.lost_units > 20_000.0);
        // brands sum to the totals, worst first
        assert!(near(r.by_brand.iter().map(|b| b.profit_change).sum::<f64>(), r.totals.profit_change));
        assert!(r.by_brand.windows(2).all(|w| w[0].profit_change <= w[1].profit_change));
        assert_eq!(r.by_brand.len(), 7);
        assert_eq!(r.top_models.len(), 5);
        assert!(r.top_models.windows(2).all(|w| w[0].profit_change <= w[1].profit_change));
        let names: Vec<&str> = r.top_models.iter().map(|m| m.label.as_str()).collect();
        assert!(names.contains(&"Enyaq") && names.contains(&"ID.4"), "subsidy hits the EVs: {names:?}");
        assert!((3..=10).contains(&r.contracts.len()) && r.contracts[0].priority == 1, "{}", r.contracts.len());
        assert!(r.contracts.windows(2).all(|w| w[0].extra_cost >= w[1].extra_cost));
        assert!(r.contracts.iter().all(|c| !c.drivers.is_empty()));
    }

    #[test]
    fn the_two_effects_are_separable() {
        let (s, md) = load();
        let shocks_only = run_combined(&s, &md, &CombinedRequest { subsidy_markets: vec![], ..bad_year() }).unwrap();
        assert_eq!(shocks_only.totals.lost_contribution, 0.0);
        let plain = run_scenario(&s, &ScenarioRequest { shocks: bad_year().shocks, margin_floor: None, save_as: None }).unwrap();
        assert!(near(shocks_only.totals.extra_material_cost, plain.totals.extra_cost), "no lost demand, so no scaling");
        let subsidy_only = run_combined(&s, &md, &CombinedRequest { shocks: vec![], subsidy_markets: vec!["Germany".into()], ..Default::default() }).unwrap();
        assert_eq!(subsidy_only.totals.extra_material_cost, 0.0);
        assert!(subsidy_only.contracts.is_empty());
        let both = run_combined(&s, &md, &bad_year()).unwrap();
        assert!(both.totals.profit_change < shocks_only.totals.profit_change && both.totals.profit_change < subsidy_only.totals.profit_change);
        assert!(both.totals.extra_material_cost < shocks_only.totals.extra_material_cost, "cars not sold need no extra material");
    }

    #[test]
    fn rejects_empty_and_unknown_requests() {
        let (s, md) = load();
        assert!(run_combined(&s, &md, &CombinedRequest::default()).is_err());
        assert!(run_combined(&s, &md, &CombinedRequest { subsidy_markets: vec!["Atlantis".into()], ..Default::default() }).is_err());
        assert!(run_combined(&s, &md, &CombinedRequest { shocks: vec![shock("commodity", "Unobtainium", 5.0)], ..Default::default() }).is_err());
        assert!(run_combined(&s, &md, &CombinedRequest { elasticity: Some(-1.0), ..bad_year() }).is_err());
    }
}
