//! GitHub Copilot CLI adapter, ported from ccusage's copilot adapter.
//!
//! Copilot CLI exports OpenTelemetry JSONL under ~/.copilot/otel (or the
//! file named by $COPILOT_OTEL_FILE_EXPORTER_PATH). The same inference
//! can appear as a chat span, an agent-summary span, an inference log,
//! and an agent-turn log; sources are ranked and deduped per trace /
//! response id so each call is counted once.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::adapters::util;
use crate::types::UsageRecord;

pub const AGENT: &str = "copilot";

pub fn data_dirs() -> Vec<PathBuf> {
    if let Ok(custom) = std::env::var("COPILOT_OTEL_FILE_EXPORTER_PATH") {
        let p = PathBuf::from(custom.trim());
        if p.is_dir() {
            return vec![p];
        }
        if p.is_file() {
            return p.parent().map(|d| vec![d.to_path_buf()]).unwrap_or_default();
        }
        return Vec::new();
    }
    dirs::home_dir()
        .map(|h| h.join(".copilot").join("otel"))
        .filter(|p| p.is_dir())
        .into_iter()
        .collect()
}

pub fn collect_files() -> Vec<PathBuf> {
    if let Ok(custom) = std::env::var("COPILOT_OTEL_FILE_EXPORTER_PATH") {
        let p = PathBuf::from(custom.trim());
        if p.is_file() {
            return vec![p];
        }
    }
    let mut files = Vec::new();
    for dir in data_dirs() {
        util::collect_with_ext(&dir, &["jsonl"], &mut files);
    }
    files.sort();
    files.dedup();
    files
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    AgentTurnLog,
    InferenceLog,
    AgentSummarySpan,
    ChatSpan,
}

struct Candidate {
    source: Source,
    trace_id: Option<String>,
    response_id: Option<String>,
    dedup_key: String,
    record: UsageRecord,
}

fn attr<'a>(attrs: &'a Value, key: &str) -> Option<&'a Value> {
    attrs.get(key)
}

fn attr_str<'a>(attrs: &'a Value, key: &str) -> Option<&'a str> {
    attrs
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn is_span_record(value: &Value) -> bool {
    if value.get("type").and_then(Value::as_str) == Some("span") {
        return true;
    }
    value.get("name").is_some()
        && ["spanId", "traceId", "startTime", "endTime", "duration", "kind"]
            .iter()
            .any(|k| value.get(*k).is_some() || span_context(value, k).is_some())
}

fn span_context<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get("spanContext").and_then(|c| c.get(key))
}

fn span_id_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .or_else(|| span_context(value, key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn body_str<'a>(value: &'a Value) -> Option<&'a str> {
    value
        .get("_body")
        .or_else(|| value.get("body"))
        .and_then(Value::as_str)
}

/// [seconds, nanos] hrtime arrays, smart-unit integers, or nothing.
fn record_timestamp(value: &Value, fallback: i64) -> i64 {
    for key in ["endTime", "startTime", "hrTime", "_hrTime", "time"] {
        if let Some(arr) = value.get(key).and_then(Value::as_array) {
            if arr.len() == 2 {
                let secs = util::as_u64(&arr[0]) as i64;
                let nanos = util::as_u64(&arr[1]) as i64;
                if secs > 0 {
                    return secs * 1000 + nanos / 1_000_000;
                }
            }
        }
    }
    for key in ["timestamp", "observedTimestamp"] {
        if let Some(n) = value.get(key).and_then(Value::as_i64).filter(|n| *n > 0) {
            return util::smart_unit_ms(n);
        }
    }
    if let Some(n) = value
        .get("timeUnixNano")
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
    {
        return n / 1_000_000;
    }
    fallback
}

