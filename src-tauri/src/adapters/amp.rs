//! Amp adapter, ported from ccusage's amp adapter.
//!
//! Threads are JSON files under $AMP_DATA_DIR (or ~/.local/share/amp)
//! `threads/`. Newer threads carry a `usageLedger.events` array (cache
//! tokens joined from `messages` via `toMessageId`); older ones only
//! have assistant `messages[].usage` blocks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::claude::parse_timestamp_ms;
use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "amp";

pub fn data_dirs() -> Vec<PathBuf> {
    util::env_dirs("AMP_DATA_DIR", ".local/share/amp")
        .into_iter()
        .map(|b| b.join("threads"))
        .filter(|p| p.is_dir())
        .collect()
}

pub fn collect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in data_dirs() {
        util::collect_with_ext(&dir, &["json"], &mut files);
    }
    files.sort();
    files
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    let thread_id = util::get_str(&value, &["id"])
        .map(str::to_string)
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        });

    if let Some(events) = value
        .get("usageLedger")
        .and_then(|l| l.get("events"))
        .and_then(Value::as_array)
    {
        parse_ledger(&value, events, &thread_id)
    } else {
        parse_messages(&value, &thread_id)
    }
}

/// Cache tokens live on assistant messages; the ledger references them
/// through `toMessageId`.
fn cache_by_message(value: &Value) -> HashMap<i64, (u64, u64)> {
    let mut map = HashMap::new();
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return map;
    };
    for msg in messages {
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(id) = msg.get("messageId").and_then(Value::as_i64) else {
            continue;
        };
        let Some(usage) = msg.get("usage") else { continue };
        map.insert(
            id,
            (
                util::get_u64(usage, "cacheCreationInputTokens"),
                util::get_u64(usage, "cacheReadInputTokens"),
            ),
        );
    }
    map
}

fn parse_ledger(value: &Value, events: &[Value], thread_id: &str) -> Vec<UsageRecord> {
    let cache_map = cache_by_message(value);
    let mut records = Vec::new();
    for event in events {
        let Some(ts) = util::get_str(event, &["timestamp"]).and_then(parse_timestamp_ms) else {
            continue;
        };
        let Some(model) = util::get_str(event, &["model"]) else {
            continue;
        };
        let tokens = event.get("tokens").cloned().unwrap_or(Value::Null);
        let input = util::get_u64(&tokens, "input");
        let mut output = util::get_u64(&tokens, "output");
        let (cache_creation, cache_read) = event
            .get("toMessageId")
            .and_then(Value::as_i64)
            .and_then(|id| cache_map.get(&id).copied())
            .unwrap_or((0, 0));
        if input == 0 && output == 0 && cache_creation == 0 && cache_read == 0 {
            let total = util::get_u64(&tokens, "total");
            if total == 0 {
                continue;
            }
            output = total;
        }
        records.push(record(
            thread_id,
            ts,
            model,
            input,
            output,
            cache_creation,
            cache_read,
        ));
    }
    records
}

fn parse_messages(value: &Value, thread_id: &str) -> Vec<UsageRecord> {
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for msg in messages {
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(usage) = msg.get("usage") else { continue };
        let ts_str = util::get_str(usage, &["timestamp"])
            .or_else(|| util::get_str(msg, &["timestamp"]));
        let Some(ts) = ts_str.and_then(parse_timestamp_ms) else {
            continue;
        };
        let Some(model) =
            util::get_str(usage, &["model"]).or_else(|| util::get_str(msg, &["model"]))
        else {
            continue;
        };
        let input = util::get_u64(usage, "inputTokens");
        let mut output = util::get_u64(usage, "outputTokens");
        let cache_creation = util::get_u64(usage, "cacheCreationInputTokens");
        let cache_read = util::get_u64(usage, "cacheReadInputTokens");
        if input == 0 && output == 0 && cache_creation == 0 && cache_read == 0 {
            let total = util::get_u64(usage, "totalTokens");
            if total == 0 {
                continue;
            }
            output = total;
        }
        records.push(record(
            thread_id,
            ts,
            model,
            input,
            output,
            cache_creation,
            cache_read,
        ));
    }
    records
}

fn record(
    thread_id: &str,
    ts: i64,
    model: &str,
    input: u64,
    output: u64,
    cache_creation: u64,
    cache_read: u64,
) -> UsageRecord {
    UsageRecord {
        title: String::new(),
        agent: AGENT.to_string(),
        project: "Amp".to_string(),
        session_id: thread_id.to_string(),
        timestamp_ms: ts,
        model: model.to_string(),
        reasoning_effort: String::new(),
        input_tokens: input,
        output_tokens: output,
        cache_creation_5m: cache_creation,
        cache_creation_1h: 0,
        cache_read_tokens: cache_read,
        cost_usd: None,
        // ccusage applies no dedup for amp; file re-parses already
        // replace prior rows (entries are deleted per file first).
        dedup_key: None,
    }
}
