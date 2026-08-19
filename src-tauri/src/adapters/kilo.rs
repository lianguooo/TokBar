//! Kilo adapter, ported from ccusage's kilo adapter.
//!
//! Kilo stores messages in a SQLite `kilo.db` under $KILO_DATA_DIR (or
//! ~/.local/share/kilo) with the same `message(id, session_id, data)`
//! shape as OpenCode; `data` JSON carries tokens, model and cost.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "kilo";

pub fn data_dirs() -> Vec<PathBuf> {
    util::env_dirs("KILO_DATA_DIR", ".local/share/kilo")
}

pub fn collect_files() -> Vec<PathBuf> {
    data_dirs()
        .into_iter()
        .map(|d| d.join("kilo.db"))
        .filter(|p| p.is_file())
        .collect()
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare("SELECT id, session_id, data FROM message") else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) else {
        return Vec::new();
    };

    let mut records = Vec::new();
    for (row_id, row_session, data) in rows.flatten() {
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if value.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(tokens) = value.get("tokens") else {
            continue;
        };
        let Some(model) = util::get_str(&value, &["modelID"]) else {
            continue;
        };
        let input = util::get_u64(tokens, "input");
        let mut output = util::get_u64(tokens, "output");
        // Reasoning bills as output (ccusage behavior).
        output += util::get_u64(tokens, "reasoning");
        let cache = tokens.get("cache");
        let cache_read = cache.map(|c| util::get_u64(c, "read")).unwrap_or(0);
        let cache_write = cache.map(|c| util::get_u64(c, "write")).unwrap_or(0);
        if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
            let total = util::get_u64(tokens, "total");
            if total == 0 {
                continue;
            }
            output = total;
        }
        let Some(ts) = value
            .get("time")
            .and_then(|t| t.get("created"))
            .and_then(Value::as_i64)
            .filter(|n| *n > 0)
            .map(util::smart_unit_ms)
        else {
            continue;
        };
        let session_id = row_session
            .filter(|s| !s.is_empty())
            .or_else(|| util::get_str(&value, &["session_id", "sessionID"]).map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string());
        let message_id = util::get_str(&value, &["id"])
            .map(str::to_string)
            .unwrap_or_else(|| format!("{}:{row_id}", path.display()));
        let cost = value
            .get("cost")
            .and_then(Value::as_f64)
            .filter(|c| c.is_finite() && *c > 0.0);
        records.push(UsageRecord {
            title: String::new(),
            agent: AGENT.to_string(),
            project: "Kilo".to_string(),
            session_id,
            timestamp_ms: ts,
            model: model.to_string(),
            reasoning_effort: String::new(),
            input_tokens: input,
            output_tokens: output,
            cache_creation_5m: cache_write,
            cache_creation_1h: 0,
            cache_read_tokens: cache_read,
            cost_usd: cost,
            dedup_key: Some(format!("kilo:{message_id}")),
        });
    }
    records
}
