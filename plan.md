# Plan: Volkswagen mini-ERP + Knowledge Graph

Status: planning. Nothing below is implemented yet. All data is synthetic and illustrative.

## 1. Goal

Build a mini ERP for a car company that combines two stores to answer business and finance questions:

- **Knowledge graph (Oxigraph in production, GraphDB in development):** structure and meaning. Who supplies what, which contract covers which component, which models use it, which brand and profit center own the model, which clauses link a contract to a commodity index.
- **SQLite:** numbers and transactions. Index values over time, FX rates, budgets and actuals, sales orders, price lists, volumes.

The two are joined by IRI. Every ERP row that refers to a KG entity stores `kg_iri` (for example `http://example.org/volkswagen/Golf`).

Rule of thumb: if a value changes often or is a transaction, it goes in SQLite. If it is a relationship or a classification, it goes in the KG.

Target audience for the questions: finance, procurement, business strategy and sales. Not manufacturing or operations. The current TTL is a business model (portfolio, pricing, customers, supplier contracts), so there is no bill-of-materials or raw-material layer.

## 2. Architecture

Decisions taken:

- **Backend:** Rust (`axum`) JSON API. One binary, with the built web UI embedded in it.
- **UI:** React + TypeScript SPA using shadcn/ui, built in CI (Node is needed only at build time).
- **KG in production:** Oxigraph embedded in the app process. No separate KG server.
- **KG in development:** GraphDB, for visual browsing only (see 2.2).
- **ERP store:** SQLite (WAL mode), file on a volume.
- **Hosting:** tiny VPS, Docker Compose, Caddy in front for HTTPS and auth.

```
Internet -> Caddy (:443, TLS, basic auth) -> app (:8000, one container)
                                              |- SQLite   data/erp.db   (WAL)
                                              \- Oxigraph (in memory), rebuilt from the *.ttl files at startup
```

Query flow for a typical question ("rubber +15%"):

1. SPARQL walks index -> indexation clause -> contract -> component -> model -> brand -> profit center and returns IRIs plus the explanation path.
2. SQL computes the euro impact for those IRIs.
3. The service merges, ranks and returns results with the path that explains each item.

### 2.1 Why Oxigraph instead of GraphDB in production

- GraphDB is a JVM service. With a 2 GB heap it needs roughly 3 GB of RAM, which does not fit a tiny VPS.
- Oxigraph is a Rust triple store with SPARQL 1.1 and Turtle loading, usable as an embedded library. Measured in the step 1 spike (section 12): 2,263 triples load in about 15 ms and the whole process uses under 9 MB of RSS.
- Same TTL files and same SPARQL in both stores.
- Cost: no OWL2-RL reasoning. Everything the reasoner would have done is replaced by explicit rules (section 3).
- Fallback if embedding causes problems: run Oxigraph in server mode as a second container. The adapter boundary (`kg.rs`) hides this from the rest of the app.

### 2.2 Development versus production

| | Development | Production |
| --- | --- | --- |
| KG | GraphDB (Workbench for browsing) plus embedded Oxigraph in the app | Embedded Oxigraph only |
| Reasoning in GraphDB | Ruleset **empty or rdfs only**, never `owl2-rl`, so GraphDB shows the same triples the app produces | n/a |
| Entry point | `localhost:5173` (Vite UI, proxies `/api`), `localhost:8000` (API), `localhost:7200` (GraphDB) | Caddy on 443 |

The materialisation rules (section 3) are applied to the data before it is loaded into either store, so both see identical triples.

### 2.3 Application layer

**Backend (Rust):** `axum` (HTTP), `rusqlite` (SQLite), `oxigraph` (embedded KG), `serde` (JSON), `tracing` (logs), `rust-embed` (serves the built web bundle from inside the binary). Optional: `utoipa` to generate an OpenAPI spec, and `openapi-typescript` to generate the frontend's API types from it, so the two sides cannot drift.

**Frontend (`web/`):** React + TypeScript + Vite, Tailwind, **shadcn/ui** components, TanStack Table (data tables) and TanStack Query (API calls and caching), Recharts through shadcn's chart components, Cytoscape.js for the explain graph, React Router. Exact versions to be checked at project start.

Backend layers:

```
routers    /api/*  (JSON only)      plus static bundle and SPA fallback to index.html
    |
services   one function per target question, plus scenario runner
    |
adapters   kg.rs (SPARQL)           db.rs (SQLite)
```

