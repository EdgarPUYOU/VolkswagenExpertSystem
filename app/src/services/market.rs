//! Market, regulation and portfolio-strategy questions (Q10 to Q14).
//!
//! The graph says where each model is sold, which regulations apply, which competitor models rival
//! ours and which brand group sits on which site. SQLite holds planned units per market and competitor
//! prices. Behavioural parameters (price elasticity) are inputs with stated defaults, not facts.

use super::finance::*;
use super::portfolio::run;
use crate::kg::Kg;
use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

pub const DEFAULT_SUBSIDY_ELASTICITY: f64 = 1.5;
pub const DEFAULT_CROSS_ELASTICITY: f64 = 0.8;

#[derive(Clone, Debug, Serialize)]
pub struct Market {
    pub iri: String,
    pub label: String,
    pub country: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Subsidy {
    pub iri: String,
    pub label: String,
    pub eur: f64,
    pub powertrain: String,
    pub market: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Co2Target {
    pub iri: String,
    pub label: String,
    pub grams: f64,
    pub penalty_per_gram: f64,
    pub markets: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Rival {
    pub iri: String,
    pub label: String,
    pub competitor: String,
    pub price: f64,
    pub powertrain: String,
}

pub struct MarketData {
    pub markets: BTreeMap<String, Market>,
    /// (model, market) -> planned units
    pub units: HashMap<(String, String), f64>,
    pub subsidies: Vec<Subsidy>,
    pub co2: Option<Co2Target>,
    pub powertrain: HashMap<String, String>,
    pub co2_g: HashMap<String, f64>,
    pub rivals: BTreeMap<String, Rival>,
    /// our model -> rival IRIs
    pub competes: HashMap<String, Vec<String>>,
    /// (customer label, model, country)
    pub leads: Vec<(String, String, String)>,
    /// (group label, brand IRI, site label)
    pub sites: Vec<(String, String, String)>,
}

fn f(r: &crate::kg::Row, k: &str) -> f64 {
    r.get(k).and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

impl MarketData {
    pub fn load(kg: &Kg, conn: &Connection) -> Result<MarketData> {
        let q = |s: &str| run(kg, s);
        let markets = q("SELECT ?m ?l ?c WHERE { ?m a vw:Market ; rdfs:label ?l ; vw:marketCountry ?c }")?
            .into_iter()
            .map(|r| (r["m"].clone(), Market { iri: r["m"].clone(), label: r["l"].clone(), country: r["c"].clone() }))
            .collect();
        let subsidies = q("SELECT ?s ?l ?e ?p ?m WHERE { ?s a vw:EVSubsidy ; rdfs:label ?l ; vw:subsidyEur ?e ;
                            vw:appliesToPowertrain ?p ; vw:appliesInMarket ?m } ORDER BY ?s")?
            .iter()
            .map(|r| Subsidy { iri: r["s"].clone(), label: r["l"].clone(), eur: f(r, "e"), powertrain: r["p"].clone(), market: r["m"].clone() })
            .collect();
        let mut co2: Option<Co2Target> = None;
        for r in q("SELECT ?t ?l ?g ?p ?m WHERE { ?t a vw:CO2FleetTarget ; rdfs:label ?l ; vw:targetGramsPerKm ?g ;
                      vw:penaltyEurPerGram ?p ; vw:appliesInMarket ?m } ORDER BY ?t ?m")? {
            match co2.as_mut() {
                Some(t) if t.iri == r["t"] => t.markets.push(r["m"].clone()),
                Some(_) => {} // only the first CO2 target is modelled
                None => co2 = Some(Co2Target { iri: r["t"].clone(), label: r["l"].clone(), grams: f(&r, "g"), penalty_per_gram: f(&r, "p"), markets: vec![r["m"].clone()] }),
            }
        }
        let powertrain = q("SELECT ?m ?p WHERE { ?m a vw:Model ; vw:hasPowertrain ?p }")?.into_iter().map(|r| (r["m"].clone(), r["p"].clone())).collect();
        let co2_g = q("SELECT ?m ?g WHERE { ?m a vw:Model ; vw:co2GramsPerKm ?g }")?.iter().map(|r| (r["m"].clone(), f(r, "g"))).collect();

        let mut prices: HashMap<String, f64> = HashMap::new();
        let mut stmt = conn.prepare("SELECT competitor_model_iri, price_eur FROM competitor_prices")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))? {
            let (i, p) = row?;
            prices.insert(i, p);
        }
        let rivals = q("SELECT ?c ?l ?co ?p WHERE { ?c a vw:CompetitorModel ; rdfs:label ?l ; vw:competitorOf ?o ; vw:competitorPowertrain ?p .
                         ?o rdfs:label ?co } ORDER BY ?c")?
            .iter()
            .map(|r| (r["c"].clone(), Rival { iri: r["c"].clone(), label: r["l"].clone(), competitor: r["co"].clone(), price: prices.get(&r["c"]).copied().unwrap_or(0.0), powertrain: r["p"].clone() }))
            .collect();
        let mut competes: HashMap<String, Vec<String>> = HashMap::new();
        for r in q("SELECT ?m ?c WHERE { ?m vw:competesWith ?c } ORDER BY ?m ?c")? {
            competes.entry(r["m"].clone()).or_default().push(r["c"].clone());
        }
        let leads = q("SELECT ?cu ?l ?m ?country WHERE { ?cu vw:highIntentForModel ?m ; vw:livesInRegion ?r . ?r vw:regionCountry ?country .
                       OPTIONAL { ?cu rdfs:label ?l } } ORDER BY ?cu ?m")?
            .iter()
            .map(|r| (r.get("l").cloned().unwrap_or_else(|| short(&r["cu"])), r["m"].clone(), r["country"].clone()))
            .collect();
        let sites = q("SELECT ?gl ?b ?sl WHERE { ?g a vw:BrandGroup ; rdfs:label ?gl ; vw:hasBrand ?b . ?b vw:hasProductionSite ?s .
                       OPTIONAL { ?s rdfs:label ?sl } } ORDER BY ?g ?b")?
            .iter()
            .map(|r| (r["gl"].clone(), r["b"].clone(), r.get("sl").cloned().unwrap_or_default()))
            .collect();

        let mut units = HashMap::new();
        let mut stmt = conn.prepare("SELECT model_iri, market_iri, units FROM sales_plan")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?)))? {
            let (m, mk, u) = row?;
            units.insert((m, mk), u);
        }
        Ok(MarketData { markets, units, subsidies, co2, powertrain, co2_g, rivals, competes, leads, sites })
    }

    pub fn resolve_market(&self, name: &str) -> Result<&Market> {
        let n = name.to_lowercase();
        self.markets
            .values()
            .find(|m| {
                m.iri == name
                    || m.iri == format!("{NS}{name}")
                    || m.iri == format!("{NS}Market_{name}")
                    || m.label.to_lowercase() == n
                    || m.country.to_lowercase() == n
            })
            .map_or_else(|| bad(format!("unknown market '{name}'")), Ok)
    }

    pub fn units_in(&self, model: &str, market: &str) -> f64 {
        self.units.get(&(model.to_string(), market.to_string())).copied().unwrap_or(0.0)
    }

    pub fn is_electric(&self, model: &str) -> bool {
        self.powertrain.get(model).is_some_and(|p| p.ends_with("BatteryElectric"))
    }
}

