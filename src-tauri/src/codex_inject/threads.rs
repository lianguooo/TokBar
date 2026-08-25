//! Delete a conversation from Codex's own thread database.
//!
//! The durable conversation is backed by the `threads` table under
//! `$CODEX_HOME/sqlite/*.db` (older builds: `state_5.sqlite`), *not* by the
//! JSONL rollout that TokBar parses. Newer desktop builds also mirror visible
//! rows into `local_thread_catalog`. Removing the rollout and `threads` row
//! while leaving that catalog entry produces a sidebar ghost that fails with
//! "no rollout found", so all three stores are handled together here.
//!
//! Ported from CodexPlusPlus `storage.rs`, trimmed to the delete path. Every
//! delete writes an undo bundle first: the removed rows as JSON, and the
//! rollout file moved aside rather than base64-embedded (no extra dependency,
//! and it survives large transcripts).

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::types::{ToSqlOutput, Value as SqlValue, ValueRef};
use rusqlite::{Connection, OpenFlags, ToSql};
use serde::Serialize;
use serde_json::{json, Map, Value};

/// Child tables cascaded along with the thread. Deliberately an allowlist and
/// not "every table with a thread_id column": missing rows in an unknown table
/// are harmless, deleting from one we do not understand is not. Tables absent
/// from this Codex build are skipped.
const RELATED_TABLES: &[(&str, &str)] = &[
    ("thread_dynamic_tools", "thread_id = ?1"),
    ("thread_goals", "thread_id = ?1"),
    (
        "thread_spawn_edges",
        "parent_thread_id = ?1 OR child_thread_id = ?1",
    ),
    ("stage1_outputs", "thread_id = ?1"),
    ("agent_job_items", "assigned_thread_id = ?1"),
];

const BACKUP_DIR: &str = "codex-thread-backups";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDeleteOutcome {
    pub thread_id: String,
    /// Rollout JSONL this thread pointed at, when the schema records one.
    /// TokBar archives its usage before the file goes away.
    pub rollout_path: Option<String>,
    /// Catalog-only ghosts have no conversation data to restore, so cleaning
    /// one up intentionally has no undo action.
    pub undo_token: Option<String>,
}

/// SQLite files under a Codex home. Table-specific discovery filters this
/// bounded list rather than assuming every Codex build uses the same name.
fn sqlite_db_paths(codex_home: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(codex_home.join("sqlite"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("db") | Some("sqlite") | Some("sqlite3")
                )
        })
        .collect();
    paths.sort();
    let legacy = codex_home.join("state_5.sqlite");
    if !paths.contains(&legacy) && has_table_at(&legacy, "threads") {
        paths.push(legacy);
    }
    paths
}

/// Session databases under a Codex home, newest schema first.
pub fn session_db_paths(codex_home: &Path) -> Vec<PathBuf> {
    sqlite_db_paths(codex_home)
        .into_iter()
        .filter(|path| has_table_at(path, "threads"))
        .collect()
}

/// Desktop catalog databases. Codex currently calls this `codex-dev.db`, but
/// table discovery keeps the integration compatible with renamed builds.
fn catalog_db_paths(codex_home: &Path) -> Vec<PathBuf> {
    sqlite_db_paths(codex_home)
        .into_iter()
        .filter(|path| has_table_at(path, "local_thread_catalog"))
        .collect()
}

/// The sidebar exposes ids as `local:<uuid>` while the database stores the
/// bare uuid. Missing this makes every delete fail with "not found".
fn normalize_thread_id(thread_id: &str) -> &str {
    thread_id
        .trim()
        .strip_prefix("local:")
        .unwrap_or_else(|| thread_id.trim())
}

