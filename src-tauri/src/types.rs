use serde::Serialize;

/// A normalized usage record, unified across all AI coding agents.
/// Mirrors ccusage's UsageEntry after adapter normalization.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    /// Agent identifier: "claude-code" | "codex"
    pub agent: String,
    /// Human-readable project name extracted from the log path.
    pub project: String,
    pub session_id: String,
    /// Human-readable session title (first user message); empty when the
    /// agent's logs carry no recoverable prompt. Same value on every record
    /// of a session.
    pub title: String,
    /// Epoch milliseconds (UTC).
    pub timestamp_ms: i64,
    pub model: String,
    /// 模型调用时的思考强度；不提供该信息的 Agent 保持为空。
    pub reasoning_effort: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// 5-minute ephemeral cache creation tokens (or total cache creation
    /// when the log does not break it down).
    pub cache_creation_5m: u64,
    /// 1-hour ephemeral cache creation tokens (billed at 2x input rate).
    pub cache_creation_1h: u64,
    pub cache_read_tokens: u64,
    /// Pre-computed cost from the log file (costUSD), if present.
    pub cost_usd: Option<f64>,
    /// Uniqueness key for deduplication (messageId:requestId for Claude).
    pub dedup_key: Option<String>,
}

impl UsageRecord {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            + self.output_tokens
            + self.cache_creation_5m
            + self.cache_creation_1h
            + self.cache_read_tokens
    }
}
