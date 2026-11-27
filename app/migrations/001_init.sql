-- Saved scenario runs: inputs and outputs, so runs can be compared and replayed.
CREATE TABLE scenario_runs (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    params_json TEXT NOT NULL,
    result_json TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
