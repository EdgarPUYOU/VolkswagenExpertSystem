//! One function per target question. Each takes a `Snapshot` and returns plain serialisable data.

use super::finance::*;
use anyhow::Result;
use chrono::{Duration, Months, NaiveDate};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

fn model_revenue(m: &Model) -> f64 {
    m.price * m.planned
}

fn annual_spend(snap: &Snapshot, c: &Contract) -> Result<f64> {
    Ok(snap.price_at(c, &snap.asof, &Shocks::default())? * c.projected)
}

// ---------------------------------------------------------------- Q3: currency exposure

#[derive(Serialize)]
pub struct CurrencySpend {
    pub currency: String,
    pub code: String,
    pub spend_eur: f64,
    pub share_pct: f64,
    pub contracts: usize,
    pub foreign: bool,
}

#[derive(Serialize)]
pub struct ContractSpend {
    pub iri: String,
    pub label: String,
    pub supplier: String,
    pub currency: String,
    pub spend_eur: f64,
}

#[derive(Serialize)]
pub struct CurrencyExposure {
    pub asof: String,
    pub total_spend_eur: f64,
    pub foreign_share_pct: f64,
    pub by_currency: Vec<CurrencySpend>,
    pub contracts: Vec<ContractSpend>,
}

pub fn currency_exposure(snap: &Snapshot) -> Result<CurrencyExposure> {
    let mut by: BTreeMap<String, (f64, usize)> = BTreeMap::new();
    let mut contracts = Vec::new();
    for c in &snap.contracts {
        let spend = annual_spend(snap, c)?;
        let e = by.entry(c.currency.clone()).or_default();
        e.0 += spend;
        e.1 += 1;
        contracts.push(ContractSpend {
            iri: c.iri.clone(),
            label: c.label.clone(),
            supplier: snap.supplier_label(&c.supplier),
            currency: snap.currency_codes.get(&c.currency).cloned().unwrap_or_default(),
            spend_eur: spend,
        });
    }
    let total: f64 = by.values().map(|v| v.0).sum();
    let mut by_currency: Vec<CurrencySpend> = by
        .into_iter()
        .map(|(iri, (spend, n))| CurrencySpend {
            code: snap.currency_codes.get(&iri).cloned().unwrap_or_default(),
            foreign: iri != REPORTING_CURRENCY,
            currency: iri,
            spend_eur: spend,
            share_pct: spend / total * 100.0,
            contracts: n,
        })
        .collect();
    by_currency.sort_by(|a, b| b.spend_eur.total_cmp(&a.spend_eur));
    let foreign: f64 = by_currency.iter().filter(|c| c.foreign).map(|c| c.spend_eur).sum();
    contracts.sort_by(|a, b| b.spend_eur.total_cmp(&a.spend_eur));
    Ok(CurrencyExposure {
        asof: snap.asof.clone(),
        total_spend_eur: total,
        foreign_share_pct: foreign / total * 100.0,
        by_currency,
        contracts,
    })
}

// ---------------------------------------------------------------- Q4: commodity sensitivity

#[derive(Serialize)]
pub struct SensitivityRow {
    pub profit_center: String,
    pub label: String,
    pub revenue: f64,
    pub linked_material_share_pct: f64,
    /// index label -> extra annual cost for a +bump move in that index alone
    pub per_index: BTreeMap<String, f64>,
    pub all_indices_extra_cost: f64,
    pub all_indices_pct_of_revenue: f64,
}

#[derive(Serialize)]
pub struct Sensitivity {
    pub bump_pct: f64,
    pub indices: Vec<String>,
    pub rows: Vec<SensitivityRow>,
}