- Routers contain no business logic. Each of the 19 questions is one service function returning a typed result: rows, totals and the explanation path.
- The API is the only interface to the backend. The UI, the pytest suite and any future client (for example a natural-language box) all use it.
- The web bundle is built in CI and embedded in the binary, so production is still one binary with no Node at runtime.
- Development: `cargo run -- serve` for the API and `npm run dev` (Vite, port 5173) for the UI, with Vite proxying `/api` to the app.
- SPARQL result cache in memory, invalidated when the KG is reloaded. Path queries do not change between runs; only the SQL arithmetic does.
- Scenario runs are saved in a `scenario_runs` table (inputs, outputs, timestamp) so runs can be compared and replayed.
- SQLite schema changes are numbered SQL files (`001_init.sql`, `002_finance.sql`), applied at startup.
- The app has subcommands: `serve`, `load-kg` (rebuild Oxigraph from `ontology/`), `migrate`.

Scope: read-only analysis in phase 1. Editing contracts or orders would need a write rule that keeps SQLite and the KG in sync, so it is deferred to phase 2.

Not in phase 1: natural-language question box. If added later, an LLM would only choose one of the 19 question services and fill in parameters (tool calls). It would not write free SPARQL.

Tooling: the synthetic finance data is generated by the app itself (`vw-erp seed`, deterministic, run automatically on the first start when the tables are empty). Python is used only by `tools/gen_supply_abox.py`, which regenerates `VW_Supply_Abox.ttl` and applies the one-time contract patch (currency and end dates).

### 2.4 Deployment (tiny VPS)

- Production compose has two services: `caddy` and `app`. Port 8000 is not published; only Caddy exposes 443.
- Caddy provides automatic HTTPS and basic auth. The site is public, so auth is required even though the data is synthetic.
- The KG is held in memory and rebuilt from the TTL files at every start (about 15 ms today), so only `data/erp.db` needs backing up (cron copy of the file, or Litestream). If the KG ever outgrows memory, Oxigraph's RocksDB storage is a feature flag away.
- Image: multi-stage build. A Node stage builds the `web/` bundle, a Rust stage builds the binary with the bundle embedded, and a small runtime image holds only the binary. Built in CI, pushed to a registry, deployed with `docker compose pull && docker compose up -d` over SSH. No Kubernetes.
- `/health` endpoint checks SQLite and the KG.
- Logs: structured JSON to stdout; each question run logs its SPARQL time and SQL time.

### 2.5 Compose files

**Development (`docker-compose.yml`)**

- `graphdb`: `ontotext/graphdb:11.5.2`, port 7200 bound to localhost, home volume `./infra/graphdb-home`, TTL mounted read-only at `/root/graphdb-import`, heap 2g, healthcheck on `/rest/repositories`. Browsing only; the app does not depend on it.
- `app`: built from `./app`, mounts `./ontology` and `./data`, port 8000.
- `sqlite-web`: optional, behind the `tools` profile.

**Production (`docker-compose.prod.yml`)**

- `caddy` and `app` only, as in 2.4.

Open check: confirm the GraphDB image has `curl` for the healthcheck. If not, drop the healthcheck (the app does not depend on it).

### 2.6 Repo layout (proposed)

```
ontology/   *.ttl (not moved yet: they stay at the repo root), new modules
rules/      *.ru SPARQL update rule files (generic, portfolio, contracts, sales)
app/        Cargo.toml, src/{main,config,routers,services,kg,db}.rs, migrations/*.sql, Dockerfile
web/        React + Vite + shadcn/ui source (package.json, src/, components.json)
tools/      gen_supply_abox.py (generates VW_Supply_Abox.ttl)
tests/      pytest, one test per target question (hits /api)
data/       erp.db, kg/ (gitignored)
infra/      graphdb-home (gitignored), Caddyfile
docker-compose.yml
docker-compose.prod.yml
plan.md
```

Open decision: moving the TTL files into `ontology/` touches files shared with the GitHub collaborator. The alternative is to leave them at the root and mount them file by file.

### 2.7 UI modules

Built with shadcn/ui (Card, Table, Tabs, Dialog, Sheet, Select, Slider, Command palette, Chart).

