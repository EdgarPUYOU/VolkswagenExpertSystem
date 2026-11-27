# Progress

Last updated: 2026-11-27 (step 5 and Makefile added). Branch: `feat/architecture-skeleton` (nothing committed yet).

The full design is in [plan.md](plan.md). This file tracks what is done and what is next.

## Summary

| Step (plan.md section 9) | Status |
| --- | --- |
| 1. Oxigraph spike | Done |
| 2. TTL fixes and materialisation rules | Done in Oxigraph; GraphDB comparison not done |
| 3. Rust and web skeleton | Done, except Docker verification |
| 4. Finance and contract terms (Q1 to Q9, Q19) | Done: data, rules, API, UI, tests |
| 5. Market, regulation, competitors (Q10 to Q14) | Done: data, rules, API, UI, tests |
| 6. Sales, campaigns, financing (Q15 to Q17) | Done: data, API, UI, chat tools, tests |
| 7. Scenarios (Q18) | Done: combined service, saved runs, API, UI, chat tool, tests |
| 8. Move numeric facts out of the TTL | Done: SQLite is the source, written into the graph at load |
| 9. Deploy to the VPS | Not started |

Plan sections 1 and 2 (goal and architecture) are implemented. The first demo (scenario lab, explain view, contracts, suppliers, markets) runs end to end.

## Done

### Planning

- Chose the architecture: Rust (`axum`) JSON API, embedded Oxigraph for the knowledge graph, SQLite for ERP data, React + shadcn/ui front end, Caddy in front, deployed to a small VPS.
- Defined the business-side ontology modules (Finance, Contract terms, Market and regulation, Sales and revenue, Risk and scenario), 19 target questions, 12 UI modules, and the data needed to answer every question.

### Step 1: Oxigraph spike

Measured with `vw-erp bench` on the 9 existing TTL files (2,263 triples, release build):

- Load time: about 15 ms.
- Memory: 7.5 MB after load, 8.7 MB after 200 runs of each query. About 11 MB while serving.
- Query mean latency: 0.05 to 0.5 ms (models 0.3, contracts 0.5, contract-supplier-model path 0.4, subclass walk 0.05).
- Binary: 12.3 MB including the web bundle.

Decision: embedded Oxigraph is viable on a tiny VPS. It runs in memory and is rebuilt from the TTL files at startup.

### Step 3: Skeleton

- `app/` (Rust):
  - Subcommands: `serve`, `load-kg`, `migrate`, `healthcheck`, `bench`.
  - Endpoints: `/health`, `/api/kg/graphs`, `/api/models` (15 rows), `/api/contracts` (24 rows).
  - SQLite in WAL mode with numbered migrations (first table: `scenario_runs`).
  - Each TTL file loads into its own named graph; queries run over the union.
  - The built web bundle is embedded, with SPA fallback; unknown `/api/*` paths return 404.
  - 5 Rust tests pass (`cargo test`).
- `web/` (React 19, Vite, TypeScript, Tailwind, shadcn/ui, TanStack Query and Table v8, React Router):
  - Sidebar shell with four pages: Dashboard, Portfolio, Contracts, Admin.
  - Checked in a browser against the embedded build: renders with real data, no console errors.
- Deployment files: `Dockerfile` (Node, Rust, distroless), `docker-compose.yml` (dev, GraphDB on localhost), `docker-compose.prod.yml` (Caddy with HTTPS and basic auth, plus app), `infra/Caddyfile`, `.env.example`, `.dockerignore`, `.gitignore`.

### Step 2: Rules

- Fixed the three invalid `owl:equivalentClass` declarations in `VWGroup_Tbox.ttl`.
- Wrote `rules/*.ru` (SPARQL updates): generic OWL/RDFS materialisation (subproperty, inverse, subclass, six defined classes) and all 15 SWRL rules (3 portfolio, 2 contracts, 10 sales).
- The engine applies them until a fixpoint: 3 passes, 554 inferred triples, about 20 ms. Total 2,814 triples, about 9.7 MB RSS.
- Checked against the data by hand: 4 electric models, group model counts 5/8/2, 16 of 24 contracts flagged for review (25 reasons), one high-intent lead (Customer_001, for ID4 and Enyaq).
- 12 Rust tests pass. The Contracts page now shows the inferred review reasons.

