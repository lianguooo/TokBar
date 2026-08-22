use chrono::{Local, TimeZone};
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::cost::CostMode;

const MILLIS_PER_HOUR: i64 = 3_600_000;

/// SQL expression for the effective cost under a cost mode.
fn cost_expr(mode: CostMode) -> &'static str {
    match mode {
        CostMode::Auto => "COALESCE(cost_usd, calculated_cost)",
        CostMode::Calculate => "calculated_cost",
        CostMode::Display => "COALESCE(cost_usd, 0)",
    }
}

fn archive_cost_expr(mode: CostMode) -> &'static str {
    match mode {
        CostMode::Auto => "cost_auto",
        CostMode::Calculate => "cost_calculate",
        CostMode::Display => "cost_display",
    }
}

fn range_clause(since_ms: Option<i64>, until_ms: Option<i64>) -> String {
    let mut clause = String::from("1=1");
    if let Some(s) = since_ms {
        clause.push_str(&format!(" AND timestamp_ms >= {s}"));
    }
    if let Some(u) = until_ms {
        clause.push_str(&format!(" AND timestamp_ms < {u}"));
    }
    clause
}

/// Archived usage is intentionally day-granular. App ranges are aligned to
/// local day boundaries; an exclusive `until` therefore maps to `< YYYY-MM-DD`.
fn archive_range_clause(since_ms: Option<i64>, until_ms: Option<i64>) -> String {
    let mut clause = String::from("1=1");
    if let Some(s) = since_ms.and_then(|ms| Local.timestamp_millis_opt(ms).single()) {
        clause.push_str(&format!(" AND date_local >= '{}'", s.format("%Y-%m-%d")));
    }
    if let Some(u) = until_ms.and_then(|ms| Local.timestamp_millis_opt(ms).single()) {
        clause.push_str(&format!(" AND date_local < '{}'", u.format("%Y-%m-%d")));
    }
    clause
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub cost: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub requests: i64,
    pub sessions: i64,
    pub active_days: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentBreakdown {
    pub agent: String,
    pub cost: f64,
    pub total_tokens: i64,
    pub requests: i64,
    pub sessions: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub totals: Totals,
    pub by_agent: Vec<AgentBreakdown>,
}

pub fn overview(
    conn: &Connection,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    mode: CostMode,
) -> Result<Overview, String> {
    let cost = cost_expr(mode);
    let archive_cost = archive_cost_expr(mode);
    let range = range_clause(since_ms, until_ms);
    let archive_range = archive_range_clause(since_ms, until_ms);
    let mut totals = conn
        .query_row(
            &format!(
                "SELECT COALESCE(SUM({cost}),0), COALESCE(SUM(input_tokens),0),
                        COALESCE(SUM(output_tokens),0),
                        COALESCE(SUM(cache_creation_5m + cache_creation_1h),0),
                        COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(total_tokens),0),
                        COUNT(*), COUNT(DISTINCT agent || ':' || session_id),
                        COUNT(DISTINCT date_local)
                 FROM entries WHERE {range}"
            ),
            [],
            |row| {
                Ok(Totals {
                    cost: row.get(0)?,
                    input_tokens: row.get(1)?,
                    output_tokens: row.get(2)?,
                    cache_creation_tokens: row.get(3)?,
                    cache_read_tokens: row.get(4)?,
                    total_tokens: row.get(5)?,
                    requests: row.get(6)?,
                    sessions: row.get(7)?,
                    active_days: row.get(8)?,
                })
            },
        )
        .map_err(|e| e.to_string())?;

    let archived: (f64, i64, i64, i64, i64, i64, i64) = conn
        .query_row(
            &format!(
                "SELECT COALESCE(SUM({archive_cost}),0), COALESCE(SUM(input_tokens),0),
                        COALESCE(SUM(output_tokens),0),
                        COALESCE(SUM(cache_creation_5m + cache_creation_1h),0),
                        COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(total_tokens),0),
                        COALESCE(SUM(requests),0)
                 FROM usage_archive WHERE {archive_range}"
            ),
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;
    totals.cost += archived.0;
    totals.input_tokens += archived.1;
    totals.output_tokens += archived.2;
    totals.cache_creation_tokens += archived.3;
    totals.cache_read_tokens += archived.4;
    totals.total_tokens += archived.5;
    totals.requests += archived.6;
    totals.active_days = conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM (
                   SELECT date_local FROM entries WHERE {range}
                   UNION
                   SELECT date_local FROM usage_archive WHERE {archive_range}
                 )"
            ),
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(&format!(
            "WITH combined AS (
               SELECT agent, COALESCE(SUM({cost}),0) AS cost,
                      COALESCE(SUM(total_tokens),0) AS total_tokens,
                      COUNT(*) AS requests, COUNT(DISTINCT session_id) AS sessions
               FROM entries WHERE {range} GROUP BY agent
               UNION ALL
               SELECT agent, COALESCE(SUM({archive_cost}),0),
                      COALESCE(SUM(total_tokens),0), COALESCE(SUM(requests),0), 0
               FROM usage_archive WHERE {archive_range} GROUP BY agent
             )
             SELECT agent, SUM(cost), SUM(total_tokens), SUM(requests), SUM(sessions)
             FROM combined GROUP BY agent ORDER BY 2 DESC"
        ))
        .map_err(|e| e.to_string())?;
    let by_agent = stmt
        .query_map([], |row| {
            Ok(AgentBreakdown {
                agent: row.get(0)?,
                cost: row.get(1)?,
                total_tokens: row.get(2)?,
                requests: row.get(3)?,
                sessions: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();

    Ok(Overview { totals, by_agent })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyRow {
    pub date: String,
    pub agent: String,
    pub cost: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub requests: i64,
}

pub fn daily(
    conn: &Connection,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    mode: CostMode,
) -> Result<Vec<DailyRow>, String> {
    let cost = cost_expr(mode);
    let archive_cost = archive_cost_expr(mode);
    let range = range_clause(since_ms, until_ms);
    let archive_range = archive_range_clause(since_ms, until_ms);
    let mut stmt = conn
        .prepare(&format!(
            "WITH combined AS (
               SELECT date_local, agent, {cost} AS cost,
                      input_tokens, output_tokens, cache_creation_5m, cache_creation_1h,
                      cache_read_tokens, total_tokens, 1 AS requests
               FROM entries WHERE {range}
               UNION ALL
               SELECT date_local, agent, {archive_cost},
                      input_tokens, output_tokens, cache_creation_5m, cache_creation_1h,
                      cache_read_tokens, total_tokens, requests
               FROM usage_archive WHERE {archive_range}
             )
             SELECT date_local, agent, COALESCE(SUM(cost),0),
                    COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_creation_5m + cache_creation_1h),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(total_tokens),0),
                    COALESCE(SUM(requests),0)
             FROM combined GROUP BY date_local, agent ORDER BY date_local ASC"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(DailyRow {
                date: row.get(0)?,
                agent: row.get(1)?,
                cost: row.get(2)?,
                input_tokens: row.get(3)?,
                output_tokens: row.get(4)?,
                cache_creation_tokens: row.get(5)?,
                cache_read_tokens: row.get(6)?,
                total_tokens: row.get(7)?,
                requests: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    Ok(rows)
}

/// Same row shape as daily, but bucketed by local hour ("HH:00") —
/// used when the UI is showing a single day.
pub fn hourly(
    conn: &Connection,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    mode: CostMode,
) -> Result<Vec<DailyRow>, String> {
    let cost = cost_expr(mode);
    let range = range_clause(since_ms, until_ms);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT strftime('%H:00', timestamp_ms/1000, 'unixepoch', 'localtime'), agent,
                    COALESCE(SUM({cost}),0),
                    COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_creation_5m + cache_creation_1h),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(total_tokens),0), COUNT(*)
             FROM entries WHERE {range}
             GROUP BY 1, agent ORDER BY 1 ASC"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(DailyRow {
                date: row.get(0)?,
                agent: row.get(1)?,
                cost: row.get(2)?,
                input_tokens: row.get(3)?,
                output_tokens: row.get(4)?,
                cache_creation_tokens: row.get(5)?,
                cache_read_tokens: row.get(6)?,
                total_tokens: row.get(7)?,
                requests: row.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    Ok(rows)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    pub model: String,
    pub cost: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub requests: i64,
}

pub fn models(
    conn: &Connection,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    mode: CostMode,
) -> Result<Vec<ModelRow>, String> {
    let cost = cost_expr(mode);
    let archive_cost = archive_cost_expr(mode);
    let range = range_clause(since_ms, until_ms);
    let archive_range = archive_range_clause(since_ms, until_ms);
    let mut stmt = conn
        .prepare(&format!(
            "WITH combined AS (
               SELECT model, {cost} AS cost, input_tokens, output_tokens,
                      cache_creation_5m, cache_creation_1h, cache_read_tokens,
                      total_tokens, 1 AS requests
               FROM entries WHERE {range}
               UNION ALL
               SELECT model, {archive_cost}, input_tokens, output_tokens,
                      cache_creation_5m, cache_creation_1h, cache_read_tokens,
                      total_tokens, requests
               FROM usage_archive WHERE {archive_range}
             )
             SELECT model, COALESCE(SUM(cost),0), COALESCE(SUM(input_tokens),0),
                    COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_creation_5m + cache_creation_1h),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(total_tokens),0),
                    COALESCE(SUM(requests),0)
             FROM combined GROUP BY model ORDER BY 2 DESC"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ModelRow {
                model: row.get(0)?,
                cost: row.get(1)?,
                input_tokens: row.get(2)?,
                output_tokens: row.get(3)?,
                cache_creation_tokens: row.get(4)?,
                cache_read_tokens: row.get(5)?,
                total_tokens: row.get(6)?,
                requests: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    Ok(rows)
}