fn unit_margin(snap: &Snapshot, m: &Model) -> Result<f64> {
    Ok(m.price - snap.material_cost(m, &snap.asof, &Shocks::default())? - snap.overhead_of(m))
}

// ---------------------------------------------------------------- Q10: subsidy removal

#[derive(Serialize)]
pub struct SubsidyModel {
    pub iri: String,
    pub label: String,
    pub brand: String,
    pub subsidy_eur: f64,
    pub price: f64,
    pub units_in_market: f64,
    pub demand_loss_pct: f64,
    pub lost_units: f64,
    pub lost_revenue: f64,
    pub lost_contribution: f64,
}

#[derive(Serialize)]
pub struct SubsidyLead {
    pub customer: String,
    pub model: String,
}

#[derive(Serialize)]
pub struct SubsidyRemoval {
    pub market: String,
    pub subsidies: Vec<String>,
    pub elasticity: f64,
    pub lost_units: f64,
    pub lost_revenue: f64,
    pub lost_contribution: f64,
    pub models: Vec<SubsidyModel>,
    pub leads: Vec<SubsidyLead>,
}

/// Q10. Removing a subsidy raises the buyer's price by `subsidy / price`; demand falls by `elasticity` times that.
pub fn subsidy_removal(snap: &Snapshot, md: &MarketData, market: &str, elasticity: f64) -> Result<SubsidyRemoval> {
    if elasticity < 0.0 {
        return bad("elasticity must not be negative");
    }
    let mk = md.resolve_market(market)?;
    let applicable: Vec<&Subsidy> = md.subsidies.iter().filter(|s| s.market == mk.iri).collect();
    let mut models = Vec::new();
    for m in &snap.models {
        let Some(s) = applicable.iter().find(|s| md.powertrain.get(&m.iri) == Some(&s.powertrain)) else { continue };
        let units = md.units_in(&m.iri, &mk.iri);
        if units == 0.0 {
            continue;
        }
        let loss = (elasticity * s.eur / m.price).min(1.0);
        let lost = units * loss;
        models.push(SubsidyModel {
            iri: m.iri.clone(),
            label: m.label.clone(),
            brand: m.brand_label.clone(),
            subsidy_eur: s.eur,
            price: m.price,
            units_in_market: units,
            demand_loss_pct: loss * 100.0,
            lost_units: lost,
            lost_revenue: lost * m.price,
            lost_contribution: lost * unit_margin(snap, m)?,
        });
    }
    models.sort_by(|a, b| b.lost_revenue.total_cmp(&a.lost_revenue));
    let leads = md
        .leads
        .iter()
        .filter(|(_, model, country)| country == &mk.country && models.iter().any(|m| &m.iri == model))
        .map(|(c, model, _)| SubsidyLead { customer: c.clone(), model: snap.models.iter().find(|m| &m.iri == model).map_or_else(|| short(model), |m| m.label.clone()) })
        .collect();
    Ok(SubsidyRemoval {
        market: mk.label.clone(),
        subsidies: applicable.iter().map(|s| s.label.clone()).collect(),
        elasticity,
        lost_units: models.iter().map(|m| m.lost_units).sum(),
        lost_revenue: models.iter().map(|m| m.lost_revenue).sum(),
        lost_contribution: models.iter().map(|m| m.lost_contribution).sum(),
        models,
        leads,
    })
}