1. **Executive dashboard:** KPI tiles (planned revenue, contribution margin, supply spend, share of spend that is commodity-indexed, contracts under review), margin by brand, commodity index trends, alerts list.
2. **Scenario lab:** pick a shock (commodity, FX, regulation, competitor price, supplier lead time), set its size, run. Shows profit impact by brand, top models hit, contracts to renegotiate first. Runs can be saved and compared side by side. Q1, Q2, Q18.
3. **Explain view:** the path from a shock to an item, for example Golf, TyreSet_Standard, Contract_22, rubber index at 40% weight. Q19.
4. **Contracts:** list with filters (expiring, under-committed, indexed, single-sourced). Detail page shows clauses, discount tiers and potential saving. Q5, Q8.
5. **Suppliers:** risk rating, country and currency, dependency (which models and how much revenue), alternative suppliers. Q3, Q6, Q7, Q9.
6. **Portfolio and models:** brand tree; model detail with a margin waterfall (price, component cost, overhead, margin). Q13, Q14.
7. **Commodities and FX:** index and rate charts, and an exposure map of which contracts and profit centers follow each index. Q4.
8. **Markets and regulation:** subsidies, CO2 fleet position by brand, tariffs, competitor price comparison. Q10 to Q12.
9. **Sales and leads:** high-intent leads and the models they match, campaigns versus capacity, lease versus loan comparison. Q15 to Q17.
10. **Budget vs actual:** by profit center and cost element.
11. **Question library:** the 19 questions as runnable cards with parameters. They map one to one to the pytest suite, so this doubles as the demo menu.
12. **Admin:** KG load status, rule-run log, data freshness, health.

Build priority follows the build order in section 9: modules 1, 3, 4, 5 and the first part of 2 for the first demo.

### 2.8 Chat bubble and the language-model seam

A floating "Ask the data" bubble is available on every page. It answers questions by calling the same analyses as the pages and draws the result in the conversation: KPI tiles, tables, bar charts and links to the matching page. The conversation survives page changes and reloads (browser session storage).

Backend (`services/chat.rs`):

- **Tools.** 16 tools wrap the question services (added: `subsidy_removal`, `co2_fleet`, `competitor_price_cut`, `site_dependency`, `powertrain_mix`): `run_scenario`, `currency_exposure`, `commodity_sensitivity`, `discount_savings`, `supplier_risk`, `single_sourced_components`, `expiring_contracts`, `lead_time_impact`, `margin_trace`, `dashboard`, `list_entities`. Each has a name, description and JSON schema, and returns a `ToolOutput`: a one-paragraph `summary` (what a language model reads back), UI `blocks`, and the full `data`. Tools never accept SPARQL or SQL.
- **Planner.** Today a rule-based planner maps a sentence to one tool call. It understands commodity and currency names, "up/down/rise/fall" with a percentage ("steel up 10% and aluminium down 5%" gets a size each), month counts, "double/triple" for lead times, model names (including "ID.4", "e-tron"), and supplier keywords. A move without a size assumes 10% and says so. Anything else gets the help text with example questions.
- **Endpoints.** `POST /api/chat` (message in, `Reply` out), `GET /api/chat/status`, `GET /api/chat/tools` (definitions in the Anthropic tool format: `name`, `description`, `input_schema`), `POST /api/chat/tools/{name}` (run one tool with JSON arguments).

Connecting a language model later only replaces the planner; the tools, the endpoints and the UI stay as they are:

1. Read the key from the environment (for example `ANTHROPIC_API_KEY`); keep it on the server and never send it to the browser. Add an HTTP client crate for the call.
2. Send the user's message (and the history the request already carries) with `chat::tools()` as the tool list and a system prompt that says: answer only from tool results, state that the data is synthetic, and use `list_entities` to resolve names.
3. On a tool-use response, run `chat::call_tool` in-process and return the `summary` plus a trimmed `data` as the tool result. Keep the returned `blocks` for the UI. Repeat until the model gives a final answer.
4. Return a `Reply` whose `text` is the model's answer, `blocks` are the collected blocks, `tool_calls` lists what ran, and `planner` is `"llm"`; set `llm: true` in `/api/chat/status` (the bubble then labels itself "Language model with tools").
5. Keep the rule-based planner as the fallback when no key is set or the call fails.

## 3. Reasoning without a reasoner

Neither GraphDB-in-dev nor Oxigraph-in-prod runs SWRL or OWL2-RL in this setup. The 15 existing SWRL rules are only stored as triples today and produce nothing. Replacement:

1. **Materialisation rules** (`rules/*.ru`, SPARQL `INSERT ... WHERE`, written into the named graph `.../graph/inferred`). The app applies all rule files in name order, repeatedly, until a full pass adds nothing (a fixpoint), so rules can build on each other regardless of file order:
   - `owl:inverseOf` pairs: `hasModel` / `modelOf`, `hasBrand` / `belongsToGroup`, `contractCoversComponent` / `componentHasSupplyContract`.
   - `rdfs:subPropertyOf`: `hasEngineModel` and `hasSeatModel` to `hasComponent`, `hasSeatManufacturer` to `componentSupplier`, `seatProducedAt` to `componentProducedAt`.
   - `rdfs:subClassOf` and `rdf:type` propagation (for example every `TyreSet` is a `Component`). Queries may also use `rdfs:subClassOf*` property paths instead.
   - Definitions that are `owl:equivalentClass` restrictions: `ElectricModel`, `HybridModel`, `CombustionModel`, `BrandWithElectricModels`, `BrandWithModels`, `ModelsWithProductionSite`.
