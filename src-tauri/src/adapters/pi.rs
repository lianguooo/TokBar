//! pi-agent adapter, ported from ccusage's pi adapter.
//!
//! Sessions are JSONL files under $PI_AGENT_DIR (or
//! ~/.pi/agent/sessions). Assistant `message` lines carry usage with
//! camelCase cache fields and an optional pre-computed `cost.total`.
//!
//! Deviation from ccusage: model names are stored without the "[pi] "
//! display prefix so LiteLLM pricing matching keeps working; the agent
//! column already attributes the usage.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "pi";

pub fn data_dirs() -> Vec<PathBuf> {
    util::env_dirs("PI_AGENT_DIR", ".pi/agent/sessions")
}

pub fn collect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in data_dirs() {
        util::collect_with_ext(&dir, &["jsonl"], &mut files);
    }
    files.sort();
    files
}

/// Path component right after "sessions" is the project name.
fn project_from_path(path: &Path) -> String {
    let mut take_next = false;
    for comp in path.components() {
        let name = comp.as_os_str().to_string_lossy();
        if take_next {
            return name.to_string();
        }
        if name == "sessions" {
            take_next = true;
        }
    }
    "unknown".to_string()
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let project = project_from_path(path);
    let session_id = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .map(|stem| {
            stem.split_once('_')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or(stem)
        })
        .unwrap_or_default();

    let mut records = Vec::new();
    for line in content.lines() {
        if !line.contains("\"usage\"") || !line.contains("\"message\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(kind) = value.get("type").and_then(Value::as_str) {
            if kind != "message" {
                continue;
            }
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
        let Some(ts) = value
            .get("timestamp")
            .and_then(util::ts_from_value)
        else {
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
        let model = util::get_str(message, &["model"]).unwrap_or("unknown");
        let cost = usage
            .get("cost")
            .and_then(|c| c.get("total"))
            .and_then(Value::as_f64)
            .filter(|c| c.is_finite() && *c > 0.0);
        let dedup_key = format!(
            "pi:{project}:{session_id}:{ts}:{model}:{input}:{output}:{cache_write}:{cache_read}"
        );
        records.push(UsageRecord {
            agent: AGENT.to_string(),
            project: project.clone(),
            session_id: session_id.clone(),
            timestamp_ms: ts,
            model: model.to_string(),
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
