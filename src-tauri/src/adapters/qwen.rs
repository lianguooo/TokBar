//! Qwen Code adapter, ported from ccusage's qwen adapter.
//!
//! Chats live at $QWEN_DATA_DIR (or ~/.qwen) under
//! `projects/{project}/chats/*.jsonl`; assistant lines carry a Gemini
//! style `usageMetadata` block.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::claude::parse_timestamp_ms;
use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "qwen";

pub fn data_dirs() -> Vec<PathBuf> {
    util::env_dirs("QWEN_DATA_DIR", ".qwen")
        .into_iter()
        .map(|b| b.join("projects"))
        .filter(|p| p.is_dir())
        .collect()
}

pub fn collect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in data_dirs() {
        util::collect_with_ext(&root, &["jsonl"], &mut files);
    }
    // Keep only projects/{project}/chats/{file}.jsonl shaped paths.
    files.retain(|f| {
        f.parent()
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == "chats")
    });
    files.sort();
    files
}

/// `projects/{project}/chats/{file}` -> project.
fn project_from_path(path: &Path) -> Option<String> {
    let chats = path.parent()?;
    let project = chats.parent()?;
    if project.parent()?.file_name()? == "projects" {
        Some(project.file_name()?.to_string_lossy().to_string())
    } else {
        None
    }
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let project = project_from_path(path).unwrap_or_else(|| "qwen".to_string());
    let file_stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let fallback_ts = util::mtime_ms(path);

    let mut records = Vec::new();
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(usage) = value.get("usageMetadata") else {
            continue;
        };
        let input = util::get_u64(usage, "promptTokenCount");
        let mut output = util::get_u64(usage, "candidatesTokenCount");
        let cache_read = util::get_u64(usage, "cachedContentTokenCount");
        // Reasoning tokens bill as output (ccusage behavior).
        let thoughts = util::get_u64(usage, "thoughtsTokenCount");
        output += thoughts;
        if input == 0 && output == 0 && cache_read == 0 {
            let total = util::get_u64(usage, "totalTokenCount");
            if total == 0 {
                continue;
            }
            output = total;
        }
        let ts = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp_ms)
            .unwrap_or(fallback_ts);
        let model = util::get_str(&value, &["model"]).unwrap_or("unknown");
        let session_id = util::get_str(&value, &["sessionId"])
            .map(str::to_string)
            .unwrap_or_else(|| format!("{project}-{file_stem}"));
        let dedup_key = format!(
            "qwen:{session_id}:{ts}:{model}:{input}:{output}:{cache_read}"
        );
        records.push(UsageRecord {
            agent: AGENT.to_string(),
            project: project.clone(),
            session_id,
            timestamp_ms: ts,
            model: model.to_string(),
            input_tokens: input,
            output_tokens: output,
            cache_creation_5m: 0,
            cache_creation_1h: 0,
            cache_read_tokens: cache_read,
            cost_usd: None,
            dedup_key: Some(dedup_key),
        });
    }
    records
}
