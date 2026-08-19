use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::types::UsageRecord;

pub const AGENT: &str = "kimi";

#[derive(Debug, Deserialize)]
struct RawLine {
    /// Epoch seconds (float).
    timestamp: Option<f64>,
    message: Option<RawMessage>,
}

#[derive(Debug, Deserialize)]
struct RawMessage {
    #[serde(rename = "type")]
    kind: Option<String>,
    payload: Option<RawPayload>,
}

#[derive(Debug, Deserialize)]
struct RawPayload {
    token_usage: Option<RawTokenUsage>,
    message_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawTokenUsage {
    #[serde(default)]
    input_other: u64,
    #[serde(default)]
    output: u64,
    #[serde(default)]
    input_cache_read: u64,
    #[serde(default)]
    input_cache_creation: u64,
}

/// Kimi CLI sessions: $KIMI_DATA_DIR or ~/.kimi, usage in
/// sessions/{group}/{session}/wire.jsonl
pub fn data_dirs() -> Vec<PathBuf> {
    let base = std::env::var("KIMI_DATA_DIR")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|h| h.join(".kimi")));
    base.map(|b| b.join("sessions"))
        .filter(|p| p.is_dir())
        .into_iter()
        .collect()
}

pub fn collect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in data_dirs() {
        collect_wire(&dir, &mut files);
    }
    files
}

fn collect_wire(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_wire(&path, files);
        } else if path.file_name().is_some_and(|n| n == "wire.jsonl") {
            files.push(path);
        }
    }
}

/// Parse a Kimi wire.jsonl: "StatusUpdate" messages carry per-step
/// token_usage (ccusage kimi adapter behavior).
pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let session_id = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let fallback_ts = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let mut records = Vec::new();
    for line in content.lines() {
        // Cheap pre-filter, same as ccusage.
        if !line.contains("\"StatusUpdate\"") || !line.contains("\"token_usage\"") {
            continue;
        }
        let Ok(raw) = serde_json::from_str::<RawLine>(line) else {
            continue;
        };
        let Some(message) = raw.message else { continue };
        if message.kind.as_deref() != Some("StatusUpdate") {
            continue;
        }
        let Some(payload) = message.payload else { continue };
        let Some(usage) = payload.token_usage else { continue };
        if usage.input_other == 0
            && usage.output == 0
            && usage.input_cache_read == 0
            && usage.input_cache_creation == 0
        {
            continue;
        }
        let ts = raw
            .timestamp
            .filter(|s| s.is_finite())
            .map(|s| (s * 1000.0) as i64)
            .unwrap_or(fallback_ts);
        let message_id = payload.message_id.unwrap_or_default();
        let dedup_key = format!(
            "kimi:{}:{}:{}:{}:{}:{}:{}",
            session_id,
            message_id,
            ts,
            usage.input_other,
            usage.output,
            usage.input_cache_creation,
            usage.input_cache_read
        );
        records.push(UsageRecord {
            agent: AGENT.to_string(),
            project: "Kimi CLI".to_string(),
            session_id: session_id.clone(),
            timestamp_ms: ts,
            model: "kimi-for-coding".to_string(),
            reasoning_effort: String::new(),
            input_tokens: usage.input_other,
            output_tokens: usage.output,
            cache_creation_5m: usage.input_cache_creation,
            cache_creation_1h: 0,
            cache_read_tokens: usage.input_cache_read,
            cost_usd: None,
            dedup_key: Some(dedup_key),
        });
    }
    records
}
