use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{Duration, Local, TimeZone, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::{adapters, db};

pub const DEFAULT_RETENTION_DAYS: i64 = 30;
pub(crate) const SUPPORTED_AGENTS: &[&str] = &[adapters::claude::AGENT, adapters::codex::AGENT];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionSourcePreview {
    pub agent: String,
    pub sessions: usize,
    pub files: usize,
    pub bytes: u64,
    pub total_tokens: i64,
    pub total_cost: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPreview {
    pub retention_days: i64,
    pub cutoff_ms: i64,
    pub sessions: usize,
    pub files: usize,
    pub bytes: u64,
    pub total_tokens: i64,
    pub total_cost: f64,
    pub skipped_sessions: usize,
    pub sources: Vec<RetentionSourcePreview>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionResult {
    pub preview: RetentionPreview,
    pub archived_files: usize,
    pub deleted_files: usize,
    pub pending_files: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CandidateFile {
    pub(crate) path: String,
    pub(crate) agent: String,
    pub(crate) max_ts: i64,
    pub(crate) mtime_ms: i64,
    pub(crate) size: i64,
    pub(crate) total_tokens: i64,
    pub(crate) total_cost: f64,
    pub(crate) sessions: HashSet<String>,
}

fn cutoff_ms(retention_days: i64) -> Result<i64, String> {
    if retention_days <= 0 {
        return Err("retention days must be positive".to_string());
    }
    let date = Local::now().date_naive() - Duration::days(retention_days);
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "invalid retention cutoff".to_string())?;
    Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|dt| dt.timestamp_millis())
        .ok_or_else(|| "could not resolve local retention cutoff".to_string())
}

pub(crate) fn is_supported(agent: &str) -> bool {
    SUPPORTED_AGENTS.contains(&agent)
}

pub(crate) fn source_path_allowed(agent: &str, path: &Path) -> bool {
    if !is_supported(agent)
        || path.extension().is_none_or(|ext| ext != "jsonl")
        || std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return false;
    }
    let Ok(canonical) = std::fs::canonicalize(path) else {
        return false;
    };
    adapters::by_agent(agent).is_some_and(|adapter| {
        (adapter.data_dirs)()
            .into_iter()
            .filter_map(|root| std::fs::canonicalize(root).ok())
            .any(|root| canonical.starts_with(root))
    })
}

fn candidate_files(conn: &Connection, cutoff: i64) -> Result<Vec<CandidateFile>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT e.file_path, e.agent, MAX(e.timestamp_ms), sf.mtime_ms, sf.size,
                    COALESCE(SUM(e.total_tokens),0),
                    COALESCE(SUM(COALESCE(e.cost_usd, e.calculated_cost)),0)
             FROM entries e
             JOIN scanned_files sf ON sf.path = e.file_path
             WHERE e.agent IN ('claude-code', 'codex')
             GROUP BY e.file_path, e.agent, sf.mtime_ms, sf.size
             HAVING MAX(e.timestamp_ms) < ?1 AND sf.mtime_ms < ?1
             ORDER BY e.agent, e.file_path",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![cutoff], |row| {
            Ok(CandidateFile {
                path: row.get(0)?,
                agent: row.get(1)?,
                max_ts: row.get(2)?,
                mtime_ms: row.get(3)?,
                size: row.get(4)?,
                total_tokens: row.get(5)?,
                total_cost: row.get(6)?,
                sessions: HashSet::new(),
            })
        })
        .map_err(|e| e.to_string())?;

    let mut candidates = Vec::new();
    for row in rows {
        let mut candidate = row.map_err(|e| e.to_string())?;
        let path = Path::new(&candidate.path);
        if !source_path_allowed(&candidate.agent, path) {
            continue;
        }
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        let actual_mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if actual_mtime != candidate.mtime_ms || meta.len() as i64 != candidate.size {
            continue;
        }
        let mut sessions_stmt = conn
            .prepare("SELECT DISTINCT session_id FROM entries WHERE file_path = ?1")
            .map_err(|e| e.to_string())?;
        candidate.sessions = sessions_stmt
            .query_map(params![candidate.path], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();
        let mut whole_sessions_are_old = true;
        for session in &candidate.sessions {
            let last_ts: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(timestamp_ms),0) FROM entries
                     WHERE agent = ?1 AND session_id = ?2",
                    params![candidate.agent, session],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if last_ts >= cutoff {
                whole_sessions_are_old = false;
                break;
            }
        }
        if !whole_sessions_are_old {
            continue;
        }
        candidates.push(candidate);
    }
    Ok(candidates)
}