// ---------------------------------------------------------------- Q11: CO2 fleet target

#[derive(Serialize, Clone)]
pub struct FleetRow {
    pub label: String,
    pub cars_in_target_markets: f64,
    pub avg_g_per_km: f64,
    pub target_now: f64,
    pub penalty_now: f64,
    pub target_new: f64,
    pub penalty_new: f64,
    pub breaches_new: bool,
}

#[derive(Serialize)]
pub struct Co2Fleet {
    pub regulation: String,
    pub tighten_pct: f64,
    pub target_now: f64,
    pub target_new: f64,
    pub penalty_per_gram: f64,
    pub brands: Vec<FleetRow>,
    /// Same arithmetic with all brands pooled, as if the group could pool its fleet.
    pub pooled: FleetRow,
}

/// Fleet average and penalty for `models` (each with a unit scale), at a given target.
fn fleet(md: &MarketData, t: &Co2Target, items: &[(&Model, f64)], target: f64) -> (f64, f64, f64) {
    let (mut cars, mut grams) = (0.0, 0.0);
    for (m, scale) in items {
        let Some(g) = md.co2_g.get(&m.iri) else { continue };
        let u: f64 = t.markets.iter().map(|mk| md.units_in(&m.iri, mk)).sum::<f64>() * scale;
        cars += u;
        grams += u * g;
    }
    let avg = if cars > 0.0 { grams / cars } else { 0.0 };
    (cars, avg, (avg - target).max(0.0) * cars * t.penalty_per_gram)
}

