//! Sales questions (Q15 to Q17): leads on models under margin pressure, campaigns against
//! production capacity, lease versus loan profitability. Rates and capacities are illustrative
//! inputs in SQLite; the graph says which campaign and product belongs to which model and market.

use super::finance::*;
use super::market::MarketData;
use super::portfolio::run;
use crate::kg::Kg;
use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;

/// A model "fell" when its margin lost at least this many points over the look-back window.
const FALL_PP: f64 = 0.3;
/// Average outstanding share of an amortising loan over its life.
const LOAN_AVG_BALANCE: f64 = 0.55;

#[derive(Clone, Debug)]
pub struct Campaign {
    pub iri: String,
    pub label: String,
    pub model: String,
    pub spend: f64,
    pub incremental_units: f64,
}

#[derive(Clone, Debug)]
pub struct Product {
    pub iri: String,
    pub label: String,
    pub kind: String,
    pub model: String,
    pub market: String,
    pub term_months: f64,
    pub rate: f64,
    pub funding: f64,
    pub default_loss: f64,
    pub residual: f64,
    pub residual_risk: f64,
    pub subvention: f64,
    pub down: f64,
}

pub struct SalesData {
    pub campaigns: Vec<Campaign>,
    pub capacity: HashMap<String, f64>,
    pub products: Vec<Product>,
}

impl SalesData {
    pub fn load(kg: &Kg, conn: &Connection) -> Result<SalesData> {
        let spend: HashMap<String, (f64, f64)> = {
            let mut stmt = conn.prepare("SELECT campaign_iri, spend_eur, expected_incremental_units FROM campaign_spend")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, f64>(1)?, r.get::<_, f64>(2)?))))?;
            rows.collect::<Result<_, _>>()?
        };
        let campaigns = run(kg, "SELECT ?c ?l ?m WHERE { ?c a vw:IncentiveCampaign ; rdfs:label ?l ; vw:campaignAppliesTo ?m } ORDER BY ?c")?
            .iter()
            .filter_map(|r| {
                let (s, u) = spend.get(&r["c"]).copied()?;
                Some(Campaign { iri: r["c"].clone(), label: r["l"].clone(), model: r["m"].clone(), spend: s, incremental_units: u })
            })
            .collect();
        let capacity = {
            let mut stmt = conn.prepare("SELECT model_iri, annual_capacity FROM model_capacity")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut terms: HashMap<String, [f64; 8]> = HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT product_iri, term_months, rate_pct, funding_cost_pct, default_loss_pct, residual_pct, residual_risk_pct, subvention_pct, down_payment_pct FROM financing_terms",
        )?;
        for row in stmt.query_map([], |r| {
            let mut v = [0.0; 8];
            for (i, slot) in v.iter_mut().enumerate() {
                *slot = r.get::<_, f64>(i + 1)?;
            }
            Ok((r.get::<_, String>(0)?, v))
        })? {
            let (k, v) = row?;
            terms.insert(k, v);
        }
        let products = run(
            kg,
            "SELECT ?p ?l ?t ?m ?mk WHERE { ?p a vw:FinancingProduct ; rdfs:label ?l ; vw:financingFor ?m ; vw:offeredInMarket ?mk ;
                 a ?t . FILTER(?t IN (vw:Lease, vw:Loan)) } ORDER BY ?p",
        )?
        .iter()
        .filter_map(|r| {
            let t = terms.get(&r["p"])?;
            Some(Product {
                iri: r["p"].clone(),
                label: r["l"].clone(),
                kind: short(&r["t"]),
                model: r["m"].clone(),
                market: r["mk"].clone(),
                term_months: t[0],
                rate: t[1],
                funding: t[2],
                default_loss: t[3],
                residual: t[4],
                residual_risk: t[5],
                subvention: t[6],
                down: t[7],
            })
        })
        .collect();
        Ok(SalesData { campaigns, capacity, products })
    }
}

fn margin_pct(snap: &Snapshot, m: &Model, month: &str) -> Result<f64> {
    let cost = snap.material_cost(m, month, &Shocks::default())? + snap.overhead_of(m);
    Ok((m.price - cost) / m.price * 100.0)
}