fn build_preview(
    conn: &Connection,
    retention_days: i64,
    cutoff: i64,
    candidates: &[CandidateFile],
) -> Result<RetentionPreview, String> {
    let all_old_sessions: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (
               SELECT agent, session_id FROM entries
               GROUP BY agent, session_id HAVING MAX(timestamp_ms) < ?1
             )",
            params![cutoff],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let mut unique_sessions: HashSet<String> = HashSet::new();
    let mut by_agent: HashMap<String, RetentionSourcePreview> = HashMap::new();
    for candidate in candidates {
        for session in &candidate.sessions {
            unique_sessions.insert(format!("{}\0{session}", candidate.agent));
        }
        let source =
            by_agent
                .entry(candidate.agent.clone())
                .or_insert_with(|| RetentionSourcePreview {
                    agent: candidate.agent.clone(),
                    sessions: 0,
                    files: 0,
                    bytes: 0,
                    total_tokens: 0,
                    total_cost: 0.0,
                });
        source.files += 1;
        source.bytes += candidate.size.max(0) as u64;
        source.total_tokens += candidate.total_tokens;
        source.total_cost += candidate.total_cost;
    }
    for source in by_agent.values_mut() {
        source.sessions = unique_sessions
            .iter()
            .filter(|key| key.starts_with(&format!("{}\0", source.agent)))
            .count();
    }
    let mut sources: Vec<_> = by_agent.into_values().collect();
    sources.sort_by(|a, b| a.agent.cmp(&b.agent));
    Ok(RetentionPreview {
        retention_days,
        cutoff_ms: cutoff,
        sessions: unique_sessions.len(),
        files: candidates.len(),
        bytes: candidates.iter().map(|c| c.size.max(0) as u64).sum(),
        total_tokens: candidates.iter().map(|c| c.total_tokens).sum(),
        total_cost: candidates.iter().map(|c| c.total_cost).sum(),
        skipped_sessions: (all_old_sessions - unique_sessions.len() as i64).max(0) as usize,
        sources,
    })
}