/// Per-model breakdown for one session (expandable session detail).
pub fn session_models(
    conn: &Connection,
    agent: &str,
    session_id: &str,
    mode: CostMode,
) -> Result<Vec<ModelRow>, String> {
    let cost = cost_expr(mode);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT model, COALESCE(SUM({cost}),0),
                    COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0),
                    COALESCE(SUM(cache_creation_5m + cache_creation_1h),0),
                    COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(total_tokens),0), COUNT(*)
             FROM entries WHERE agent = ?1 AND session_id = ?2
             GROUP BY model ORDER BY 2 DESC"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![agent, session_id], |row| {
            Ok(ModelRow {
                model: row.get(0)?,
                cost: row.get(1)?,
                input_tokens: row.get(2)?,
                output_tokens: row.get(3)?,
                cache_creation_tokens: row.get(4)?,
                cache_read_tokens: row.get(5)?,
                total_tokens: row.get(6)?,
                requests: row.get(7)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    Ok(rows)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub session_id: String,
    pub agent: String,
    pub project: String,
    /// Session title (first user message); empty when unavailable.
    pub title: String,
    pub first_ts: i64,
    pub last_ts: i64,
    pub cost: f64,
    pub total_tokens: i64,
    pub requests: i64,
    pub models: String,
}

pub fn sessions(
    conn: &Connection,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    mode: CostMode,
    limit: i64,
) -> Result<Vec<SessionRow>, String> {
    let cost = cost_expr(mode);
    let range = range_clause(since_ms, until_ms);
    // 会话页按模型与思考强度去重展示，空强度仍保持原模型名称。
    let mut stmt = conn
        .prepare(&format!(
            "SELECT session_id, agent, project, MAX(title), MIN(timestamp_ms), MAX(timestamp_ms),
                    COALESCE(SUM({cost}),0), COALESCE(SUM(total_tokens),0), COUNT(*),
                    GROUP_CONCAT(DISTINCT CASE
                        WHEN reasoning_effort = '' THEN model
                        ELSE model || '·' || reasoning_effort
                    END)
             FROM entries WHERE {range}
             GROUP BY agent, session_id ORDER BY MAX(timestamp_ms) DESC LIMIT ?1"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(SessionRow {
                session_id: row.get(0)?,
                agent: row.get(1)?,
                project: row.get(2)?,
                title: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                first_ts: row.get(4)?,
                last_ts: row.get(5)?,
                cost: row.get(6)?,
                total_tokens: row.get(7)?,
                requests: row.get(8)?,
                models: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    Ok(rows)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub project: String,
    pub cost: f64,
    pub total_tokens: i64,
    pub requests: i64,
    pub sessions: i64,
}

pub fn projects(
    conn: &Connection,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    mode: CostMode,
    limit: i64,
) -> Result<Vec<ProjectRow>, String> {
    let cost = cost_expr(mode);
    let range = range_clause(since_ms, until_ms);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT project, COALESCE(SUM({cost}),0), COALESCE(SUM(total_tokens),0), COUNT(*),
                    COUNT(DISTINCT agent || ':' || session_id)
             FROM entries WHERE {range} GROUP BY project ORDER BY 2 DESC LIMIT ?1"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(ProjectRow {
                project: row.get(0)?,
                cost: row.get(1)?,
                total_tokens: row.get(2)?,
                requests: row.get(3)?,
                sessions: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    Ok(rows)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub actual_end_ms: Option<i64>,
    pub is_active: bool,
    pub is_gap: bool,
    pub cost: f64,
    pub total_tokens: i64,
    pub requests: i64,
    pub models: Vec<String>,
    /// tokens per minute over the block's elapsed time (active blocks only)
    pub burn_rate_tpm: Option<f64>,
    pub burn_rate_cost_per_hour: Option<f64>,
}

/// 5-hour billing blocks, ported from ccusage blocks.rs:
/// sort by time, split when gap-from-start or gap-from-last exceeds the
/// session duration, floor block starts to the hour, insert gap blocks,
/// and mark the trailing block active when still inside its window.
pub fn blocks(
    conn: &Connection,
    since_ms: Option<i64>,
    mode: CostMode,
    session_duration_hours: f64,
) -> Result<Vec<Block>, String> {
    let cost = cost_expr(mode);
    let range = range_clause(since_ms, None);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT timestamp_ms, {cost}, total_tokens, model FROM entries
             WHERE {range} ORDER BY timestamp_ms ASC"
        ))
        .map_err(|e| e.to_string())?;
    let entries: Vec<(i64, f64, i64, String)> = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();

    let duration_ms = (session_duration_hours * MILLIS_PER_HOUR as f64) as i64;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut blocks: Vec<Block> = Vec::new();
    let mut current: Vec<&(i64, f64, i64, String)> = Vec::new();
    let mut current_start: Option<i64> = None;

    let floor_to_hour = |ts: i64| (ts / MILLIS_PER_HOUR) * MILLIS_PER_HOUR;

    let make_block = |start: i64, entries: &[&(i64, f64, i64, String)], now_ms: i64| -> Block {
        let end = start + duration_ms;
        let actual_end = entries.last().map(|e| e.0);
        let is_active = actual_end
            .map(|ae| now_ms < end && now_ms - ae < duration_ms)
            .unwrap_or(false);
        let cost_sum: f64 = entries.iter().map(|e| e.1).sum();
        let tokens: i64 = entries.iter().map(|e| e.2).sum();
        let mut models: Vec<String> = entries.iter().map(|e| e.3.clone()).collect();
        models.sort();
        models.dedup();
        let (burn_tpm, burn_cph) = if is_active {
            let elapsed_min = ((now_ms - start) as f64 / 60_000.0).max(1.0);
            (
                Some(tokens as f64 / elapsed_min),
                Some(cost_sum / (elapsed_min / 60.0)),
            )
        } else {
            (None, None)
        };
        Block {
            id: chrono::Utc
                .timestamp_millis_opt(start)
                .single()
                .map(|d| d.to_rfc3339())
                .unwrap_or_default(),
            start_ms: start,
            end_ms: end,
            actual_end_ms: actual_end,
            is_active,
            is_gap: false,
            cost: cost_sum,
            total_tokens: tokens,
            requests: entries.len() as i64,
            models,
            burn_rate_tpm: burn_tpm,
            burn_rate_cost_per_hour: burn_cph,
        }
    };

    for entry in &entries {
        match current_start {
            None => {
                current_start = Some(floor_to_hour(entry.0));
            }
            Some(start) => {
                let last_ts = current.last().map(|e| e.0).unwrap_or(start);
                let since_start = entry.0 - start;
                let since_last = entry.0 - last_ts;
                if since_start > duration_ms || since_last > duration_ms {
                    blocks.push(make_block(start, &current, now_ms));
                    if since_last > duration_ms {
                        blocks.push(Block {
                            id: chrono::Utc
                                .timestamp_millis_opt(last_ts)
                                .single()
                                .map(|d| d.to_rfc3339())
                                .unwrap_or_default(),
                            start_ms: last_ts,
                            end_ms: entry.0,
                            actual_end_ms: None,
                            is_active: false,
                            is_gap: true,
                            cost: 0.0,
                            total_tokens: 0,
                            requests: 0,
                            models: Vec::new(),
                            burn_rate_tpm: None,
                            burn_rate_cost_per_hour: None,
                        });
                    }
                    current_start = Some(floor_to_hour(entry.0));
                    current.clear();
                }
            }
        }
        current.push(entry);
    }
    if let (Some(start), false) = (current_start, current.is_empty()) {
        blocks.push(make_block(start, &current, now_ms));
    }
    blocks.reverse(); // most recent first
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analytics_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE entries (
               agent TEXT NOT NULL, session_id TEXT NOT NULL, project TEXT NOT NULL,
               title TEXT NOT NULL, timestamp_ms INTEGER NOT NULL, date_local TEXT NOT NULL,
               model TEXT NOT NULL, reasoning_effort TEXT NOT NULL,
               input_tokens INTEGER NOT NULL, output_tokens INTEGER NOT NULL,
               cache_creation_5m INTEGER NOT NULL, cache_creation_1h INTEGER NOT NULL,
               cache_read_tokens INTEGER NOT NULL, total_tokens INTEGER NOT NULL,
               cost_usd REAL, calculated_cost REAL NOT NULL
             );
             CREATE TABLE usage_archive (
               source_key TEXT NOT NULL, date_local TEXT NOT NULL, agent TEXT NOT NULL,
               model TEXT NOT NULL, input_tokens INTEGER NOT NULL,
               output_tokens INTEGER NOT NULL, cache_creation_5m INTEGER NOT NULL,
               cache_creation_1h INTEGER NOT NULL, cache_read_tokens INTEGER NOT NULL,
               total_tokens INTEGER NOT NULL, requests INTEGER NOT NULL,
               cost_auto REAL NOT NULL, cost_calculate REAL NOT NULL,
               cost_display REAL NOT NULL, archived_at_ms INTEGER NOT NULL
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO entries VALUES
             ('codex','live-session','project','title',2000,'2026-08-20','gpt-live','',
              10,20,3,4,5,42,2.0,3.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO usage_archive VALUES
             ('old-source','2026-07-01','claude-code','claude-old',
              100,200,30,40,50,420,2,7.0,8.0,6.0,1000)",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn daily_combines_live_and_archived_usage() {
        let rows = daily(&analytics_conn(), None, None, CostMode::Auto).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].date, "2026-07-01");
        assert_eq!(rows[0].total_tokens, 420);
        assert_eq!(rows[0].requests, 2);
        assert_eq!(rows[0].cost, 7.0);
        assert_eq!(rows[1].total_tokens, 42);
        assert_eq!(rows[1].cost, 2.0);
    }

    #[test]
    fn overview_preserves_cost_modes_and_counts_only_live_sessions() {
        let conn = analytics_conn();
        let auto = overview(&conn, None, None, CostMode::Auto).unwrap();
        let calculate = overview(&conn, None, None, CostMode::Calculate).unwrap();
        let display = overview(&conn, None, None, CostMode::Display).unwrap();
        assert_eq!(auto.totals.cost, 9.0);
        assert_eq!(calculate.totals.cost, 11.0);
        assert_eq!(display.totals.cost, 8.0);
        assert_eq!(auto.totals.total_tokens, 462);
        assert_eq!(auto.totals.requests, 3);
        assert_eq!(auto.totals.sessions, 1);
        assert_eq!(auto.totals.active_days, 2);
    }

    #[test]
    fn models_keeps_archived_model_breakdown() {
        let rows = models(&analytics_conn(), None, None, CostMode::Auto).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].model, "claude-old");
        assert_eq!(rows[0].requests, 2);
        assert_eq!(rows[1].model, "gpt-live");
    }
}
