//! Goose adapter, ported from ccusage's goose adapter.
//!
//! Sessions live in a SQLite `sessions.db` (per platform data dir, or
//! $GOOSE_PATH_ROOT/data/sessions). Rows hold cumulative per-session
//! totals; the per-session dedup key plus "more tokens wins" keeps the
//! freshest snapshot.

use std::path::{Path, PathBuf};

use crate::adapters::claude::parse_timestamp_ms;
use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "goose";

fn db_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(root) = std::env::var("GOOSE_PATH_ROOT") {
        let root = root.trim();
        if !root.is_empty() {
            paths.push(PathBuf::from(root).join("data/sessions/sessions.db"));
        }
    }
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".local/share/goose/sessions/sessions.db"));
        paths.push(home.join("Library/Application Support/goose/sessions/sessions.db"));
        paths.push(home.join(".local/share/Block/goose/sessions/sessions.db"));
    }
    paths.retain(|p| p.is_file());
    paths.sort();
    paths.dedup();
    paths
}

pub fn data_dirs() -> Vec<PathBuf> {
    db_candidates()
        .into_iter()
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect()
}

pub fn collect_files() -> Vec<PathBuf> {
    db_candidates()
}

/// `created_at` may be an integer (s or ms), RFC3339, or a bare
/// "YYYY-MM-DD[ HH:MM:SS]" string.
fn parse_created_at(raw: &rusqlite::types::Value) -> Option<i64> {
    use rusqlite::types::Value as Sql;
    match raw {
        Sql::Integer(n) if *n > 0 => Some(util::smart_unit_ms(*n)),
        Sql::Real(f) if *f > 0.0 => Some(util::smart_unit_ms(*f as i64)),
        Sql::Text(s) => {
            let s = s.trim().to_string();
            if let Some(ms) = parse_timestamp_ms(&s) {
                return Some(ms);
            }
            if s.len() == 19 {
                let normalized = format!("{}T{}Z", &s[..10], &s[11..]);
                if let Some(ms) = parse_timestamp_ms(&normalized) {
                    return Some(ms);
                }
            }
            if s.len() == 10 {
                return parse_timestamp_ms(&format!("{s}T00:00:00Z"));
            }
            None
        }
        _ => None,
    }
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, model_config_json, provider_name, created_at,
                total_tokens, input_tokens, output_tokens,
                accumulated_total_tokens, accumulated_input_tokens, accumulated_output_tokens
         FROM sessions
         WHERE model_config_json IS NOT NULL AND TRIM(model_config_json) != ''",
    ) else {
        return Vec::new();
    };

    let positive = |v: Option<i64>| v.filter(|n| *n > 0).map(|n| n as u64);
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, rusqlite::types::Value>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<i64>>(7)?,
            row.get::<_, Option<i64>>(8)?,
            row.get::<_, Option<i64>>(9)?,
        ))
    }) else {
        return Vec::new();
    };

    let mut records = Vec::new();
    for row in rows.flatten() {
        let (id, model_json, _provider, created_at, total, input, output, acc_total, acc_in, acc_out) =
            row;
        if id.trim().is_empty() {
            continue;
        }
        let model = serde_json::from_str::<serde_json::Value>(&model_json)
            .ok()
            .and_then(|v| {
                v.get("model_name")
                    .and_then(serde_json::Value::as_str)
                    .map(|s| s.trim().to_string())
            })
            .filter(|m| !m.is_empty());
        let Some(model) = model else { continue };

        // Accumulated columns take precedence over the direct ones.
        let input_tokens = positive(acc_in).or(positive(input)).unwrap_or(0);
        let mut output_tokens = positive(acc_out).or(positive(output)).unwrap_or(0);
        let total_tokens = positive(acc_total)
            .or(positive(total))
            .unwrap_or(input_tokens + output_tokens);
        if input_tokens == 0 && output_tokens == 0 && total_tokens == 0 {
            continue;
        }
        // Reasoning is the gap between the stored total and in+out;
        // bill it as output (ccusage behavior).
        output_tokens += total_tokens.saturating_sub(input_tokens + output_tokens);

        let Some(ts) = parse_created_at(&created_at) else {
            continue;
        };
        records.push(UsageRecord {
            agent: AGENT.to_string(),
            project: "Goose".to_string(),
            session_id: id.clone(),
            timestamp_ms: ts,
            model,
            input_tokens,
            output_tokens,
            cache_creation_5m: 0,
            cache_creation_1h: 0,
            cache_read_tokens: 0,
            cost_usd: None,
            dedup_key: Some(format!("goose:{id}")),
        });
    }
    records
}