fn month_minus(month: &str, n: u32) -> Result<String> {
    let (y, mo) = month.split_once('-').and_then(|(y, m)| Some((y.parse::<i32>().ok()?, m.parse::<i32>().ok()?))).ok_or_else(|| BadRequest(format!("bad month '{month}'")))?;
    let idx = y * 12 + (mo - 1) - n as i32;
    Ok(format!("{}-{:02}", idx.div_euclid(12), idx.rem_euclid(12) + 1))
}

// ---------------------------------------------------------------- Q15

#[derive(Serialize)]
pub struct LeadRow {
    pub customer: String,
    pub model_iri: String,
    pub model: String,
    pub margin_pct: f64,
    pub margin_change_pp: f64,
    pub reasons: Vec<String>,
    pub pressure: bool,
    pub revenue: f64,
}

#[derive(Serialize)]
pub struct LeadPressure {
    pub asof: String,
    pub floor_pct: f64,
    pub buffer_pp: f64,
    pub months: u32,
    pub pressured_count: usize,
    pub leads: Vec<LeadRow>,
}

/// Q15. A model is under pressure when its margin is within `buffer_pp` of the floor (or below it),
/// or has fallen at least 0.3 points over the look-back window.
pub fn lead_pressure(snap: &Snapshot, md: &MarketData, floor_pct: f64, buffer_pp: f64, months: u32) -> Result<LeadPressure> {
    if !(0.0..100.0).contains(&floor_pct) || buffer_pp < 0.0 || !(1..=30).contains(&months) {
        return bad("floor must be 0 to 100 percent, buffer not negative, months 1 to 30");
    }
    let from = month_minus(&snap.asof, months)?;
    let mut leads = Vec::new();
    for (customer, model_iri) in &snap.leads {
        let Some(m) = snap.models.iter().find(|m| &m.iri == model_iri) else { continue };
        let now = margin_pct(snap, m, &snap.asof)?;
        let then = margin_pct(snap, m, &from).map_err(|_| BadRequest(format!("no price history back to {from}")))?;
        let change = now - then;
        let mut reasons = Vec::new();
        if now < floor_pct {
            reasons.push(format!("margin {now:.1}% is below the {floor_pct:.0}% floor"));
        } else if now < floor_pct + buffer_pp {
            reasons.push(format!("margin {now:.1}% is within {buffer_pp:.1} points of the {floor_pct:.0}% floor"));
        }
        if change <= -FALL_PP {
            reasons.push(format!("margin fell {:.2} points in {months} months", -change));
        }
        let pressure = !reasons.is_empty();
        if md.is_electric(model_iri) {
            reasons.push("depends on EV subsidy".into());
        }
        leads.push(LeadRow {
            customer: customer.clone(),
            model_iri: model_iri.clone(),
            model: m.label.clone(),
            margin_pct: now,
            margin_change_pp: change,
            reasons,
            pressure,
            revenue: m.price * m.planned,
        });
    }
    leads.sort_by(|a, b| b.pressure.cmp(&a.pressure).then(a.margin_pct.total_cmp(&b.margin_pct)));
    Ok(LeadPressure { asof: snap.asof.clone(), floor_pct, buffer_pp, months, pressured_count: leads.iter().filter(|l| l.pressure).count(), leads })
}

// ---------------------------------------------------------------- Q16

#[derive(Serialize)]
pub struct CampaignRow {
    pub iri: String,
    pub label: String,
    pub model: String,
    pub spend: f64,
    pub expected_incremental_units: f64,
    pub planned_units: f64,
    pub capacity_units: f64,
    pub utilisation_pct: f64,
    pub headroom_units: f64,
    pub at_capacity: bool,
    pub wasted_spend: f64,
}

#[derive(Serialize)]
pub struct CampaignCapacity {
    pub threshold_pct: f64,
    pub total_spend: f64,
    pub wasted_spend: f64,
    pub campaigns: Vec<CampaignRow>,
}

