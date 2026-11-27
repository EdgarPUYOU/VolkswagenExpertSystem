use crate::kg::{Kg, Row};
use anyhow::Result;

const PREFIXES: &str = "PREFIX vw: <http://example.org/volkswagen/>\n\
                        PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n\
                        PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>\n\
                        PREFIX owl: <http://www.w3.org/2002/07/owl#>\n\
                        PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>\n";

pub const MODELS_QUERY: &str = "SELECT ?model ?label ?brand ?price ?planned WHERE {
    ?model a vw:Model ; vw:modelOf ?b ; vw:price ?price .
    ?b rdfs:label ?brand .
    OPTIONAL { ?model rdfs:label ?label }
    OPTIONAL { ?model vw:plannedAnnualProduction ?planned }
} ORDER BY ?brand ?label";

pub const CONTRACTS_QUERY: &str = "SELECT ?contract ?label ?supplier ?committed ?projected ?price
       (GROUP_CONCAT(DISTINCT STR(?reason); separator=',') AS ?reasons) WHERE {
    ?contract a vw:SupplyContract ;
              vw:contractSupplier ?s ;
              vw:currentContractedAnnualVolume ?committed ;
              vw:projectedAnnualComponentVolume ?projected ;
              vw:currentUnitPriceEUR ?price .
    OPTIONAL { ?contract rdfs:label ?label }
    OPTIONAL { ?s rdfs:label ?supplier }
    OPTIONAL { ?contract vw:contractReviewReason ?reason }
} GROUP BY ?contract ?label ?supplier ?committed ?projected ?price ORDER BY ?contract";

/// Contract -> supplier -> component -> models, a four-hop path.
pub const PATH_QUERY: &str = "SELECT ?model ?contract ?supplier WHERE {
    ?contract vw:contractCoversComponent ?c ;
              vw:contractSupplier ?supplier ;
              vw:contractIncludesModel ?model .
    ?model vw:hasComponent ?c .
}";

/// Transitive subclass walk, to check property paths work.
pub const SUBCLASS_QUERY: &str = "SELECT ?cls WHERE { ?cls rdfs:subClassOf* vw:Product }";

pub fn run(kg: &Kg, query: &str) -> Result<Vec<Row>> {
    kg.select(&format!("{PREFIXES}{query}"))
}

pub fn models(kg: &Kg) -> Result<Vec<Row>> {
    run(kg, MODELS_QUERY)
}

