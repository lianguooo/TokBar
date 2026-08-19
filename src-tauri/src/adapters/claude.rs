use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::types::UsageRecord;

pub const AGENT: &str = "claude-code";

/// Raw JSONL line schema, ported from ccusage types.rs.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEntry {
    session_id: Option<String>,
    timestamp: Option<String>,
    message: Option<RawMessage>,
    #[serde(rename = "costUSD")]
    cost_usd: Option<f64>,
    request_id: Option<String>,
    is_api_error_message: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawMessage {
    usage: Option<RawUsage>,
    model: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    cache_creation: Option<RawCacheCreation>,
    /// "standard" | "fast" — fast/priority tier is billed at a multiplier.
    speed: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawCacheCreation {
    #[serde(default)]
    ephemeral_5m_input_tokens: u64,
    #[serde(default)]
    ephemeral_1h_input_tokens: u64,
}

/// Discover Claude Code data directories, in ccusage priority order:
/// 1. $CLAUDE_CONFIG_DIR (comma-separated)
/// 2. $XDG_CONFIG_HOME/claude or ~/.config/claude
/// 3. ~/.claude
pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs_out = Vec::new();
    if let Ok(env_paths) = std::env::var("CLAUDE_CONFIG_DIR") {
        for p in env_paths.split(',') {
            let p = p.trim();
            if p.is_empty() {
                continue;
            }
            let path = PathBuf::from(p);
            let projects = if path.ends_with("projects") {
                path
            } else {
                path.join("projects")
            };
            if projects.is_dir() {
                dirs_out.push(projects);
            }
        }
        if !dirs_out.is_empty() {
            return dirs_out;
        }
    }
    if let Some(home) = dirs::home_dir() {
        for base in [home.join(".config").join("claude"), home.join(".claude")] {
            let projects = base.join("projects");
            if projects.is_dir() {
                dirs_out.push(projects);
            }
        }
    }
    dirs_out
}

/// Recursively collect all .jsonl files under the projects dirs.
pub fn collect_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in data_dirs() {
        collect_jsonl(&dir, &mut files);
    }
    files
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

/// Parse one JSONL file into normalized usage records.
/// Skips lines without usage data, synthetic models, and API error messages.
pub fn parse_file(path: &Path) -> Vec<UsageRecord> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let session_id = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let project = project_name_from_path(path);
    let title = extract_title(&content);

    let mut records = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(raw) = serde_json::from_str::<RawEntry>(line) else {
            continue;
        };
        if raw.is_api_error_message == Some(true) {
            continue;
        }
        let Some(message) = raw.message else { continue };
        let Some(usage) = message.usage else { continue };
        let mut model = message.model.unwrap_or_default();
        if model.is_empty() || model == "<synthetic>" {
            continue;
        }
        // ccusage reports fast-tier usage as "<model>-fast" so the fast
        // multiplier applies and breakdowns separate the two tiers.
        if usage.speed.as_deref() == Some("fast") {
            model.push_str("-fast");
        }
        if usage.input_tokens == 0
            && usage.output_tokens == 0
            && usage.cache_creation_input_tokens == 0
            && usage.cache_read_input_tokens == 0
        {
            continue;
        }
        let Some(ts) = raw.timestamp.as_deref().and_then(parse_timestamp_ms) else {
            continue;
        };
        // cache_creation breakdown (5m/1h) supersedes the legacy total field.
        let (cache_5m, cache_1h) = match &usage.cache_creation {
            Some(b) => (b.ephemeral_5m_input_tokens, b.ephemeral_1h_input_tokens),
            None => (usage.cache_creation_input_tokens, 0),
        };
        // Dedup key = messageId:requestId, ccusage's uniqueness hash.
        let dedup_key = message
            .id
            .as_ref()
            .map(|mid| format!("{}:{}", mid, raw.request_id.as_deref().unwrap_or("")));

        records.push(UsageRecord {
            title: title.clone(),
            agent: AGENT.to_string(),
            project: project.clone(),
            session_id: raw.session_id.clone().unwrap_or_else(|| session_id.clone()),
            timestamp_ms: ts,
            model,
            reasoning_effort: String::new(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_creation_5m: cache_5m,
            cache_creation_1h: cache_1h,
            cache_read_tokens: usage.cache_read_input_tokens,
            cost_usd: raw.cost_usd,
            dedup_key,
        });
    }
    records
}

/// First real user prompt in a session file, used as its title. Skips
/// tool-result turns (content is a list of tool_result blocks) and injected
/// context (command wrappers, system reminders — all `<...>`-prefixed).
fn extract_title(content: &str) -> String {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("user") {
            continue;
        }
        if v.get("isMeta").and_then(|m| m.as_bool()) == Some(true) {
            continue;
        }
        let candidate = match v.pointer("/message/content") {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Array(blocks)) => blocks
                .iter()
                .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .and_then(|b| b.get("text").and_then(|t| t.as_str()))
                .map(|s| s.to_string()),
            _ => None,
        };
        if let Some(title) = candidate.as_deref().and_then(super::util::clean_title) {
            return title;
        }
    }
    String::new()
}

#[cfg(test)]
mod title_tests {
    use super::extract_title;

    #[test]
    fn extract_title_takes_first_real_user_prompt() {
        // First user turn is a tool_result list (skipped), then a string prompt.
        let content = concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"out"}]}}"#,
            "\n",
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":"排查计费重复问题"}}"#,
        );
        assert_eq!(extract_title(content), "排查计费重复问题");
    }

    #[test]
    fn extract_title_reads_text_block_and_skips_injected() {
        let content = concat!(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<system-reminder>noise</system-reminder>"}]}}"#,
            "\n",
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"add dark mode"}]}}"#,
        );
        assert_eq!(extract_title(content), "add dark mode");
    }

    #[test]
    fn extract_title_empty_when_no_prompt() {
        let content = r#"{"type":"assistant","message":{"role":"assistant","content":"hi"}}"#;
        assert_eq!(extract_title(content), "");
    }
}

pub fn parse_timestamp_ms(ts: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// Extract a readable project name from the encoded directory name,
/// e.g. "-Users-alice-Documents-MyProject-TokBar" -> "TokBar".
/// Simplified port of ccusage project_names.rs.
fn project_name_from_path(path: &Path) -> String {
    let Some(dir_name) = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_string())
    else {
        return "Unknown Project".to_string();
    };
    if dir_name.is_empty() || dir_name == "unknown" {
        return "Unknown Project".to_string();
    }
    let segments: Vec<&str> = dir_name.split('-').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return dir_name;
    }
    // Hex/UUID-looking paths: keep the last two segments.
    let hexish = segments.len() >= 5
        && segments
            .iter()
            .all(|s| s.chars().all(|c| c.is_ascii_hexdigit()));
    if hexish {
        return segments[segments.len() - 2..].join("-");
    }
    // Path-encoded names: take what follows the username for /Users/<u>/ or /home/<u>/.
    let lower: Vec<String> = segments.iter().map(|s| s.to_lowercase()).collect();
    if let Some(idx) = lower.iter().position(|s| s == "users" || s == "home") {
        if segments.len() > idx + 2 {
            let tail = &segments[idx + 2..];
            // Drop common parent folders to keep the meaningful tail.
            let skip = ["documents", "projects", "code", "src", "dev", "work", "repos"];
            let meaningful: Vec<&str> = tail
                .iter()
                .copied()
                .skip_while(|s| skip.contains(&s.to_lowercase().as_str()))
                .collect();
            if !meaningful.is_empty() {
                return meaningful.join("-");
            }
            return tail.join("-");
        }
    }
    dir_name.trim_start_matches('-').to_string()
}