/// Every database holding this thread, with the rollout it points at.
///
/// Deliberately not "the first match": a machine can carry both
/// `~/.codex/state_5.sqlite` and a stale `~/.codex/sqlite/state_5.sqlite`
/// containing the same thread ids. Deleting from only one leaves the
/// conversation in Codex while reporting success.
pub fn find_thread_all(codex_home: &Path, thread_id: &str) -> Vec<(PathBuf, Option<String>)> {
    let thread_id = normalize_thread_id(thread_id);
    session_db_paths(codex_home)
        .into_iter()
        .filter_map(|db_path| {
            let db = open_read_only(&db_path).ok()?;
            let rollout: Option<String> = db
                .query_row(
                    &format!(
                        "SELECT {} FROM threads WHERE id = ?1",
                        rollout_column_expr(&db)
                    ),
                    [thread_id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .ok()?;
            Some((db_path, rollout.filter(|path| !path.trim().is_empty())))
        })
        .collect()
}

/// First database holding the thread, for callers that only need the rollout.
pub fn find_thread(codex_home: &Path, thread_id: &str) -> Option<(PathBuf, Option<String>)> {
    let matches = find_thread_all(codex_home, thread_id);
    // Prefer a match that actually records a rollout: the stale database may
    // have an empty column while the live one points at a real file.
    matches
        .iter()
        .find(|(_, rollout)| rollout.is_some())
        .cloned()
        .or_else(|| matches.into_iter().next())
}

/// Every desktop catalog that still exposes this local conversation.
fn find_catalog_all(codex_home: &Path, thread_id: &str) -> Vec<PathBuf> {
    let thread_id = normalize_thread_id(thread_id);
    catalog_db_paths(codex_home)
        .into_iter()
        .filter(|db_path| {
            open_read_only(db_path).is_ok_and(|db| {
                db.query_row(
                    "SELECT EXISTS(
                       SELECT 1 FROM local_thread_catalog
                       WHERE host_id = 'local' AND thread_id = ?1
                     )",
                    [thread_id],
                    |row| row.get::<_, i64>(0),
                )
                .is_ok_and(|found| found != 0)
            })
        })
        .collect()
}

/// Remove a thread, its cascaded rows and its rollout file, leaving an undo
/// bundle behind. Rolls the database back if anything fails mid-way.
pub fn delete_thread(
    codex_home: &Path,
    store_dir: &Path,
    thread_id: &str,
) -> Result<ThreadDeleteOutcome, String> {
    let thread_id = normalize_thread_id(thread_id);
    if thread_id.is_empty() {
        return Err("thread id is required".to_string());
    }
    let matches = find_thread_all(codex_home, thread_id);
    let catalog_matches = find_catalog_all(codex_home, thread_id);
    if matches.is_empty() && catalog_matches.is_empty() {
        return Err("conversation not found in Codex storage".to_string());
    }
    let rollout_path = matches.iter().find_map(|(_, rollout)| rollout.clone());

    // Collect every database's rows before touching any of them, so a failure
    // partway through still leaves a complete undo bundle.
    let mut databases = Vec::new();
    for (db_path, _) in &matches {
        let db = Connection::open(db_path)
            .map_err(|e| format!("failed to open {}: {e}", db_path.display()))?;
        let mut tables = Map::new();
        let thread_rows = select_rows(&db, "SELECT * FROM threads WHERE id = ?1", thread_id)?;
        if thread_rows.is_empty() {
            continue;
        }
        tables.insert("threads".to_string(), Value::Array(thread_rows));
        for (table, where_clause) in RELATED_TABLES {
            if !has_table(&db, table)? {
                continue;
            }
            let rows = select_rows(
                &db,
                &format!("SELECT * FROM \"{table}\" WHERE {where_clause}"),
                thread_id,
            )?;
            tables.insert((*table).to_string(), Value::Array(rows));
        }
        databases.push(json!({
            "dbPath": db_path.to_string_lossy(),
            "tables": Value::Object(tables),
        }));
    }
    for db_path in &catalog_matches {
        let db = Connection::open(db_path)
            .map_err(|e| format!("failed to open {}: {e}", db_path.display()))?;
        let rows = select_rows(
            &db,
            "SELECT * FROM local_thread_catalog
             WHERE host_id = 'local' AND thread_id = ?1",
            thread_id,
        )?;
        if rows.is_empty() {
            continue;
        }
        let mut tables = Map::new();
        tables.insert("local_thread_catalog".to_string(), Value::Array(rows));
        databases.push(json!({
            "dbPath": db_path.to_string_lossy(),
            "tables": Value::Object(tables),
        }));
    }
    if databases.is_empty() {
        return Err("conversation not found in Codex storage".to_string());
    }

    // A catalog-only match is an already-broken ghost: there is no rollout or
    // durable thread left to restore, so cleanup succeeds without offering an
    // undo that would merely recreate the dead sidebar row.
    let has_conversation = !matches.is_empty();
    let (undo_token, bundle_path, rollout_backup) = if has_conversation {
        // The undo bundle is written before anything is removed, so a failure
        // between here and the commit still leaves a recoverable copy.
        let token = new_token();
        let backup_dir = store_dir.join(BACKUP_DIR);
        fs::create_dir_all(&backup_dir)
            .map_err(|e| format!("failed to create {}: {e}", backup_dir.display()))?;
        let rollout_backup = rollout_path
            .as_deref()
            .filter(|path| Path::new(path).is_file())
            .map(|source| {
                (
                    backup_dir.join(format!("{token}.rollout.jsonl")),
                    PathBuf::from(source),
                )
            });
        if let Some((backup, source)) = rollout_backup.as_ref() {
            copy_file(source, backup)?;
        }
        let bundle = json!({
            "token": token,
            "threadId": thread_id,
            "rolloutPath": rollout_path,
            "rolloutBackup": rollout_backup.as_ref().map(|(b, _)| b.to_string_lossy()),
            "databases": Value::Array(databases.clone()),
        });
        let bundle_path = backup_dir.join(format!("{token}.json"));
        fs::write(
            &bundle_path,
            serde_json::to_vec_pretty(&bundle).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("failed to write {}: {e}", bundle_path.display()))?;
        (Some(token), Some(bundle_path), rollout_backup)
    } else {
        (None, None, None)
    };

    let mut failures = Vec::new();
    for entry in &databases {
        let db_path = entry
            .get("dbPath")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let tables = entry.get("tables").and_then(Value::as_object);
        let result = if tables.is_some_and(|tables| tables.contains_key("threads")) {
            delete_from_db(Path::new(db_path), thread_id)
        } else if tables.is_some_and(|tables| tables.contains_key("local_thread_catalog")) {
            delete_catalog_from_db(Path::new(db_path), thread_id)
        } else {
            continue;
        };
        match result {
            Ok(()) => {}
            Err(error) => failures.push(format!("{db_path}: {error}")),
        }
    }
    if !failures.is_empty() {
        let rollback = restore_database_entries(&databases).err();
        if let Some(path) = bundle_path.as_ref() {
            let _ = fs::remove_file(path);
        }
        if let Some((backup, _)) = rollout_backup.as_ref() {
            let _ = fs::remove_file(backup);
        }
        let mut error = failures.join("; ");
        if let Some(rollback) = rollback {
            error.push_str(&format!("; rollback failed: {rollback}"));
        }
        return Err(error);
    }

    // Only now is the rollout removed: with the rows gone the conversation is
    // already out of the sidebar, and the copy is safely aside.
    if let Some(path) = rollout_path.as_deref() {
        if let Err(error) = fs::remove_file(path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                let rollback = restore_database_entries(&databases).err();
                if let Some(bundle) = bundle_path.as_ref() {
                    let _ = fs::remove_file(bundle);
                }
                if let Some((backup, _)) = rollout_backup.as_ref() {
                    let _ = fs::remove_file(backup);
                }
                return Err(match rollback {
                    Some(rollback) => {
                        format!("failed to remove rollout: {error}; rollback failed: {rollback}")
                    }
                    None => format!("failed to remove rollout: {error}"),
                });
            }
        }
    }
    Ok(ThreadDeleteOutcome {
        thread_id: thread_id.to_string(),
        rollout_path,
        undo_token,
    })
}