fn classify(value: &Value, attrs: &Value) -> Option<Source> {
    let op = attr_str(attrs, "gen_ai.operation.name").unwrap_or("");
    let name = value.get("name").and_then(Value::as_str).unwrap_or("");
    if is_span_record(value) {
        if op == "chat" || name.starts_with("chat ") {
            return Some(Source::ChatSpan);
        }
        if op == "invoke_agent" || name.starts_with("invoke_agent ") {
            return Some(Source::AgentSummarySpan);
        }
        return None;
    }
    let event = attr_str(attrs, "event.name").unwrap_or("");
    let body = body_str(value).unwrap_or("");
    if event == "copilot_chat.agent.turn" || body.starts_with("copilot_chat.agent.turn") {
        return Some(Source::AgentTurnLog);
    }
    if event == "gen_ai.client.inference.operation.details"
        || body.starts_with("GenAI inference:")
    {
        return Some(Source::InferenceLog);
    }
    None
}

pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let fallback_ts = util::mtime_ms(path);

    // First pass: trace context (model / session per traceId) and
    // candidate extraction.
    let mut trace_model: HashMap<String, String> = HashMap::new();
    let mut trace_session: HashMap<String, String> = HashMap::new();
    let mut raw: Vec<(usize, Value, Source)> = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(attrs) = value.get("attributes").cloned() else {
            continue;
        };
        let Some(source) = classify(&value, &attrs) else {
            continue;
        };
        if let Some(trace) = span_id_field(&value, "traceId") {
            if let Some(m) =
                attr_str(&attrs, "gen_ai.response.model").or_else(|| attr_str(&attrs, "gen_ai.request.model"))
            {
                trace_model.entry(trace.clone()).or_insert_with(|| m.to_string());
            }
            if let Some(s) = session_from_attrs(&attrs) {
                trace_session.entry(trace).or_insert(s);
            }
        }
        raw.push((idx, value, source));
    }

    let mut candidates: Vec<Candidate> = Vec::new();
    for (idx, value, source) in &raw {
        if let Some(c) = build_candidate(
            value,
            *source,
            *idx,
            fallback_ts,
            &trace_model,
            &trace_session,
        ) {
            candidates.push(c);
        }
    }

    // Cross-source dedup: a lower-priority source is dropped when a
    // higher-priority source already covers its trace or response id.
    let ids_of = |src: Source| -> (HashSet<String>, HashSet<String>) {
        let mut traces = HashSet::new();
        let mut responses = HashSet::new();
        for c in candidates.iter().filter(|c| c.source == src) {
            traces.extend(c.trace_id.clone());
            responses.extend(c.response_id.clone());
        }
        (traces, responses)
    };
    let (chat_traces, chat_resps) = ids_of(Source::ChatSpan);
    let (inf_traces, inf_resps) = ids_of(Source::InferenceLog);
    let (turn_traces, turn_resps) = ids_of(Source::AgentTurnLog);

    let mut seen_keys = HashSet::new();
    let mut records = Vec::new();
    for c in candidates {
        let covered = |traces: &HashSet<String>, resps: &HashSet<String>| {
            c.trace_id.as_ref().is_some_and(|t| traces.contains(t))
                || c.response_id.as_ref().is_some_and(|r| resps.contains(r))
        };
        let emit = match c.source {
            Source::ChatSpan => true,
            Source::InferenceLog => !covered(&chat_traces, &chat_resps),
            Source::AgentTurnLog => {
                !covered(&chat_traces, &chat_resps) && !covered(&inf_traces, &inf_resps)
            }
            Source::AgentSummarySpan => {
                !covered(&chat_traces, &chat_resps)
                    && !covered(&inf_traces, &inf_resps)
                    && !covered(&turn_traces, &turn_resps)
            }
        };
        if emit && seen_keys.insert(c.dedup_key.clone()) {
            records.push(c.record);
        }
    }
    records
}

fn session_from_attrs(attrs: &Value) -> Option<String> {
    for key in [
        "gen_ai.conversation.id",
        "copilot_chat.session_id",
        "copilot_chat.chat_session_id",
        "session.id",
        "github.copilot.interaction_id",
        "gen_ai.response.id",
    ] {
        if let Some(s) = attr_str(attrs, key) {
            return Some(s.to_string());
        }
    }
    None
}

