-- Campaign spend, production capacity and financing terms.
CREATE TABLE campaign_spend (
    campaign_iri               TEXT NOT NULL PRIMARY KEY,
    spend_eur                  REAL NOT NULL,
    expected_incremental_units REAL NOT NULL
);

CREATE TABLE model_capacity (
    model_iri       TEXT NOT NULL PRIMARY KEY,
    annual_capacity REAL NOT NULL
);

-- Percentages are plain percent (6.9 = 6.9%). Loans use down_payment_pct and default_loss_pct;
-- leases use residual_pct and residual_risk_pct (expected loss on the residual value).
CREATE TABLE financing_terms (
    product_iri        TEXT NOT NULL PRIMARY KEY,
    term_months        INTEGER NOT NULL,
    rate_pct           REAL NOT NULL,
    funding_cost_pct   REAL NOT NULL,
    default_loss_pct   REAL NOT NULL,
    residual_pct       REAL NOT NULL DEFAULT 0,
    residual_risk_pct  REAL NOT NULL DEFAULT 0,
    subvention_pct     REAL NOT NULL DEFAULT 0,
    down_payment_pct   REAL NOT NULL DEFAULT 0
);