pub fn commodity_sensitivity(snap: &Snapshot, bump_pct: f64) -> Result<Sensitivity> {
    let run_for = |targets: &[String]| {
        run_scenario(
            snap,
            &ScenarioRequest {
                shocks: targets.iter().map(|t| ShockSpec { kind: "commodity".into(), target: t.clone(), percent: bump_pct }).collect(),
                margin_floor: None,
                save_as: None,
            },
        )
    };
    let all: Vec<String> = snap.index_labels.keys().cloned().collect();
    let combined = run_for(&all)?;
    let singles: Vec<(String, ScenarioResult)> =
        all.iter().map(|i| Ok((snap.index_label(i), run_for(std::slice::from_ref(i))?))).collect::<Result<_>>()?;

    let none = Shocks::default();
    let mut rows = Vec::new();
    for pc in &combined.by_profit_center {
        let mut per_index = BTreeMap::new();
        for (label, r) in &singles {
            let v = r.by_profit_center.iter().find(|p| p.iri == pc.iri).map_or(0.0, |p| p.extra_cost);
            per_index.insert(label.clone(), v);
        }
        // share of material cost that follows an index (sum of weight * pass-through per contract)
        let (mut linked, mut material) = (0.0, 0.0);
        for m in snap.models.iter().filter(|m| m.pc_iri == pc.iri) {
            for comp in snap.model_components.get(&m.iri).map(Vec::as_slice).unwrap_or(&[]) {
                if let Some(c) = snap.contract_for(comp) {
                    let price = snap.price_at(c, &snap.asof, &none)? * m.planned;
                    material += price;
                    linked += price * c.clauses.iter().map(|cl| cl.weight * cl.pass).sum::<f64>();
                }
            }
        }
        rows.push(SensitivityRow {
            profit_center: pc.iri.clone(),
            label: pc.label.clone(),
            revenue: pc.revenue,
            linked_material_share_pct: if material > 0.0 { linked / material * 100.0 } else { 0.0 },
            per_index,
            all_indices_extra_cost: pc.extra_cost,
            all_indices_pct_of_revenue: pc.extra_cost_pct_of_revenue,
        });
    }
    rows.sort_by(|a, b| b.all_indices_pct_of_revenue.total_cmp(&a.all_indices_pct_of_revenue));
    Ok(Sensitivity { bump_pct, indices: singles.into_iter().map(|s| s.0).collect(), rows })
}

// ---------------------------------------------------------------- Q5: volume-discount savings

#[derive(Serialize)]
pub struct SavingRow {
    pub iri: String,
    pub label: String,
    pub supplier: String,
    pub component: String,
    pub projected: f64,
    pub tier: f64,
    pub current_price: f64,
    pub discount_price: f64,
    pub annual_saving: f64,
}

#[derive(Serialize)]
pub struct Savings {
    pub total_annual_saving: f64,
    pub rows: Vec<SavingRow>,
}

pub fn discount_savings(snap: &Snapshot) -> Savings {
    let mut rows: Vec<SavingRow> = snap
        .contracts
        .iter()
        .filter_map(|c| {
            let (tier, discount) = (c.tier?, c.discount_price?);
            (c.projected >= tier && discount < c.base_price).then(|| SavingRow {
                iri: c.iri.clone(),
                label: c.label.clone(),
                supplier: snap.supplier_label(&c.supplier),
                component: snap.label_of_component(&c.component),
                projected: c.projected,
                tier,
                current_price: c.base_price,
                discount_price: discount,
                annual_saving: (c.base_price - discount) * c.projected,
            })
        })
        .collect();
    rows.sort_by(|a, b| b.annual_saving.total_cmp(&a.annual_saving));
    Savings { total_annual_saving: rows.iter().map(|r| r.annual_saving).sum(), rows }
}

// ---------------------------------------------------------------- Q6: supplier failure

#[derive(Serialize)]
pub struct SupplierRisk {
    pub iri: String,
    pub label: String,
    pub country: String,
    pub lead_time_days: f64,
    pub contracts: usize,
    pub components: Vec<String>,
    pub models: Vec<String>,
    pub revenue_at_risk: f64,
    pub revenue_share_pct: f64,
    pub annual_spend: f64,
    pub single_sourced_components: usize,
    pub alternatives: Vec<String>,
}

pub fn supplier_risk(snap: &Snapshot) -> Result<Vec<SupplierRisk>> {
    let total_revenue: f64 = snap.models.iter().map(model_revenue).sum();
    let mut out = Vec::new();
    for (iri, s) in &snap.suppliers {
        let contracts: Vec<&Contract> = snap.contracts.iter().filter(|c| &c.supplier == iri).collect();
        if contracts.is_empty() {
            continue;
        }
        let mut models: BTreeMap<String, f64> = BTreeMap::new();
        for c in &contracts {
            for m in snap.models_using(&c.component) {
                models.insert(m.label.clone(), model_revenue(m));
            }
        }
        let mut alternatives = BTreeSet::new();
        for c in &contracts {
            for a in &c.alternatives {
                alternatives.insert(snap.supplier_label(a));
            }
        }
        let revenue: f64 = models.values().sum();
        let mut spend = 0.0;
        for c in &contracts {
            spend += annual_spend(snap, c)?;
        }
        out.push(SupplierRisk {
            iri: iri.clone(),
            label: s.label.clone(),
            country: s.country.clone(),
            lead_time_days: s.lead_time_days,
            contracts: contracts.len(),
            components: contracts.iter().map(|c| snap.label_of_component(&c.component)).collect(),
            models: models.keys().cloned().collect(),
            revenue_at_risk: revenue,
            revenue_share_pct: revenue / total_revenue * 100.0,
            annual_spend: spend,
            single_sourced_components: contracts.iter().filter(|c| snap.critical_components.contains(&c.component)).count(),
            alternatives: alternatives.into_iter().collect(),
        });
    }
    out.sort_by(|a, b| b.revenue_at_risk.total_cmp(&a.revenue_at_risk));
    Ok(out)
}

