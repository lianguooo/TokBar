//! Shared helpers for agent adapters.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Resolve data directories from a comma-separated env var, falling back
/// to `default_rel` under $HOME. Only existing directories are returned.
pub fn env_dirs(var: &str, default_rel: &str) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(raw) = std::env::var(var) {
        for part in raw.split([',', ':']) {
            let part = part.trim();
            if !part.is_empty() {
                dirs.push(PathBuf::from(part));
            }
        }
    }
    if dirs.is_empty() {
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(default_rel));
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs.retain(|p| p.is_dir());
    dirs
}

/// Recursively collect files whose extension matches one of `exts`.
pub fn collect_with_ext(dir: &Path, exts: &[&str], files: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_with_ext(&path, exts, files);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.contains(&e))
        {
            files.push(path);
        }
    }
}

/// File mtime as epoch milliseconds (0 when unavailable).
pub fn mtime_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Read a token count that may be an integer or a finite float.
pub fn as_u64(v: &Value) -> u64 {
    match v {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| f.is_finite() && *f > 0.0).map(|f| f.trunc() as u64))
            .unwrap_or(0),
        _ => 0,
    }
}

/// `obj[key]` as u64 via [`as_u64`] (0 when missing).
pub fn get_u64(obj: &Value, key: &str) -> u64 {
    obj.get(key).map(as_u64).unwrap_or(0)
}

/// First non-empty string among `keys`.
pub fn get_str<'a>(obj: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|k| obj.get(*k).and_then(Value::as_str))
        .map(str::trim)
        .find(|s| !s.is_empty())
}

/// Interpret an integer timestamp that may be in seconds, milliseconds,
/// microseconds, or nanoseconds (ccusage smart-unit detection).
pub fn smart_unit_ms(n: i64) -> i64 {
    if n >= 100_000_000_000_000_000 {
        n / 1_000_000
    } else if n >= 100_000_000_000_000 {
        n / 1_000
    } else if n >= 100_000_000_000 {
        n
    } else {
        n.saturating_mul(1000)
    }
}

/// Timestamp from a JSON value that is either an integer (s/ms) or an
/// RFC3339 string.
pub fn ts_from_value(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().filter(|x| *x > 0).map(smart_unit_ms),
        Value::String(s) => crate::adapters::claude::parse_timestamp_ms(s),
        _ => None,
    }
}
