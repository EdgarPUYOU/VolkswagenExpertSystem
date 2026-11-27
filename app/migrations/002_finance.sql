-- Time series and finance numbers. Rows are keyed by the IRI of the knowledge-graph individual.

-- Monthly commodity index values (index = 100 in January 2024).
CREATE TABLE index_values (
    index_iri TEXT NOT NULL,
    month     TEXT NOT NULL,            -- 'YYYY-MM'
    value     REAL NOT NULL,
    PRIMARY KEY (index_iri, month)
);

-- Monthly FX rates as units of the foreign currency per 1 EUR.
CREATE TABLE fx_rates (
    currency_iri TEXT NOT NULL,
    month        TEXT NOT NULL,         -- 'YYYY-MM'
    per_eur      REAL NOT NULL,
    PRIMARY KEY (currency_iri, month)
);

-- Non-material cost per vehicle, by cost element (direct material comes from the contracts).
CREATE TABLE model_cost (
    model_iri        TEXT NOT NULL,
    cost_element_iri TEXT NOT NULL,
    eur_per_unit     REAL NOT NULL,
    PRIMARY KEY (model_iri, cost_element_iri)
);

CREATE TABLE budget (
    profit_center_iri TEXT NOT NULL,
    period            TEXT NOT NULL,    -- 'YYYY-Qn'
    cost_element_iri  TEXT NOT NULL,
    amount_eur        REAL NOT NULL,
    PRIMARY KEY (profit_center_iri, period, cost_element_iri)
);

CREATE TABLE actuals (
    profit_center_iri TEXT NOT NULL,
    period            TEXT NOT NULL,
    cost_element_iri  TEXT NOT NULL,
    amount_eur        REAL NOT NULL,
    PRIMARY KEY (profit_center_iri, period, cost_element_iri)
);