### Step 4: Finance and contract terms (first demo)

- New TTL modules: Finance (currencies, profit centers, cost elements, 6 commodity indices), Supply (indexation clauses, sourcing, alternative suppliers, payment terms, launch dates), Risk (derived exposure classes). 14 TTL files and 3,500 triples (800 inferred) in total.
- Existing contract files changed: `contractCurrency` is now an object property; the nine Swedish and Czech contracts are in SEK and CZK; contract end dates are varied (2027-03 to 2029-12).
- `rules/40_exposure.ru`: commodity-exposed contracts, models and profit centers; FX-exposed contracts; single-sourced critical components (15).
- SQLite: migration 002 and a deterministic `vw-erp seed` (35 months of index and FX series, overhead per model, 2027 budget, 2026 actuals). It runs automatically on the first start.
- API and services for Q1 to Q9 and Q19, plus scenario runs that can be saved and reloaded, a dashboard endpoint, and market series.
- UI: Dashboard, Scenario lab, Explain a margin, Contracts (review, expiring, volume discounts), Suppliers (dependency, single-sourced, lead-time shock), Commodities and FX. Checked in a browser, no console errors.
- 29 Rust tests pass; clippy is clean; bench: 16 ms load, 32 ms rules, about 10 MB RSS, 15 MB binary.
- Headline results (illustrative data): rubber +15% costs about EUR 101M a year; steel +10% with aluminium +8% pushes four models (Scania R-series, MAN TGX, ID.4, Enyaq) under a 15% margin; 39% of spend is in SEK or CZK; 8 contracts expire within a year, 5 of them also under-committed; doubling the battery lead time moves the ID.4 launch by 120 days; Golf's margin fell 0.56 points in six months, mostly from rubber.

### Step 5: Market, regulation, competitors (Q10 to Q14)

- New TTL: `VW_Market_Tbox/Abox` (5 markets with currencies, EU CO2 target, German and French EV subsidies, US tariff, 5 competitors with 8 models, `co2GramsPerKm`, `priceSegment`, `soldInMarket`, `competesWith` on our models). 16 TTL files, about 3,780 triples (830 inferred).
- `rules/50_market.ru`: `SubsidyDependentModel` (4 electric models) and `CO2PenaltyRiskModel` (7 cars above target).
- SQLite migration 003: `sales_plan` (planned units per model and market) and `competitor_prices`; `vw-erp seed` fills both. Existing databases are reseeded on start when `sales_plan` is empty.
- `services/market.rs`: Q10 subsidy removal (lost units, revenue, contribution, affected leads), Q11 CO2 fleet by brand with a tightening percentage and a pooled view, Q12 competitor price cut, Q13 site dependency per brand group, Q14 margin per powertrain plus a combustion-to-electric shift what-if.
- API: `/api/strategy/{subsidy,co2,competitor,sites,mix,markets}`. UI: new page "Markets and regulation" with one tab per question.
- 7 new Rust tests, 50 in total; clippy clean.
- Headline results (illustrative): ending the German subsidy at elasticity 1.5 loses about 21,600 cars and EUR 1.09B revenue across ID.4, Enyaq, e-tron and Taycan, and touches Customer 001's two leads; at a 120 g/km target Porsche and Lamborghini already breach, and a 10% tighter target adds Audi (EUR 164M), while Volkswagen and Skoda stay under; a 10% Tesla cut undercuts ID.4, Taycan and e-tron but not Enyaq; Volkswagen Group cars depend 65% on Wolfsburg; shifting 10% of combustion to electric helps Volkswagen, Skoda and Porsche (via lower CO2 penalty) and costs Audi.
- Assumptions to state when demoing: price elasticities are inputs (defaults 1.5 for subsidy, 0.8 for competitor cut); CO2 target of 120 g/km and EUR 95 per g/km are illustrative; only the first CO2 target is modelled.
- Chat: 5 new tools (`subsidy_removal`, `co2_fleet`, `competitor_price_cut`, `site_dependency`, `powertrain_mix`; 16 in total), `list_entities` also lists markets and competitors, planner rules and 5 new suggestions. `call_tool` and `plan` now take `MarketData`. 52 Rust tests.
- `Makefile` added: `make dev`, `make prod`, `make prod-docker`, `make test` and more (`make help`).

