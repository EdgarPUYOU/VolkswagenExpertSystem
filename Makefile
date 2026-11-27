# Dev and prod entry points. Run `make help` for the list.
.DEFAULT_GOAL := help
.PHONY: help env dev dev-api dev-web install web-build build prod prod-docker prod-docker-down graphdb seed test lint bench clean

BIND ?= 0.0.0.0:8000
BIN  := app/target/release/vw-erp

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-18s %s\n", $$1, $$2}'

env: ## Create .env from .env.example if it does not exist (then edit it)
	@test -f .env && echo '.env already exists' || (cp .env.example .env && echo 'created .env, fill in the values')

install: ## Install web dependencies
	cd web && npm install

dev: ## Dev: API on :8000 and Vite UI on :5173 together (Ctrl-C stops both)
	$(MAKE) -j2 dev-api dev-web

dev-api: ## Dev: API only (loads ontology/*.ttl, seeds SQLite on first start)
	cd app && cargo run -- serve --bind $(BIND)

dev-web: ## Dev: Vite UI only, proxies /api to :8000
	cd web && npm run dev

web-build: ## Build the web bundle (embedded into the binary)
	cd web && npm run build

build: web-build ## Release binary with the web bundle embedded
	cd app && cargo build --release

prod: build ## Prod: build then run the single binary (put Caddy in front for TLS and auth)
	cd app && ../$(BIN) serve --bind $(BIND)

prod-docker: ## Prod: Docker Compose with Caddy and the app (needs .env, see .env.example)
	docker compose -f docker-compose.prod.yml up -d --build

prod-docker-down: ## Stop the prod Docker Compose stack
	docker compose -f docker-compose.prod.yml down

graphdb: ## Dev: GraphDB Workbench on :7200 for browsing the KG
	docker compose up -d graphdb

seed: ## Regenerate the synthetic finance and market data in SQLite
	cd app && cargo run -- seed

test: ## Rust tests and web type check
	cd app && cargo test
	cd web && npx tsc -b

lint: ## Clippy
	cd app && cargo clippy --all-targets

bench: ## Benchmark the embedded store
	cd app && cargo run --release -- bench

clean: ## Remove build output
	cd app && cargo clean
	rm -rf web/dist