fn fleet_row(md: &MarketData, t: &Co2Target, label: &str, items: &[(&Model, f64)], tighten_pct: f64) -> FleetRow {
    let new_target = t.grams * (1.0 - tighten_pct / 100.0);
    let (cars, avg, now) = fleet(md, t, items, t.grams);
    let (_, _, new) = fleet(md, t, items, new_target);
    FleetRow {
        label: label.to_string(),
        cars_in_target_markets: cars,
        avg_g_per_km: avg,
        target_now: t.grams,
        penalty_now: now,
        target_new: new_target,
        penalty_new: new,
        breaches_new: new > 0.0,
    }
}

/// Q11. Cars only (models with a CO2 figure), counted in the markets the target applies to.
pub fn co2_fleet(snap: &Snapshot, md: &MarketData, tighten_pct: f64) -> Result<Co2Fleet> {
    if !(0.0..100.0).contains(&tighten_pct) {
        return bad("tightening must be between 0 and 100 percent");
    }
    let Some(t) = &md.co2 else { return bad("no CO2 fleet target in the knowledge graph") };
    let mut by_brand: BTreeMap<String, Vec<(&Model, f64)>> = BTreeMap::new();
    for m in snap.models.iter().filter(|m| md.co2_g.contains_key(&m.iri)) {
        by_brand.entry(m.brand_label.clone()).or_default().push((m, 1.0));
    }
    let brands: Vec<FleetRow> = by_brand.iter().map(|(b, items)| fleet_row(md, t, b, items, tighten_pct)).collect();
    let all: Vec<(&Model, f64)> = by_brand.values().flatten().copied().collect();
    let pooled = fleet_row(md, t, "All brands pooled", &all, tighten_pct);
    let mut brands = brands;
    brands.sort_by(|a, b| b.penalty_new.total_cmp(&a.penalty_new));
    Ok(Co2Fleet {
        regulation: t.label.clone(),
        tighten_pct,
        target_now: t.grams,
        target_new: t.grams * (1.0 - tighten_pct / 100.0),
        penalty_per_gram: t.penalty_per_gram,
        brands,
        pooled,
    })
}

// ---------------------------------------------------------------- Q12: competitor price cut

#[derive(Serialize)]
pub struct CompetitorHit {
    pub model: String,
    pub brand: String,
    pub rival: String,
    pub competitor: String,
    pub our_price: f64,
    pub rival_price_before: f64,
    pub rival_price_after: f64,
    pub undercut: bool,
    pub revenue_exposed: f64,
    pub volume_loss_pct: f64,
    pub revenue_at_risk: f64,
    pub contribution_at_risk: f64,
}

#[derive(Serialize)]
pub struct CompetitorCut {
    pub competitor: String,
    pub cut_pct: f64,
    pub elasticity: f64,
    pub revenue_exposed: f64,
    pub revenue_at_risk: f64,
    pub contribution_at_risk: f64,
    pub hits: Vec<CompetitorHit>,
}

