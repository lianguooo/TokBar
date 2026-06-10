use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::adapters::claude::parse_timestamp_ms;
use crate::types::UsageRecord;

pub const AGENT: &str = "codex";

#[derive(Debug, Deserialize)]
struct RawLine {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    payload: Option<RawPayload>,
}

#[derive(Debug, Deserialize)]
struct RawPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    info: Option<RawInfo>,
    model: Option<String>,
    model_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawInfo {
    model: Option<String>,
    last_token_usage: Option<RawTokenUsage>,
    /// Cumulative session totals; older Codex versions only write this,
    /// so per-turn usage is derived as the delta from the previous event.
    total_token_usage: Option<RawTokenUsage>,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
struct RawTokenUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    reasoning_output_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
}

/// Codex usage dirs: `sessions/` plus `archived_sessions/` (ccusage
/// scans both) under every Codex home.
///
/// Homes come from $CODEX_HOME (comma-separated, ccusage parity) when
/// set. Otherwise ~/.codex plus any sibling `~/.codex{-_.}*` home that
/// has a `sessions/` dir — multi-account setups launch the second
/// Codex with CODEX_HOME pointing at such a clone, and a GUI app never
/// sees that per-shell variable.
pub fn data_dirs() -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(raw) = std::env::var("CODEX_HOME") {
        for part in raw.split(',') {
            let part = part.trim();
            if !part.is_empty() {
                bases.push(PathBuf::from(part));
            }
        }
    } else if let Some(home) = dirs::home_dir() {
        bases.push(home.join(".codex"));
        if let Ok(read) = std::fs::read_dir(&home) {
            for entry in read.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let is_clone = name
                    .strip_prefix(".codex")
                    .is_some_and(|rest| rest.starts_with(['-', '_', '.']));
                if is_clone && path.join("sessions").is_dir() {
                    bases.push(path);
                }
            }
        }
    }
    bases.sort();
    bases.dedup();
    bases
        .iter()
        .flat_map(|base| [base.join("sessions"), base.join("archived_sessions")])
        .filter(|p| p.is_dir())
        .collect()
}

pub fn collect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in data_dirs() {
        collect_jsonl(&dir, &mut files);
    }
    files
}

/// Codex does not record the service tier per event; ccusage detects it
/// from `service_tier = "fast" | "priority"` in the Codex config.toml and
/// bills all usage at the fast multiplier when set.
pub fn fast_tier_enabled() -> bool {
    static FAST: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FAST.get_or_init(detect_fast_tier)
}

fn detect_fast_tier() -> bool {
    let base = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")));
    let Some(config) = base.map(|b| b.join("config.toml")) else {
        return false;
    };
    let Ok(content) = std::fs::read_to_string(config) else {
        return false;
    };
    content.lines().any(|line| {
        let setting = line.split('#').next().unwrap_or_default().trim();
        let Some((key, value)) = setting.split_once('=') else {
            return false;
        };
        key.trim() == "service_tier"
            && matches!(value.trim().trim_matches(['"', '\'']), "fast" | "priority")
    })
}

fn collect_jsonl(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, files);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            files.push(path);
        }
    }
}

/// Parse a Codex session JSONL: "event_msg" lines whose payload is a
/// "token_count" event carrying last_token_usage (per-turn deltas).
/// Identical events across session files are deduplicated by content key,
/// matching ccusage's codex adapter.
pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let session_id = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let fast = fast_tier_enabled();

    let mut records = Vec::new();
    // token_count events usually omit the model; track the session's
    // current model from turn_context lines (ccusage parser behavior).
    let mut current_model: Option<String> = None;
    let mut previous_totals: Option<RawTokenUsage> = None;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(raw) = serde_json::from_str::<RawLine>(line) else {
            continue;
        };
        let Some(payload) = raw.payload else { continue };
        if let Some(m) = payload.model.clone().or_else(|| payload.model_name.clone()) {
            if !m.is_empty() {
                current_model = Some(m);
            }
        }
        if raw.kind.as_deref() != Some("event_msg")
            || payload.kind.as_deref() != Some("token_count")
        {
            continue;
        }
        let Some(info) = payload.info else { continue };
        if let Some(m) = info.model.clone().filter(|m| !m.is_empty()) {
            current_model = Some(m);
        }
        let totals = info.total_token_usage;
        let usage = info
            .last_token_usage
            .or_else(|| totals.map(|t| subtract_usage(&t, previous_totals.as_ref())));
        if let Some(t) = totals {
            previous_totals = Some(t);
        }
        let Some(usage) = usage else { continue };
        if usage.input_tokens == 0
            && usage.cached_input_tokens == 0
            && usage.output_tokens == 0
            && usage.reasoning_output_tokens == 0
        {
            continue;
        }
        let Some(ts) = raw.timestamp.as_deref().and_then(parse_timestamp_ms) else {
            continue;
        };
        let mut model = current_model.clone().unwrap_or_else(|| "gpt-5".to_string());
        if fast {
            model.push_str("-fast");
        }
        // Codex reports input inclusive of cached reads; the billable
        // fresh input is input - cached (ccusage non_cached_input_tokens).
        let cached = usage.cached_input_tokens.min(usage.input_tokens);
        let fresh_input = usage.input_tokens - cached;
        // Content-based dedup key: identical events in different session
        // files (e.g. resumed sessions) count once.
        let dedup_key = format!(
            "codex:{}:{}:{}:{}:{}:{}",
            ts, model, usage.input_tokens, cached, usage.output_tokens, usage.reasoning_output_tokens
        );
        records.push(UsageRecord {
            agent: AGENT.to_string(),
            project: "Codex CLI".to_string(),
            session_id: session_id.clone(),
            timestamp_ms: ts,
            model,
            input_tokens: fresh_input,
            output_tokens: usage.output_tokens,
            cache_creation_5m: 0,
            cache_creation_1h: 0,
            cache_read_tokens: cached,
            cost_usd: None,
            dedup_key: Some(dedup_key),
        });
    }
    records
}

/// Per-turn usage as the delta between cumulative totals (ccusage
/// subtract_codex_raw_usage).
fn subtract_usage(total: &RawTokenUsage, previous: Option<&RawTokenUsage>) -> RawTokenUsage {
    let Some(prev) = previous else { return *total };
    RawTokenUsage {
        input_tokens: total.input_tokens.saturating_sub(prev.input_tokens),
        cached_input_tokens: total
            .cached_input_tokens
            .saturating_sub(prev.cached_input_tokens),
        output_tokens: total.output_tokens.saturating_sub(prev.output_tokens),
        reasoning_output_tokens: total
            .reasoning_output_tokens
            .saturating_sub(prev.reasoning_output_tokens),
        total_tokens: total.total_tokens.saturating_sub(prev.total_tokens),
    }
}
