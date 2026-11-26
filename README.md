# Volkswagen Group Expert System

This project is a modular OWL/RDF knowledge graph for exploring Volkswagen Group's vehicle portfolio, matching potential customers to models, and identifying supply contracts that may be worth reviewing when planned production increases.

The ontology uses the namespace `http://example.org/volkswagen/`. Its Turtle files are divided into T-Boxes (vocabulary and schema), A-Boxes (instance data), and SWRL rules. The files do not automatically import one another: load the relevant files into the same ontology or RDF graph when working with a use case.

> **Important:** Customer profiles, component suppliers, supplier locations, prices, contract terms, and planned production targets are synthetic or illustrative examples. They are for demonstrating ontology structure and inference, not for real purchasing, production, or customer decisions. Review the source data before reusing or extending it.

## Project files

| File | Purpose |
| --- | --- |
| `VWGroup_Tbox.ttl` | Core vehicle schema: organizations, brands, models, production sites, powertrains, vehicle types, seats, and their properties. |
| `VWGroup_Abox.ttl` | Core instance data: brand groups, brands, 15 vehicle models, model attributes, seat models, and production sites. Each model's component assignments and illustrative planned annual production are grouped with its other model facts. |
| `VWGroup_rules.ttl` | Three portfolio and production-site SWRL rules. |
| `VW_Sales_Tbox.ttl` | Sales schema: customer types and profile attributes, preferences, matching relations, and recommendation classes. |
| `VW_Sales_Abox.ttl` | Five synthetic customer profiles and illustrative model-to-interest/use tags. |
| `VW_Sales_rules.ttl` | Ten SWRL rules for customer/model matching, likely interest, and near-term purchase intent. |
| `VW_Contracts_Tbox.ttl` | Component, supplier, supply-contract, planned-production, and renegotiation vocabulary. |
| `VW_Contracts_Abox.ttl` | Illustrative components and suppliers, plus 24 example supply contracts with volume and price terms. |
| `VW_Contracts_rules.ttl` | Two SWRL rules for flagging contracts and components for review. |

## Use cases

### 1. Product portfolio and production-site analysis

The core module represents the company, brand groups, brands, models, vehicle characteristics, and production sites. Its SWRL rules can:

1. Infer the models included in a brand group's portfolio from its brands and their models.
2. Infer which sites a brand uses for its models when model-to-site facts are present.
3. Infer the sites used by a brand group from its member brands.

### 2. Customer-to-model matching

The Sales module compares a customer's stated preferences with model facts. The rules can infer whether a model:

- Fits the customer's maximum budget.
- Meets the minimum seat requirement.
- Matches the preferred powertrain and vehicle type.
- Matches stated interests or intended vehicle use.
- Is suitable based on the budget, seating, and powertrain checks.
- Is a likely match when the required preference matches and personalization consent is explicitly recorded.
- Is a high-intent lead when a likely match is combined with a “ready to purchase” stage and a purchase horizon within three months.

Age and income are represented as broad bands, and location as a broad region. These fields are descriptive context; they are not used by the included lead-matching rules. Individual profiles are synthetic, and the business-customer example does not include personal demographic data.

### 3. Component contract review

The Contracts module links models to components and supply contracts to their covered components, suppliers, and included models. It flags a contract and its component as review candidates when either:

- The projected annual component volume exceeds the contract's current annual commitment; or
- The projected volume reaches a discount tier whose unit price is lower than the current unit price.

The projected component volumes in the example contracts are precomputed totals over the models listed on each contract. They are not dynamically aggregated by SWRL. If you change a model's planned production, component assignments, or the models covered by a contract, recalculate the relevant `projectedAnnualComponentVolume` values in `VW_Contracts_Abox.ttl`.

## Loading and running the ontology

1. Open the Turtle files in an RDF/OWL tool such as Protégé.
2. Load or merge the files for the use case into one ontology or RDF graph:

   | Use case | Files to load |
   | --- | --- |
   | Portfolio | `VWGroup_Tbox.ttl`, `VWGroup_Abox.ttl`, `VWGroup_rules.ttl` |
   | Sales matching | The three core files above, plus `VW_Sales_Tbox.ttl`, `VW_Sales_Abox.ttl`, and `VW_Sales_rules.ttl` |
   | Contract review | The three core files above, plus `VW_Contracts_Tbox.ttl`, `VW_Contracts_Abox.ttl`, and `VW_Contracts_rules.ttl` |
   | All use cases | All nine Turtle files |

3. Enable a SWRL-capable rule engine compatible with the rules and built-ins used here. An OWL ontology editor or OWL reasoner alone may not execute SWRL rules.
4. Run the rules and inspect inferred assertions such as `vw:suitableModel`, `vw:likelyInterestedIn`, `vw:highIntentForModel`, and `vw:ContractReviewCandidate`.

The SWRL rules describe inferences; this project does not include a bundled application or a configured reasoner. Results depend on the rule engine and the data loaded with the rules.

## Data notes and extension guidance

- Keep schema additions in the appropriate T-Box and instance facts in the corresponding A-Box.
- Reuse the shared `vw:` namespace and existing model/component individuals when adding facts.
- Keep each model's component assignments and `plannedAnnualProduction` in its model block in `VWGroup_Abox.ttl`.
- Keep contract-specific terms and aggregate projected component volumes in `VW_Contracts_Abox.ttl`.
- The example component assignments are one component per vehicle for purposes of volume calculations. Update the projected contract totals if component quantities per vehicle should differ.
- Customer matching is based on explicit preferences and matching tags, not on a statistical or machine-learning prediction. Consent is required by the likely-interest rules.

## Validation

The project files are Turtle documents and can be syntax-checked with RDFLib. With Python and RDFLib installed, run this from the project directory:

```powershell
python -m pip install rdflib
python -c "from pathlib import Path; from rdflib import Graph; files=sorted(Path('.').glob('*.ttl')); [Graph().parse(str(f), format='turtle') for f in files]; print(f'Parsed {len(files)} Turtle files successfully')"
```

Parsing checks Turtle syntax only. It does not execute SWRL rules or verify that a particular reasoner will produce the expected inferences.