2. **Ported SWRL rules**: the 3 portfolio rules, the 10 sales rules, and the 2 contract rules become SPARQL rules where they only need graph patterns.
3. **Business inference** (section 4.5): `CommodityExposedContract`, `SingleSourcedCriticalComponent`, `SubsidyDependentModel`, and so on, also as SPARQL rules.
4. **Arithmetic rules** (projected volume versus commitment, discount-tier saving, margin) run in the Rust services against SQLite.

Rule files live in git and are covered by the question tests, so a rule change that breaks an answer is caught.

## 4. Ontology enrichment (business side)

What the TTL already supports: model price, planned production, brand and group hierarchy, supply contracts with unit price, commitment and discount tiers, customer segments and purchase intent.

### 4.1 Finance (`VW_Finance_Tbox/Abox`)

- Classes: `Currency`, `ProfitCenter` (one per brand), `CostCenter`, `CostElement` (DirectMaterial, Logistics, Warranty, Marketing, Labour), `CommodityIndex` (NaturalRubber, Steel, Aluminium, Lithium, Copper, EnergyEU), `FXPair`.
- Properties: `brandProfitCenter`, `contractCurrency` (change from a string to an object property to a `Currency`), `costElementOf`, `indexCurrency`.
- SQLite: `index_values(index_iri, date, value)`, `fx_rates`, `budget(profit_center, period, cost_element, amount)`, `actuals`.

### 4.2 Contract terms (extends Contracts)

- Classes: `IndexationClause`, `PaymentTerm` (Net30, Net60), `PenaltyClause` (shortfall under committed volume), `RenewalOption`.
- Properties: `hasIndexationClause`, `indexedTo`, `indexWeight` (for example 0.40 of the tyre price follows rubber), `passThroughRatio`, `hasPaymentTerm`, `alternativeSupplier`, `sourcingType` (Single / Dual / Multi).

### 4.3 Market and regulation (`VW_Market_Tbox/Abox`)

- Classes: `Market` (country with a currency; `MarketRegion` already exists), `Regulation` with subclasses `CO2FleetTarget`, `ImportTariff`, `EVSubsidy`; `Competitor`, `CompetitorModel`.
- Properties: `soldInMarket`, `co2GramsPerKm` (on Model), `appliesToPowertrain`, `appliesInMarket`, `competesWith`, `priceSegment`, `launchDate`.

### 4.4 Sales and revenue (extends Sales)

- Classes: `SalesChannel` (Dealer, Fleet, Online), `Dealer`, `FinancingProduct` (Lease, Loan), `IncentiveCampaign`, `PriceList`.
- Properties: `campaignAppliesTo`, `offeredVia`, `financingFor`, `leadChannel`.
- SQLite: `sales_orders`, `price_list_history`, `campaign_spend`, `residual_values`.

### 4.5 Risk and scenario (`VW_Risk_Tbox/Abox`)

- Classes: `RiskFactor` (CommodityRisk, FXRisk, RegulatoryRisk, SupplierRisk, DemandRisk), `Scenario`.
- Properties: `scenarioShocks`, `shockPercent`, `exposedTo` (inferred), `supplierRiskRating`, `supplierCountryRisk`.
- Inferred classes (SPARQL rules): `CommodityExposedContract`, `CommodityExposedModel`, `CommodityExposedProfitCenter`, `FXExposedContract`, `SingleSourcedCriticalComponent`, `SubsidyDependentModel`, `CO2PenaltyRiskModel`.

### 4.6 SQLite schema (first pass)

`materials` and BOM tables are dropped. Core tables: `commodity_indices`, `index_values`, `fx_rates`, `profit_centers`, `models`, `production_plan`, `suppliers`, `components`, `supply_contracts` (with computed projected volume), `budget`, `actuals`, `sales_orders`, `price_list_history`, `campaign_spend`, `residual_values`, `financing_rates`, `model_overhead_cost`. Later: `purchase_orders`, `inventory`.

## 5. Target questions

Marked "today" are answerable with the current TTL. The rest need the new modules.

### Finance

1. Rubber index +15% next quarter: euro cost by brand? (KG: index -> clause -> contract -> models -> profit center. SQL: price, volume, pass-through, budget.)
2. Which models drop below 15% contribution margin if steel +10% and aluminium +8% happen together?
3. What share of supply spend is exposed to SEK or CZK?
4. Which profit center is most sensitive to commodity prices?
5. Today: which contracts reach a discount tier under the planned production, and what is the saving?

