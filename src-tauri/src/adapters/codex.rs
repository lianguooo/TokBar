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
struct RawLineKind<'a> {
    #[serde(rename = "type", borrow)]
    kind: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct RawPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    info: Option<RawInfo>,
    model: Option<String>,
    model_name: Option<String>,
    thread_source: Option<String>,
    effort: Option<String>,
    collaboration_mode: Option<RawCollaborationMode>,
}

#[derive(Debug, Deserialize)]
struct RawCollaborationMode {
    settings: Option<RawCollaborationSettings>,
}

#[derive(Debug, Deserialize)]
struct RawCollaborationSettings {
    reasoning_effort: Option<String>,
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

/// 发现所有 Codex home 目录：
/// 1. $CODEX_HOME 环境变量（逗号分隔，兼容 ccusage）
/// 2. 默认 ~/.codex 加上同级的 `.codex{-_.}*` 克隆目录（多账号场景）
fn codex_homes() -> Vec<PathBuf> {
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
                // 同级 .codex-xxx / .codex_xxx / .codex.xxx 克隆目录
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
}

/// 主 Codex home：配置改写（provider / auth.json 切换）的唯一目标。
/// 与扫描用的 `codex_homes()` 不同 —— 那份会排序去重并带上克隆目录，
/// 这里必须取 $CODEX_HOME 的第一项，没有则回落 ~/.codex。
pub fn primary_home() -> PathBuf {
    if let Ok(raw) = std::env::var("CODEX_HOME") {
        if let Some(first) = raw.split(',').map(str::trim).find(|part| !part.is_empty()) {
            return PathBuf::from(first);
        }
    }
    dirs::home_dir()
        .map(|home| home.join(".codex"))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

/// Codex usage dirs: `sessions/` plus `archived_sessions/` (ccusage
/// scans both) under every Codex home.
pub fn data_dirs() -> Vec<PathBuf> {
    codex_homes()
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
/// from `service_tier = "fast" | "priority"` in the Codex config.toml.
/// Because the rollout carries no historical tier, the scanner samples
/// this at scan time and records it on a timeline (see `db::scan_all`),
/// so a later config change no longer re-prices past usage. This reads
/// the *current* config fresh on every call — no process-wide caching.
pub fn config_fast_tier() -> bool {
    detect_fast_tier()
}

/// 遍历所有 Codex home 的 config.toml，任一含 fast/priority 即启用 fast tier。
/// 旧实现仅检查单个 home，Windows 上因路径差异可能漏检。
fn detect_fast_tier() -> bool {
    codex_homes().into_iter().any(|base| {
        let config = base.join("config.toml");
        let Ok(content) = std::fs::read_to_string(config) else {
            return false;
        };
        config_has_fast_tier(&content)
    })
}

/// 检查 config.toml 内容是否包含 `service_tier = "fast"` 或 `"priority"`。
fn config_has_fast_tier(content: &str) -> bool {
    content.lines().any(|line| {
        // 去除行内注释后解析 key = value
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
    parse_content(&content, &session_id)
}

/// 解析单个 Codex rollout，并剔除子代理继承的父任务历史快照。
/// 计费所需的 fast/priority 判定在 DB 层按时间轴决定，这里只输出原始模型名。
fn parse_content(content: &str, session_id: &str) -> Vec<UsageRecord> {
    let mut records = Vec::new();
    let (project, title) = extract_meta(content);
    // 旧版子代理没有通信边界标记，只有确认边界存在时才能剔除边界前的继承历史。
    let is_subagent_rollout = content
        .lines()
        .filter_map(|line| serde_json::from_str::<RawLine>(line.trim()).ok())
        .any(|raw| {
            raw.kind.as_deref() == Some("session_meta")
                && raw
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.thread_source.as_deref())
                    == Some("subagent")
        });
    let has_communication_boundary = is_subagent_rollout
        && content
            .lines()
            .filter_map(|line| serde_json::from_str::<RawLineKind<'_>>(line.trim()).ok())
            .any(|raw| raw.kind == Some("inter_agent_communication_metadata"));
    // token_count events usually omit the model; track the session's
    // current model from turn_context lines (ccusage parser behavior).
    let mut current_model: Option<String> = None;
    let mut current_effort: Option<String> = None;
    let mut previous_totals: Option<RawTokenUsage> = None;
    let mut usage_started = !has_communication_boundary;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(raw) = serde_json::from_str::<RawLine>(line) else {
            continue;
        };

        // 新版子代理从通信边界后开始计量，边界前累计值仍参与后续差分。
        if has_communication_boundary
            && raw.kind.as_deref() == Some("inter_agent_communication_metadata")
        {
            usage_started = true;
            continue;
        }

        let Some(payload) = raw.payload else { continue };
        if let Some(m) = payload.model.clone().or_else(|| payload.model_name.clone()) {
            if !m.is_empty() {
                current_model = Some(m);
            }
        }
        // 优先读取当前 turn 的 effort，兼容其位于协作模式 settings 中的日志结构。
        if let Some(effort) = payload
            .effort
            .clone()
            .filter(|effort| !effort.is_empty())
            .or_else(|| {
                payload
                    .collaboration_mode
                    .as_ref()
                    .and_then(|mode| mode.settings.as_ref())
                    .and_then(|settings| settings.reasoning_effort.clone())
                    .filter(|effort| !effort.is_empty())
            })
        {
            current_effort = Some(effort);
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
        // 累计值差分可同时规避 Codex 重复写入的 last_token_usage 终态事件。
        let usage = totals
            .map(|t| {
                let counters_regressed = previous_totals.as_ref().is_some_and(|previous| {
                    t.input_tokens < previous.input_tokens
                        || t.cached_input_tokens < previous.cached_input_tokens
                        || t.output_tokens < previous.output_tokens
                        || t.reasoning_output_tokens < previous.reasoning_output_tokens
                        || t.total_tokens < previous.total_tokens
                });
                if counters_regressed {
                    // 恢复或日志轮转造成累计值回退时，回退到本次用量，避免漏计当前请求。
                    info.last_token_usage.unwrap_or(t)
                } else {
                    subtract_usage(&t, previous_totals.as_ref())
                }
            })
            .or(info.last_token_usage);
        if let Some(t) = totals {
            previous_totals = Some(t);
        }
        if !usage_started {
            continue;
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
        let model = current_model.clone().unwrap_or_else(|| "gpt-5".to_string());
        // Codex reports input inclusive of cached reads; the billable
        // fresh input is input - cached (ccusage non_cached_input_tokens).
        let cached = usage.cached_input_tokens.min(usage.input_tokens);
        let fresh_input = usage.input_tokens - cached;
        // Content-based dedup key: identical events in different session
        // files (e.g. resumed sessions) count once.
        let dedup_key = format!(
            "codex:{}:{}:{}:{}:{}:{}",
            ts,
            model,
            usage.input_tokens,
            cached,
            usage.output_tokens,
            usage.reasoning_output_tokens
        );
        records.push(UsageRecord {
            title: title.clone(),
            agent: AGENT.to_string(),
            project: project.clone(),
            session_id: session_id.to_string(),
            timestamp_ms: ts,
            model,
            reasoning_effort: current_effort.clone().unwrap_or_default(),
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

/// Session project + title from a rollout. Project is the working directory's
/// last path component (from `session_meta.cwd`), falling back to "Codex CLI";
/// title is the first real user prompt, skipping Codex's injected `<...>`
/// context blocks (plugin lists, environment context, user instructions).
fn extract_meta(content: &str) -> (String, String) {
    let mut project = String::new();
    let mut title = String::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let kind = v.get("type").and_then(|t| t.as_str());
        if project.is_empty() && kind == Some("session_meta") {
            if let Some(cwd) = v.pointer("/payload/cwd").and_then(|c| c.as_str()) {
                let name = std::path::Path::new(cwd)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !name.is_empty() {
                    project = name;
                }
            }
        }
        if title.is_empty()
            && v.pointer("/payload/role").and_then(|r| r.as_str()) == Some("user")
        {
            if let Some(blocks) = v.pointer("/payload/content").and_then(|c| c.as_array()) {
                let text = blocks
                    .iter()
                    .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("input_text"))
                    .and_then(|b| b.get("text").and_then(|t| t.as_str()));
                if let Some(t) = text.and_then(super::util::clean_title) {
                    title = t;
                }
            }
        }
        if !project.is_empty() && !title.is_empty() {
            break;
        }
    }
    if project.is_empty() {
        project = "Codex CLI".to_string();
    }
    (project, title)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 子代理只应统计通信边界后的累计增量，不能计入父任务历史快照。
    #[test]
    fn subagent_skips_inherited_history_and_duplicate_totals() {
        let content = r#"
{"timestamp":"2026-08-18T00:00:00.000Z","type":"session_meta","payload":{"thread_source":"subagent"}}
{"timestamp":"2026-08-18T00:00:00.001Z","type":"turn_context","payload":{"model":"gpt-5.6-sol"}}
{"timestamp":"2026-08-18T00:00:00.002Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10,"reasoning_output_tokens":2,"total_tokens":110},"last_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10,"reasoning_output_tokens":2,"total_tokens":110}}}}
{"timestamp":"2026-08-18T00:00:00.003Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":220,"cached_input_tokens":180,"output_tokens":20,"reasoning_output_tokens":4,"total_tokens":240},"last_token_usage":{"input_tokens":120,"cached_input_tokens":100,"output_tokens":10,"reasoning_output_tokens":2,"total_tokens":130}}}}
{"timestamp":"2026-08-18T00:00:01.000Z","type":"turn_context","payload":{"model":"gpt-5.6-luna","effort":"high"}}
{"timestamp":"2026-08-18T00:00:01.001Z","type":"inter_agent_communication_metadata","payload":{}}
{"timestamp":"2026-08-18T00:00:01.002Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":250,"cached_input_tokens":200,"output_tokens":25,"reasoning_output_tokens":5,"total_tokens":275},"last_token_usage":{"input_tokens":30,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":1,"total_tokens":35}}}}
{"timestamp":"2026-08-18T00:00:01.003Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":250,"cached_input_tokens":200,"output_tokens":25,"reasoning_output_tokens":5,"total_tokens":275},"last_token_usage":{"input_tokens":30,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":1,"total_tokens":35}}}}
{"timestamp":"2026-08-18T00:00:01.004Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":290,"cached_input_tokens":230,"output_tokens":33,"reasoning_output_tokens":6,"total_tokens":323},"last_token_usage":{"input_tokens":40,"cached_input_tokens":30,"output_tokens":8,"reasoning_output_tokens":1,"total_tokens":48}}}}
"#;

        let records = parse_content(content, "subagent-session");

        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record.model == "gpt-5.6-luna"));
        assert!(records
            .iter()
            .all(|record| record.reasoning_effort == "high"));
        assert_eq!(records[0].input_tokens, 10);
        assert_eq!(records[0].cache_read_tokens, 20);
        assert_eq!(records[0].output_tokens, 5);
        assert_eq!(records[1].input_tokens, 10);
        assert_eq!(records[1].cache_read_tokens, 30);
        assert_eq!(records[1].output_tokens, 8);
    }

    /// 无通信边界的旧版子代理应按普通会话统计，避免全量重扫丢失历史用量。
    #[test]
    fn legacy_subagent_without_boundary_keeps_usage() {
        let content = r#"
{"timestamp":"2026-08-18T00:00:00.000Z","type":"session_meta","payload":{"thread_source":"subagent"}}
{"timestamp":"2026-08-18T00:00:00.001Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","effort":"medium"}}
{"timestamp":"2026-08-18T00:00:00.002Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10,"reasoning_output_tokens":2,"total_tokens":110},"last_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10,"reasoning_output_tokens":2,"total_tokens":110}}}}
{"timestamp":"2026-08-18T00:00:00.003Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":150,"cached_input_tokens":120,"output_tokens":20,"reasoning_output_tokens":4,"total_tokens":170},"last_token_usage":{"input_tokens":50,"cached_input_tokens":40,"output_tokens":10,"reasoning_output_tokens":2,"total_tokens":60}}}}
"#;

        let records = parse_content(content, "legacy-subagent-session");

        assert_eq!(records.len(), 2);
        assert!(records.iter().all(|record| record.model == "gpt-5.6-sol"));
        assert!(records
            .iter()
            .all(|record| record.reasoning_effort == "medium"));
        assert_eq!(records[0].total_tokens(), 110);
        assert_eq!(records[1].total_tokens(), 60);
    }

    /// 普通任务应按累计值差分，并忽略累计值未变化的重复终态事件。
    #[test]
    fn regular_session_uses_cumulative_deltas() {
        let content = r#"
{"timestamp":"2026-08-18T00:00:00.000Z","type":"session_meta","payload":{"thread_source":"vscode"}}
{"timestamp":"2026-08-18T00:00:00.001Z","type":"turn_context","payload":{"model":"gpt-5.6-sol"}}
{"timestamp":"2026-08-18T00:00:00.002Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10,"reasoning_output_tokens":2,"total_tokens":110},"last_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10,"reasoning_output_tokens":2,"total_tokens":110}}}}
{"timestamp":"2026-08-18T00:00:00.003Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10,"reasoning_output_tokens":2,"total_tokens":110},"last_token_usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":10,"reasoning_output_tokens":2,"total_tokens":110}}}}
{"timestamp":"2026-08-18T00:00:00.004Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":150,"cached_input_tokens":120,"output_tokens":20,"reasoning_output_tokens":4,"total_tokens":170},"last_token_usage":{"input_tokens":50,"cached_input_tokens":40,"output_tokens":10,"reasoning_output_tokens":2,"total_tokens":60}}}}
"#;

        let records = parse_content(content, "regular-session");

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].total_tokens(), 110);
        assert_eq!(records[1].total_tokens(), 60);
    }

    /// 旧版日志缺少累计值时，仍需兼容 last_token_usage 单次用量。
    #[test]
    fn legacy_session_falls_back_to_last_usage() {
        let content = r#"
{"timestamp":"2026-08-18T00:00:00.000Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-08-18T00:00:00.001Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":50,"cached_input_tokens":30,"output_tokens":5,"reasoning_output_tokens":1,"total_tokens":55}}}}
"#;

        let records = parse_content(content, "legacy-session");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].input_tokens, 20);
        assert_eq!(records[0].cache_read_tokens, 30);
        assert_eq!(records[0].output_tokens, 5);
    }

    /// 项目名取 session_meta.cwd 末段，标题取首条真实用户消息（跳过注入的 <...> 块）。
    #[test]
    fn extract_meta_uses_cwd_and_first_user_prompt() {
        let content = r#"
{"type":"session_meta","payload":{"cwd":"/Users/alice/Documents/ai-employee","thread_source":"user"}}
{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"<app-context>desktop</app-context>"}]}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<recommended_plugins>ignore me</recommended_plugins>"}]}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"帮我重构支付模块\n第二行"}]}}
"#;
        let (project, title) = extract_meta(content);
        assert_eq!(project, "ai-employee");
        assert_eq!(title, "帮我重构支付模块");
    }

    /// 无 cwd / 无用户消息时回退到 "Codex CLI" 且标题为空。
    #[test]
    fn extract_meta_falls_back_without_cwd_or_user() {
        let (project, title) = extract_meta(r#"{"type":"session_meta","payload":{"thread_source":"user"}}"#);
        assert_eq!(project, "Codex CLI");
        assert_eq!(title, "");
    }

    /// 协作模式内的 reasoning_effort 应作为旧日志结构的兼容来源。
    #[test]
    fn effort_falls_back_to_collaboration_settings() {
        let content = r#"
{"timestamp":"2026-08-18T00:00:00.000Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","collaboration_mode":{"settings":{"reasoning_effort":"xhigh"}}}}
{"timestamp":"2026-08-18T00:00:00.001Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":50,"cached_input_tokens":30,"output_tokens":5,"reasoning_output_tokens":1,"total_tokens":55}}}}
"#;

        let records = parse_content(content, "fallback-effort-session");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].model, "gpt-5.6-sol");
        assert_eq!(records[0].reasoning_effort, "xhigh");
    }
}