### Step 6: Sales, campaigns, financing (Q15 to Q17)

- New TTL: `VW_Campaign_Tbox/Abox` (6 incentive campaigns, 4 Taycan financing products: lease 36 and 48 months, loan 48 and 60 months, offered in the Czech market). Taycan is now also sold in the Czech Republic (market data), so unit splits changed slightly.
- SQLite migration 004: `campaign_spend`, `model_capacity`, `financing_terms`; the seed fills them (capacity is planned production times a factor, 1.0 for ID.4 and Taycan).
- `services/sales.rs`: Q15 lead pressure (margin within a buffer of the floor, or fell 0.3 points over the look-back window), Q16 campaign spend wasted on models without headroom, Q17 lease versus loan profit per contract.
- API: `/api/sales/{leads,campaigns,financing}`. UI: "Sales and leads" page (built by a subagent against a fixed contract, then checked live). Chat: 3 new tools (19 in total) and suggestions.
- 57 Rust tests; clippy clean.
- Headline results (illustrative): both high-intent leads (Customer 001, Enyaq and ID.4) sit on models near the 15% floor and down about 1.1 points in 6 months; EUR 35.5M of EUR 83M campaign spend goes to extra cars that cannot be built (ID.4, Taycan fully, Enyaq partly); the Taycan 60-month loan earns EUR 5.7K per contract, EUR 840 more than the best lease.
- Assumptions: loan interest is charged on 55% of the principal on average; lease finance charge is on the average of price and residual; vehicle margin is the same either way; rates, capacities and campaign effects are made up.

### Step 7: Combined scenario, "bad year" (Q18)

- `services/combined.rs`: commodity and FX shocks plus the end of EV subsidies in one run. Profit change per model is minus (extra material cost on cars still sold plus contribution lost on cars no longer sold), so the two effects are not double counted. Output: totals, profit change per brand (worst first), top 5 models, up to 10 contracts to renegotiate first (ranked by extra cost, with months to end).
- API: `POST /api/scenarios/combined` (saves when `save_as` is set), `GET /api/scenarios/combined` (saved runs with totals, for side-by-side comparison), `GET /api/scenarios/combined/{id}`. Migration 005 adds `scenario_runs.kind` so single-shock and combined runs stay in separate lists.
- UI: "Bad year" page (built by a subagent against a fixed contract, checked live) with By brand, Top 5 models, Contracts and Saved runs tabs. Chat: tool `combined_scenario` (20 in total) and a planner rule for "bad year" and for subsidy questions that also name a commodity or currency move.
- 61 Rust tests; clippy clean.
- Headline result (illustrative): rubber +15%, krona +5% and no German subsidy cost about EUR 488M a year, 2.6% of contribution (EUR 323M extra material cost, EUR 165M lost demand on about 21,400 cars). Scania and MAN take the largest brand hit (about EUR 143M and 107M, all material cost); Enyaq, ID.4 and e-tron are hit mostly by lost demand; the heavy-duty engine contract is first to renegotiate.
- Assumptions: subsidy elasticity is an input (default 1.5); shocks apply to the latest month's prices; saved runs store the request and result, not the data they ran on.

### Step 8: Numeric facts moved out of the TTL

- Model price, planned production and last-year production, and the contract unit price, committed volume, discount price and discount tier are no longer in the TTL files (165 triples removed from `VWGroup_Abox.ttl` and `VW_Contracts_Abox.ttl`). They live in SQLite (migration 006: `model_facts`, `contract_terms`, filled from the old values).
- At load, `facts.rs` writes them into a named graph `graph/erp-facts` before the rules run, so the SPARQL rules and every service see the same triples as before. `Kg::load` now takes the database connection.
- `projectedAnnualComponentVolume` is no longer stored: it is computed (sum of planned production of the models using the component). The computed values equal the old hand-written ones for all 24 contracts.
- Verified: the 165 facts, the total triple count (3,832) and the headline results (revenue, rubber +15%, 16 of 24 contracts flagged) are identical to before. 64 Rust tests, including one that changes a price in SQLite and checks the graph and the budget rule follow.
- Contract dates, powertrains, seats and other classifications stay in the TTL.
- Consequence: loading the TTL files alone (for example into dev GraphDB) no longer gives prices or volumes. Use the app (`vw-erp query`) or add an export of the facts graph if GraphDB browsing needs them.

