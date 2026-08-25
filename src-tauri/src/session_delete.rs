//! Delete a single session's source logs on demand.
//!
//! The age-based sweep in `retention` decides *when* files go; this decides
//! *which one*, driven by a row in the sessions table. Both funnel into
//! `retention::archive_and_forget`, so a session removed here leaves the same
//! database shape as one removed by the retention policy: the daily/monthly
//! usage totals survive in `usage_archive`, and a tombstone stops a later
//! rescan from resurrecting the rows.
//!
//! Three refusals keep this from taking more than the user asked for:
//!   * unsupported agents (anything but Claude Code / Codex, whose logs are
//!     one JSONL file per session),
//!   * files that also hold *other* sessions,
//!   * files whose size or mtime drifted from what was scanned.

use std::collections::HashSet;
use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::db;
use crate::retention::{self, CandidateFile};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDeletePreview {
    pub agent: String,
    pub session_id: String,
    /// Files that will actually be removed.
    pub files: usize,
    pub bytes: u64,
    pub total_tokens: i64,
    pub total_cost: f64,
    /// Skipped because the file also contains other sessions.
    pub shared_files: usize,
    /// Skipped because the file changed since the last scan.
    pub stale_files: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDeleteResult {
    pub preview: SessionDeletePreview,
    pub archived_files: usize,
    pub deleted_files: usize,
    pub pending_files: usize,
}

struct Candidates {
    files: Vec<CandidateFile>,
    shared: usize,
    stale: usize,
}

pub fn preview(
    conn: &Connection,
    agent: &str,
    session_id: &str,
) -> Result<SessionDeletePreview, String> {
    let candidates = collect(conn, agent, session_id)?;
    Ok(build_preview(agent, session_id, &candidates))
}

pub fn delete(
    conn: &mut Connection,
    agent: &str,
    session_id: &str,
) -> Result<SessionDeleteResult, String> {
    let candidates = collect(conn, agent, session_id)?;
    let preview = build_preview(agent, session_id, &candidates);
    if candidates.files.is_empty() {
        // Nothing removable: say why rather than reporting a silent success.
        if candidates.shared > 0 {
            return Err("this session shares its log file with other sessions".to_string());
        }
        if candidates.stale > 0 {
            return Err("the log file changed since the last scan; refresh and retry".to_string());
        }
        return Err("no removable log file found for this session".to_string());
    }

    let archived_at = Utc::now().timestamp_millis();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for candidate in &candidates.files {
        retention::archive_and_forget(&tx, candidate, archived_at)?;
    }
    tx.commit().map_err(|e| e.to_string())?;

    let (deleted_files, pending_files) = retention::retry_pending_deletions(conn)?;
    Ok(SessionDeleteResult {
        archived_files: candidates.files.len(),
        preview,
        deleted_files,
        pending_files,
    })
}

/// Archive one known source file by path, for callers that already know which
/// file is about to disappear -- the in-Codex delete removes the rollout
/// itself, but TokBar still wants the cost and token totals kept.
///
/// Returns false when the path is not something TokBar scanned, which is not
/// an error: there is simply no usage to preserve.
pub fn archive_file(conn: &mut Connection, agent: &str, file_path: &str) -> Result<bool, String> {
    if !retention::is_supported(agent) {
        return Ok(false);
    }
    let row = conn
        .query_row(
            "SELECT MAX(e.timestamp_ms), sf.mtime_ms, sf.size,
                    COALESCE(SUM(e.total_tokens),0),
                    COALESCE(SUM(COALESCE(e.cost_usd, e.calculated_cost)),0)
             FROM entries e
             JOIN scanned_files sf ON sf.path = e.file_path
             WHERE e.file_path = ?1
             GROUP BY sf.mtime_ms, sf.size",
            params![file_path],
            |row| {
                Ok(CandidateFile {
                    path: file_path.to_string(),
                    agent: agent.to_string(),
                    max_ts: row.get(0)?,
                    mtime_ms: row.get(1)?,
                    size: row.get(2)?,
                    total_tokens: row.get(3)?,
                    total_cost: row.get(4)?,
                    sessions: HashSet::new(),
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(candidate) = row else {
        return Ok(false);
    };

    let archived_at = Utc::now().timestamp_millis();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    retention::archive_and_forget(&tx, &candidate, archived_at)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(true)
}

/// Exact inverse of `archive_file`, for undoing an in-Codex delete: drop the
/// cold rows and the tombstone so the restored file is ingested normally again
/// on the next scan, instead of being suppressed and double-counted.
pub fn unarchive_file(conn: &Connection, agent: &str, file_path: &str) -> Result<(), String> {
    let key = db::source_key(agent, file_path);
    for sql in [
        "DELETE FROM usage_archive WHERE source_key = ?1",
        "DELETE FROM retention_tombstones WHERE source_key = ?1",
        "DELETE FROM retention_pending_files WHERE source_key = ?1",
    ] {
        conn.execute(sql, params![key]).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn build_preview(
    agent: &str,
    session_id: &str,
    candidates: &Candidates,
) -> SessionDeletePreview {
    SessionDeletePreview {
        agent: agent.to_string(),
        session_id: session_id.to_string(),
        files: candidates.files.len(),
        bytes: candidates.files.iter().map(|c| c.size.max(0) as u64).sum(),
        total_tokens: candidates.files.iter().map(|c| c.total_tokens).sum(),
        total_cost: candidates.files.iter().map(|c| c.total_cost).sum(),
        shared_files: candidates.shared,
        stale_files: candidates.stale,
    }
}

fn collect(conn: &Connection, agent: &str, session_id: &str) -> Result<Candidates, String> {
    if session_id.trim().is_empty() {
        return Err("session id is required".to_string());
    }
    if !retention::is_supported(agent) {
        return Err(format!(
            "deleting single sessions is only supported for Claude Code and Codex, not {agent}"
        ));
    }

    // Totals are per file, not per session: a file that qualifies is removed
    // whole, so its whole usage is what gets archived.
    let mut stmt = conn
        .prepare(
            "SELECT e.file_path, MAX(e.timestamp_ms), sf.mtime_ms, sf.size,
                    COALESCE(SUM(e.total_tokens),0),
                    COALESCE(SUM(COALESCE(e.cost_usd, e.calculated_cost)),0)
             FROM entries e
             JOIN scanned_files sf ON sf.path = e.file_path
             WHERE e.file_path IN (
               SELECT DISTINCT file_path FROM entries WHERE agent = ?1 AND session_id = ?2
             )
             GROUP BY e.file_path, sf.mtime_ms, sf.size
             ORDER BY e.file_path",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![agent, session_id], |row| {
            Ok(CandidateFile {
                path: row.get(0)?,
                agent: agent.to_string(),
                max_ts: row.get(1)?,
                mtime_ms: row.get(2)?,
                size: row.get(3)?,
                total_tokens: row.get(4)?,
                total_cost: row.get(5)?,
                sessions: HashSet::new(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    let mut files = Vec::new();
    let mut shared = 0;
    let mut stale = 0;
    for mut candidate in rows {
        let path = Path::new(&candidate.path);
        // Same containment check the retention sweep uses: never touch a path
        // outside the agent's own data dirs, and never follow a symlink out.
        // Counted as stale because in practice it fails when the file already
        // vanished, and "refresh and retry" is the right advice for that.
        if !retention::source_path_allowed(&candidate.agent, path) {
            stale += 1;
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
        // Deleting a file that also holds other conversations would take data
        // the user never asked to remove.
        if candidate.sessions.len() != 1 || !candidate.sessions.contains(session_id) {
            shared += 1;
            continue;
        }
        let Ok(meta) = std::fs::metadata(path) else {
            stale += 1;
            continue;
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        // A file that grew since the scan may hold turns TokBar has not read;
        // archiving stale totals would under-report them.
        if mtime != candidate.mtime_ms || meta.len() as i64 != candidate.size {
            stale += 1;
            continue;
        }
        files.push(candidate);
    }
    Ok(Candidates {
        files,
        shared,
        stale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    /// Build a Claude-style log under `root` and register it in a fresh DB.
    fn seed(
        root: &Path,
        session_id: &str,
        file_name: &str,
        extra_session: Option<&str>,
    ) -> (Connection, std::path::PathBuf) {
        let projects = root.join("projects").join("sample");
        std::fs::create_dir_all(&projects).unwrap();
        let source = projects.join(file_name);
        std::fs::write(&source, "session content").unwrap();
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

        let conn = db::open(&root.join("tokbar-test.db")).unwrap();
        let path = source.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO scanned_files(path,agent,mtime_ms,size) VALUES(?1,'claude-code',?2,?3)",
            params![path, mtime_ms, meta.len() as i64],
        )
        .unwrap();
        let insert = |key: &str, session: &str| {
            conn.execute(
                "INSERT INTO entries(
                   dedup_key,file_path,agent,project,session_id,timestamp_ms,date_local,
                   model,reasoning_effort,title,input_tokens,output_tokens,
                   cache_creation_5m,cache_creation_1h,cache_read_tokens,total_tokens,
                   cost_usd,calculated_cost
                 ) VALUES(?1,?2,'claude-code','sample',?3,1,'2025-01-01','claude-test','','t',
                   10,20,3,4,5,42,2.0,3.0)",
                params![key, path, session],
            )
            .unwrap();
        };
        insert("key-a", session_id);
        if let Some(other) = extra_session {
            insert("key-b", other);
        }
        (conn, source)
    }

    /// Serialize against the retention tests: both repoint `CLAUDE_CONFIG_DIR`.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::retention::ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn temp_root(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tokbar-session-delete-{tag}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    #[test]
    fn deletes_the_source_file_but_keeps_daily_usage() {
        let _env_guard = env_guard();
        let root = temp_root("solo");
        let (mut conn, source) = seed(&root, "session-a", "session-a.jsonl", None);
        let previous = std::env::var_os("CLAUDE_CONFIG_DIR");
        std::env::set_var("CLAUDE_CONFIG_DIR", &root);

        let result = delete(&mut conn, "claude-code", "session-a").unwrap();

        assert_eq!(result.preview.files, 1);
        assert_eq!(result.deleted_files, 1);
        assert!(!source.exists());
        let live: i64 = conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(live, 0);
        let rows = crate::aggregate::daily(&conn, None, None, crate::cost::CostMode::Auto).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total_tokens, 42);

        match previous {
            Some(value) => std::env::set_var("CLAUDE_CONFIG_DIR", value),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_a_file_shared_with_another_session() {
        let _env_guard = env_guard();
        let root = temp_root("shared");
        let (mut conn, source) = seed(&root, "session-a", "shared.jsonl", Some("session-b"));
        let previous = std::env::var_os("CLAUDE_CONFIG_DIR");
        std::env::set_var("CLAUDE_CONFIG_DIR", &root);

        let error = delete(&mut conn, "claude-code", "session-a").unwrap_err();

        assert!(error.contains("shares its log file"), "{error}");
        assert!(source.exists());
        let live: i64 = conn
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(live, 2);

        match previous {
            Some(value) => std::env::set_var("CLAUDE_CONFIG_DIR", value),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_unsupported_agents() {
        let root = temp_root("agent");
        std::fs::create_dir_all(&root).unwrap();
        let mut conn = db::open(&root.join("tokbar-test.db")).unwrap();

        let error = delete(&mut conn, "gemini", "session-a").unwrap_err();

        assert!(error.contains("only supported"), "{error}");
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }
}