fn build_candidate(
    value: &Value,
    source: Source,
    idx: usize,
    fallback_ts: i64,
    trace_model: &HashMap<String, String>,
    trace_session: &HashMap<String, String>,
) -> Option<Candidate> {
    let attrs = value.get("attributes")?;
    let input_raw = attr(attrs, "gen_ai.usage.input_tokens").map(util::as_u64).unwrap_or(0);
    let mut output = attr(attrs, "gen_ai.usage.output_tokens").map(util::as_u64).unwrap_or(0);
    let cache_read = attr(attrs, "gen_ai.usage.cache_read.input_tokens")
        .map(util::as_u64)
        .unwrap_or(0);
    let cache_creation = attr(attrs, "gen_ai.usage.cache_write.input_tokens")
        .or_else(|| attr(attrs, "gen_ai.usage.cache_creation.input_tokens"))
        .map(util::as_u64)
        .filter(|n| *n > 0)
        .unwrap_or(0);
    let reasoning = attr(attrs, "gen_ai.usage.reasoning.output_tokens")
        .or_else(|| attr(attrs, "gen_ai.usage.reasoning_tokens"))
        .map(util::as_u64)
        .filter(|n| *n > 0)
        .unwrap_or(0);
    let total = attr(attrs, "gen_ai.usage.total_tokens")
        .or_else(|| attr(attrs, "gen_ai.usage.total.token_count"))
        .map(util::as_u64)
        .filter(|n| *n > 0)
        .unwrap_or(0);

    // Input includes cache reads in copilot telemetry; split them out.
    let input = input_raw.saturating_sub(cache_read.min(input_raw));
    output += reasoning;
    if input == 0 && output == 0 && cache_read == 0 && cache_creation == 0 {
        if total == 0 {
            return None;
        }
        output = total;
    }

    let trace_id = span_id_field(value, "traceId");
    let span_id = span_id_field(value, "spanId");
    let response_id = attr_str(attrs, "gen_ai.response.id").map(str::to_string);
    let ts = record_timestamp(value, fallback_ts);

    let model = attr_str(attrs, "gen_ai.response.model")
        .or_else(|| attr_str(attrs, "gen_ai.request.model"))
        .map(str::to_string)
        .or_else(|| {
            trace_id
                .as_ref()
                .and_then(|t| trace_model.get(t).cloned())
        })
        .unwrap_or_else(|| "unknown".to_string());
    let session_id = session_from_attrs(attrs)
        .or_else(|| {
            trace_id
                .as_ref()
                .and_then(|t| trace_session.get(t).cloned())
        })
        .or_else(|| trace_id.clone())
        .unwrap_or_else(|| "unknown-session".to_string());

    let turn_index = attr(attrs, "turn.index")
        .or_else(|| attr(attrs, "copilot_chat.turn.index"))
        .map(util::as_u64)
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("idx-{idx}"));
    let dedup_key = match (source, &trace_id, &span_id) {
        (Source::AgentTurnLog, Some(t), _) => format!("copilot:agent-turn:{t}:{turn_index}"),
        (Source::AgentTurnLog, None, _) => {
            format!("copilot:agent-turn:{session_id}:{turn_index}:{idx}")
        }
        (_, Some(t), Some(s)) => format!("copilot:{t}:{s}"),
        _ => format!("copilot:{session_id}:{ts}:{idx}"),
    };

    Some(Candidate {
        source,
        trace_id,
        response_id,
        dedup_key: dedup_key.clone(),
        record: UsageRecord {
            title: String::new(),
            agent: AGENT.to_string(),
            project: "GitHub Copilot CLI".to_string(),
            session_id,
            timestamp_ms: ts,
            model,
            reasoning_effort: String::new(),
            input_tokens: input,
            output_tokens: output,
            cache_creation_5m: cache_creation,
            cache_creation_1h: 0,
            cache_read_tokens: cache_read,
            cost_usd: None,
            dedup_key: Some(dedup_key),
        },
    })
}
