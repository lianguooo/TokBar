//! OpenCode adapter, ported from ccusage's opencode adapter.
//!
//! Data lives in $OPENCODE_DATA_DIR (or ~/.local/share/opencode): a SQLite
//! `opencode.db` with a `message` table plus per-message JSON files under
//! `storage/message/`. Both carry the same record shape; deduplication by
//! message id collapses overlap.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "opencode";

pub fn data_dirs() -> Vec<PathBuf> {
    util::env_dirs("OPENCODE_DATA_DIR", ".local/share/opencode")
}

pub fn collect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for base in data_dirs() {
        // Prefer opencode.db; fall back to the first opencode-*.db.
        let main_db = base.join("opencode.db");
        if main_db.is_file() {
            files.push(main_db);
        } else if let Ok(read) = std::fs::read_dir(&base) {
            let mut candidates: Vec<PathBuf> = read
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("opencode-") && n.ends_with(".db"))
                })
                .collect();
            candidates.sort();
            files.extend(candidates.into_iter().next());
        }
        let messages = base.join("storage").join("message");
        if messages.is_dir() {
            util::collect_with_ext(&messages, &["json"], &mut files);
        }
    }
    files
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    if path.extension().is_some_and(|e| e == "db") {
        parse_db(path)
    } else {
        parse_json(path)
    }
}

fn parse_db(path: &Path) -> Vec<UsageRecord> {
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
    for (id, session, data) in rows.flatten() {
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if let Some(rec) = record_from_value(&value, Some(&id), session.as_deref()) {
            records.push(rec);
        }
    }
    records
}

fn parse_json(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    record_from_value(&value, None, None).into_iter().collect()
}

fn record_from_value(
    value: &Value,
    id_hint: Option<&str>,
    session_hint: Option<&str>,
) -> Option<UsageRecord> {
    let tokens = value.get("tokens")?;
    let model = util::get_str(value, &["modelID"])?;
    let input = util::get_u64(tokens, "input");
    let mut output = util::get_u64(tokens, "output");
    // OpenCode reports reasoning separately; ccusage bills it as output.
    output += util::get_u64(tokens, "reasoning");
    let cache = tokens.get("cache");
    let cache_read = cache.map(|c| util::get_u64(c, "read")).unwrap_or(0);
    let cache_write = cache.map(|c| util::get_u64(c, "write")).unwrap_or(0);
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
        let total = util::get_u64(tokens, "total");
        if total == 0 {
            return None;
        }
        output = total;
    }

    let timestamp_ms = value
        .get("time")
        .and_then(|t| t.get("created"))
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .map(util::smart_unit_ms)?;
    let message_id = id_hint
        .map(str::to_string)
        .or_else(|| util::get_str(value, &["id"]).map(str::to_string))
        .unwrap_or_default();
    let session_id = session_hint
        .map(str::to_string)
        .or_else(|| util::get_str(value, &["sessionID"]).map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());
    let cost = value
        .get("cost")
        .and_then(Value::as_f64)
        .filter(|c| c.is_finite() && *c > 0.0);

    Some(UsageRecord {
        title: String::new(),
        agent: AGENT.to_string(),
        project: "OpenCode".to_string(),
        session_id,
        timestamp_ms,
        model: normalize_model(model),
        reasoning_effort: String::new(),
        input_tokens: input,
        output_tokens: output,
        cache_creation_5m: cache_write,
        cache_creation_1h: 0,
        cache_read_tokens: cache_read,
        cost_usd: cost,
        dedup_key: (!message_id.is_empty()).then(|| format!("opencode:{message_id}")),
    })
}

/// ccusage's opencode model normalization: alias fix-ups plus
/// `claude-{family}-{major}.{minor}` -> `claude-{family}-{major}-{minor}`.
fn normalize_model(model: &str) -> String {
    match model {
        "gemini-3-pro-high" => return "gemini-3-pro-preview".to_string(),
        "k2p6" => return "kimi-k2.6".to_string(),
        _ => {}
    }
    if let Some(rest) = model.strip_prefix("claude-") {
        let bytes = rest.as_bytes();
        if bytes.len() >= 3 {
            let n = bytes.len();
            if bytes[n - 2] == b'.'
                && bytes[n - 1].is_ascii_digit()
                && bytes[n - 3].is_ascii_digit()
            {
                let mut fixed = model.to_string();
                fixed.replace_range(model.len() - 2..model.len() - 1, "-");
                return fixed;
            }
        }
    }
    model.to_string()
}