### Procurement

6. Today: which suppliers hurt most if they fail (supplier -> contracts -> models, plus revenue at risk)?
7. Which components are single-sourced with no alternative, and how much revenue depends on them?
8. Which contracts expire within 12 months and also have committed volume below projected volume?
9. If Battery Systems lead time doubles, which EV launches or lead deliveries slip?

### Strategy

10. Germany ends its EV subsidy: which models and hot leads are affected?
11. EU tightens the CO2 fleet target: which brand breaches, and what is the penalty?
12. A competitor cuts its EV price by 10%: which of our models and how much revenue are exposed?
13. Today: which brand group depends most on one production site?
14. Should the production mix shift toward EVs (margin per powertrain, subsidy and CO2 status)?

### Sales

15. Today: which high-intent leads match models whose margin is under pressure?
16. Which incentive campaign spends on models already at planned capacity?
17. Is lease or loan more profitable for Taycan buyers in the Prague market?

### Scenario combinations

18. "Bad year": rubber +15%, SEK +5%, EV subsidy removed in Germany. Profit change per brand, top 5 models hit, contracts to renegotiate first.
19. Trace why the Golf margin dropped (Golf -> TyreSet_Standard -> Contract_22 -> indexed to rubber at 40% weight).

First demo set: 1, 6, 10, 19. It shows commodity, supplier, regulation and explainability, and needs only Finance, Contract terms and a small Market module.

## 6. Data needed

**KG additions (about 700 to 900 triples):**

- 4 currencies, 7 commodity indices, 7 profit centers, 5 markets.
- About 16 indexation clauses across the 24 existing contracts. The other 8 get none, so "exposed" is not true for everything.
- 6 to 8 alternative suppliers. Some components stay single-sourced on purpose (Q7).
- Regulations: EU CO2 fleet target, Germany EV subsidy, a US tariff.
- `co2GramsPerKm` and `launchDate` on all 15 models.
- About 10 competitor models mapped by `priceSegment`.
- 6 campaigns, 4 financing products, 4 scenarios, supplier and country risk ratings.

**SQLite (about 4,000 rows):**

- Monthly index values for 7 indices over 36 months; FX rates for 3 pairs over 36 months.
- Budget and actuals: 7 profit centers x 4 quarters x 5 cost elements.
- Monthly sales orders per model and market for 24 months.
- Price list history, campaign spend, residual values, financing rates.
- Overhead and labour cost per model. Without these, margin is price minus component cost only (about 70%), which is unrealistic.

**Changes to existing data:**

- All contracts are priced in EUR today, so Q3 would return nothing. Re-denominate the Swedish and Czech supplier contracts.
- Contract end dates need to vary across 2026 to 2029, otherwise Q8 returns all or none.
- `projectedAnnualComponentVolume` becomes computed in SQLite. The hand-written values drift whenever production plans change.

**Calibration:** tune the numbers so headline scenarios give non-trivial answers (for example, steel +10% and aluminium +8% push a few models under 15% margin, not all or none). This is demo design and must be stated in the README so outputs are not read as real findings.

## 7. Fixes to existing files

- `VWGroup_Tbox.ttl` lines 210, 223, 236 declare `vw:ElectricModel a owl:equivalentClass` (and the Hybrid and Combustion equivalents). Invalid typing. Should be `a owl:Class ; owl:equivalentClass [...]`.
- `ComponentCategory` individuals duplicate the component subclasses (`TyreSet` vs `TyreSetCategory`). Keep one.
- TTL comments and the README refer to old file names (`VWGroup2.ttl`, `VW_Contracts_data.ttl`, `VW_Sales_data.ttl`).

## 8. Sufficiency check

A test suite with at least one test per question. Implemented as Rust tests on the service layer (`cargo test`, 29 tests): each builds the KG, applies the rules, seeds an in-memory SQLite database and asserts the headline result of the question. A failing test means the data is missing something. HTTP-level checks (routes, status codes) were done by hand with curl; a pytest suite that calls `/api` is still open.

## 9. Build order