fn delete_from_db(db_path: &Path, thread_id: &str) -> Result<(), String> {
    let mut db = Connection::open(db_path)
        .map_err(|e| format!("failed to open {}: {e}", db_path.display()))?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    for (table, where_clause) in RELATED_TABLES {
        if !has_table(&tx, table)? {
            continue;
        }
        tx.execute(
            &format!("DELETE FROM \"{table}\" WHERE {where_clause}"),
            [thread_id],
        )
        .map_err(|e| format!("failed to clear {table}: {e}"))?;
    }
    tx.execute("DELETE FROM threads WHERE id = ?1", [thread_id])
        .map_err(|e| format!("failed to delete thread: {e}"))?;
    tx.commit().map_err(|e| e.to_string())
}

/// Mirror Codex desktop's `applyAuthoritativeRemoval`: remove the visible
/// local catalog row and advance its revision so a restarted renderer cannot
/// resurrect a conversation whose rollout has already gone.
fn delete_catalog_from_db(db_path: &Path, thread_id: &str) -> Result<(), String> {
    let mut db = Connection::open(db_path)
        .map_err(|e| format!("failed to open {}: {e}", db_path.display()))?;
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let changed = tx
        .execute(
            "DELETE FROM local_thread_catalog
             WHERE host_id = 'local' AND thread_id = ?1",
            [thread_id],
        )
        .map_err(|e| format!("failed to clear local thread catalog: {e}"))?;
    if changed > 0 {
        mark_catalog_changed(&tx);
    }
    tx.commit().map_err(|e| e.to_string())
}

