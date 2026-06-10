//! Gemini CLI adapter, ported from ccusage's gemini adapter.
//!
//! Data lives in $GEMINI_DATA_DIR (or ~/.gemini/tmp) as .json session
//! files and .jsonl streams. Token fields come under several aliases;
//! cached tokens may or may not be included in the reported totals, so
//! the input/cache split follows ccusage's overlap-subtraction rules.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "gemini";

pub fn data_dirs() -> Vec<PathBuf> {
    util::env_dirs("GEMINI_DATA_DIR", ".gemini/tmp")
}

pub fn collect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in data_dirs() {
        util::collect_with_ext(&dir, &["json", "jsonl"], &mut files);
    }
    files.sort();
    files.dedup();
    files
}

#[derive(Debug, Default, Clone, Copy)]
struct Tokens {
    input: u64,
    output: u64,
    cached: u64,
    thoughts: u64,
    tool: u64,
    total: u64,
}

impl Tokens {
    fn is_zero(&self) -> bool {
        self.input == 0 && self.output == 0 && self.cached == 0 && self.thoughts == 0
            && self.tool == 0 && self.total == 0
    }
}

fn parse_tokens(v: &Value) -> Option<Tokens> {
    let obj = v.as_object()?;
    let pick = |keys: &[&str]| -> u64 {
        keys.iter()
            .filter_map(|k| obj.get(*k))
            .map(util::as_u64)
            .find(|n| *n > 0)
            .unwrap_or(0)
    };
    let t = Tokens {
        input: pick(&["input", "prompt", "input_tokens", "prompt_tokens"]),
        output: pick(&["output", "candidates", "output_tokens", "candidates_tokens"]),
        cached: pick(&["cached", "cached_tokens"]),
        thoughts: pick(&["thoughts", "reasoning", "thoughts_tokens", "reasoning_tokens"]),
        tool: pick(&["tool", "tool_tokens"]),
        total: pick(&["total", "total_tokens"]),
    };
    (!t.is_zero()).then_some(t)
}

/// Build a usage record from raw token counts. `always_subtract_cache`
/// matches ccusage: aggregate stats always treat `cached` as overlapping
/// input; direct events only when the totals indicate the overlap.
fn build_record(
    t: Tokens,
    always_subtract_cache: bool,
    model: &str,
    session_id: &str,
    ts: i64,
    dedup_key: Option<String>,
) -> Option<UsageRecord> {
    let mut input = t.input;
    let cache_read = t.cached;
    let direct_overlap = t.cached > 0
        && t.total == t.input + t.output + t.thoughts + t.tool
        && t.total != t.input + t.output + t.thoughts + t.tool + t.cached;
    if always_subtract_cache || !direct_overlap {
        input = input.saturating_sub(t.cached.min(input));
    }
    // Tool tokens count as input; thoughts (reasoning) bill as output.
    input += t.tool;
    let mut output = t.output + t.thoughts;
    if input == 0 && output == 0 && cache_read == 0 {
        if t.total == 0 {
            return None;
        }
        output = t.total;
    }
    Some(UsageRecord {
        agent: AGENT.to_string(),
        project: "Gemini".to_string(),
        session_id: session_id.to_string(),
        timestamp_ms: ts,
        model: model.to_string(),
        input_tokens: input,
        output_tokens: output,
        cache_creation_5m: 0,
        cache_creation_1h: 0,
        cache_read_tokens: cache_read,
        cost_usd: None,
        dedup_key,
    })
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let file_stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let fallback_ts = util::mtime_ms(path);

    if path.extension().is_some_and(|e| e == "jsonl") {
        parse_jsonl(&content, &file_stem, fallback_ts)
    } else {
        let Ok(value) = serde_json::from_str::<Value>(&content) else {
            return Vec::new();
        };
        parse_session_value(&value, &file_stem, fallback_ts)
    }
}