pub fn contracts(kg: &Kg) -> Result<Vec<Row>> {
    run(kg, CONTRACTS_QUERY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn kg() -> Kg {
        crate::kg::load_for_test().0
    }

    #[test]
    fn loads_all_modules_into_named_graphs() {
        let (kg, stats) = Kg::load_dir(Path::new("..")).unwrap();
        assert_eq!(stats.len(), 18, "nine original TTL files plus finance, supply, risk, market and campaign modules");
        assert_eq!(stats.iter().map(|s| s.triples).sum::<usize>(), kg.len().unwrap());
    }

    #[test]
    fn models_and_contracts_are_found_across_graphs() {
        let kg = kg();
        assert_eq!(models(&kg).unwrap().len(), 15);
        assert_eq!(contracts(&kg).unwrap().len(), 24);
    }

    #[test]
    fn path_query_joins_contract_supplier_model() {
        let rows = run(&kg(), PATH_QUERY).unwrap();
        assert!(!rows.is_empty());
    }

    #[test]
    fn subclass_property_path_walks_the_hierarchy() {
        let rows = run(&kg(), SUBCLASS_QUERY).unwrap();
        let has = |name: &str| rows.iter().any(|r| r["cls"].ends_with(name));
        assert!(has("Product") && has("Component") && has("TyreSet"));
    }
}

#[cfg(test)]
mod inference_tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    fn kg() -> Kg {
        crate::kg::load_for_test().0
    }

    /// Run a two-column query and return "a -> b" pairs with IRIs shortened.
    fn pairs(kg: &Kg, q: &str) -> BTreeSet<String> {
        let short = |s: &String| s.rsplit('/').next().unwrap().to_string();
        run(kg, q)
            .unwrap()
            .iter()
            .map(|r| format!("{} -> {}", short(&r["a"]), short(&r["b"])))
            .collect()
    }

    fn names(kg: &Kg, q: &str) -> BTreeSet<String> {
        run(kg, q).unwrap().iter().map(|r| r["a"].rsplit('/').next().unwrap().to_string()).collect()
    }

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reaches_a_fixpoint_and_is_idempotent() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        let (kg, _, first) = Kg::load(Path::new(".."), Path::new("../rules"), &conn).unwrap();
        assert!(first.inferred > 0);
        let before = kg.len().unwrap();
        let again = kg.infer(Path::new("../rules")).unwrap();
        assert_eq!(again.passes, 1, "second run derives nothing");
        assert_eq!(kg.len().unwrap(), before);
    }

    #[test]
    fn defined_classes_are_classified_from_powertrain() {
        let kg = kg();
        assert_eq!(
            names(&kg, "SELECT ?a WHERE { ?a a vw:ElectricModel }"),
            set(&["ID4", "Enyaq", "Taycan", "e-tron"])
        );
        assert_eq!(names(&kg, "SELECT ?a WHERE { ?a a vw:HybridModel }"), set(&[]));
        assert_eq!(names(&kg, "SELECT ?a WHERE { ?a a vw:CombustionModel }").len(), 11);
        assert_eq!(
            names(&kg, "SELECT ?a WHERE { ?a a vw:BrandWithElectricModels }"),
            set(&["Volkswagen", "Skoda", "Porsche", "Audi"])
        );
    }

    #[test]
    fn inverse_subproperty_and_subclass_inference() {
        let kg = kg();
        // owl:inverseOf: only hasModel is asserted, modelOf is derived (and the other way round)
        assert!(names(&kg, "SELECT ?a WHERE { ?a vw:belongsToGroup vw:Normal }").contains("Volkswagen"));
        // rdfs:subPropertyOf: hasEngineModel implies hasComponent; hasModel implies hasProduct
        assert!(pairs(&kg, "SELECT ?a ?b WHERE { ?a vw:hasProduct ?b }").contains("Volkswagen -> Golf"));
        // rdfs:subClassOf: a TyreSet is a Component, a Product and an Offering
        assert!(names(&kg, "SELECT ?a WHERE { ?a a vw:Offering }").contains("TyreSet_Standard"));
        // client type propagates from the customer subclasses
        assert!(names(&kg, "SELECT ?a WHERE { ?a a vw:Client }").contains("Customer_001"));
    }

    #[test]
    fn portfolio_rules() {
        let kg = kg();
        let count = |g: &str| {
            run(&kg, &format!("SELECT ?a ?b WHERE {{ vw:{g} vw:groupHasModel ?b . BIND(vw:{g} AS ?a) }}")).unwrap().len()
        };
        assert_eq!((count("Normal"), count("SportLuxury"), count("Trucks")), (5, 8, 2));
        assert_eq!(
            pairs(&kg, "SELECT ?a ?b WHERE { ?a vw:groupUsesSite ?b }").len(),
            7,
            "one site per brand, seven brands"
        );
        assert!(pairs(&kg, "SELECT ?a ?b WHERE { ?a vw:groupUsesSite ?b }").contains("Trucks -> Munich"));
    }

    #[test]
    fn contract_review_rules_match_the_numbers() {
        let kg = kg();
        let over = names(
            &kg,
            "SELECT ?a WHERE { ?a vw:contractReviewReason vw:VolumeExceedsCommitment }",
        );
        // every contract flagged for volume really has projected > committed, and none is missed
        let expected: BTreeSet<String> = contracts(&kg)
            .unwrap()
            .iter()
            .filter(|c| c["projected"].parse::<f64>().unwrap() > c["committed"].parse::<f64>().unwrap())
            .map(|c| c["contract"].rsplit('/').next().unwrap().to_string())
            .collect();
        assert_eq!(over, expected);
        let discount = names(&kg, "SELECT ?a WHERE { ?a vw:contractReviewReason vw:VolumeDiscountAvailable }");
        assert!(discount.contains("SupplyContract_19_Infotainment_Standard"));
        assert!(!discount.contains("SupplyContract_03_Porsche911Engine"));
    }

    #[test]
    fn contracts_listing_carries_the_inferred_review_reasons() {
        let kg = kg();
        let rows = contracts(&kg).unwrap();
        assert_eq!(rows.len(), 24, "grouping must not drop or duplicate contracts");
        let flagged: Vec<_> = rows.iter().filter(|r| r.get("reasons").is_some_and(|s| !s.is_empty())).collect();
        let candidates = names(&kg, "SELECT ?a WHERE { ?a a vw:ContractReviewCandidate }");
        assert_eq!(flagged.len(), candidates.len());
        assert_eq!(flagged.len(), 16);
        let c01 = rows.iter().find(|r| r["contract"].ends_with("SupplyContract_01_EA211_15TSI")).unwrap();
        assert!(c01["reasons"].contains("VolumeExceedsCommitment") && c01["reasons"].contains("VolumeDiscountAvailable"));
    }

    #[test]
    fn sales_rules_match_customers_to_models() {
        let kg = kg();
        let suitable = pairs(&kg, "SELECT ?a ?b WHERE { ?a vw:suitableModel ?b }");
        assert!(suitable.contains("Customer_001 -> ID4") && suitable.contains("Customer_001 -> Enyaq"));
        assert!(suitable.contains("Customer_002 -> Golf"));
        // budget rule: the 55k EV shopper is not offered the e-tron or Taycan
        assert!(!suitable.contains("Customer_001 -> e-tron") && !suitable.contains("Customer_001 -> Taycan"));
        let likely = pairs(&kg, "SELECT ?a ?b WHERE { ?a vw:likelyInterestedIn ?b }");
        assert!(likely.contains("Customer_003 -> 911") && likely.contains("Customer_005 -> TGX"));
        assert_eq!(
            pairs(&kg, "SELECT ?a ?b WHERE { ?a vw:highIntentForModel ?b }"),
            set(&["Customer_001 -> ID4", "Customer_001 -> Enyaq"])
        );
    }
}