1. **Oxigraph spike (decision gate).** Load all current TTL into Oxigraph, run a few representative SPARQL queries, measure RAM and query time. If the numbers are bad, fall back to Oxigraph server mode or revisit the store.
2. **Fix the TTL typing errors (section 7) and write the materialisation rules** (section 3, item 1). Port the SWRL rules to SPARQL. Check the results in dev GraphDB (no-inference ruleset) and in Oxigraph. **Done in Oxigraph (section 12); the GraphDB comparison is still open.**
3. **Rust and web skeleton.** `axum` app with config, `/health`, `load-kg` and `migrate` subcommands, SQLite and Oxigraph adapters. `web/` app with shadcn/ui set up, an app shell (sidebar, routing) and one page calling `/api`. Multi-stage Dockerfile, dev and prod compose files, Caddyfile.
4. **Finance and contract terms** (ABox/TBox, SQLite migrations, seed) plus services and pages for Q1 to Q9 and Q19. **Done (section 12).**
5. **Market, regulation and competitors.** Covers Q10 to Q14. **Done (section 12).**
6. **Sales, campaigns and financing.** Covers Q15 to Q17. **Done (section 12).**
7. **Scenarios.** Covers Q18 and the saved scenario runs. **Done (section 12).**
8. **Move numeric facts** (price, planned production, contract volumes and prices) out of the TTL into SQLite.
9. **Deploy to the VPS** (can happen earlier, after step 4, to demo online).

Each data step ships its seed data and the matching tests.

## 10. Open decisions

Decided:

- Rust (`axum`) JSON API, Oxigraph embedded in production, GraphDB for development browsing only.
- UI is a React + shadcn/ui single-page app embedded in the Rust binary (chosen over HTMX because the UI is dashboard-heavy with interactive scenarios, charts and tables).
- Hosted on a tiny VPS behind Caddy with basic auth.
- Phase 1 is read-only analysis. No natural-language box in phase 1.

Still open:

- VPS size (RAM). Assumed 512 MB to 1 GB. Confirm before the benchmark in step 1.
- Move TTL files to `ontology/` or leave at the root (shared with the GitHub collaborator). For now they stay at the root: the app reads them through `ONTOLOGY_DIR` (default `..`) and the Docker image copies `*.ttl` from the root. Moving them later only changes that setting and one `COPY` line.
- Domain name and TLS (Caddy can handle it, needs a domain pointing to the VPS).
- Backup method for `erp.db`: cron copy or Litestream.
- Whether GraphDB in dev is worth keeping once the app has an Explain view.
- Work on a new branch. Commits only after explicit confirmation of staged files and message.

## 11. Constraints and caveats

- All suppliers, prices, contract terms, customers and scenario results are synthetic. Label them as such.
- No reasoner runs in production. Inferred triples exist only because the rule files in `rules/` produce them.
- Oxigraph memory and latency figures come from a 2,263-triple graph. Re-run `vw-erp bench` after each data step, because the KG will grow with the new modules.
- The Dockerfile and both compose files have been written but not run: Docker is not installed on the development machine. Test them on the VPS or a machine with Docker before relying on them.
- GraphDB in dev must not use `owl2-rl`, otherwise it shows triples that the app does not have.
- The site is public once deployed. Keep basic auth on, and do not expose the app port or any KG endpoint directly.
- The existing `projectedAnnualComponentVolume` values are precomputed by hand and may be stale until computed in SQLite.

## 12. Status

Branch `feat/architecture-skeleton`. Sections 1 and 2, step 2 (rules) and step 4 (finance, first demo) are implemented; nothing is committed yet.

Done:

- **Step 1, Oxigraph spike.** `vw-erp bench` (release build, 9 TTL files, 2,263 triples): load 14.6 ms; RSS 5.7 MB before load, 7.5 MB after load, 8.7 MB after 200 runs of each query. Query mean latency: models (15 rows) 0.3 ms, contracts (24 rows) 0.5 ms, contract-supplier-model path (94 rows) 0.4 ms, `rdfs:subClassOf*` walk 0.05 ms. The binary is 12.3 MB including the web bundle. Running `serve` used about 11 MB of RSS. Decision: Oxigraph embedded is viable; no fallback needed.
- **Step 3, skeleton (except Docker verification).**
  - `app/`: `axum` server with subcommands `serve`, `load-kg`, `migrate`, `healthcheck`, `bench`. Config through flags or environment variables. KG adapter (each TTL file in its own named graph, queries run over the union). SQLite adapter (WAL, numbered embedded migrations, first table `scenario_runs`). Thin routers and one service module (`portfolio`). Endpoints: `/health`, `/api/kg/graphs`, `/api/models`, `/api/contracts`. The built web bundle is embedded with SPA fallback, and unknown `/api/*` paths return 404. 5 Rust tests pass.
  - `web/`: React 19, Vite, TypeScript, Tailwind, shadcn/ui (Button, Card, Table, Badge, Separator), TanStack Query and Table v8, React Router. App shell with sidebar and four pages: Dashboard, Portfolio, Contracts, Admin. Checked in a browser against the embedded build: renders, no console errors.
  - `Dockerfile` (Node, Rust, distroless), `docker-compose.yml` (dev), `docker-compose.prod.yml` (Caddy and app), `infra/Caddyfile`, `.env.example`, `.dockerignore`, `.gitignore`.

