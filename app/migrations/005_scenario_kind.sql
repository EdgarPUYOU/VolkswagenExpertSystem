-- Saved runs now come in two kinds: single-shock scenarios and combined ("bad year") scenarios.
ALTER TABLE scenario_runs ADD COLUMN kind TEXT NOT NULL DEFAULT 'scenario';
