//! Hermes Agent adapter, ported from ccusage's hermes adapter.
//!
//! Hermes keeps one cumulative row per session in the `sessions` table
//! of $HERMES_HOME/state.db (default ~/.hermes/state.db). The
//! per-session dedup key plus "more tokens wins" keeps the latest
//! snapshot as the session grows.

use std::path::{Path, PathBuf};

use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "hermes";

fn db_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Ok(raw) = std::env::var("HERMES_HOME") {
        for part in raw.split(',') {
            let part = part.trim();
            if !part.is_empty() {
                paths.push(PathBuf::from(part).join("state.db"));
            }
        }
    } else if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".hermes").join("state.db"));
    }
    paths.retain(|p| p.is_file());
    paths.sort();
    paths.dedup();
    paths
}

pub fn data_dirs() -> Vec<PathBuf> {
    db_paths()
        .into_iter()
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect()
}

pub fn collect_files() -> Vec<PathBuf> {
    db_paths()
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, model, started_at, input_tokens, output_tokens,
                cache_read_tokens, cache_write_tokens, reasoning_tokens,
                estimated_cost_usd, actual_cost_usd
         FROM sessions
         WHERE model IS NOT NULL AND TRIM(model) != ''",
    ) else {
        return Vec::new();
    };

    // Columns may be INTEGER or REAL; read leniently.
    let lenient_u64 = |row: &rusqlite::Row, idx: usize| -> u64 {
        row.get::<_, i64>(idx)
            .ok()
            .filter(|n| *n >= 0)
            .map(|n| n as u64)
            .or_else(|| {
                row.get::<_, f64>(idx)
                    .ok()
                    .filter(|f| f.is_finite() && *f > 0.0)
                    .map(|f| f.trunc() as u64)
            })
            .unwrap_or(0)
    };

    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<f64>>(2)?,
            lenient_u64(row, 3),
            lenient_u64(row, 4),
            lenient_u64(row, 5),
            lenient_u64(row, 6),
            lenient_u64(row, 7),
            row.get::<_, Option<f64>>(8)?,
            row.get::<_, Option<f64>>(9)?,
        ))
    }) else {
        return Vec::new();
    };

    let mut records = Vec::new();
    for row in rows.flatten() {
        let (id, model, started_at, input, output, cache_read, cache_write, reasoning, est, actual) =
            row;
        if id.trim().is_empty() {
            continue;
        }
        // Reasoning bills as output (ccusage behavior).
        let output = output + reasoning;
        let cost = actual
            .filter(|c| c.is_finite() && *c > 0.0)
            .or_else(|| est.filter(|c| c.is_finite() && *c > 0.0));
        if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 && cost.is_none() {
            continue;
        }
        let ts = started_at
            .filter(|t| t.is_finite() && *t > 0.0)
            .map(|t| util::smart_unit_ms(t as i64))
            .unwrap_or_else(|| util::mtime_ms(path));
        records.push(UsageRecord {
            agent: AGENT.to_string(),
            project: "Hermes".to_string(),
            session_id: id.clone(),
            timestamp_ms: ts,
            model: model.trim().to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_creation_5m: cache_write,
            cache_creation_1h: 0,
            cache_read_tokens: cache_read,
            cost_usd: cost,
            dedup_key: Some(format!("hermes:{id}")),
        });
    }
    records
}
