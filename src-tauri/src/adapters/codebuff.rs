//! Codebuff adapter, ported from ccusage's codebuff adapter.
//!
//! Chats live under ~/.config/manicode{,-dev,-staging}/projects (or
//! $CODEBUFF_DATA_DIR) as `{project}/chats/{chat-id}/chat-messages.json`
//! arrays. Usage hides in up to three metadata layers which are merged
//! field-by-field, earlier layers winning.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "codebuff";

pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(raw) = std::env::var("CODEBUFF_DATA_DIR") {
        for part in raw.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let p = PathBuf::from(part);
            if p.file_name().is_some_and(|n| n == "projects") {
                dirs.push(p);
            } else {
                dirs.push(p.join("projects"));
            }
        }
    } else if let Some(home) = dirs::home_dir() {
        for channel in ["manicode", "manicode-dev", "manicode-staging"] {
            dirs.push(home.join(".config").join(channel).join("projects"));
        }
    }
    dirs.retain(|p| p.is_dir());
    dirs.sort();
    dirs.dedup();
    dirs
}

pub fn collect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in data_dirs() {
        collect_chat_files(&root, &mut files);
    }
    files.sort();
    files.dedup();
    files
}

fn collect_chat_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_chat_files(&path, files);
        } else if path.file_name().is_some_and(|n| n == "chat-messages.json") {
            files.push(path);
        }
    }
}

fn is_assistant(msg: &Value) -> bool {
    let role = util::get_str(msg, &["variant"]).or_else(|| util::get_str(msg, &["role"]));
    matches!(role, Some("ai" | "agent" | "assistant"))
}

#[derive(Default, Clone, Copy)]
struct Usage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
    total: u64,
}

fn parse_usage_object(v: &Value) -> Usage {
    let pick = |keys: &[&str]| -> u64 {
        keys.iter()
            .filter_map(|k| {
                // Support one level of dotted nesting (promptTokensDetails.cachedTokens).
                let mut cur = v;
                for part in k.split('.') {
                    cur = cur.get(part)?;
                }
                Some(util::as_u64(cur))
            })
            .max()
            .unwrap_or(0)
    };
    Usage {
        input: pick(&["inputTokens", "input_tokens", "promptTokens", "prompt_tokens"]),
        output: pick(&[
            "outputTokens",
            "output_tokens",
            "completionTokens",
            "completion_tokens",
        ]),
        cache_read: pick(&[
            "cacheReadInputTokens",
            "cache_read_input_tokens",
            "promptTokensDetails.cachedTokens",
            "prompt_tokens_details.cached_tokens",
        ]),
        cache_creation: pick(&[
            "cacheCreationInputTokens",
            "cache_creation_input_tokens",
            "cacheCreationTokens",
            "cache_creation_tokens",
            "cachedTokensCreated",
            "cached_tokens_created",
        ]),
        total: pick(&["totalTokens", "total_tokens", "total"]),
    }
}

fn merge(base: Usage, fallback: Usage) -> Usage {
    Usage {
        input: if base.input > 0 { base.input } else { fallback.input },
        output: if base.output > 0 { base.output } else { fallback.output },
        cache_read: if base.cache_read > 0 { base.cache_read } else { fallback.cache_read },
        cache_creation: if base.cache_creation > 0 {
            base.cache_creation
        } else {
            fallback.cache_creation
        },
        total: if base.total > 0 { base.total } else { fallback.total },
    }
}

/// Deep fallback: the last assistant entry of
/// metadata.runState.sessionState.mainAgentState.messageHistory carries
/// providerOptions with usage.
fn run_state_usage(metadata: &Value) -> (Usage, Option<String>) {
    let history = metadata
        .pointer("/runState/sessionState/mainAgentState/messageHistory")
        .and_then(Value::as_array);
    let Some(history) = history else {
        return (Usage::default(), None);
    };
    for msg in history.iter().rev() {
        if !is_assistant(msg) {
            continue;
        }
        if let Some(opts) = msg.get("providerOptions") {
            let usage = opts
                .get("usage")
                .map(parse_usage_object)
                .unwrap_or_default();
            let model = util::get_str(opts, &["model"]).map(str::to_string);
            return (usage, model);
        }
    }
    (Usage::default(), None)
}

/// chat ids look like "2026-01-02T03-04-05.678Z": two dashes in the time
/// part stand in for colons.
fn ts_from_chat_id(chat_id: &str) -> Option<i64> {
    if chat_id.len() < 20 || chat_id.as_bytes().get(10) != Some(&b'T') {
        return None;
    }
    let (date, time) = chat_id.split_at(11);
    let fixed = format!("{date}{}", time.replacen('-', ":", 2));
    crate::adapters::claude::parse_timestamp_ms(&fixed)
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(Value::Array(messages)) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };

    // sessions are {channel}/{project}/{chat_id} from the path:
    // .config/{channel}/projects/{project}/chats/{chat_id}/chat-messages.json
    let chat_id = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let project = path
        .ancestors()
        .nth(3)
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let channel = path
        .ancestors()
        .nth(5)
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "manicode".to_string());
    let session_id = format!("{channel}/{project}/{chat_id}");
    let fallback_ts = ts_from_chat_id(&chat_id).unwrap_or_else(|| util::mtime_ms(path));

    let mut records = Vec::new();
    for (ordinal, msg) in messages.iter().enumerate() {
        if !is_assistant(msg) {
            continue;
        }
        let metadata = msg.get("metadata").cloned().unwrap_or(Value::Null);
        let direct = metadata.get("usage").map(parse_usage_object).unwrap_or_default();
        let nested = metadata
            .pointer("/codebuff/usage")
            .map(parse_usage_object)
            .unwrap_or_default();
        let (deep, deep_model) = run_state_usage(&metadata);
        let usage = merge(merge(direct, nested), deep);

        let input = usage.input;
        let mut output = usage.output;
        if input == 0 && output == 0 && usage.cache_read == 0 && usage.cache_creation == 0 {
            if usage.total == 0 {
                continue;
            }
            output = usage.total;
        }

        let ts = msg
            .get("timestamp")
            .or_else(|| msg.get("createdAt"))
            .or_else(|| metadata.get("timestamp"))
            .and_then(util::ts_from_value)
            .unwrap_or(fallback_ts);
        let model = util::get_str(&metadata, &["model"])
            .map(str::to_string)
            .or_else(|| metadata.pointer("/codebuff/model").and_then(Value::as_str).map(str::to_string))
            .or(deep_model)
            .unwrap_or_else(|| "codebuff-unknown".to_string());
        let dedup_key = match util::get_str(msg, &["id"]) {
            Some(id) => format!("codebuff:{session_id}:{id}"),
            None => format!(
                "codebuff:{session_id}:{ts}:{model}:{ordinal}:{input}:{output}"
            ),
        };
        records.push(UsageRecord {
            agent: AGENT.to_string(),
            project: "Codebuff".to_string(),
            session_id: session_id.clone(),
            timestamp_ms: ts,
            model,
            input_tokens: input,
            output_tokens: output,
            cache_creation_5m: usage.cache_creation,
            cache_creation_1h: 0,
            cache_read_tokens: usage.cache_read,
            cost_usd: None,
            dedup_key: Some(dedup_key),
        });
    }
    records
}
