//! SQLite adapter. WAL mode, one connection behind a mutex (enough for a mini ERP).
//! Migrations are numbered SQL files embedded in the binary and applied in order.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

const MIGRATIONS: &[(&str, &str)] = &[
    ("001_init", include_str!("../migrations/001_init.sql")),
    ("002_finance", include_str!("../migrations/002_finance.sql")),
    ("003_market", include_str!("../migrations/003_market.sql")),
    ("004_sales", include_str!("../migrations/004_sales.sql")),
    ("005_scenario_kind", include_str!("../migrations/005_scenario_kind.sql")),
];

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(conn)
}

/// Apply pending migrations. Returns how many were applied.
/// `PRAGMA user_version` stores how many migrations have run.
pub fn migrate(conn: &mut Connection) -> Result<usize> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let mut applied = 0;
    for (i, (name, sql)) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql).with_context(|| format!("migration {name}"))?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
        applied += 1;
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_once() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), MIGRATIONS.len());
        assert_eq!(migrate(&mut conn).unwrap(), 0, "second run is a no-op");
        conn.execute("INSERT INTO scenario_runs (name, params_json, result_json) VALUES ('t','{}','{}')", [])
            .unwrap();
    }
}
