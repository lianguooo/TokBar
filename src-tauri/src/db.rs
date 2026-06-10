use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{Local, TimeZone};
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::adapters;
use crate::cost::calculate_cost;
use crate::pricing::PricingMap;
use crate::types::UsageRecord;

/// Bump when parsing/pricing semantics change so cached entries rebuild.
const SCHEMA_VERSION: i64 = 3;

pub fn open(db_path: &Path) -> Result<Connection, String> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS scanned_files (
           path TEXT PRIMARY KEY,
           agent TEXT NOT NULL,
           mtime_ms INTEGER NOT NULL,
           size INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS entries (
           id INTEGER PRIMARY KEY,
           dedup_key TEXT UNIQUE,
           file_path TEXT NOT NULL,
           agent TEXT NOT NULL,
           project TEXT NOT NULL,
           session_id TEXT NOT NULL,
           timestamp_ms INTEGER NOT NULL,
           date_local TEXT NOT NULL,
           model TEXT NOT NULL,
           input_tokens INTEGER NOT NULL,
           output_tokens INTEGER NOT NULL,
           cache_creation_5m INTEGER NOT NULL,
           cache_creation_1h INTEGER NOT NULL,
           cache_read_tokens INTEGER NOT NULL,
           total_tokens INTEGER NOT NULL,
           cost_usd REAL,
           calculated_cost REAL NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_entries_ts ON entries(timestamp_ms);
         CREATE INDEX IF NOT EXISTS idx_entries_date ON entries(date_local);
         CREATE INDEX IF NOT EXISTS idx_entries_file ON entries(file_path);
         CREATE INDEX IF NOT EXISTS idx_entries_session ON entries(agent, session_id);",
    )
    .map_err(|e| e.to_string())?;

    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap_or(0);
    if version != SCHEMA_VERSION {
        conn.execute_batch(&format!(
            "DELETE FROM entries; DELETE FROM scanned_files; PRAGMA user_version = {SCHEMA_VERSION};"
        ))
        .map_err(|e| e.to_string())?;
    }
    Ok(conn)
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStats {
    pub files_total: usize,
    pub files_parsed: usize,
    pub files_removed: usize,
    pub entries_inserted: usize,
    pub duration_ms: u128,
}

/// Incremental scan: re-parse only files whose mtime/size changed,
/// drop entries for deleted files, dedup via UNIQUE(dedup_key) with
/// "more tokens wins" conflict resolution (ccusage replace strategy).
/// `on_progress(done, total)` is invoked per parsed file.
pub fn scan_all(
    conn: &mut Connection,
    pricing: &PricingMap,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<ScanStats, String> {
    let started = std::time::Instant::now();
    let mut stats = ScanStats::default();

    let sources: Vec<(&str, Vec<PathBuf>)> = adapters::ALL
        .iter()
        .map(|a| (a.agent, (a.collect_files)()))
        .collect();

    // Pass 1: handle deleted files and find changed/new files.
    let mut changed: Vec<(&str, PathBuf, i64, i64)> = Vec::new();
    for (agent, files) in &sources {
        stats.files_total += files.len();

        let current: std::collections::HashSet<String> = files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let known: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT path FROM scanned_files WHERE agent = ?1")
                .map_err(|e| e.to_string())?;
            let rows: Vec<String> = stmt
                .query_map(params![agent], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .collect();
            rows
        };
        for gone in known.iter().filter(|p| !current.contains(*p)) {
            conn.execute("DELETE FROM entries WHERE file_path = ?1", params![gone])
                .map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM scanned_files WHERE path = ?1", params![gone])
                .map_err(|e| e.to_string())?;
            stats.files_removed += 1;
        }

        for file in files {
            let path_str = file.to_string_lossy().to_string();
            let Ok(meta) = std::fs::metadata(file) else {
                continue;
            };
            let mtime_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let size = meta.len() as i64;
            let unchanged: bool = conn
                .query_row(
                    "SELECT 1 FROM scanned_files WHERE path = ?1 AND mtime_ms = ?2 AND size = ?3",
                    params![path_str, mtime_ms, size],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if !unchanged {
                changed.push((agent, file.clone(), mtime_ms, size));
            }
        }
    }

    // Pass 2: parse changed files with progress reporting.
    let total_changed = changed.len();
    for (i, (agent, file, mtime_ms, size)) in changed.iter().enumerate() {
        let path_str = file.to_string_lossy().to_string();
        let records = adapters::by_agent(agent)
            .map(|a| (a.parse_file)(file))
            .unwrap_or_default();

        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM entries WHERE file_path = ?1", params![path_str])
            .map_err(|e| e.to_string())?;
        for rec in &records {
            stats.entries_inserted += insert_record(&tx, &path_str, rec, pricing)?;
        }
        tx.execute(
            "INSERT INTO scanned_files (path, agent, mtime_ms, size) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET mtime_ms = excluded.mtime_ms, size = excluded.size",
            params![path_str, agent, mtime_ms, size],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        stats.files_parsed += 1;
        on_progress(i + 1, total_changed);
    }

    stats.duration_ms = started.elapsed().as_millis();
    Ok(stats)
}

fn insert_record(
    tx: &rusqlite::Transaction,
    file_path: &str,
    rec: &UsageRecord,
    pricing: &PricingMap,
) -> Result<usize, String> {
    let calculated = calculate_cost(rec, pricing);
    let date_local = Local
        .timestamp_millis_opt(rec.timestamp_ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let n = tx
        .execute(
            "INSERT INTO entries (
               dedup_key, file_path, agent, project, session_id, timestamp_ms, date_local,
               model, input_tokens, output_tokens, cache_creation_5m, cache_creation_1h,
               cache_read_tokens, total_tokens, cost_usd, calculated_cost
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
             ON CONFLICT(dedup_key) DO UPDATE SET
               file_path = excluded.file_path,
               input_tokens = excluded.input_tokens,
               output_tokens = excluded.output_tokens,
               cache_creation_5m = excluded.cache_creation_5m,
               cache_creation_1h = excluded.cache_creation_1h,
               cache_read_tokens = excluded.cache_read_tokens,
               total_tokens = excluded.total_tokens,
               cost_usd = excluded.cost_usd,
               calculated_cost = excluded.calculated_cost
             WHERE excluded.total_tokens > entries.total_tokens",
            params![
                rec.dedup_key,
                file_path,
                rec.agent,
                rec.project,
                rec.session_id,
                rec.timestamp_ms,
                date_local,
                rec.model,
                rec.input_tokens as i64,
                rec.output_tokens as i64,
                rec.cache_creation_5m as i64,
                rec.cache_creation_1h as i64,
                rec.cache_read_tokens as i64,
                rec.total_tokens() as i64,
                rec.cost_usd,
                calculated
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(n)
}