/// Q16. Extra cars a campaign sells beyond the model's headroom cannot be built; the matching share
/// of the spend is wasted.
pub fn campaign_capacity(snap: &Snapshot, sd: &SalesData, threshold_pct: f64) -> Result<CampaignCapacity> {
    if !(0.0..=200.0).contains(&threshold_pct) {
        return bad("threshold must be between 0 and 200 percent");
    }
    let mut rows = Vec::new();
    for c in &sd.campaigns {
        let Some(m) = snap.models.iter().find(|m| m.iri == c.model) else { continue };
        let capacity = sd.capacity.get(&m.iri).copied().unwrap_or(m.planned);
        let headroom = (capacity - m.planned).max(0.0);
        let excess_share = if c.incremental_units > 0.0 { ((c.incremental_units - headroom) / c.incremental_units).clamp(0.0, 1.0) } else { 0.0 };
        let util = m.planned / capacity * 100.0;
        rows.push(CampaignRow {
            iri: c.iri.clone(),
            label: c.label.clone(),
            model: m.label.clone(),
            spend: c.spend,
            expected_incremental_units: c.incremental_units,
            planned_units: m.planned,
            capacity_units: capacity,
            utilisation_pct: util,
            headroom_units: headroom,
            at_capacity: util >= threshold_pct,
            wasted_spend: c.spend * excess_share,
        });
    }
    rows.sort_by(|a, b| b.wasted_spend.total_cmp(&a.wasted_spend).then(b.spend.total_cmp(&a.spend)));
    Ok(CampaignCapacity { threshold_pct, total_spend: rows.iter().map(|r| r.spend).sum(), wasted_spend: rows.iter().map(|r| r.wasted_spend).sum(), campaigns: rows })
}

// ---------------------------------------------------------------- Q17

#[derive(Serialize)]
pub struct ProductRow {
    pub iri: String,
    pub label: String,
    pub kind: String,
    pub term_months: f64,
    pub rate_pct: f64,
    pub income: f64,
    pub funding_cost: f64,
    pub risk_cost: f64,
    pub subvention_cost: f64,
    pub profit: f64,
    pub profit_pct_of_price: f64,
}

#[derive(Serialize)]
pub struct FinancingComparison {
    pub model: String,
    pub market: String,
    pub price: f64,
    pub best: String,
    pub best_kind: String,
    /// Profit of the best product minus profit of the best product of the other kind.
    pub kind_gap: f64,
    pub products: Vec<ProductRow>,
}

