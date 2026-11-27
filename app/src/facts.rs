//! Numeric business facts live in SQLite (`model_facts`, `contract_terms`). At load time they are
//! written into the knowledge graph as triples in their own named graph, so the SPARQL rules and the
//! queries see them exactly as if they were still in the TTL files.
//!
//! The projected annual volume of a component is computed here: the sum of planned production of the
//! models that use it (each model counted once per contract).

use crate::kg::{Fact, Kg};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{BTreeSet, HashMap};

const NS: &str = "http://example.org/volkswagen/";
const DECIMAL: &str = "http://www.w3.org/2001/XMLSchema#decimal";
const INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";

fn price(subject: &str, predicate: &str, v: f64) -> Fact {
    Fact { subject: subject.into(), predicate: predicate.into(), value: format!("{v:.2}"), datatype: DECIMAL }
}

fn count(subject: &str, predicate: &str, v: f64) -> Fact {
    Fact { subject: subject.into(), predicate: predicate.into(), value: format!("{v:.0}"), datatype: INTEGER }
}

pub fn from_db(kg: &Kg, conn: &Connection) -> Result<Vec<Fact>> {
    let mut facts = Vec::new();
    let mut planned: HashMap<String, f64> = HashMap::new();
    let mut stmt = conn.prepare("SELECT model_iri, price_eur, planned_units, produced_last_year FROM model_facts ORDER BY model_iri")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?)))? {
        let (iri, p, plan, last) = row?;
        facts.push(price(&iri, "price", p));
        facts.push(count(&iri, "plannedAnnualProduction", plan));
        facts.push(count(&iri, "carsProducedPerYear", last));
        planned.insert(iri, plan);
    }

    // models using each component, from the asserted triples (the rules have not run yet)
    let mut users: HashMap<String, BTreeSet<String>> = HashMap::new();
    for r in kg.select("PREFIX vw: <http://example.org/volkswagen/> SELECT ?m ?c WHERE { ?m a vw:Model ; vw:hasComponent ?c }")? {
        users.entry(r["c"].clone()).or_default().insert(r["m"].clone());
    }
    let mut covers: HashMap<String, BTreeSet<String>> = HashMap::new();
    for r in kg.select("PREFIX vw: <http://example.org/volkswagen/> SELECT ?k ?c WHERE { ?k a vw:SupplyContract ; vw:contractCoversComponent ?c }")? {
        covers.entry(r["k"].clone()).or_default().insert(r["c"].clone());
    }

    let mut stmt = conn.prepare("SELECT contract_iri, unit_price_eur, committed_volume, discount_price_eur, discount_tier_volume FROM contract_terms ORDER BY contract_iri")?;
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?, r.get::<_, Option<f64>>(3)?, r.get::<_, Option<f64>>(4)?))
    })? {
        let (iri, unit, committed, disc, tier) = row?;
        facts.push(price(&iri, "currentUnitPriceEUR", unit));
        facts.push(count(&iri, "currentContractedAnnualVolume", committed));
        if let Some(d) = disc {
            facts.push(price(&iri, "discountUnitPriceEUR", d));
        }
        if let Some(t) = tier {
            facts.push(count(&iri, "discountTierAnnualVolume", t));
        }
        let models: BTreeSet<&String> = covers.get(&iri).into_iter().flatten().filter_map(|c| users.get(c)).flatten().collect();
        let projected: f64 = models.iter().filter_map(|m| planned.get(*m)).sum();
        facts.push(count(&iri, "projectedAnnualComponentVolume", projected));
    }
    debug_assert!(facts.iter().all(|f| f.subject.starts_with(NS)));
    Ok(facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::portfolio::run;

    #[test]
    fn numbers_are_no_longer_in_the_ttl_files() {
        let (kg, stats) = Kg::load_dir(std::path::Path::new("..")).unwrap();
        let rows = kg.select("PREFIX vw: <http://example.org/volkswagen/> SELECT ?s WHERE { ?s vw:price ?p }").unwrap();
        assert!(rows.is_empty(), "prices come from SQLite now");
        assert!(stats.iter().all(|s| !s.graph.ends_with("erp-facts")));
    }

    #[test]
    fn facts_cover_every_model_and_contract_and_projected_volume_is_computed() {
        let (kg, conn) = crate::kg::load_for_test();
        let facts = from_db(&kg, &conn).unwrap();
        assert_eq!(facts.len(), 15 * 3 + 24 * 5);
        let rows = run(
            &kg,
            "SELECT ?k ?proj (SUM(?pl) AS ?calc) WHERE { ?k a vw:SupplyContract ; vw:contractCoversComponent ?c ; vw:projectedAnnualComponentVolume ?proj .
               ?m a vw:Model ; vw:hasComponent ?c ; vw:plannedAnnualProduction ?pl } GROUP BY ?k ?proj",
        )
        .unwrap();
        assert_eq!(rows.len(), 24);
        assert!(rows.iter().all(|r| r["proj"] == r["calc"]), "projected volume is the sum of planned production");
    }

    #[test]
    fn changing_a_number_in_sqlite_changes_the_graph_and_the_rules() {
        let (_, conn) = crate::kg::load_for_test();
        let budget_rule = "SELECT ?c WHERE { ?c vw:withinBudget vw:ID4 }";
        let (kg, _, _) = Kg::load(std::path::Path::new(".."), std::path::Path::new("../rules"), &conn).unwrap();
        let before = run(&kg, budget_rule).unwrap().len();
        conn.execute("UPDATE model_facts SET price_eur = 500000 WHERE model_iri LIKE '%/ID4'", []).unwrap();
        conn.execute("UPDATE model_facts SET planned_units = planned_units * 2 WHERE model_iri LIKE '%/Golf'", []).unwrap();
        let (kg, _, _) = Kg::load(std::path::Path::new(".."), std::path::Path::new("../rules"), &conn).unwrap();
        let price = run(&kg, "SELECT ?p WHERE { vw:ID4 vw:price ?p }").unwrap();
        assert_eq!(price[0]["p"].parse::<f64>().unwrap(), 500000.0);
        assert!(before > 0 && run(&kg, budget_rule).unwrap().is_empty(), "a price above every budget drops the budget match");
        let tyre = run(&kg, "SELECT ?v WHERE { ?k vw:contractCoversComponent vw:TyreSet_Standard ; vw:projectedAnnualComponentVolume ?v }").unwrap();
        assert!(tyre[0]["v"].parse::<f64>().unwrap() > 600_000.0, "doubling the Golf plan raises the tyre volume: {tyre:?}");
    }
}