/// These bookkeeping tables have changed across Codex builds. Updating them
/// is useful for the current desktop renderer but deliberately best-effort;
/// the authoritative row removal must remain compatible with older schemas.
fn mark_catalog_changed(db: &Connection) {
    if has_table(db, "local_thread_catalog_sync_state").unwrap_or(false) {
        let _ = db.execute(
            "UPDATE local_thread_catalog_sync_state
             SET observation_sequence = observation_sequence + 1
             WHERE host_id = 'local'",
            [],
        );
    }
    if has_table(db, "local_thread_catalog_metadata").unwrap_or(false) {
        let _ = db.execute(
            "UPDATE local_thread_catalog_metadata
             SET catalog_revision = catalog_revision + 1 WHERE id = 1",
            [],
        );
    }
}

/// Restore the captured rows for either an explicit undo or compensation
/// after a cross-database delete fails. `INSERT OR REPLACE` makes this safe to
/// run against databases that were never reached by the failed delete.
fn restore_database_entries(databases: &[Value]) -> Result<usize, String> {
    let mut restored = 0usize;
    for entry in databases {
        let Some(db_path) = entry.get("dbPath").and_then(Value::as_str) else {
            continue;
        };
        let Some(tables) = entry.get("tables").and_then(Value::as_object) else {
            continue;
        };
        let mut db =
            Connection::open(db_path).map_err(|e| format!("failed to open {db_path}: {e}"))?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let mut restored_rows = false;
        let mut restored_catalog = false;
        for (table, rows) in tables {
            if !has_table(&tx, table)? {
                continue;
            }
            for row in rows.as_array().into_iter().flatten() {
                insert_row(&tx, table, row)?;
                restored_rows = true;
                restored_catalog |= table == "local_thread_catalog";
            }
        }
        if restored_catalog {
            mark_catalog_changed(&tx);
        }
        tx.commit().map_err(|e| e.to_string())?;
        restored += usize::from(restored_rows);
    }
    Ok(restored)
}

/// Put a deleted conversation back from its undo bundle.
pub fn undo(store_dir: &Path, token: &str) -> Result<String, String> {
    let backup_dir = store_dir.join(BACKUP_DIR);
    let bundle_path = backup_dir.join(format!("{}.json", sanitize_token(token)));
    let bundle: Value = serde_json::from_slice(
        &fs::read(&bundle_path).map_err(|_| format!("undo bundle not found: {token}"))?,
    )
    .map_err(|e| format!("failed to parse undo bundle: {e}"))?;

    let thread_id = bundle
        .get("threadId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let databases = bundle
        .get("databases")
        .and_then(Value::as_array)
        .ok_or("undo bundle has no databases")?;

    let restored = restore_database_entries(databases)?;
    if restored == 0 {
        return Err("undo bundle restored nothing".to_string());
    }

    if let (Some(backup), Some(target)) = (
        bundle.get("rolloutBackup").and_then(Value::as_str),
        bundle.get("rolloutPath").and_then(Value::as_str),
    ) {
        if Path::new(backup).is_file() {
            copy_file(Path::new(backup), Path::new(target))?;
            let _ = fs::remove_file(backup);
        }
    }
    let _ = fs::remove_file(&bundle_path);
    Ok(thread_id)
}

fn open_read_only(path: &Path) -> Result<Connection, rusqlite::Error> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
}

fn has_table_at(path: &Path, table: &str) -> bool {
    open_read_only(path).is_ok_and(|db| has_table(&db, table).unwrap_or(false))
}

fn has_table(db: &Connection, table: &str) -> Result<bool, String> {
    db.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .map_err(|e| e.to_string())
}