/// Q17. Profit per contract of each financing product, on top of the vehicle margin.
/// Loan: interest on the average balance, less funding cost, default loss and subvention.
/// Lease: finance charge on the average of price and residual, less funding cost, residual-value
/// risk, default loss and subvention.
pub fn financing_comparison(snap: &Snapshot, md: &MarketData, sd: &SalesData, model: &str, market: &str) -> Result<FinancingComparison> {
    let m = snap.resolve_model(model)?;
    let mk = md.resolve_market(market)?;
    let mut rows: Vec<ProductRow> = sd
        .products
        .iter()
        .filter(|p| p.model == m.iri && p.market == mk.iri)
        .map(|p| {
            let years = p.term_months / 12.0;
            let price = m.price;
            let (income, funding, risk) = if p.kind == "Loan" {
                let principal = price * (1.0 - p.down / 100.0);
                let base = principal * years * LOAN_AVG_BALANCE;
                (base * p.rate / 100.0, base * p.funding / 100.0, principal * p.default_loss / 100.0)
            } else {
                let residual = price * p.residual / 100.0;
                let base = (price + residual) / 2.0 * years;
                (base * p.rate / 100.0, base * p.funding / 100.0, residual * p.residual_risk / 100.0 + price * p.default_loss / 100.0)
            };
            let subvention = price * p.subvention / 100.0;
            let profit = income - funding - risk - subvention;
            ProductRow {
                iri: p.iri.clone(),
                label: p.label.clone(),
                kind: p.kind.clone(),
                term_months: p.term_months,
                rate_pct: p.rate,
                income,
                funding_cost: funding,
                risk_cost: risk,
                subvention_cost: subvention,
                profit,
                profit_pct_of_price: profit / price * 100.0,
            }
        })
        .collect();
    if rows.is_empty() {
        return bad(format!("no financing product for {} in {}", m.label, mk.label));
    }
    rows.sort_by(|a, b| b.profit.total_cmp(&a.profit));
    let best = &rows[0];
    let other = rows.iter().find(|r| r.kind != best.kind).map_or(0.0, |r| r.profit);
    Ok(FinancingComparison {
        model: m.label.clone(),
        market: mk.label.clone(),
        price: m.price,
        best: best.label.clone(),
        best_kind: best.kind.clone(),
        kind_gap: best.profit - other,
        products: rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load() -> (Snapshot, MarketData, SalesData) {
        let (kg, mut conn) = crate::kg::load_for_test();
        crate::seed::seed(&kg, &mut conn).unwrap();
        (Snapshot::load(&kg, &conn).unwrap(), MarketData::load(&kg, &conn).unwrap(), SalesData::load(&kg, &conn).unwrap())
    }

    #[test]
    fn month_arithmetic_crosses_years() {
        assert_eq!(month_minus("2026-11", 6).unwrap(), "2026-05");
        assert_eq!(month_minus("2026-02", 3).unwrap(), "2025-11");
        assert!(month_minus("nonsense", 1).is_err());
    }

    #[test]
    fn q15_both_leads_sit_on_models_under_pressure() {
        let (s, md, _) = load();
        let r = lead_pressure(&s, &md, 15.0, 1.0, 6).unwrap();
        assert_eq!(r.leads.len(), 2);
        assert_eq!(r.pressured_count, 2);
        for l in &r.leads {
            assert!(l.pressure && l.reasons.iter().any(|x| x.contains("floor")) && l.reasons.iter().any(|x| x.contains("fell")), "{:?}", l.reasons);
            assert!(l.reasons.iter().any(|x| x.contains("subsidy")));
        }
        let easy = lead_pressure(&s, &md, 10.0, 0.0, 1).unwrap();
        assert!(easy.leads.iter().all(|l| !l.pressure || l.margin_change_pp <= -0.3), "a low floor leaves only falling margins");
        assert!(lead_pressure(&s, &md, 15.0, 1.0, 0).is_err());
        assert!(lead_pressure(&s, &md, 15.0, 1.0, 99).is_err());
        assert!(lead_pressure(&s, &md, 150.0, 1.0, 6).is_err());
    }

    #[test]
    fn q16_campaigns_on_full_models_waste_their_spend() {
        let (s, _, sd) = load();
        let r = campaign_capacity(&s, &sd, 95.0).unwrap();
        assert_eq!(r.campaigns.len(), 6);
        let by = |n: &str| r.campaigns.iter().find(|c| c.model == n).unwrap();
        assert!(by("ID.4").at_capacity && by("ID.4").wasted_spend == by("ID.4").spend, "no headroom, all wasted");
        assert!(by("Taycan").at_capacity);
        let enyaq = by("Enyaq");
        assert!(enyaq.at_capacity && enyaq.wasted_spend > 0.0 && enyaq.wasted_spend < enyaq.spend, "headroom covers part of the extra cars");
        assert!(!by("Golf").at_capacity && by("Golf").wasted_spend == 0.0);
        assert!((r.wasted_spend - r.campaigns.iter().map(|c| c.wasted_spend).sum::<f64>()).abs() < 1e-6);
        assert_eq!(r.campaigns[0].model, "ID.4", "largest waste first");
        assert!(campaign_capacity(&s, &sd, 99.0).unwrap().campaigns.iter().find(|c| c.model == "Enyaq").is_some_and(|c| !c.at_capacity));
        assert!(campaign_capacity(&s, &sd, -1.0).is_err());
    }

    #[test]
    fn q17_a_loan_beats_a_lease_for_the_taycan_in_prague() {
        let (s, md, sd) = load();
        let r = financing_comparison(&s, &md, &sd, "Taycan", "Czech Republic").unwrap();
        assert_eq!(r.products.len(), 4);
        assert_eq!((r.best.as_str(), r.best_kind.as_str()), ("Taycan loan, 60 months", "Loan"));
        assert!((r.kind_gap - 840.0).abs() < 1e-6, "{}", r.kind_gap);
        let loan60 = &r.products[0];
        assert!((loan60.profit - (loan60.income - loan60.funding_cost - loan60.risk_cost - loan60.subvention_cost)).abs() < 1e-6);
        assert!(r.products.windows(2).all(|w| w[0].profit >= w[1].profit));
        assert!(financing_comparison(&s, &md, &sd, "Taycan", "Prague").is_err(), "markets are countries; Prague is not one");
        assert!(financing_comparison(&s, &md, &sd, "Golf", "Czech Republic").is_err(), "no product for the Golf");
        assert!(financing_comparison(&s, &md, &sd, "Nope", "Czech Republic").is_err());
    }
}
