# Volkswagen Group Expert System

A mini ERP for a car group that answers finance, procurement, strategy and sales questions by combining two stores:

- a **knowledge graph** (Turtle files in `ontology/`, loaded into an embedded Oxigraph store) for structure and meaning: who supplies what, which contract covers which component, which models use it, which clauses link a contract to a commodity index, which regulations apply where;
- **SQLite** for numbers: index values, FX rates, prices, planned production, contract terms, budgets, campaigns and financing terms.

The two are joined by IRI. A Rust (`axum`) API runs the analyses, a React front end shows them, and a chat bubble answers questions through the same analyses (a language model when `AI_API_KEY` is set, keyword matching otherwise).

> **Important:** every supplier, price, contract, customer, regulation and scenario result is synthetic and illustrative. Overheads, index paths and elasticities are calibrated so the demo questions give non-trivial answers. Nothing here describes the real company and nothing here should inform a real purchasing, pricing or investment decision.

## Quick start

```bash
make help        # list every shortcut
make env         # copy .env.example to .env (optional: add AI_API_KEY for the language-model chat)
make install     # web dependencies (once)
make dev         # API on :8000 and UI on :5173
make test        # Rust tests and web type check
make prod        # release binary with the UI embedded
```

Needs Rust (stable) and Node 24. Docker is only needed for `make prod-docker` and the optional GraphDB browser (`make graphdb`).

## Repository layout

| Path | What it holds |
| --- | --- |
| `ontology/` | Turtle files: T-Boxes (schema), A-Boxes (instances) and the original SWRL rule files. One named graph per file. |
| `rules/` | SPARQL update rules (`*.ru`) that materialise inference at load time. Generic OWL/RDFS rules, the ported SWRL rules, exposure and market rules. |
| `app/` | Rust API: `kg.rs` (Oxigraph adapter and rule engine), `facts.rs`, `db.rs` and `migrations/` (SQLite), `services/` (one module per question group, chat tools, language-model planner), `routers/`. |
| `web/` | React + TypeScript + Vite + shadcn/ui front end. Built into the Rust binary. |
| `tools/` | `gen_supply_abox.py` regenerates `ontology/VW_Supply_Abox.ttl`. |
| `infra/` | `Caddyfile` for production (HTTPS and basic auth). |
| `docs/` | `plan.md` (design and decisions) and `progress.md` (status and findings). |
| `Dockerfile`, `docker-compose*.yml` | Multi-stage image; dev compose (GraphDB) and prod compose (Caddy and app). |
| `data/` | SQLite file at run time (git-ignored). |

## Ontology modules (`ontology/`)

The namespace is `http://example.org/volkswagen/`. Files do not import one another; the app loads all of them.

| Module | Files |
| --- | --- |
| Core portfolio | `VWGroup_Tbox`, `VWGroup_Abox`, `VWGroup_rules` |
| Sales and customers | `VW_Sales_Tbox`, `VW_Sales_Abox`, `VW_sales_rules` |
| Contracts and components | `VW_Contracts_Tbox`, `VW_Contracts_Abox`, `VW_Contracts_rules` |
| Finance | `VW_Finance_Tbox`, `VW_Finance_Abox` (currencies, profit centers, cost elements, commodity indices) |
| Supply terms | `VW_Supply_Tbox`, `VW_Supply_Abox` (indexation clauses, sourcing, alternative suppliers, payment terms, launch dates; the A-box is generated) |
| Risk | `VW_Risk_Tbox` (derived exposure classes) |
| Market and regulation | `VW_Market_Tbox`, `VW_Market_Abox` (markets, CO2 target, subsidies, tariff, competitors) |
| Campaigns and financing | `VW_Campaign_Tbox`, `VW_Campaign_Abox` |

Numbers that change or are transactions are **not** in the Turtle files. Model prices and planned production, and contract prices, volumes and discount tiers, live in SQLite and are written into the graph (`graph/erp-facts`) at load time so the rules see them. The projected volume of a component is computed from planned production.

## Reasoning

No OWL or SWRL reasoner runs. The `*_rules.ttl` files keep the original SWRL rules as documentation; the same logic runs as SPARQL updates in `rules/`, applied in name order until nothing new is derived (a fixpoint, three passes today). Rules cover inverse and sub-property axioms, defined classes, customer-to-model matching and lead intent, contract review (volume above commitment, discount tier reached), commodity and currency exposure, single-sourced components, subsidy dependence and CO2 risk. Inferred triples go to `graph/inferred`.

## What it answers

Nineteen questions, each a service in `app/src/services/`, a page or tab in the UI, a chat tool, and a Rust test: commodity and FX shocks by brand and model, currency exposure, commodity sensitivity, volume-discount savings, supplier dependency, single-sourced parts, expiring contracts, lead-time shocks, subsidy removal, CO2 fleet targets, competitor price cuts, production-site dependency, powertrain mix, high-intent leads on margin-pressured models, campaigns against capacity, lease versus loan, the combined "bad year", and the margin trace that explains why a model's margin moved. See `docs/plan.md` section 5 for the list.

## Configuration

Settings come from flags, environment variables or a `.env` file (see `.env.example`). The language-model chat uses any OpenAI-compatible API through `AI_API_KEY`, `AI_BASE_URL` and `AI_MODEL`. The key stays on the server.

## Validation

- `make test` runs the Rust tests (rules, services, chat, language-model transport against a local stub) and type-checks the web app.
- Turtle syntax only: `python -c "from pathlib import Path; from rdflib import Graph; files=sorted(Path('ontology').glob('*.ttl')); [Graph().parse(str(f), format='turtle') for f in files]; print(f'Parsed {len(files)} files')"` (needs `rdflib`). Parsing does not run any rules.
- Browsing the graph in GraphDB (`make graphdb`, then import from the server files) shows the Turtle files only: prices and volumes live in SQLite, so use `vw-erp query '<SPARQL>'` to inspect the full graph with facts and inferred triples.

## Extending

- Put schema additions in the matching T-Box and instances in the matching A-Box; reuse the `vw:` namespace and existing individuals.
- Put changing numbers in a SQLite migration (`app/migrations/`), not in Turtle.
- Put new inference in a `rules/*.ru` file and cover it with a test.