/// Stateful JSONL stream: lines may update the sticky sessionId/model,
/// emit direct `type: "gemini"` events (deduped by id, last one wins),
/// or carry aggregate `stats` objects.
fn parse_jsonl(content: &str, file_stem: &str, fallback_ts: i64) -> Vec<UsageRecord> {
    let mut session: Option<String> = None;
    let mut model: Option<String> = None;
    let mut records: Vec<UsageRecord> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();

    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(s) = util::get_str(&value, &["sessionId", "session_id"]) {
            session = Some(s.to_string());
        }
        if let Some(m) = util::get_str(&value, &["model"]) {
            model = Some(m.to_string());
        }
        let sid = session.clone().unwrap_or_else(|| file_stem.to_string());

        if value.get("type").and_then(Value::as_str) == Some("gemini") {
            let Some(tokens) = value.get("tokens").and_then(parse_tokens) else {
                continue;
            };
            let ts = value
                .get("timestamp")
                .or_else(|| value.get("created_at"))
                .and_then(util::ts_from_value)
                .unwrap_or(fallback_ts);
            let m = util::get_str(&value, &["model"])
                .map(str::to_string)
                .or_else(|| model.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let id = util::get_str(&value, &["id"]).map(str::to_string);
            let dedup = id
                .as_ref()
                .map(|i| format!("gemini:{sid}:{i}"));
            let Some(rec) = build_record(tokens, false, &m, &sid, ts, dedup) else {
                continue;
            };
            match id.and_then(|i| by_id.insert(i, records.len())) {
                Some(prev) => records[prev] = rec,
                None => records.push(rec),
            }
        } else if let Some(stats) = value.get("stats").or_else(|| {
            value.get("result").and_then(|r| r.get("stats"))
        }) {
            let ts = value
                .get("timestamp")
                .and_then(util::ts_from_value)
                .unwrap_or(fallback_ts);
            records.extend(parse_stats(stats, model.as_deref(), &sid, ts));
        }
    }
    records
}

/// Single-file JSON session: `messages` array, a bare direct event, or
/// an aggregate `stats` object.
fn parse_session_value(value: &Value, file_stem: &str, fallback_ts: i64) -> Vec<UsageRecord> {
    let session_id = util::get_str(value, &["sessionId", "session_id"])
        .map(str::to_string)
        .unwrap_or_else(|| file_stem.to_string());
    let session_ts = util::get_str(value, &["startTime", "lastUpdated"])
        .and_then(crate::adapters::claude::parse_timestamp_ms)
        .unwrap_or(fallback_ts);
    let mut records = Vec::new();

    if let Some(messages) = value.get("messages").and_then(Value::as_array) {
        for (i, msg) in messages.iter().enumerate() {
            if msg.get("type").and_then(Value::as_str) != Some("gemini") {
                continue;
            }
            let Some(tokens) = msg.get("tokens").and_then(parse_tokens) else {
                continue;
            };
            let ts = msg
                .get("timestamp")
                .or_else(|| msg.get("created_at"))
                .and_then(util::ts_from_value)
                .unwrap_or(session_ts);
            let model = util::get_str(msg, &["model"]).unwrap_or("unknown");
            let dedup = util::get_str(msg, &["id"])
                .map(|id| format!("gemini:{session_id}:{id}"))
                .or_else(|| Some(format!("gemini:{session_id}:msg{i}")));
            records.extend(build_record(tokens, false, model, &session_id, ts, dedup));
        }
    } else if value.get("type").and_then(Value::as_str) == Some("gemini") {
        if let Some(tokens) = value.get("tokens").and_then(parse_tokens) {
            let model = util::get_str(value, &["model"]).unwrap_or("unknown");
            let dedup = util::get_str(value, &["id"])
                .map(|id| format!("gemini:{session_id}:{id}"));
            records.extend(build_record(
                tokens, false, model, &session_id, session_ts, dedup,
            ));
        }
    }
    if let Some(stats) = value
        .get("stats")
        .or_else(|| value.get("result").and_then(|r| r.get("stats")))
    {
        records.extend(parse_stats(stats, None, &session_id, session_ts));
    }
    records
}

/// Aggregate stats: per-model token blocks under `stats.models`, else a
/// single block attributed to `model_hint`.
fn parse_stats(
    stats: &Value,
    model_hint: Option<&str>,
    session_id: &str,
    ts: i64,
) -> Vec<UsageRecord> {
    let mut records = Vec::new();
    if let Some(models) = stats.get("models").and_then(Value::as_object) {
        for (model, block) in models {
            let tokens = block
                .get("tokens")
                .and_then(parse_tokens)
                .or_else(|| parse_tokens(block));
            if let Some(t) = tokens {
                let dedup = format!("gemini:{session_id}:stats:{model}:{ts}");
                records.extend(build_record(t, true, model, session_id, ts, Some(dedup)));
            }
        }
    } else if let Some(t) = stats.get("tokens").and_then(parse_tokens).or_else(|| parse_tokens(stats)) {
        let model = model_hint.unwrap_or("unknown");
        let dedup = format!("gemini:{session_id}:stats:{model}:{ts}");
        records.extend(build_record(t, true, model, session_id, ts, Some(dedup)));
    }
    records
}