- **Step 2, rules.**
  - TTL fix: `VWGroup_Tbox.ttl` declared `ElectricModel`, `HybridModel` and `CombustionModel` with the invalid pattern `a owl:equivalentClass ; owl:Class [...]`; each is now `owl:equivalentClass [...]`.
  - `rules/00_generic.ru`: `rdfs:subPropertyOf`, `owl:inverseOf`, `rdfs:subClassOf` type propagation, and the six defined classes (`ElectricModel`, `HybridModel`, `CombustionModel`, `BrandWithModels`, `BrandWithElectricModels`, `ModelsWithProductionSite`).
  - `rules/10_portfolio.ru`, `20_contracts.ru`, `30_sales.ru`: all 15 SWRL rules ported (3 portfolio, 2 contracts, 10 sales).
  - Engine: `Kg::infer` applies the `.ru` files until a fixpoint. Result: 3 passes, 554 inferred triples, about 20 ms; total 2,814 triples, about 9.7 MB RSS.
  - Results checked by hand against the data: 4 electric models, 11 combustion, no plug-in hybrids; group model counts 5, 8 and 2; 7 group-to-site links; 25 review reasons on 16 of 24 contracts (16 over commitment, 9 discount tier reached); suitable models for 5 customers; one high-intent lead (Customer_001 for ID4 and Enyaq).
  - 12 Rust tests cover the rules, including that a second inference run derives nothing.
  - The Contracts page now shows the inferred review reasons, and the Dashboard counts flagged contracts from them.

- **Step 4, finance and contract terms (first demo).** Answers Q1 to Q9 and Q19.
  - New TTL modules (5 files, 14 in total now): `VW_Finance_*` (currencies, profit centers per brand, cost elements, 6 commodity indices), `VW_Supply_*` (indexation clauses, sourcing type, alternative suppliers, payment terms, planned launch dates), `VW_Risk_Tbox` (derived exposure classes). `VW_Supply_Abox.ttl` is generated by `tools/gen_supply_abox.py`.
  - Changes to existing contract files: `contractCurrency` is now an object property to `vw:Currency` (was a string); the nine Swedish and Czech contracts are in SEK and CZK; end dates now run from 2027-03-31 to 2029-12-31 instead of all being 2027-12-31.
  - Data added: 35 clauses on 16 of 24 contracts (8 contracts have fixed prices), 6 alternative suppliers, 15 of 24 components single-sourced with no alternative, 4 planned launches (the battery-fed EVs).
  - `rules/40_exposure.ru`: `CommodityExposed{Contract,Model,ProfitCenter}`, `FXExposedContract`, `SingleSourcedCriticalComponent`, `exposedToIndex`, `exposedToCurrency`. The graph now holds 3,500 triples (800 inferred); rules still reach a fixpoint in 3 passes (about 32 ms).
  - SQLite migration `002_finance.sql` and `vw-erp seed`: 35 months of index values and FX rates, overhead per model (solved so each model starts at a chosen margin), 2027 budget and 2026 actuals by profit center and cost element. The seed is deterministic and repeatable.
  - Pricing model (`services/finance.rs`): effective EUR unit price = base price x (1 + sum of weight x pass-through x (index now / index at contract start - 1)) x FX factor for foreign-currency contracts.
  - API: `POST /api/scenarios/run` (Q1, Q2; supports commodity and FX shocks, a margin floor, and saving a run), `GET /api/scenarios` and `/api/scenarios/{id}`, `/api/exposure/currency` (Q3), `/api/exposure/commodity` (Q4), `/api/contracts/savings` (Q5), `/api/suppliers/risk` (Q6), `/api/components/single-sourced` (Q7), `/api/contracts/expiring` (Q8), `/api/suppliers/lead-time` (Q9), `/api/models/{model}/margin-trace` (Q19), plus `/api/dashboard` and `/api/market/series`.
  - UI: Dashboard (live KPIs, margin by profit center, market movers), Scenario lab (presets, multiple shocks, saved runs, results by profit center, model and contract), Explain a margin, Contracts (review, expiring, volume discounts), Suppliers (dependency, single-sourced parts, lead-time shock), Commodities and FX (price charts, commodity sensitivity, currency exposure). Checked in a browser with no console errors.
  - 29 Rust tests pass, clippy is clean.
  - Headline results on the illustrative data: rubber +15% costs about EUR 101M a year (Golf margin falls 0.16 points); steel +10% with aluminium +8% pushes exactly Scania R-series, MAN TGX, ID.4 and Enyaq under a 15% margin; 39% of material spend is in SEK or CZK; 8 contracts expire within 12 months and 5 of those are also under-committed; doubling the battery supplier's lead time moves the ID.4 launch from 2027-03-15 to 2027-07-13 and touches one high-intent lead; Golf's margin fell 0.56 points in 6 months, mostly from natural rubber (+EUR 45 per car) and the krona-linked contracts.

