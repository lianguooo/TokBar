//! OpenClaw adapter, ported from ccusage's openclaw adapter.
//!
//! Session transcripts are JSONL files under $OPENCLAW_DIR (or the
//! legacy ~/.openclaw / ~/.clawdbot / ~/.moltbot / ~/.moldbot homes).
//! `model_change` / `model-snapshot` lines update the active model;
//! assistant `message` lines carry usage and an optional cost.
//!
//! Deviation from ccusage: model names are stored without the
//! "[openclaw] " display prefix so LiteLLM pricing matching keeps
//! working; the agent column already attributes the usage.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "openclaw";

pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(raw) = std::env::var("OPENCLAW_DIR") {
        for part in raw.split(',') {
            let part = part.trim();
            if !part.is_empty() {
                dirs.push(PathBuf::from(part));
            }
        }
    } else if let Some(home) = dirs::home_dir() {
        for legacy in [".openclaw", ".clawdbot", ".moltbot", ".moldbot"] {
            dirs.push(home.join(legacy));
        }
    }
    dirs.retain(|p| p.is_dir());
    dirs.sort();
    dirs.dedup();
    dirs
}

pub fn collect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in data_dirs() {
        collect_jsonl_like(&dir, &mut files);
    }
    files.sort();
    files
}

/// `.jsonl` plus archived `.jsonl.deleted.*` / `.jsonl.reset.*` files.
fn collect_jsonl_like(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            collect_jsonl_like(&path, files);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".jsonl") || n.contains(".jsonl."))
        {
            files.push(path);
        }
    }
}

/// modelId/model/provider may sit on the record or inside `data`.
fn snapshot_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    util::get_str(value, keys)
        .or_else(|| value.get("data").and_then(|d| util::get_str(d, keys)))
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let session_id = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .map(|n| n.split(".jsonl").next().unwrap_or(&n).to_string())
        .unwrap_or_default();
    let fallback_ts = util::mtime_ms(path);

    let mut current_model: Option<String> = None;
    let mut records = Vec::new();

    for line in content.lines() {
        if !line.contains("model_change")
            && !line.contains("model-snapshot")
            && !line.contains("\"usage\"")
        {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let is_snapshot = kind == "model_change"
            || (kind == "custom"
                && value.get("customType").and_then(Value::as_str) == Some("model-snapshot"));
        if is_snapshot {
            if let Some(m) = snapshot_field(&value, &["modelId", "model"]) {
                current_model = Some(m.to_string());
            }
            continue;
        }

        if kind != "message" {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(usage) = message.get("usage") else {
            continue;
        };
        let input = util::get_u64(usage, "input");
        let mut output = util::get_u64(usage, "output");
        let cache_read = util::get_u64(usage, "cacheRead");
        let cache_write = util::get_u64(usage, "cacheWrite");
        if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
            let total = util::get_u64(usage, "totalTokens");
            if total == 0 {
                continue;
            }
            output = total;
        }
        let ts = message
            .get("timestamp")
            .or_else(|| value.get("timestamp"))
            .and_then(util::ts_from_value)
            .unwrap_or(fallback_ts);
        let model = util::get_str(message, &["modelId", "model"])
            .map(str::to_string)
            .or_else(|| current_model.clone())
            .unwrap_or_else(|| "unknown".to_string());
        let cost = message
            .get("cost")
            .and_then(|c| c.get("total"))
            .and_then(Value::as_f64)
            .filter(|c| c.is_finite() && *c > 0.0);
        let dedup_key = format!(
            "openclaw:{session_id}:{ts}:{model}:{input}:{output}:{cache_write}:{cache_read}"
        );
        records.push(UsageRecord {
            agent: AGENT.to_string(),
            project: "OpenClaw".to_string(),
            session_id: session_id.clone(),
            timestamp_ms: ts,
            model,
            input_tokens: input,
            output_tokens: output,
            cache_creation_5m: cache_write,
            cache_creation_1h: 0,
            cache_read_tokens: cache_read,
            cost_usd: cost,
            dedup_key: Some(dedup_key),
        });
    }
    records
}
