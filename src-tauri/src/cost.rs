use crate::pricing::{ModelPricing, TIER_THRESHOLD};
use crate::types::UsageRecord;

/// Cost mode semantics, identical to ccusage:
/// - Auto: prefer the log's costUSD, otherwise calculate from tokens.
/// - Calculate: always calculate from tokens.
/// - Display: only use the log's costUSD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostMode {
    Auto,
    Calculate,
    Display,
}

impl CostMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "calculate" => Self::Calculate,
            "display" => Self::Display,
            _ => Self::Auto,
        }
    }
}

/// Tiered (long-context) pricing: first 200k tokens at base rate,
/// the remainder at the above-200k rate when one exists.
fn tiered_cost(tokens: u64, base: f64, above: Option<f64>) -> f64 {
    if tokens == 0 {
        return 0.0;
    }
    if let Some(above) = above {
        if tokens > TIER_THRESHOLD {
            return TIER_THRESHOLD as f64 * base + (tokens - TIER_THRESHOLD) as f64 * above;
        }
    }
    tokens as f64 * base
}

/// Calculate cost from token counts with pricing already resolved (the
/// scanner memoizes `PricingMap::resolve` per distinct model string).
/// 1-hour cache creation is billed at 2x the input rate (ccusage
/// cost.rs behavior).
pub fn calculate_cost_with(record: &UsageRecord, p: &ModelPricing) -> f64 {
    cost_from_tokens(record, p)
}

fn cost_from_tokens(r: &UsageRecord, p: &ModelPricing) -> f64 {
    let cache_1h_base = p.input() * 2.0;
    let cache_1h_above = p.input_cost_per_token_above_200k_tokens.map(|c| c * 2.0);

    tiered_cost(
        r.input_tokens,
        p.input(),
        p.input_cost_per_token_above_200k_tokens,
    ) + tiered_cost(
        r.output_tokens,
        p.output(),
        p.output_cost_per_token_above_200k_tokens,
    ) + tiered_cost(
        r.cache_creation_5m,
        p.cache_create(),
        p.cache_creation_input_token_cost_above_200k_tokens,
    ) + tiered_cost(r.cache_creation_1h, cache_1h_base, cache_1h_above)
        + tiered_cost(
            r.cache_read_tokens,
            p.cache_read(),
            p.cache_read_input_token_cost_above_200k_tokens,
        )
}

/// Resolve the effective cost for a record under a given mode.
pub fn resolve_cost(cost_usd: Option<f64>, calculated: f64, mode: CostMode) -> f64 {
    match mode {
        CostMode::Auto => cost_usd.unwrap_or(calculated),
        CostMode::Calculate => calculated,
        CostMode::Display => cost_usd.unwrap_or(0.0),
    }
}