// ---------------------------------------------------------------- Q7: single-sourced components

#[derive(Serialize)]
pub struct SingleSource {
    pub component: String,
    pub label: String,
    pub contract: String,
    pub supplier: String,
    pub lead_time_days: f64,
    pub contract_end: String,
    pub models: Vec<String>,
    pub revenue_dependent: f64,
}

pub fn single_sourced(snap: &Snapshot) -> Vec<SingleSource> {
    let mut out: Vec<SingleSource> = snap
        .critical_components
        .iter()
        .filter_map(|comp| {
            let c = snap.contract_for(comp)?;
            let users = snap.models_using(comp);
            Some(SingleSource {
                component: comp.clone(),
                label: snap.label_of_component(comp),
                contract: c.label.clone(),
                supplier: snap.supplier_label(&c.supplier),
                lead_time_days: snap.suppliers.get(&c.supplier).map_or(0.0, |s| s.lead_time_days),
                contract_end: c.end.clone(),
                models: users.iter().map(|m| m.label.clone()).collect(),
                revenue_dependent: users.iter().map(|m| model_revenue(m)).sum(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.revenue_dependent.total_cmp(&a.revenue_dependent));
    out
}

// ---------------------------------------------------------------- Q8: expiring contracts

#[derive(Serialize, Debug)]
pub struct ExpiringContract {
    pub iri: String,
    pub label: String,
    pub supplier: String,
    pub component: String,
    pub end: String,
    pub days_left: i64,
    pub committed: f64,
    pub projected: f64,
    pub under_committed: bool,
    pub annual_spend: f64,
    pub sourcing: String,
    pub alternatives: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct Expiring {
    pub today: String,
    pub months: u32,
    pub cutoff: String,
    pub contracts: Vec<ExpiringContract>,
}

pub fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").or_else(|_| bad(format!("'{s}' is not a date, expected YYYY-MM-DD")))
}

pub fn expiring(snap: &Snapshot, today: &str, months: u32) -> Result<Expiring> {
    let today_d = parse_date(today)?;
    let cutoff = today_d
        .checked_add_months(Months::new(months))
        .map_or_else(|| bad("months out of range"), Ok)?;
    let mut contracts = Vec::new();
    for c in &snap.contracts {
        let end = parse_date(&c.end)?;
        if end < today_d || end > cutoff {
            continue;
        }
        contracts.push(ExpiringContract {
            iri: c.iri.clone(),
            label: c.label.clone(),
            supplier: snap.supplier_label(&c.supplier),
            component: snap.label_of_component(&c.component),
            end: c.end.clone(),
            days_left: (end - today_d).num_days(),
            committed: c.committed,
            projected: c.projected,
            under_committed: c.projected > c.committed,
            annual_spend: annual_spend(snap, c)?,
            sourcing: c.sourcing.clone(),
            alternatives: c.alternatives.iter().map(|a| snap.supplier_label(a)).collect(),
        });
    }
    contracts.sort_by_key(|c| c.days_left);
    Ok(Expiring { today: today.to_string(), months, cutoff: cutoff.to_string(), contracts })
}

// ---------------------------------------------------------------- Q9: supplier lead-time shock

#[derive(Serialize, Debug)]
pub struct LeadTimeModel {
    pub iri: String,
    pub label: String,
    pub via_components: Vec<String>,
    pub planned_launch: Option<String>,
    pub new_launch: Option<String>,
    pub revenue_at_risk: f64,
    pub high_intent_leads: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct LeadTimeImpact {
    pub supplier: String,
    pub label: String,
    pub lead_time_days: f64,
    pub new_lead_time_days: f64,
    pub delay_days: f64,
    pub models: Vec<LeadTimeModel>,
    pub total_revenue_at_risk: f64,
    pub leads_affected: usize,
}

pub fn lead_time_impact(snap: &Snapshot, supplier: &str, factor: f64) -> Result<LeadTimeImpact> {
    if !factor.is_finite() || factor < 1.0 {
        return bad("factor must be at least 1");
    }
    let iri = snap.resolve_supplier(supplier)?;
    let s = &snap.suppliers[&iri];
    let delay = (s.lead_time_days * (factor - 1.0)).round();
    let mut via: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for c in snap.contracts.iter().filter(|c| c.supplier == iri) {
        for m in snap.models_using(&c.component) {
            via.entry(m.iri.clone()).or_default().insert(snap.label_of_component(&c.component));
        }
    }
    let mut models = Vec::new();
    for (m_iri, comps) in via {
        let m = snap.models.iter().find(|m| m.iri == m_iri).unwrap();
        let new_launch = match &m.launch {
            Some(d) => Some((parse_date(d)? + Duration::days(delay as i64)).to_string()),
            None => None,
        };
        models.push(LeadTimeModel {
            iri: m.iri.clone(),
            label: m.label.clone(),
            via_components: comps.into_iter().collect(),
            planned_launch: m.launch.clone(),
            new_launch,
            revenue_at_risk: model_revenue(m) * delay / 365.0,
            high_intent_leads: snap.leads.iter().filter(|(_, lm)| lm == &m.iri).map(|(l, _)| l.clone()).collect(),
        });
    }
    models.sort_by(|a, b| b.revenue_at_risk.total_cmp(&a.revenue_at_risk));
    let leads: BTreeSet<&String> = models.iter().flat_map(|m| m.high_intent_leads.iter()).collect();
    Ok(LeadTimeImpact {
        supplier: iri,
        label: s.label.clone(),
        lead_time_days: s.lead_time_days,
        new_lead_time_days: s.lead_time_days * factor,
        delay_days: delay,
        total_revenue_at_risk: models.iter().map(|m| m.revenue_at_risk).sum(),
        leads_affected: leads.len(),
        models,
    })
}

// ---------------------------------------------------------------- Q19: why did a margin move

#[derive(Serialize, Debug)]
pub struct TraceDriver {
    pub index: String,
    pub weight: f64,
    pub pass_through: f64,
    pub delta_eur_per_unit: f64,
}

#[derive(Serialize, Debug)]
pub struct TraceComponent {
    pub component: String,
    pub contract: String,
    pub supplier: String,
    pub currency: String,
    pub price_from: f64,
    pub price_to: f64,
    pub delta_eur_per_unit: f64,
    pub drivers: Vec<TraceDriver>,
    pub fx_effect_eur_per_unit: f64,
    /// Knowledge-graph path from the model to each index, for display.
    pub path: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct SeriesPoint {
    pub month: String,
    pub margin_pct: f64,
}

#[derive(Serialize, Debug)]
pub struct MarginTrace {
    pub model: String,
    pub label: String,
    pub from: String,
    pub to: String,
    pub price: f64,
    pub margin_from_pct: f64,
    pub margin_to_pct: f64,
    pub change_pp: f64,
    pub material_change_eur_per_unit: f64,
    pub components: Vec<TraceComponent>,
    pub series: Vec<SeriesPoint>,
}

fn months_between(snap: &Snapshot, from: &str, to: &str) -> Vec<String> {
    let mut all: BTreeSet<String> = BTreeSet::new();
    for i in snap.index_labels.keys().take(1) {
        for m in (0..200).map(crate::seed::month_label) {
            if snap.index_value(i, &m).is_ok() && m.as_str() >= from && m.as_str() <= to {
                all.insert(m);
            }
        }
    }
    all.into_iter().collect()
}

pub fn margin_trace(snap: &Snapshot, model: &str, from: Option<&str>, to: Option<&str>) -> Result<MarginTrace> {
    let m = snap.resolve_model(model)?;
    let to = to.unwrap_or(&snap.asof).to_string();
    let from = match from {
        Some(f) => f.to_string(),
        None => {
            let d = parse_date(&format!("{to}-01"))?;
            d.checked_sub_months(Months::new(6)).map_or_else(|| bad("bad date"), |d| Ok(d.format("%Y-%m").to_string()))?
        }
    };
    for month in [&from, &to] {
        if snap.index_labels.keys().next().is_none_or(|i| snap.index_value(i, month).is_err()) {
            return bad(format!("no data for month '{month}'"));
        }
    }
    let none = Shocks::default();
    let oh = snap.overhead_of(m);
    let margin = |month: &str| -> Result<f64> { Ok((m.price - snap.material_cost(m, month, &none)? - oh) / m.price * 100.0) };

    let mut components = Vec::new();
    for comp in snap.model_components.get(&m.iri).map(Vec::as_slice).unwrap_or(&[]) {
        let Some(c) = snap.contract_for(comp) else { continue };
        let (p_from, p_to) = (snap.price_at(c, &from, &none)?, snap.price_at(c, &to, &none)?);
        let base_month = &c.start[..7];
        let mut drivers = Vec::new();
        let mut path = Vec::new();
        let comp_label = snap.label_of_component(comp);
        for cl in &c.clauses {
            let i0 = snap.index_value(&cl.index, base_month)?;
            let d = c.base_price * cl.weight * cl.pass * (snap.index_value(&cl.index, &to)? - snap.index_value(&cl.index, &from)?) / i0;
            path.push(format!("{} -> {} -> {} -> {}", m.label, comp_label, short(&c.iri), short(&cl.index)));
            drivers.push(TraceDriver { index: snap.index_label(&cl.index), weight: cl.weight, pass_through: cl.pass, delta_eur_per_unit: d });
        }
        let delta = p_to - p_from;
        let fx_effect = delta - drivers.iter().map(|d| d.delta_eur_per_unit).sum::<f64>();
        drivers.sort_by(|a, b| b.delta_eur_per_unit.abs().total_cmp(&a.delta_eur_per_unit.abs()));
        components.push(TraceComponent {
            component: comp_label,
            contract: c.label.clone(),
            supplier: snap.supplier_label(&c.supplier),
            currency: snap.currency_codes.get(&c.currency).cloned().unwrap_or_default(),
            price_from: p_from,
            price_to: p_to,
            delta_eur_per_unit: delta,
            drivers,
            fx_effect_eur_per_unit: fx_effect,
            path,
        });
    }
    components.sort_by(|a, b| b.delta_eur_per_unit.abs().total_cmp(&a.delta_eur_per_unit.abs()));

    let mut series = Vec::new();
    for month in months_between(snap, &from, &to) {
        series.push(SeriesPoint { margin_pct: margin(&month)?, month });
    }
    let (mf, mt) = (margin(&from)?, margin(&to)?);
    Ok(MarginTrace {
        model: m.iri.clone(),
        label: m.label.clone(),
        from,
        to,
        price: m.price,
        margin_from_pct: mf,
        margin_to_pct: mt,
        change_pp: mt - mf,
        material_change_eur_per_unit: components.iter().map(|c| c.delta_eur_per_unit).sum(),
        components,
        series,
    })
}

// ---------------------------------------------------------------- dashboard

#[derive(Serialize, Debug)]
pub struct Mover {
    pub label: String,
    pub kind: String,
    pub change_6m_pct: f64,
}

#[derive(Serialize, Debug)]
pub struct Dashboard {
    pub asof: String,
    pub planned_revenue: f64,
    pub contribution_margin_pct: f64,
    pub material_spend: f64,
    pub foreign_currency_share_pct: f64,
    pub commodity_linked_share_pct: f64,
    pub contracts: usize,
    pub contracts_under_committed: usize,
    pub contracts_expiring_12m: usize,
    pub single_sourced_components: usize,
    pub movers: Vec<Mover>,
    pub margin_by_profit_center: Vec<(String, f64)>,
}

pub fn dashboard(snap: &Snapshot, today: &str) -> Result<Dashboard> {
    let none = Shocks::default();
    let (mut revenue, mut profit, mut material, mut linked) = (0.0, 0.0, 0.0, 0.0);
    let mut by_pc: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for m in &snap.models {
        let mat = snap.material_cost(m, &snap.asof, &none)?;
        let p = (m.price - mat - snap.overhead_of(m)) * m.planned;
        revenue += model_revenue(m);
        profit += p;
        material += mat * m.planned;
        for comp in snap.model_components.get(&m.iri).map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(c) = snap.contract_for(comp) {
                linked += snap.price_at(c, &snap.asof, &none)? * m.planned * c.clauses.iter().map(|cl| cl.weight * cl.pass).sum::<f64>();
            }
        }
        let e = by_pc.entry(m.pc_label.clone()).or_default();
        e.0 += model_revenue(m);
        e.1 += p;
    }
    let currency = currency_exposure(snap)?;
    let mut movers: Vec<Mover> = series_overview(snap)
        .into_iter()
        .map(|s| Mover { label: s.label, kind: s.kind, change_6m_pct: s.change_6m_pct })
        .collect();
    movers.sort_by(|a, b| b.change_6m_pct.abs().total_cmp(&a.change_6m_pct.abs()));
    Ok(Dashboard {
        asof: snap.asof.clone(),
        planned_revenue: revenue,
        contribution_margin_pct: profit / revenue * 100.0,
        material_spend: material,
        foreign_currency_share_pct: currency.foreign_share_pct,
        commodity_linked_share_pct: linked / material * 100.0,
        contracts: snap.contracts.len(),
        contracts_under_committed: snap.contracts.iter().filter(|c| c.projected > c.committed).count(),
        contracts_expiring_12m: expiring(snap, today, 12)?.contracts.len(),
        single_sourced_components: snap.critical_components.len(),
        movers,
        margin_by_profit_center: {
            let mut v: Vec<_> = by_pc.into_iter().map(|(k, (r, p))| (k, p / r * 100.0)).collect();
            v.sort_by(|a, b| b.1.total_cmp(&a.1));
            v
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kg::Kg;
    use std::collections::BTreeSet;

    fn setup() -> (Kg, rusqlite::Connection) {
        let (kg, mut conn) = crate::kg::load_for_test();
        crate::seed::seed(&kg, &mut conn).unwrap();
        (kg, conn)
    }

    fn snap() -> Snapshot {
        let (kg, conn) = setup();
        Snapshot::load(&kg, &conn).unwrap()
    }

    fn scenario(snap: &Snapshot, shocks: &[(&str, &str, f64)]) -> ScenarioResult {
        run_scenario(
            snap,
            &ScenarioRequest {
                shocks: shocks.iter().map(|(k, t, p)| ShockSpec { kind: k.to_string(), target: t.to_string(), percent: *p }).collect(),
                margin_floor: None,
                save_as: None,
            },
        )
        .unwrap()
    }

    fn labels<'a>(it: impl Iterator<Item = &'a str>) -> BTreeSet<String> {
        it.map(String::from).collect()
    }

    #[test]
    fn seed_is_repeatable_and_deterministic() {
        let (kg, mut conn) = setup();
        let first = Snapshot::load(&kg, &conn).unwrap();
        crate::seed::seed(&kg, &mut conn).unwrap();
        let second = Snapshot::load(&kg, &conn).unwrap();
        assert_eq!(first.asof, "2026-11");
        let i = format!("{NS}Index_NaturalRubber");
        assert_eq!(first.index_value(&i, "2026-11").unwrap(), second.index_value(&i, "2026-11").unwrap());
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM index_values", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 210, "reseeding replaces rows instead of adding them");
    }

    #[test]
    fn price_equals_contract_price_in_the_start_month() {
        let snap = snap();
        for c in &snap.contracts {
            let p = snap.price_at(c, &c.start[..7], &Shocks::default()).unwrap();
            assert!((p - c.base_price).abs() < 1e-9, "{}: {p} vs {}", c.label, c.base_price);
        }
    }

    #[test]
    fn models_start_at_their_target_margin_with_no_shock() {
        let snap = snap();
        let r = scenario(&snap, &[("commodity", "Steel", 0.0)]);
        assert_eq!(r.totals.extra_cost, 0.0);
        let golf = r.by_model.iter().find(|m| m.label == "Golf").unwrap();
        assert!((golf.margin_before_pct - 17.2).abs() < 1e-6);
        assert!(r.by_model.iter().all(|m| (m.margin_before_pct - m.margin_after_pct).abs() < 1e-9));
    }

    #[test]
    fn q1_rubber_only_hits_the_tyre_contracts() {
        let snap = snap();
        let r = scenario(&snap, &[("commodity", "NaturalRubber", 15.0)]);
        let m = r.totals.extra_cost / 1e6;
        assert!((80.0..120.0).contains(&m), "extra cost {m} M EUR");
        let hit: BTreeSet<String> = r.by_contract.iter().map(|c| short(&c.iri)).collect();
        let expected: BTreeSet<String> =
            ["SupplyContract_22_TyreSet_Standard", "SupplyContract_23_TyreSet_Performance", "SupplyContract_24_TyreSet_HeavyDuty"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        assert_eq!(hit, expected);
        assert_eq!(r.by_contract[0].iri, format!("{NS}SupplyContract_22_TyreSet_Standard"), "standard tyres are the biggest");
        assert_eq!(r.by_profit_center.len(), 7);
        assert!(r.by_profit_center.iter().all(|p| p.material_budget > 0.0 && p.extra_cost > 0.0));
        let by_pc: f64 = r.by_profit_center.iter().map(|p| p.extra_cost).sum();
        let by_contract: f64 = r.by_contract.iter().map(|c| c.extra_cost).sum();
        assert!((by_pc - r.totals.extra_cost).abs() < 1.0 && (by_contract - r.totals.extra_cost).abs() < 1.0);
        let golf = r.by_model.iter().find(|m| m.label == "Golf").unwrap();
        assert!(golf.margin_after_pct < golf.margin_before_pct);
    }

    #[test]
    fn q2_steel_and_aluminium_push_four_models_under_the_floor() {
        let snap = snap();
        let r = scenario(&snap, &[("commodity", "Steel", 10.0), ("commodity", "Aluminium", 8.0)]);
        let crossing = labels(r.by_model.iter().filter(|m| m.crossed_floor).map(|m| m.label.as_str()));
        assert_eq!(crossing, labels(["Scania R-series", "MAN TGX", "ID.4", "Enyaq"].into_iter()));
        assert_eq!(r.totals.models_crossing_floor, 4);
    }

    #[test]
    fn fx_shock_only_touches_contracts_in_that_currency() {
        let snap = snap();
        let r = scenario(&snap, &[("fx", "SEK", 5.0)]);
        let sek: BTreeSet<String> = snap.contracts.iter().filter(|c| c.currency == format!("{NS}SEK")).map(|c| c.iri.clone()).collect();
        assert_eq!(r.by_contract.iter().map(|c| c.iri.clone()).collect::<BTreeSet<_>>(), sek);
        for c in &r.by_contract {
            assert!((c.price_after / c.price_before - 1.05).abs() < 1e-9, "a 5% stronger krona costs 5% more in EUR");
        }
    }

    #[test]
    fn bad_scenario_input_is_a_bad_request() {
        let snap = snap();
        for (kind, target, pct) in [("commodity", "Unobtainium", 5.0), ("fx", "XXX", 5.0), ("weather", "Steel", 5.0), ("commodity", "Steel", -100.0)] {
            let err = run_scenario(
                &snap,
                &ScenarioRequest { shocks: vec![ShockSpec { kind: kind.into(), target: target.into(), percent: pct }], margin_floor: None, save_as: None },
            )
            .unwrap_err();
            assert!(err.downcast_ref::<BadRequest>().is_some(), "{kind} {target} {pct}");
        }
        assert!(run_scenario(&snap, &ScenarioRequest { shocks: vec![], margin_floor: None, save_as: None }).is_err());
    }

    #[test]
    fn q3_a_large_share_of_spend_is_in_krona_and_koruna() {
        let e = currency_exposure(&snap()).unwrap();
        let codes: BTreeSet<_> = e.by_currency.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(codes, ["CZK", "EUR", "SEK"].into_iter().collect());
        assert!((35.0..45.0).contains(&e.foreign_share_pct), "{}", e.foreign_share_pct);
        let sum: f64 = e.by_currency.iter().map(|c| c.share_pct).sum();
        assert!((sum - 100.0).abs() < 1e-9);
    }

    #[test]
    fn q4_porsche_and_lamborghini_are_least_commodity_sensitive() {
        let s = commodity_sensitivity(&snap(), 10.0).unwrap();
        assert_eq!(s.rows.len(), 7);
        assert_eq!(s.indices.len(), 6);
        let last_two: BTreeSet<_> = s.rows[5..].iter().map(|r| r.label.as_str()).collect();
        assert!(last_two.contains("Lamborghini profit center") && last_two.contains("Porsche profit center"));
        assert!(s.rows.windows(2).all(|w| w[0].all_indices_pct_of_revenue >= w[1].all_indices_pct_of_revenue));
    }

    #[test]
    fn q5_discount_savings_match_the_rule_output() {
        let snap = snap();
        let s = discount_savings(&snap);
        assert_eq!(s.rows.len(), 10);
        assert!(s.rows.iter().all(|r| r.projected >= r.tier && r.discount_price < r.current_price));
        assert!((s.total_annual_saving - s.rows.iter().map(|r| r.annual_saving).sum::<f64>()).abs() < 1e-6);
    }

    #[test]
    fn q6_battery_supplier_covers_exactly_the_four_evs() {
        let risk = supplier_risk(&snap()).unwrap();
        let b = risk.iter().find(|r| r.label.contains("Battery")).unwrap();
        assert_eq!(b.models, vec!["Enyaq", "ID.4", "Taycan", "e-tron"].into_iter().map(String::from).collect::<Vec<_>>());
        assert_eq!(b.single_sourced_components, 3);
        assert!(b.alternatives.is_empty());
        assert!(risk.windows(2).all(|w| w[0].revenue_at_risk >= w[1].revenue_at_risk));
    }

    #[test]
    fn q7_single_sourced_components_come_from_the_rules() {
        let snap = snap();
        let rows = single_sourced(&snap);
        assert_eq!(rows.len(), 15);
        for r in &rows {
            let c = snap.contract_for(&r.component).unwrap();
            assert!(c.alternatives.is_empty() && c.sourcing == "SingleSource");
        }
        assert!(rows.windows(2).all(|w| w[0].revenue_dependent >= w[1].revenue_dependent));
    }

    #[test]
    fn q8_eight_contracts_expire_within_a_year_and_five_are_under_committed() {
        let snap = snap();
        let e = expiring(&snap, "2026-11-27", 12).unwrap();
        assert_eq!(e.contracts.len(), 8);
        assert_eq!(e.contracts.iter().filter(|c| c.under_committed).count(), 5);
        assert!(e.contracts.windows(2).all(|w| w[0].days_left <= w[1].days_left));
        assert!(e.contracts.iter().all(|c| c.end.as_str() <= "2027-11-27"));
        assert_eq!(expiring(&snap, "2026-11-27", 3).unwrap().contracts.len(), 0);
        assert!(expiring(&snap, "soon", 12).unwrap_err().downcast_ref::<BadRequest>().is_some());
    }

    #[test]
    fn q9_doubling_battery_lead_time_delays_the_evs_and_their_lead() {
        let snap = snap();
        let r = lead_time_impact(&snap, "Supplier_BatterySystems", 2.0).unwrap();
        assert_eq!((r.lead_time_days, r.delay_days), (120.0, 120.0));
        let id4 = r.models.iter().find(|m| m.label == "ID.4").unwrap();
        assert_eq!(id4.planned_launch.as_deref(), Some("2027-03-15"));
        assert_eq!(id4.new_launch.as_deref(), Some("2027-07-13"));
        assert_eq!(r.leads_affected, 1);
        assert_eq!(r.models.len(), 4);
        assert!(lead_time_impact(&snap, "Supplier_BatterySystems", 0.5).is_err());
        assert!(lead_time_impact(&snap, "Nobody", 2.0).is_err());
    }

    #[test]
    fn q19_golf_margin_fall_is_explained_by_rubber_steel_and_fx() {
        let snap = snap();
        let t = margin_trace(&snap, "Golf", None, None).unwrap();
        assert_eq!((t.from.as_str(), t.to.as_str()), ("2026-05", "2026-11"));
        assert!(t.change_pp < 0.0 && t.material_change_eur_per_unit > 0.0);
        // the margin change is exactly the material cost change over the price
        assert!((t.change_pp + t.material_change_eur_per_unit / t.price * 100.0).abs() < 1e-9);
        let comp_sum: f64 = t.components.iter().map(|c| c.delta_eur_per_unit).sum();
        assert!((comp_sum - t.material_change_eur_per_unit).abs() < 1e-9);
        let tyres = t.components.iter().find(|c| c.component.contains("tyre")).unwrap();
        let rubber = tyres.drivers.iter().find(|d| d.index == "Natural rubber").unwrap();
        assert!(rubber.delta_eur_per_unit > 40.0 && tyres.fx_effect_eur_per_unit > 0.0);
        assert!(tyres.path.iter().any(|p| p.ends_with("Index_NaturalRubber") && p.starts_with("Golf")));
        assert_eq!(t.series.len(), 7);
        assert!((t.series.last().unwrap().margin_pct - t.margin_to_pct).abs() < 1e-9);
        assert!(margin_trace(&snap, "Golf", Some("1999-01"), None).is_err());
    }

    #[test]
    fn market_series_cover_every_index_and_foreign_currency() {
        let out = series_overview(&snap());
        assert_eq!(out.iter().filter(|s| s.kind == "index").count(), 6);
        assert_eq!(out.iter().filter(|s| s.kind == "fx").count(), 3);
        assert!(out.iter().all(|s| s.points.len() == 35));
        let rubber = out.iter().find(|s| s.label == "Natural rubber").unwrap();
        assert!(rubber.change_6m_pct > 10.0, "rubber has risen: {}", rubber.change_6m_pct);
    }

    #[test]
    fn dashboard_adds_up() {
        let snap = snap();
        let d = dashboard(&snap, "2026-11-27").unwrap();
        assert_eq!((d.contracts, d.contracts_expiring_12m, d.single_sourced_components), (24, 8, 15));
        assert!((d.contribution_margin_pct - 19.39).abs() < 0.01, "{}", d.contribution_margin_pct);
        assert!(d.movers[0].change_6m_pct.abs() >= d.movers[1].change_6m_pct.abs());
        assert_eq!(d.margin_by_profit_center.len(), 7);
    }
}
