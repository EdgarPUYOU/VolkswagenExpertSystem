-- Planned units per model and market (budget year) and competitor list prices.
CREATE TABLE sales_plan (
    model_iri  TEXT NOT NULL,
    market_iri TEXT NOT NULL,
    units      REAL NOT NULL,
    PRIMARY KEY (model_iri, market_iri)
);

CREATE TABLE competitor_prices (
    competitor_model_iri TEXT NOT NULL PRIMARY KEY,
    price_eur            REAL NOT NULL
);