/// Older Codex builds have no `rollout_path`; select a literal instead so the
/// lookup still works and simply reports no rollout.
fn rollout_column_expr(db: &Connection) -> &'static str {
    let has_column = db
        .prepare("SELECT * FROM threads LIMIT 0")
        .map(|stmt| {
            stmt.column_names()
                .iter()
                .any(|name| *name == "rollout_path")
        })
        .unwrap_or(false);
    if has_column {
        "rollout_path"
    } else {
        "NULL"
    }
}

fn select_rows(db: &Connection, sql: &str, thread_id: &str) -> Result<Vec<Value>, String> {
    let mut stmt = db.prepare(sql).map_err(|e| e.to_string())?;
    let columns: Vec<String> = stmt.column_names().into_iter().map(String::from).collect();
    let rows = stmt
        .query_map([thread_id], |row| {
            let mut object = Map::new();
            for (index, name) in columns.iter().enumerate() {
                object.insert(name.clone(), sql_to_json(row.get_ref(index)?));
            }
            Ok(Value::Object(object))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

fn insert_row(db: &Connection, table: &str, row: &Value) -> Result<(), String> {
    let Some(object) = row.as_object() else {
        return Ok(());
    };
    if object.is_empty() {
        return Ok(());
    }
    let columns: Vec<&String> = object.keys().collect();
    let placeholders = (1..=columns.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let column_list = columns
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let values: Vec<OwnedSql> = columns
        .iter()
        .map(|name| OwnedSql(json_to_sql(&object[*name])))
        .collect();
    let params: Vec<&dyn ToSql> = values.iter().map(|value| value as &dyn ToSql).collect();
    db.execute(
        &format!("INSERT OR REPLACE INTO \"{table}\" ({column_list}) VALUES ({placeholders})"),
        params.as_slice(),
    )
    .map_err(|e| format!("failed to restore {table}: {e}"))?;
    Ok(())
}

struct OwnedSql(SqlValue);

impl ToSql for OwnedSql {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Owned(self.0.clone()))
    }
}

/// BLOBs round-trip as an array of byte values; everything else maps onto its
/// natural JSON type.
fn sql_to_json(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(v) => json!(v),
        ValueRef::Real(v) => json!(v),
        ValueRef::Text(v) => Value::String(String::from_utf8_lossy(v).to_string()),
        ValueRef::Blob(v) => json!(v),
    }
}

fn json_to_sql(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(v) => SqlValue::Integer(i64::from(*v)),
        Value::Number(v) => v
            .as_i64()
            .map(SqlValue::Integer)
            .or_else(|| v.as_f64().map(SqlValue::Real))
            .unwrap_or(SqlValue::Null),
        Value::String(v) => SqlValue::Text(v.clone()),
        Value::Array(items) => SqlValue::Blob(
            items
                .iter()
                .filter_map(|item| item.as_u64())
                .map(|byte| byte as u8)
                .collect(),
        ),
        Value::Object(_) => SqlValue::Text(value.to_string()),
    }
}

/// Copy rather than rename: the rollout and the backup dir can sit on
/// different volumes, where rename fails.
fn copy_file(source: &Path, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    fs::copy(source, target)
        .map(|_| ())
        .map_err(|e| format!("failed to copy {}: {e}", source.display()))
}

