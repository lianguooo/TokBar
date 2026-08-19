//! Droid (Factory) adapter, ported from ccusage's droid adapter.
//!
//! Each session under $DROID_SESSIONS_DIR (or ~/.factory/sessions)
//! keeps a `{session}.settings.json` with cumulative `tokenUsage`
//! totals. Snapshots grow over time; the per-session dedup key plus
//! TokBar's "more tokens wins" conflict rule keeps the latest one.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::claude::parse_timestamp_ms;
use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "droid";

pub fn data_dirs() -> Vec<PathBuf> {
    util::env_dirs("DROID_SESSIONS_DIR", ".factory/sessions")
}

pub fn collect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in data_dirs() {
        collect_settings(&dir, &mut files);
    }
    files.sort();
    files
}

fn collect_settings(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_settings(&path, files);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".settings.json"))
        {
            files.push(path);
        }
    }
}

/// `custom:` prefix and `[...]` suffix stripped, lowercased, dots and
/// runs of separators collapsed to single dashes.
fn normalize_model(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    if let Some(rest) = s.strip_prefix("custom:") {
        s = rest.to_string();
    }
    if let Some(open) = s.find('[') {
        s.truncate(open);
    }
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for ch in s.trim().to_lowercase().chars() {
        let mapped = if ch == '.' || ch.is_whitespace() || ch == '-' { '-' } else { ch };
        if mapped == '-' {
            if !prev_dash && !out.is_empty() {
                out.push('-');
            }
            prev_dash = true;
        } else {
            out.push(mapped);
            prev_dash = false;
        }
    }
    out.trim_end_matches('-').to_string()
}

fn default_model_for_provider(provider: &str) -> &'static str {
    let p = provider.trim().to_lowercase().replace('-', "_");
    match p.as_str() {
        "" | "claude" | "anthropic" => "claude-unknown",
        "openai" => "gpt-unknown",
        "google" | "google_ai" | "gemini" | "vertex" | "vertex_ai" => "gemini-unknown",
        "xai" | "x_ai" | "grok" => "grok-unknown",
        _ => "unknown",
    }
}

/// Fallback: scan the sibling `{session}.jsonl` for a "Model: ..." line
/// (first 500 lines, ccusage behavior).
fn model_from_sibling_jsonl(settings_path: &Path) -> Option<String> {
    let name = settings_path.file_name()?.to_str()?;
    let prefix = name.strip_suffix(".settings.json")?;
    let jsonl = settings_path.with_file_name(format!("{prefix}.jsonl"));
    let content = std::fs::read_to_string(jsonl).ok()?;
    for line in content.lines().take(500) {
        if let Some(pos) = line.find("Model: ") {
            let rest = &line[pos + 7..];
            let model = rest
                .split(|c: char| c == '"' || c == '\\' || c == ',')
                .next()
                .unwrap_or("")
                .trim();
            if !model.is_empty() {
                let normalized = normalize_model(model);
                if !normalized.is_empty() {
                    return Some(normalized);
                }
            }
        }
    }
    None
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    let Some(usage) = value.get("tokenUsage") else {
        return Vec::new();
    };
    let input = util::get_u64(usage, "inputTokens");
    let mut output = util::get_u64(usage, "outputTokens");
    let cache_creation = util::get_u64(usage, "cacheCreationTokens");
    let cache_read = util::get_u64(usage, "cacheReadTokens");
    // Thinking tokens bill as output (ccusage behavior).
    output += util::get_u64(usage, "thinkingTokens");
    if input == 0 && output == 0 && cache_creation == 0 && cache_read == 0 {
        let total = util::get_u64(usage, "totalTokens");
        if total == 0 {
            return Vec::new();
        }
        output = total;
    }

    let provider = util::get_str(&value, &["providerLock"]).unwrap_or("");
    let model = util::get_str(&value, &["model"])
        .map(normalize_model)
        .filter(|m| !m.is_empty())
        .or_else(|| model_from_sibling_jsonl(path))
        .unwrap_or_else(|| default_model_for_provider(provider).to_string());

    let ts = util::get_str(&value, &["providerLockTimestamp"])
        .and_then(parse_timestamp_ms)
        .unwrap_or_else(|| util::mtime_ms(path));
    let session_id = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".settings.json"))
        .unwrap_or("unknown")
        .to_string();

    vec![UsageRecord {
        agent: AGENT.to_string(),
        project: "Droid".to_string(),
        session_id: session_id.clone(),
        timestamp_ms: ts,
        model,
        reasoning_effort: String::new(),
        input_tokens: input,
        output_tokens: output,
        cache_creation_5m: cache_creation,
        cache_creation_1h: 0,
        cache_read_tokens: cache_read,
        cost_usd: None,
        // Cumulative session snapshot: one row per session, latest wins.
        dedup_key: Some(format!("droid:{session_id}")),
    }]
}