/// Fold one source file's usage into the cold archive, tombstone it so a
/// later rescan cannot resurrect the rows, queue the file for deletion, and
/// drop the live rows. Shared by age-based cleanup and single-session
/// deletion so both leave the database in exactly the same shape.
pub(crate) fn archive_and_forget(
    tx: &rusqlite::Transaction<'_>,
    candidate: &CandidateFile,
    archived_at: i64,
) -> Result<(), String> {
    let key = db::source_key(&candidate.agent, &candidate.path);
    tx.execute(
        "INSERT INTO usage_archive (
           source_key, date_local, agent, model,
           input_tokens, output_tokens, cache_creation_5m, cache_creation_1h,
           cache_read_tokens, total_tokens, requests,
           cost_auto, cost_calculate, cost_display, archived_at_ms
         )
         SELECT ?1, date_local, agent, model,
                SUM(input_tokens), SUM(output_tokens), SUM(cache_creation_5m),
                SUM(cache_creation_1h), SUM(cache_read_tokens), SUM(total_tokens), COUNT(*),
                SUM(COALESCE(cost_usd, calculated_cost)), SUM(calculated_cost),
                SUM(COALESCE(cost_usd, 0)), ?2
         FROM entries WHERE file_path = ?3
         GROUP BY date_local, agent, model
         ON CONFLICT(source_key, date_local, agent, model) DO UPDATE SET
           input_tokens = usage_archive.input_tokens + excluded.input_tokens,
           output_tokens = usage_archive.output_tokens + excluded.output_tokens,
           cache_creation_5m = usage_archive.cache_creation_5m + excluded.cache_creation_5m,
           cache_creation_1h = usage_archive.cache_creation_1h + excluded.cache_creation_1h,
           cache_read_tokens = usage_archive.cache_read_tokens + excluded.cache_read_tokens,
           total_tokens = usage_archive.total_tokens + excluded.total_tokens,
           requests = usage_archive.requests + excluded.requests,
           cost_auto = usage_archive.cost_auto + excluded.cost_auto,
           cost_calculate = usage_archive.cost_calculate + excluded.cost_calculate,
           cost_display = usage_archive.cost_display + excluded.cost_display,
           archived_at_ms = excluded.archived_at_ms",
        params![key, archived_at, candidate.path],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO retention_tombstones
           (source_key, agent, archived_through_ms, purged_at_ms)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(source_key) DO UPDATE SET
           archived_through_ms = MAX(archived_through_ms, excluded.archived_through_ms),
           purged_at_ms = excluded.purged_at_ms",
        params![key, candidate.agent, candidate.max_ts, archived_at],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO retention_pending_files
           (source_key, agent, original_path, original_mtime_ms, original_size, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(source_key) DO UPDATE SET
           original_path = excluded.original_path,
           original_mtime_ms = excluded.original_mtime_ms,
           original_size = excluded.original_size,
           created_at_ms = excluded.created_at_ms,
           last_error = ''",
        params![
            key,
            candidate.agent,
            candidate.path,
            candidate.mtime_ms,
            candidate.size,
            archived_at
        ],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM entries WHERE file_path = ?1",
        params![candidate.path],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "DELETE FROM scanned_files WHERE path = ?1",
        params![candidate.path],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn preview(conn: &Connection, retention_days: i64) -> Result<RetentionPreview, String> {
    let cutoff = cutoff_ms(retention_days)?;
    let candidates = candidate_files(conn, cutoff)?;
    build_preview(conn, retention_days, cutoff, &candidates)
}

pub fn cleanup(conn: &mut Connection, retention_days: i64) -> Result<RetentionResult, String> {
    let cutoff = cutoff_ms(retention_days)?;
    let candidates = candidate_files(conn, cutoff)?;
    let preview = build_preview(conn, retention_days, cutoff, &candidates)?;
    let archived_at = Utc::now().timestamp_millis();

    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for candidate in &candidates {
        archive_and_forget(&tx, candidate, archived_at)?;
    }
    tx.commit().map_err(|e| e.to_string())?;

    let (deleted_files, pending_files) = retry_pending_deletions(conn)?;
    Ok(RetentionResult {
        preview,
        archived_files: candidates.len(),
        deleted_files,
        pending_files,
    })
}

/// Retry source deletion only while the file still matches the exact snapshot
/// that was archived. A changed file may contain new conversation data and is
/// deliberately left untouched; the scanner tombstone still filters old rows.
pub fn retry_pending_deletions(conn: &Connection) -> Result<(usize, usize), String> {
    let mut stmt = conn
        .prepare(
            "SELECT source_key, agent, original_path, original_mtime_ms, original_size
             FROM retention_pending_files ORDER BY created_at_ms",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(String, String, String, i64, i64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    drop(stmt);

    let mut deleted = 0;
    for (key, agent, path, expected_mtime, expected_size) in rows {
        let source = Path::new(&path);
        let outcome = if !source.exists() {
            Ok(())
        } else if !source_path_allowed(&agent, source) {
            Err("source path is outside the supported data directory".to_string())
        } else {
            match std::fs::metadata(source) {
                Err(error) => Err(error.to_string()),
                Ok(meta) => {
                    let mtime = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    if mtime != expected_mtime || meta.len() as i64 != expected_size {
                        Err("source changed after it was archived; not deleted".to_string())
                    } else {
                        std::fs::remove_file(source).map_err(|e| e.to_string())
                    }
                }
            }
        };
        match outcome {
            Ok(()) => {
                conn.execute(
                    "DELETE FROM retention_pending_files WHERE source_key = ?1",
                    params![key],
                )
                .map_err(|e| e.to_string())?;
                deleted += 1;
            }
            Err(error) => {
                conn.execute(
                    "UPDATE retention_pending_files SET last_error = ?2 WHERE source_key = ?1",
                    params![key, error],
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    let pending: i64 = conn
        .query_row("SELECT COUNT(*) FROM retention_pending_files", [], |row| {
            row.get(0)
        })
        .map_err(|e| e.to_string())?;
    Ok((deleted, pending.max(0) as usize))
}

/// Tests in several modules repoint `CLAUDE_CONFIG_DIR` to a temp tree. The
/// variable is process-wide, so they must not overlap.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_deletes_old_claude_file_and_keeps_daily_usage() {
        let _env_guard = ENV_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "tokbar-retention-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let projects = root.join("projects").join("sample");
        std::fs::create_dir_all(&projects).unwrap();
        let source = projects.join("old-session.jsonl");
        std::fs::write(&source, "old session content").unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&source)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
            )
            .unwrap();
        let meta = std::fs::metadata(&source).unwrap();
        let mtime_ms = meta
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let previous = std::env::var_os("CLAUDE_CONFIG_DIR");
        std::env::set_var("CLAUDE_CONFIG_DIR", &root);
        let db_path = root.join("tokbar-test.db");
        let mut conn = db::open(&db_path).unwrap();
        let path = source.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO scanned_files(path,agent,mtime_ms,size) VALUES(?1,'claude-code',?2,?3)",
            params![path, mtime_ms, meta.len() as i64],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO entries(
               dedup_key,file_path,agent,project,session_id,timestamp_ms,date_local,
               model,reasoning_effort,title,input_tokens,output_tokens,
               cache_creation_5m,cache_creation_1h,cache_read_tokens,total_tokens,
               cost_usd,calculated_cost
             ) VALUES('old-key',?1,'claude-code','private-project','old-session',1,
               '2025-01-01','claude-test','','private title',10,20,3,4,5,42,2.0,3.0)",
            params![path],
        )
        .unwrap();

        let result = cleanup(&mut conn, DEFAULT_RETENTION_DAYS).unwrap();
        assert_eq!(result.preview.sessions, 1);
        assert_eq!(result.deleted_files, 1);
        assert!(!source.exists());
        let rows = crate::aggregate::daily(&conn, None, None, crate::cost::CostMode::Auto).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total_tokens, 42);
        assert_eq!(rows[0].cost, 2.0);
        let live_entries: i64 = conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(live_entries, 0);

        match previous {
            Some(value) => std::env::set_var("CLAUDE_CONFIG_DIR", value),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }
}