fn sanitize_token(token: &str) -> String {
    token
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

fn new_token() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    format!("{seconds}-{}", crate::codex_switch::new_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(tag: &str) -> (TempRoot, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "tokbar-threads-{tag}-{}-{}",
            std::process::id(),
            crate::codex_switch::new_id()
        ));
        let home = root.join("codex");
        let store = root.join("store");
        fs::create_dir_all(home.join("sqlite")).unwrap();
        fs::create_dir_all(home.join("sessions")).unwrap();
        fs::create_dir_all(&store).unwrap();
        (TempRoot(root), home, store)
    }

    /// Build a threads table close enough to Codex's shape to exercise the
    /// cascade and the rollout hand-off.
    fn seed_db(home: &Path, thread_id: &str, rollout: &Path) {
        seed_db_at(&home.join("sqlite").join("codex.db"), thread_id, rollout);
    }

    fn seed_db_at(db_path: &Path, thread_id: &str, rollout: &Path) {
        let db = Connection::open(db_path).unwrap();
        db.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT, rollout_path TEXT);
             CREATE TABLE thread_goals (thread_id TEXT, goal TEXT);",
        )
        .unwrap();
        db.execute(
            "INSERT INTO threads (id, title, rollout_path) VALUES (?1, 'demo', ?2)",
            rusqlite::params![thread_id, rollout.to_string_lossy()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO thread_goals (thread_id, goal) VALUES (?1, 'ship it')",
            [thread_id],
        )
        .unwrap();
    }

    fn seed_catalog(home: &Path, thread_id: &str) -> PathBuf {
        let path = home.join("sqlite").join("codex-dev.db");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE local_thread_catalog (
               host_id TEXT NOT NULL,
               thread_id TEXT NOT NULL,
               display_title TEXT NOT NULL,
               missing_candidate INTEGER NOT NULL DEFAULT 0,
               PRIMARY KEY (host_id, thread_id)
             );
             CREATE TABLE local_thread_catalog_sync_state (
               host_id TEXT PRIMARY KEY,
               observation_sequence INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE local_thread_catalog_metadata (
               id INTEGER PRIMARY KEY,
               catalog_revision INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO local_thread_catalog_sync_state VALUES ('local', 12);
             INSERT INTO local_thread_catalog_metadata VALUES (1, 7);",
        )
        .unwrap();
        db.execute(
            "INSERT INTO local_thread_catalog
             (host_id, thread_id, display_title, missing_candidate)
             VALUES ('local', ?1, 'demo', 0)",
            [thread_id],
        )
        .unwrap();
        path
    }

    #[test]
    fn deletes_thread_cascade_and_rollout() {
        let (_root, home, store) = fixture("delete");
        let rollout = home.join("sessions").join("rollout-demo.jsonl");
        fs::write(&rollout, "{\"a\":1}\n").unwrap();
        seed_db(&home, "thread-a", &rollout);

        let outcome = delete_thread(&home, &store, "thread-a").unwrap();

        assert_eq!(
            outcome.rollout_path.as_deref(),
            Some(&*rollout.to_string_lossy())
        );
        assert!(!rollout.exists());
        let db = Connection::open(home.join("sqlite").join("codex.db")).unwrap();
        let threads: i64 = db
            .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
            .unwrap();
        let goals: i64 = db
            .query_row("SELECT COUNT(*) FROM thread_goals", [], |row| row.get(0))
            .unwrap();
        assert_eq!(threads, 0);
        assert_eq!(goals, 0);
    }

    #[test]
    fn undo_restores_rows_and_rollout() {
        let (_root, home, store) = fixture("undo");
        let rollout = home.join("sessions").join("rollout-demo.jsonl");
        fs::write(&rollout, "{\"a\":1}\n").unwrap();
        seed_db(&home, "thread-a", &rollout);
        let outcome = delete_thread(&home, &store, "thread-a").unwrap();

        let restored = undo(&store, outcome.undo_token.as_deref().unwrap()).unwrap();

        assert_eq!(restored, "thread-a");
        assert!(rollout.exists());
        assert_eq!(fs::read_to_string(&rollout).unwrap(), "{\"a\":1}\n");
        let db = Connection::open(home.join("sqlite").join("codex.db")).unwrap();
        let title: String = db
            .query_row("SELECT title FROM threads WHERE id='thread-a'", [], |row| {
                row.get(0)
            })
            .unwrap();
        let goals: i64 = db
            .query_row("SELECT COUNT(*) FROM thread_goals", [], |row| row.get(0))
            .unwrap();
        assert_eq!(title, "demo");
        assert_eq!(goals, 1);
    }

    /// Codex's sidebar exposes `local:<uuid>` while the database stores the
    /// bare uuid. Found on a live install: without stripping, every delete
    /// failed with "conversation not found".
    #[test]
    fn strips_the_local_prefix_the_sidebar_uses() {
        let (_root, home, store) = fixture("prefix");
        let rollout = home.join("sessions").join("rollout-demo.jsonl");
        fs::write(&rollout, "{}\n").unwrap();
        seed_db(&home, "019e875c-a1d3-7333-91ed-c7f1607cb5b9", &rollout);

        let outcome =
            delete_thread(&home, &store, "local:019e875c-a1d3-7333-91ed-c7f1607cb5b9").unwrap();

        assert_eq!(outcome.thread_id, "019e875c-a1d3-7333-91ed-c7f1607cb5b9");
        assert!(!rollout.exists());
    }

    /// A machine can carry both `~/.codex/state_5.sqlite` and a stale copy
    /// under `~/.codex/sqlite/`, sharing thread ids. Deleting from only the
    /// first one found leaves the conversation in Codex.
    #[test]
    fn deletes_from_every_database_holding_the_thread() {
        let (_root, home, store) = fixture("multi-db");
        let rollout = home.join("sessions").join("rollout-demo.jsonl");
        fs::write(&rollout, "{}\n").unwrap();
        let stale = home.join("sqlite").join("state_5.sqlite");
        let live = home.join("state_5.sqlite");
        seed_db_at(&stale, "thread-a", &rollout);
        seed_db_at(&live, "thread-a", &rollout);

        delete_thread(&home, &store, "thread-a").unwrap();

        for db_path in [&stale, &live] {
            let db = Connection::open(db_path).unwrap();
            let remaining: i64 = db
                .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
                .unwrap();
            assert_eq!(remaining, 0, "{} still has the thread", db_path.display());
        }
    }

    #[test]
    fn undo_restores_every_database_it_deleted_from() {
        let (_root, home, store) = fixture("multi-undo");
        let rollout = home.join("sessions").join("rollout-demo.jsonl");
        fs::write(&rollout, "{}\n").unwrap();
        let stale = home.join("sqlite").join("state_5.sqlite");
        let live = home.join("state_5.sqlite");
        seed_db_at(&stale, "thread-a", &rollout);
        seed_db_at(&live, "thread-a", &rollout);
        let outcome = delete_thread(&home, &store, "thread-a").unwrap();

        undo(&store, outcome.undo_token.as_deref().unwrap()).unwrap();

        for db_path in [&stale, &live] {
            let db = Connection::open(db_path).unwrap();
            let remaining: i64 = db
                .query_row("SELECT COUNT(*) FROM threads", [], |row| row.get(0))
                .unwrap();
            assert_eq!(remaining, 1, "{} was not restored", db_path.display());
        }
        assert!(rollout.exists());
    }

    #[test]
    fn missing_thread_is_reported_not_silently_ignored() {
        let (_root, home, store) = fixture("missing");
        let rollout = home.join("sessions").join("rollout-demo.jsonl");
        fs::write(&rollout, "{}\n").unwrap();
        seed_db(&home, "thread-a", &rollout);

        let error = delete_thread(&home, &store, "thread-zzz").unwrap_err();

        assert!(error.contains("not found"), "{error}");
        assert!(rollout.exists());
    }

    #[test]
    fn deletes_and_undoes_the_desktop_catalog_entry() {
        let (_root, home, store) = fixture("catalog");
        let rollout = home.join("sessions").join("rollout-demo.jsonl");
        fs::write(&rollout, "{}\n").unwrap();
        seed_db(&home, "thread-a", &rollout);
        let catalog = seed_catalog(&home, "thread-a");

        let outcome = delete_thread(&home, &store, "local:thread-a").unwrap();

        let db = Connection::open(&catalog).unwrap();
        let remaining: i64 = db
            .query_row("SELECT COUNT(*) FROM local_thread_catalog", [], |row| {
                row.get(0)
            })
            .unwrap();
        let revision: i64 = db
            .query_row(
                "SELECT catalog_revision FROM local_thread_catalog_metadata WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(revision, 8);
        drop(db);

        undo(&store, outcome.undo_token.as_deref().unwrap()).unwrap();

        let db = Connection::open(&catalog).unwrap();
        let restored: i64 = db
            .query_row("SELECT COUNT(*) FROM local_thread_catalog", [], |row| {
                row.get(0)
            })
            .unwrap();
        let revision: i64 = db
            .query_row(
                "SELECT catalog_revision FROM local_thread_catalog_metadata WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restored, 1);
        assert_eq!(revision, 9);
        assert!(rollout.exists());
    }

    #[test]
    fn catalog_only_ghost_is_removed_without_fake_undo() {
        let (_root, home, store) = fixture("catalog-ghost");
        let catalog = seed_catalog(&home, "thread-ghost");

        let outcome = delete_thread(&home, &store, "local:thread-ghost").unwrap();

        assert!(outcome.undo_token.is_none());
        assert!(outcome.rollout_path.is_none());
        let db = Connection::open(catalog).unwrap();
        let remaining: i64 = db
            .query_row("SELECT COUNT(*) FROM local_thread_catalog", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(remaining, 0);
    }
}