/// Q12. A model is exposed when a rival of the same segment, after the cut, is priced below it.
/// Its volume then falls by `elasticity` times the cut. With no competitor named, every electric rival cuts.
pub fn competitor_cut(snap: &Snapshot, md: &MarketData, competitor: Option<&str>, cut_pct: f64, elasticity: f64) -> Result<CompetitorCut> {
    if !(0.0..100.0).contains(&cut_pct) || elasticity < 0.0 {
        return bad("cut must be between 0 and 100 percent and elasticity not negative");
    }
    let filter = competitor.map(str::trim).filter(|c| !c.is_empty()).map(str::to_lowercase);
    if let Some(c) = &filter
        && !md.rivals.values().any(|r| r.competitor.to_lowercase().contains(c.as_str()))
    {
        return bad(format!("unknown competitor '{}'", competitor.unwrap_or_default()));
    }
    let cuts = |r: &Rival| match &filter {
        Some(c) => r.competitor.to_lowercase().contains(c.as_str()),
        None => r.powertrain.ends_with("BatteryElectric"),
    };
    let mut hits = Vec::new();
    for m in &snap.models {
        let margin = unit_margin(snap, m)?;
        for rid in md.competes.get(&m.iri).map(Vec::as_slice).unwrap_or(&[]) {
            let Some(r) = md.rivals.get(rid).filter(|r| cuts(r)) else { continue };
            let after = r.price * (1.0 - cut_pct / 100.0);
            let undercut = after < m.price;
            let loss = if undercut { (elasticity * cut_pct / 100.0).min(1.0) } else { 0.0 };
            let revenue = m.price * m.planned;
            hits.push(CompetitorHit {
                model: m.label.clone(),
                brand: m.brand_label.clone(),
                rival: r.label.clone(),
                competitor: r.competitor.clone(),
                our_price: m.price,
                rival_price_before: r.price,
                rival_price_after: after,
                undercut,
                revenue_exposed: if undercut { revenue } else { 0.0 },
                volume_loss_pct: loss * 100.0,
                revenue_at_risk: revenue * loss,
                contribution_at_risk: m.planned * loss * margin,
            });
        }
    }
    // a model facing several cutting rivals is counted once, at its worst
    hits.sort_by(|a, b| a.model.cmp(&b.model).then(b.revenue_at_risk.total_cmp(&a.revenue_at_risk)));
    let mut seen = std::collections::HashSet::new();
    let (mut exposed, mut at_risk, mut contribution) = (0.0, 0.0, 0.0);
    for h in &hits {
        if seen.insert(h.model.clone()) {
            exposed += h.revenue_exposed;
            at_risk += h.revenue_at_risk;
            contribution += h.contribution_at_risk;
        }
    }
    hits.sort_by(|a, b| b.revenue_at_risk.total_cmp(&a.revenue_at_risk));
    Ok(CompetitorCut {
        competitor: competitor.map(String::from).filter(|c| !c.trim().is_empty()).unwrap_or_else(|| "all electric rivals".into()),
        cut_pct,
        elasticity,
        revenue_exposed: exposed,
        revenue_at_risk: at_risk,
        contribution_at_risk: contribution,
        hits,
    })
}

// ---------------------------------------------------------------- Q13: site dependency

#[derive(Serialize)]
pub struct SiteShare {
    pub site: String,
    pub brand: String,
    pub revenue: f64,
    pub share_pct: f64,
}

#[derive(Serialize)]
pub struct GroupSites {
    pub group: String,
    pub revenue: f64,
    pub top_site: String,
    pub top_share_pct: f64,
    pub sites: Vec<SiteShare>,
}

/// Q13. Revenue (price x planned production) per brand group and production site.
pub fn site_dependency(snap: &Snapshot, md: &MarketData) -> Vec<GroupSites> {
    let mut by_group: BTreeMap<String, Vec<SiteShare>> = BTreeMap::new();
    for (group, brand, site) in &md.sites {
        let revenue: f64 = snap.models.iter().filter(|m| &m.brand_iri == brand).map(|m| m.price * m.planned).sum();
        let brand_label = snap.models.iter().find(|m| &m.brand_iri == brand).map_or_else(|| short(brand), |m| m.brand_label.clone());
        by_group.entry(group.clone()).or_default().push(SiteShare {
            site: if site.is_empty() { "unknown site".into() } else { site.clone() },
            brand: brand_label,
            revenue,
            share_pct: 0.0,
        });
    }
    let mut out: Vec<GroupSites> = by_group
        .into_iter()
        .map(|(group, mut sites)| {
            let revenue: f64 = sites.iter().map(|s| s.revenue).sum();
            for s in &mut sites {
                s.share_pct = if revenue > 0.0 { s.revenue / revenue * 100.0 } else { 0.0 };
            }
            sites.sort_by(|a, b| b.revenue.total_cmp(&a.revenue));
            GroupSites { group, revenue, top_site: sites[0].site.clone(), top_share_pct: sites[0].share_pct, sites }
        })
        .collect();
    out.sort_by(|a, b| b.top_share_pct.total_cmp(&a.top_share_pct));
    out
}