### Chat bubble

- "Ask the data" bubble on every page: answers with KPI tiles, tables, bar charts and links, keeps its history across pages and reloads, and its links can open the Scenario lab with the shocks filled in.
- Backend: 11 tools (the question services plus `list_entities`) with JSON schemas, and a rule-based planner that works without any API key. Endpoints: `POST /api/chat`, `GET /api/chat/status`, `GET /api/chat/tools`, `POST /api/chat/tools/{name}`.
- No language model is connected yet. The tools are already in the Anthropic tool format, so adding one means replacing the planner (steps in plan.md section 2.8).
- 43 Rust tests pass in total.

### Findings

- Oxigraph queries see only the default graph unless told otherwise. With one named graph per file, every query returned 0 rows until the adapter set the default graph to the union of all graphs.
- Oxigraph is built without RocksDB (in-memory only), which keeps the build fast and the binary small.
- `@tanstack/react-table` v9 changed its `ColumnDef` types, so the project is pinned to v8.
- The A-box has no `vw:producedAt` facts, so portfolio rule 2 would derive nothing. An illustrative default rule assumes a model is produced at its brand's site. Remove it when real facts exist.
- `GROUP_CONCAT` over IRIs silently returns nothing; use `STR()`.
- Contract 19 is flagged only for a reached discount tier, which the old volume-only check missed.
- Calibration is by design: overhead per model is solved so each model starts at a chosen margin, and index paths are hand-shaped. Scenario results show the machinery working, not facts about a real company.
- Component cost is only 20 to 40% of model price, so commodity shocks move margins by tenths of a point.
- Q9 "revenue delayed" is an upper bound (annual revenue x delay / 365). Q5 assumes the tier price applies to all units.
- Payment terms are in the data but unused; penalty and renewal clauses are not modelled; the Budget vs actual page is not built.

## Not verified

- **Docker files have not been built or run.** Docker is not installed on the development machine. Test `docker build .` and both compose files on the VPS or another machine before relying on them.
- The Caddy basic-auth setup and `.env` handling are untested.

## Next

1. **Connect a language model to the chat** (needs an API key and an HTTP client crate; plan.md section 2.8 lists the steps).
2. **Step 9**: deploy to the VPS (needs Docker verification first).
3. Optional: load the same TTL and rules into dev GraphDB with a no-inference ruleset and compare triple counts with the app.
4. Build the Budget vs actual page (budget and actuals tables are already seeded).
5. Add a pytest or similar HTTP-level suite against `/api`, and test the Docker setup on a machine with Docker.
6. Re-run `vw-erp bench` after each data step, because the KG keeps growing (3,500 triples now).

## Open decisions

- VPS size (RAM). Assumed 512 MB to 1 GB.
- Move TTL files to `ontology/`, or leave them at the root. They stay at the root for now (shared with the GitHub collaborator).
- Domain name for the VPS and TLS.
- Backup method for `erp.db`: cron copy or Litestream.
- Commit plan: the 46 untracked files could be split into commits for the Rust app, the web app and the deployment files. Nothing is staged; commits need explicit confirmation.

## How to run (development)

```bash
make help                       # shortcuts: make dev, make prod, make prod-docker, make test

# API on :8000 (loads the TTL files from the repo root; seeds SQLite on first start)
cd app && cargo run -- serve

# Re-seed the finance data, or explore the graph from the command line
cd app && cargo run -- seed
cd app && cargo run -- query 'SELECT ?m WHERE { ?m a vw:ElectricModel }'

# Regenerate the supply A-box (only if you change the tables in the script)
python3 tools/gen_supply_abox.py

# UI on :5173, proxies /api to :8000
cd web && npm install && npm run dev

# Tests and benchmark
cd app && cargo test            # 64 tests, run from app/ (reads ../rules and the root TTL files)
cd app && cargo run --release -- bench

# Optional: GraphDB Workbench on http://localhost:7200
docker compose up -d graphdb
```