- **Step 5, market and regulation (Q10 to Q14).** `VW_Market_*` TTL, `rules/50_market.ru`, migration 003 (`sales_plan`, `competitor_prices`), `services/market.rs`, `/api/strategy/*`, UI page "Markets and regulation", 7 tests (50 in total). Elasticities are request inputs (defaults 1.5 and 0.8); CO2 target (120 g/km, EUR 95 per g/km) is illustrative. Chat tools added (16 in total). `Makefile` added for dev and prod start.

- **Step 6, sales (Q15 to Q17).** `VW_Campaign_*` TTL, migration 004, `services/sales.rs`, `/api/sales/*`, page "Sales and leads", 3 chat tools (19 in total), 57 tests. Financing is modelled for the Taycan in the Czech market only; all rates are illustrative.

- **Step 7, combined scenario (Q18).** `services/combined.rs`, `/api/scenarios/combined*`, migration 005 (`scenario_runs.kind`), page "Bad year", chat tool `combined_scenario` (20 tools), 61 tests. Saved combined runs can be compared in the Saved runs tab.

- **Chat bubble (section 2.8).** 11 tools, a rule-based planner, `/api/chat*` endpoints and the bubble UI. 14 new Rust tests (43 in total) cover the tool catalogue, every tool with sample arguments, bad input (400), and about 40 phrasings, including mixed rises and falls, currency moves, month counts and model names. Checked in a browser: suggestions, typed questions, charts and tables in the conversation, a link that opens the Scenario lab with the shocks filled in, and history that survives a reload. No language model is connected yet.

Findings during step 4:

- **Calibration is by design.** Overhead per model is solved so that each model starts at a chosen margin, and index paths are hand-shaped (rubber up, lithium down). The scenario results above show the machinery working; they are not findings about a real company.
- Component cost is only 20 to 40% of model price, so commodity shocks move margins by tenths of a point. The seed treats the rest as overhead (labour, logistics, warranty, marketing).
- Q9 "revenue delayed" is an upper bound: annual revenue of the affected models x delay / 365, assuming production stops until the longer lead time is served.
- Q5 assumes a reached tier price applies to every unit of the projected volume.
- Not done from the data plan: payment terms are in the data but not used by any question; the penalty and renewal clauses from plan section 4.2 are not modelled; the Budget vs actual page is not built (the tables are seeded).
- Tests are Rust service tests; the planned pytest suite against `/api` is still open.

Findings during step 2:

- **Assumption added:** the A-box has no `vw:producedAt` facts, so SWRL Rule 2 would derive nothing. `rules/10_portfolio.ru` assumes a model is produced at its brand's production site when none is stated. This is illustrative; remove it when real model-to-site facts exist.
- `GROUP_CONCAT` over IRIs silently returns nothing in SPARQL; wrap the argument in `STR()`. A test now guards this.
- Contract 19 (standard infotainment) is flagged only for reaching a discount tier, although its volume is below the commitment. The old UI check (volume only) showed it as fine; the rules are the source of truth now.
- GraphDB comparison (same TTL and rules, no-inference ruleset) has not been done.

Findings during the spike:

- Oxigraph queries see only the default graph unless told otherwise. Because each file is loaded into its own named graph, the adapter sets the default graph to the union of all graphs. Without this every query returned 0 rows.
- Oxigraph is built without its RocksDB feature (in-memory only), which keeps the build fast and the binary small.
- `@tanstack/react-table` v9 changed its `ColumnDef` types; the project is pinned to v8.

Not done yet (next steps in section 9): materialisation rules and the TTL fixes (step 2), the data modules (steps 4 to 7), Docker verification, deployment to the VPS.

### How to run it (development)

```bash
# API on :8000 (loads the TTL files from the repo root)
cd app && cargo run -- serve

# UI on :5173, proxies /api to :8000
cd web && npm install && npm run dev

# Benchmark the embedded store
cd app && cargo run --release -- bench

# Optional: GraphDB Workbench on http://localhost:7200
docker compose up -d graphdb
```