// ---------------------------------------------------------------- Q14: powertrain mix

#[derive(Serialize)]
pub struct PowertrainRow {
    pub powertrain: String,
    pub models: usize,
    pub units: f64,
    pub revenue: f64,
    pub contribution: f64,
    pub margin_pct: f64,
    pub contribution_per_unit: f64,
    pub subsidy_dependent: Vec<String>,
    pub co2_penalty_risk: Vec<String>,
}

#[derive(Serialize)]
pub struct MixShift {
    pub brand: String,
    pub moved_units: f64,
    pub contribution_change: f64,
    pub avg_g_before: f64,
    pub avg_g_after: f64,
    pub co2_penalty_before: f64,
    pub co2_penalty_after: f64,
    pub net_change: f64,
}

#[derive(Serialize)]
pub struct MixAnalysis {
    pub shift_pct: f64,
    pub powertrains: Vec<PowertrainRow>,
    pub shifts: Vec<MixShift>,
}

/// Q14. Margin per powertrain, plus a what-if: move `shift_pct` of a brand's combustion cars to its
/// electric models (pro rata to their planned volume) and see contribution and CO2 penalty change.
/// Demand is assumed to follow (no price or subsidy effect).
pub fn mix_analysis(snap: &Snapshot, md: &MarketData, shift_pct: f64) -> Result<MixAnalysis> {
    if !(0.0..=100.0).contains(&shift_pct) {
        return bad("shift must be between 0 and 100 percent");
    }
    let mut rows: BTreeMap<String, PowertrainRow> = BTreeMap::new();
    let mut margins: HashMap<String, f64> = HashMap::new();
    for m in &snap.models {
        let um = unit_margin(snap, m)?;
        margins.insert(m.iri.clone(), um);
        let pt = md.powertrain.get(&m.iri).map_or_else(|| "Unknown".to_string(), |p| short(p));
        let e = rows.entry(pt.clone()).or_insert_with(|| PowertrainRow {
            powertrain: pt,
            models: 0,
            units: 0.0,
            revenue: 0.0,
            contribution: 0.0,
            margin_pct: 0.0,
            contribution_per_unit: 0.0,
            subsidy_dependent: vec![],
            co2_penalty_risk: vec![],
        });
        e.models += 1;
        e.units += m.planned;
        e.revenue += m.price * m.planned;
        e.contribution += um * m.planned;
        if md.is_electric(&m.iri) {
            e.subsidy_dependent.push(m.label.clone());
        }
        if let Some(t) = &md.co2
            && md.co2_g.get(&m.iri).is_some_and(|g| *g > t.grams)
        {
            e.co2_penalty_risk.push(m.label.clone());
        }
    }
    let mut powertrains: Vec<PowertrainRow> = rows.into_values().collect();
    for p in &mut powertrains {
        p.margin_pct = p.contribution / p.revenue * 100.0;
        p.contribution_per_unit = p.contribution / p.units;
    }

    let mut shifts = Vec::new();
    let mut brands: BTreeMap<String, Vec<&Model>> = BTreeMap::new();
    for m in snap.models.iter().filter(|m| md.co2_g.contains_key(&m.iri)) {
        brands.entry(m.brand_label.clone()).or_default().push(m);
    }
    for (brand, models) in brands {
        let (ice, ev): (Vec<&Model>, Vec<&Model>) = models.iter().partition(|m| !md.is_electric(&m.iri));
        if ice.is_empty() || ev.is_empty() {
            continue;
        }
        let s = shift_pct / 100.0;
        let ice_units: f64 = ice.iter().map(|m| m.planned).sum();
        let ev_units: f64 = ev.iter().map(|m| m.planned).sum();
        let moved = ice_units * s;
        let mut scale: HashMap<String, f64> = HashMap::new();
        let mut change = 0.0;
        for m in &ice {
            scale.insert(m.iri.clone(), 1.0 - s);
            change -= m.planned * s * margins[&m.iri];
        }
        for m in &ev {
            let factor = 1.0 + moved / ev_units;
            scale.insert(m.iri.clone(), factor);
            change += m.planned * (factor - 1.0) * margins[&m.iri];
        }
        let (grams_before, grams_after, pen_before, pen_after) = match &md.co2 {
            Some(t) => {
                let before: Vec<(&Model, f64)> = models.iter().map(|m| (*m, 1.0)).collect();
                let after: Vec<(&Model, f64)> = models.iter().map(|m| (*m, scale[&m.iri])).collect();
                let (_, ga, pa) = fleet(md, t, &before, t.grams);
                let (_, gb, pb) = fleet(md, t, &after, t.grams);
                (ga, gb, pa, pb)
            }
            None => (0.0, 0.0, 0.0, 0.0),
        };
        shifts.push(MixShift {
            brand,
            moved_units: moved,
            contribution_change: change,
            avg_g_before: grams_before,
            avg_g_after: grams_after,
            co2_penalty_before: pen_before,
            co2_penalty_after: pen_after,
            net_change: change + (pen_before - pen_after),
        });
    }
    shifts.sort_by(|a, b| b.net_change.total_cmp(&a.net_change));
    Ok(MixAnalysis { shift_pct, powertrains, shifts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn load() -> (Snapshot, MarketData) {
        let (kg, _, _) = Kg::load(Path::new(".."), Path::new("../rules")).unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        crate::seed::seed(&kg, &mut conn).unwrap();
        (Snapshot::load(&kg, &conn).unwrap(), MarketData::load(&kg, &conn).unwrap())
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6 * b.abs().max(1.0)
    }

    #[test]
    fn rules_flag_subsidy_dependent_models_and_co2_risk_cars() {
        let (kg, _, _) = Kg::load(Path::new(".."), Path::new("../rules")).unwrap();
        let rows = run(&kg, "SELECT ?m WHERE { ?m a vw:SubsidyDependentModel }").unwrap();
        assert_eq!(rows.len(), 4, "the four electric models are sold in Germany or France");
        let risk = run(&kg, "SELECT ?m WHERE { ?m a vw:CO2PenaltyRiskModel }").unwrap();
        assert_eq!(risk.len(), 7);
    }

    #[test]
    fn sales_plan_matches_planned_production_and_the_graph() {
        let (s, md) = load();
        for m in &s.models {
            let total: f64 = md.markets.keys().map(|mk| md.units_in(&m.iri, mk)).sum();
            assert!((total - m.planned).abs() <= md.markets.len() as f64, "{}: {total} vs {}", m.label, m.planned);
        }
        assert!(md.units.keys().all(|(m, mk)| md.markets.contains_key(mk) && s.models.iter().any(|x| &x.iri == m)));
    }

    #[test]
    fn q10_german_subsidy_end_hits_four_electric_models_and_two_leads() {
        let (s, md) = load();
        let r = subsidy_removal(&s, &md, "Germany", DEFAULT_SUBSIDY_ELASTICITY).unwrap();
        let names: Vec<&str> = r.models.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(names.len(), 4);
        assert!(["Enyaq", "ID.4", "e-tron", "Taycan"].iter().all(|n| names.contains(n)));
        let id4 = r.models.iter().find(|m| m.label == "ID.4").unwrap();
        assert!(near(id4.demand_loss_pct, 15.0), "1.5 x 4500 / 45000");
        assert!(r.leads.len() == 2 && r.leads.iter().all(|l| l.customer.contains("001")));
        let none = subsidy_removal(&s, &md, "Czech Republic", 1.5).unwrap();
        assert!(none.models.is_empty(), "no subsidy in the Czech market");
        assert!(subsidy_removal(&s, &md, "Atlantis", 1.5).is_err());
        let zero = subsidy_removal(&s, &md, "DE", 0.0);
        assert!(zero.is_err(), "unknown short name is rejected");
        assert_eq!(subsidy_removal(&s, &md, "Germany", 0.0).unwrap().lost_units, 0.0);
    }

    #[test]
    fn q11_tighter_target_makes_audi_breach_but_not_volkswagen() {
        let (s, md) = load();
        let now = co2_fleet(&s, &md, 0.0).unwrap();
        let breaching: Vec<&str> = now.brands.iter().filter(|b| b.breaches_new).map(|b| b.label.as_str()).collect();
        assert_eq!(breaching.len(), 2);
        assert!(breaching.contains(&"Porsche") && breaching.contains(&"Lamborghini"));
        let tight = co2_fleet(&s, &md, 10.0).unwrap();
        let audi = tight.brands.iter().find(|b| b.label == "Audi").unwrap();
        assert!(audi.penalty_now == 0.0 && audi.penalty_new > 0.0);
        assert!(!tight.brands.iter().find(|b| b.label == "Volkswagen").unwrap().breaches_new);
        assert!(tight.brands.iter().all(|b| b.penalty_new >= b.penalty_now));
        assert!(!tight.pooled.breaches_new, "pooled, the group stays under 108 g/km");
        assert!(co2_fleet(&s, &md, 100.0).is_err());
    }

    #[test]
    fn q12_ten_percent_cut_by_tesla_exposes_models_priced_above_the_cut_price() {
        let (s, md) = load();
        let r = competitor_cut(&s, &md, Some("tesla"), 10.0, DEFAULT_CROSS_ELASTICITY).unwrap();
        let hit = |m: &str| r.hits.iter().find(|h| h.model == m).unwrap();
        assert!(hit("ID.4").undercut, "Model Y falls to 42,300 below ID.4 at 45,000");
        assert!(!hit("Enyaq").undercut, "Enyaq at 42,000 stays below 42,300");
        assert!(near(hit("ID.4").volume_loss_pct, 8.0));
        assert!(r.hits.iter().all(|h| h.competitor == "Tesla"));
        // e-tron is hit by Model Y only once in the total even though two rivals could cut
        let all = competitor_cut(&s, &md, None, 10.0, 0.8).unwrap();
        let etron: f64 = all.hits.iter().filter(|h| h.model == "e-tron").map(|h| h.revenue_at_risk).sum();
        assert!(all.revenue_at_risk < all.hits.iter().map(|h| h.revenue_at_risk).sum::<f64>());
        assert!(etron > 0.0);
        assert!(competitor_cut(&s, &md, Some("Ford"), 10.0, 0.8).is_err());
        assert_eq!(competitor_cut(&s, &md, None, 0.0, 0.8).unwrap().revenue_at_risk, 0.0);
    }

    #[test]
    fn q13_volkswagen_group_leans_hardest_on_one_site() {
        let (s, md) = load();
        let r = site_dependency(&s, &md);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].group, "Normal cars");
        assert!(r[0].top_site.contains("Wolfsburg") && near(r[0].top_share_pct.round(), 65.0));
        for g in &r {
            assert!(near(g.sites.iter().map(|x| x.share_pct).sum::<f64>(), 100.0));
        }
    }

    #[test]
    fn q14_mix_shift_helps_volkswagen_and_costs_porsche_margin() {
        let (s, md) = load();
        let r = mix_analysis(&s, &md, 10.0).unwrap();
        assert_eq!(r.powertrains.len(), 3);
        let by = |n: &str| r.powertrains.iter().find(|p| p.powertrain == n).unwrap();
        assert!(by("Petrol").margin_pct > by("BatteryElectric").margin_pct);
        assert_eq!(by("BatteryElectric").subsidy_dependent.len(), 4);
        assert_eq!(r.shifts.len(), 4, "only brands with both combustion and electric cars can shift");
        let porsche = r.shifts.iter().find(|x| x.brand == "Porsche").unwrap();
        assert!(porsche.contribution_change < 0.0 && porsche.co2_penalty_after < porsche.co2_penalty_before);
        assert!(r.shifts.iter().all(|x| x.avg_g_after < x.avg_g_before));
        let zero = mix_analysis(&s, &md, 0.0).unwrap();
        assert!(zero.shifts.iter().all(|x| x.net_change.abs() < 1e-3));
        assert!(mix_analysis(&s, &md, 101.0).is_err());
    }
}
