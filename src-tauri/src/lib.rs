pub mod adapters;
pub mod aggregate;
pub mod codex_inject;
pub mod codex_switch;
pub mod cost;
pub mod db;
pub mod pricing;
pub mod retention;
pub mod session_delete;
pub mod types;

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
#[cfg(target_os = "macos")]
use tauri::utils::config::WindowEffectsConfig;
#[cfg(target_os = "macos")]
use tauri::utils::{WindowEffect, WindowEffectState};
use tauri_plugin_positioner::{Position, WindowExt};

use crate::cost::CostMode;
use crate::pricing::PricingMap;

/// macOS menu bar wants a monochrome template icon; Windows/Linux trays
/// render the icon as-is, so ship the colored app icon there.
#[cfg(target_os = "macos")]
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-icon.png");
#[cfg(not(target_os = "macos"))]
const TRAY_ICON: &[u8] = include_bytes!("../icons/32x32.png");

/// User settings persisted to settings.json in the app data dir.
#[derive(Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Settings {
    /// Menu bar text mode: "cost" | "tokens" | "off"
    tray_mode: String,
    /// Opt-in: edit `$CODEX_HOME/config.toml` and swap `auth.json` to switch
    /// Codex providers and accounts. Off by default -- TokBar is a read-only
    /// dashboard until the user says otherwise.
    codex_switch_enabled: bool,
    /// Opt-in: delete a single session's source log from the sessions table.
    /// Off by default; this removes files outside TokBar's own data dir.
    session_delete_enabled: bool,
    /// Opt-in, nested under `session_delete_enabled`: put the delete button
    /// inside Codex's own sidebar. Requires TokBar to launch Codex with a
    /// debug port, so it is a separate consent from the in-app delete.
    codex_inject_enabled: bool,
    /// Override for the Codex app bundle; empty means autodetect.
    codex_app_path: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tray_mode: "cost".to_string(),
            codex_switch_enabled: false,
            session_delete_enabled: false,
            codex_inject_enabled: false,
            codex_app_path: String::new(),
        }
    }
}

/// Both opt-in features, as one payload for the settings UI.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FeatureFlags {
    codex_switch_enabled: bool,
    session_delete_enabled: bool,
    /// Agents whose logs are one file per session, so a single session can be
    /// removed. Sent along so the UI does not have to duplicate the list.
    session_delete_agents: Vec<String>,
    codex_inject_enabled: bool,
    codex_app_path: String,
    /// Live supervisor state, so the settings card can report what happened.
    codex_inject_status: codex_inject::InjectStatus,
}

struct AppState {
    conn: Mutex<rusqlite::Connection>,
    pricing: Mutex<PricingMap>,
    cache_dir: PathBuf,
    settings: Mutex<Settings>,
    /// Serializes whole scans (manual refresh vs. file watcher) while
    /// leaving `conn` free for readers during the slow parse phase.
    scan_lock: Mutex<()>,
    /// Running CDP injection into Codex, when the opt-in is on.
    inject: Mutex<Option<codex_inject::Supervisor>>,
}

fn settings_path(cache_dir: &PathBuf) -> PathBuf {
    cache_dir.join("settings.json")
}

fn load_settings(cache_dir: &PathBuf) -> Settings {
    std::fs::read_to_string(settings_path(cache_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(cache_dir: &PathBuf, settings: &Settings) {
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(settings_path(cache_dir), json);
    }
}

fn format_tokens_short(n: i64) -> String {
    let n = n as f64;
    if n >= 1e9 {
        format!("{:.2}B", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1}M", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.1}K", n / 1e3)
    } else {
        format!("{n:.0}")
    }
}

fn parse_mode(mode: Option<String>) -> CostMode {
    CostMode::from_str(mode.as_deref().unwrap_or("auto"))
}

#[tauri::command]
fn refresh_data(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<db::ScanStats, String> {
    let stats = {
        let _scan_guard = state.scan_lock.lock().map_err(|e| e.to_string())?;
        let pricing = state.pricing.lock().map_err(|e| e.to_string())?;
        let progress_app = app.clone();
        db::scan_all(&state.conn, &pricing, move |done, total| {
            let _ = progress_app.emit("scan-progress", serde_json::json!({
                "done": done, "total": total
            }));
        })?
    };
    update_tray_title(&app, &state);
    Ok(stats)
}

/// Manually pull the latest LiteLLM pricing table, then re-price all
/// cached usage under the new rates. Unlike the daily background refresh
/// (which only affects newly scanned files), this clears the scan cache so
/// every file is re-parsed and re-costed. Returns the number of models in
/// the refreshed table. Surfaces the fetch error to the UI on failure,
/// instead of the background refresh's silent swallow.
#[tauri::command]
fn refresh_pricing(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<usize, String> {
    let count = PricingMap::refresh_online(&state.cache_dir)?;
    {
        let mut pricing = state.pricing.lock().map_err(|e| e.to_string())?;
        *pricing = PricingMap::load(Some(state.cache_dir.clone()));
    }
    {
        let _scan_guard = state.scan_lock.lock().map_err(|e| e.to_string())?;
        {
            let conn = state.conn.lock().map_err(|e| e.to_string())?;
            conn.execute("DELETE FROM scanned_files", [])
                .map_err(|e| e.to_string())?;
        }
        let pricing = state.pricing.lock().map_err(|e| e.to_string())?;
        let progress_app = app.clone();
        db::scan_all(&state.conn, &pricing, move |done, total| {
            let _ = progress_app.emit(
                "scan-progress",
                serde_json::json!({ "done": done, "total": total }),
            );
        })?;
    }
    update_tray_title(&app, &state);
    Ok(count)
}

/// Live session rows can be folded into `usage_archive` when their source
/// conversation is deleted. The tray must read both stores, just like the
/// dashboard aggregates do, or deleting a conversation makes today's number
/// jump backwards even though its usage was deliberately retained.
fn tray_usage_for_date(conn: &rusqlite::Connection, date: &str) -> rusqlite::Result<(f64, i64)> {
    conn.query_row(
        "SELECT COALESCE(SUM(cost),0), COALESCE(SUM(tokens),0)
         FROM (
           SELECT COALESCE(cost_usd, calculated_cost) AS cost,
                  total_tokens AS tokens
           FROM entries WHERE date_local = ?1
           UNION ALL
           SELECT cost_auto AS cost, total_tokens AS tokens
           FROM usage_archive WHERE date_local = ?1
         )",
        [date],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
}

/// Render today's usage next to the menu bar icon (macOS), according to
/// the configured tray display mode.
fn update_tray_title(app: &tauri::AppHandle, state: &tauri::State<AppState>) {
    let settings = state
        .settings
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    let Ok(conn) = state.conn.lock() else { return };
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let (cost, tokens) = tray_usage_for_date(&conn, &today).unwrap_or((0.0, 0));
    drop(conn);

    if let Some(tray) = app.tray_by_id("main") {
        // "off" passes an empty string: set_title(None) does not clear an
        // already-set title on macOS.
        let title = match settings.tray_mode.as_str() {
            "off" => String::new(),
            "tokens" => format_tokens_short(tokens),
            _ => format!("${cost:.2}"),
        };
        let _ = tray.set_title(Some(title));
        let _ = tray.set_tooltip(Some(format!(
            "TokBar — today ${cost:.2} / {}",
            format_tokens_short(tokens)
        )));
    }
}

#[tauri::command]
fn set_tray_mode(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    mode: String,
) -> Result<(), String> {
    if !matches!(mode.as_str(), "cost" | "tokens" | "off") {
        return Err(format!("invalid tray mode: {mode}"));
    }
    if let Ok(mut s) = state.settings.lock() {
        s.tray_mode = mode;
        save_settings(&state.cache_dir, &s);
    }
    update_tray_title(&app, &state);
    Ok(())
}

#[tauri::command]
fn get_tray_mode(state: tauri::State<AppState>) -> String {
    state
        .settings
        .lock()
        .map(|s| s.tray_mode.clone())
        .unwrap_or_else(|_| "cost".to_string())
}

#[tauri::command]
fn get_feature_flags(state: tauri::State<AppState>) -> FeatureFlags {
    read_flags(&state)
}

#[tauri::command]
fn set_feature_flag(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    flag: String,
    enabled: bool,
) -> Result<FeatureFlags, String> {
    {
        let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
        match flag.as_str() {
            "codexSwitch" => settings.codex_switch_enabled = enabled,
            "sessionDelete" => {
                settings.session_delete_enabled = enabled;
                // The in-Codex button is a strictly stronger permission; it
                // must not survive the parent being switched off.
                if !enabled {
                    settings.codex_inject_enabled = false;
                }
            }
            "codexInject" => settings.codex_inject_enabled = enabled,
            other => return Err(format!("unknown feature flag: {other}")),
        }
        save_settings(&state.cache_dir, &settings);
    }
    if matches!(flag.as_str(), "sessionDelete" | "codexInject") {
        // Switching the feature on is an explicit request to get Codex ready;
        // a Codex already serving CDP just gets attached to.
        let intent = if enabled {
            codex_inject::LaunchIntent::LaunchIfDown
        } else {
            codex_inject::LaunchIntent::AttachOnly
        };
        sync_codex_inject(&app, intent)?;
    }
    Ok(read_flags(&state))
}

fn read_flags(state: &tauri::State<AppState>) -> FeatureFlags {
    let agents = retention::SUPPORTED_AGENTS
        .iter()
        .map(|agent| agent.to_string())
        .collect();
    let settings = state.settings.lock().ok();
    let (codex_switch_enabled, session_delete_enabled, codex_inject_enabled, codex_app_path) =
        settings
            .as_ref()
            .map(|s| {
                (
                    s.codex_switch_enabled,
                    s.session_delete_enabled,
                    s.codex_inject_enabled,
                    s.codex_app_path.clone(),
                )
            })
            .unwrap_or((false, false, false, String::new()));
    drop(settings);
    let mut codex_inject_status = state
        .inject
        .lock()
        .ok()
        .and_then(|supervisor| supervisor.as_ref().map(|s| s.status()))
        .unwrap_or_default();
    // Not launched by us and no debug port anywhere: the UI offers a relaunch.
    if !codex_inject_status.running {
        codex_inject_status.needs_relaunch = !codex_inject::cdp::is_listening(
            codex_inject::DEFAULT_DEBUG_PORT,
        );
    }
    FeatureFlags {
        codex_switch_enabled,
        session_delete_enabled,
        session_delete_agents: agents,
        codex_inject_enabled,
        codex_app_path,
        codex_inject_status,
    }
}

/// Every opt-in command re-checks its flag here. Hiding the UI is not enough:
/// the disabled default has to hold at the boundary that touches the disk.
fn require_flag(state: &tauri::State<AppState>, flag: &str) -> Result<(), String> {
    let flags = read_flags(state);
    let enabled = match flag {
        "codexSwitch" => flags.codex_switch_enabled,
        "sessionDelete" => flags.session_delete_enabled,
        _ => false,
    };
    if enabled {
        Ok(())
    } else {
        Err(format!("feature \"{flag}\" is disabled in settings"))
    }
}

// --- Codex account / provider switch (opt-in) ---------------------------

#[tauri::command]
fn codex_switch_state(
    state: tauri::State<AppState>,
) -> Result<codex_switch::QuickSwitchState, String> {
    require_flag(&state, "codexSwitch")?;
    codex_switch::state(&state.cache_dir)
}

#[tauri::command]
fn codex_switch_select(
    state: tauri::State<AppState>,
    kind: String,
    id: String,
) -> Result<codex_switch::SwitchResult, String> {
    require_flag(&state, "codexSwitch")?;
    codex_switch::select(&state.cache_dir, &kind, &id)
}

#[tauri::command]
fn codex_switch_official(
    state: tauri::State<AppState>,
) -> Result<codex_switch::SwitchResult, String> {
    require_flag(&state, "codexSwitch")?;
    codex_switch::select_official(&state.cache_dir)
}

#[tauri::command]
fn codex_provider_create(
    state: tauri::State<AppState>,
    name: String,
    base_url: String,
    bearer_token: String,
    model: String,
) -> Result<codex_switch::SwitchResult, String> {
    require_flag(&state, "codexSwitch")?;
    codex_switch::create_provider(&state.cache_dir, &name, &base_url, &bearer_token, &model)
}

#[tauri::command]
fn codex_provider_update(
    state: tauri::State<AppState>,
    id: String,
    name: String,
    base_url: String,
    bearer_token: String,
    model: String,
) -> Result<codex_switch::SwitchResult, String> {
    require_flag(&state, "codexSwitch")?;
    codex_switch::update_provider(&state.cache_dir, &id, &name, &base_url, &bearer_token, &model)
}

#[tauri::command]
fn codex_provider_delete(
    state: tauri::State<AppState>,
    id: String,
) -> Result<codex_switch::SwitchResult, String> {
    require_flag(&state, "codexSwitch")?;
    codex_switch::delete_provider(&state.cache_dir, &id)
}

#[tauri::command]
fn codex_import_accounts(
    state: tauri::State<AppState>,
) -> Result<codex_switch::SwitchResult, String> {
    require_flag(&state, "codexSwitch")?;
    codex_switch::import_accounts(&state.cache_dir)
}

#[tauri::command]
fn codex_account_capture(
    state: tauri::State<AppState>,
    name: String,
    model: String,
) -> Result<codex_switch::SwitchResult, String> {
    require_flag(&state, "codexSwitch")?;
    codex_switch::capture_account(&state.cache_dir, &name, &model)
}

#[tauri::command]
fn codex_account_add(
    state: tauri::State<AppState>,
    name: String,
    current_account_name: String,
    model: String,
) -> Result<codex_switch::SwitchResult, String> {
    require_flag(&state, "codexSwitch")?;
    codex_switch::begin_add_account(&state.cache_dir, &name, &current_account_name, &model)
}

#[tauri::command]
fn codex_account_update(
    state: tauri::State<AppState>,
    id: String,
    name: String,
    model: String,
) -> Result<codex_switch::SwitchResult, String> {
    require_flag(&state, "codexSwitch")?;
    codex_switch::update_account(&state.cache_dir, &id, &name, &model)
}

#[tauri::command]
fn codex_account_delete(
    state: tauri::State<AppState>,
    id: String,
) -> Result<codex_switch::SwitchResult, String> {
    require_flag(&state, "codexSwitch")?;
    codex_switch::delete_account(&state.cache_dir, &id)
}

// --- Delete button inside Codex (opt-in, nested) ------------------------

/// Bring the supervisor in line with the current setting: start it when the
/// nested opt-in is on, stop it otherwise. Safe to call repeatedly.
///
/// `intent` says what may happen to Codex on the supervisor's first pass, and
/// applies once. Anything but `AttachOnly` comes from an explicit user action.
fn sync_codex_inject(
    app: &tauri::AppHandle,
    intent: codex_inject::LaunchIntent,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (wanted, app_path_override) = state
        .settings
        .lock()
        .map(|settings| {
            (
                settings.session_delete_enabled && settings.codex_inject_enabled,
                settings.codex_app_path.clone(),
            )
        })
        .map_err(|e| e.to_string())?;

    let mut slot = state.inject.lock().map_err(|e| e.to_string())?;
    if !wanted {
        if let Some(mut supervisor) = slot.take() {
            supervisor.stop();
        }
        return Ok(());
    }
    if slot.as_ref().is_some_and(|s| s.status().running) {
        return Ok(());
    }
    if let Some(mut stale) = slot.take() {
        stale.stop();
    }

    let app_path = codex_inject::resolve_app_path(&app_path_override)?;
    let handler_app = app.clone();
    let handler: codex_inject::ActionHandler = std::sync::Arc::new(move |action, payload| {
        handle_inject_action(&handler_app, action, payload)
    });
    *slot = Some(codex_inject::start(
        app_path,
        codex_inject::DEFAULT_DEBUG_PORT,
        intent,
        handler,
    ));
    Ok(())
}

/// Called at startup so an enabled injection reattaches to a Codex that is
/// already running with a debug port. Deliberately without a launch permit:
/// starting TokBar must not drag Codex open.
fn start_codex_inject(app: &tauri::AppHandle) -> Result<(), String> {
    sync_codex_inject(app, codex_inject::LaunchIntent::AttachOnly)
}

/// Handles one call from the injected script. Runs on the supervisor thread,
/// so every lock here is taken briefly and released before the next step.
fn handle_inject_action(
    app: &tauri::AppHandle,
    action: &str,
    payload: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let state = app.state::<AppState>();
    // Defence in depth: the page could still hold a stale bridge after the
    // user switched the feature off.
    require_flag(&state, "sessionDelete")?;
    let codex_home = codex_switch::codex_home();
    let store_dir = codex_switch::store_dir(&state.cache_dir);

    match action {
        "delete_thread" => {
            let thread_id = payload
                .get("threadId")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            // Resolve the rollout before deleting: afterwards the row that
            // points at it is gone. Archive TokBar usage only after Codex's
            // storage delete succeeds; otherwise a failed delete would still
            // make the session disappear from TokBar's live data.
            let rollout = codex_inject::threads::find_thread(&codex_home, thread_id)
                .and_then(|(_, rollout)| rollout);
            let outcome = codex_inject::threads::delete_thread(&codex_home, &store_dir, thread_id)?;
            if let Some(path) = rollout.as_deref() {
                let archive_result = {
                    let _scan_guard = state.scan_lock.lock().map_err(|e| e.to_string())?;
                    let mut conn = state.conn.lock().map_err(|e| e.to_string())?;
                    session_delete::archive_file(&mut conn, adapters::codex::AGENT, path)
                };
                if let Err(error) = archive_result {
                    let rollback = outcome
                        .undo_token
                        .as_deref()
                        .map(|token| codex_inject::threads::undo(&store_dir, token));
                    return Err(match rollback {
                        Some(Err(rollback)) => format!(
                            "failed to preserve usage totals: {error}; storage rollback failed: {rollback}"
                        ),
                        _ => format!("failed to preserve usage totals: {error}"),
                    });
                }
            }
            {
                let conn = state.conn.lock().map_err(|e| e.to_string())?;
                let _ = retention::retry_pending_deletions(&conn);
            }
            update_tray_title(app, &state);
            let _ = app.emit("usage-updated", ());
            Ok(serde_json::json!({
                "status": "ok",
                "message": "已删除",
                "undoToken": outcome.undo_token,
            }))
        }
        "undo_delete" => {
            let token = payload
                .get("token")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let thread_id = codex_inject::threads::undo(&store_dir, token)?;
            if let Some((_, Some(path))) =
                codex_inject::threads::find_thread(&codex_home, &thread_id)
            {
                let _scan_guard = state.scan_lock.lock().map_err(|e| e.to_string())?;
                {
                    let conn = state.conn.lock().map_err(|e| e.to_string())?;
                    // Clear the tombstone too, or the restored file would be
                    // suppressed on the next scan and never reappear in TokBar.
                    session_delete::unarchive_file(&conn, adapters::codex::AGENT, &path)?;
                }
                // Restore the live rows synchronously. Otherwise undo first
                // removes the cold totals and the tray briefly drops until a
                // filesystem watcher happens to rescan the restored rollout.
                let pricing = state.pricing.lock().map_err(|e| e.to_string())?;
                db::scan_all(&state.conn, &pricing, |_, _| {})?;
            }
            update_tray_title(app, &state);
            let _ = app.emit("usage-updated", ());
            Ok(serde_json::json!({ "status": "ok", "message": "已恢复" }))
        }
        other => Err(format!("unknown action: {other}")),
    }
}

#[tauri::command]
fn set_codex_app_path(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    path: String,
) -> Result<FeatureFlags, String> {
    {
        let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.codex_app_path = path.trim().to_string();
        save_settings(&state.cache_dir, &settings);
    }
    // Restart against the new bundle rather than leaving the old one attached.
    {
        let mut slot = state.inject.lock().map_err(|e| e.to_string())?;
        if let Some(mut supervisor) = slot.take() {
            supervisor.stop();
        }
    }
    sync_codex_inject(&app, codex_inject::LaunchIntent::AttachOnly)?;
    Ok(read_flags(&state))
}

/// Relaunch Codex with the debug port without touching any setting: the one
/// way back after quitting Codex, since nothing relaunches it on its own.
#[tauri::command]
fn codex_inject_restart(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<FeatureFlags, String> {
    require_flag(&state, "sessionDelete")?;
    {
        let mut slot = state.inject.lock().map_err(|e| e.to_string())?;
        if let Some(mut supervisor) = slot.take() {
            supervisor.stop();
        }
    }
    sync_codex_inject(&app, codex_inject::LaunchIntent::Restart)?;
    Ok(read_flags(&state))
}

// --- Single-session deletion (opt-in) -----------------------------------

#[tauri::command]
fn preview_session_delete(
    state: tauri::State<AppState>,
    agent: String,
    session_id: String,
) -> Result<session_delete::SessionDeletePreview, String> {
    require_flag(&state, "sessionDelete")?;
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    session_delete::preview(&conn, &agent, &session_id)
}

#[tauri::command]
fn delete_session(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    agent: String,
    session_id: String,
) -> Result<session_delete::SessionDeleteResult, String> {
    require_flag(&state, "sessionDelete")?;
    let result = {
        // Same ordering as cleanup_old_sessions: settle the scan first so the
        // mtime/size snapshot the delete checks against is current.
        let _scan_guard = state.scan_lock.lock().map_err(|e| e.to_string())?;
        {
            let pricing = state.pricing.lock().map_err(|e| e.to_string())?;
            db::scan_all(&state.conn, &pricing, |_, _| {})?;
        }
        let mut conn = state.conn.lock().map_err(|e| e.to_string())?;
        session_delete::delete(&mut conn, &agent, &session_id)?
    };
    update_tray_title(&app, &state);
    let _ = app.emit("usage-updated", ());
    Ok(result)
}

#[tauri::command]
fn preview_retention(
    state: tauri::State<AppState>,
) -> Result<retention::RetentionPreview, String> {
    let _scan_guard = state.scan_lock.lock().map_err(|e| e.to_string())?;
    {
        let pricing = state.pricing.lock().map_err(|e| e.to_string())?;
        db::scan_all(&state.conn, &pricing, |_, _| {})?;
    }
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    retention::preview(&conn, retention::DEFAULT_RETENTION_DAYS)
}

#[tauri::command]
fn cleanup_old_sessions(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<retention::RetentionResult, String> {
    let result = {
        let _scan_guard = state.scan_lock.lock().map_err(|e| e.to_string())?;
        {
            let pricing = state.pricing.lock().map_err(|e| e.to_string())?;
            db::scan_all(&state.conn, &pricing, |_, _| {})?;
        }
        let mut conn = state.conn.lock().map_err(|e| e.to_string())?;
        retention::cleanup(&mut conn, retention::DEFAULT_RETENTION_DAYS)?
    };
    update_tray_title(&app, &state);
    let _ = app.emit("usage-updated", ());
    Ok(result)
}

/// Watch all agent log directories and rescan automatically (debounced)
/// when anything changes, then notify every window.
fn spawn_usage_watcher(handle: tauri::AppHandle) {
    use notify::{RecursiveMode, Watcher};
    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let mut watcher = match notify::recommended_watcher(
            move |res: Result<notify::Event, notify::Error>| {
                if res.is_ok() {
                    let _ = tx.send(());
                }
            },
        ) {
            Ok(w) => w,
            Err(_) => return,
        };
        let dirs: Vec<PathBuf> = adapters::ALL
            .iter()
            .flat_map(|a| (a.data_dirs)())
            .collect();
        for dir in &dirs {
            let _ = watcher.watch(dir, RecursiveMode::Recursive);
        }
        if dirs.is_empty() {
            return;
        }
        while rx.recv().is_ok() {
            // Debounce: wait until the directories have been quiet for 2s.
            while rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .is_ok()
            {}
            scan_and_broadcast(&handle);
        }
    });
}

fn scan_and_broadcast(handle: &tauri::AppHandle) {
    let state: tauri::State<AppState> = handle.state();
    {
        let Ok(_scan_guard) = state.scan_lock.lock() else { return };
        let Ok(pricing) = state.pricing.lock() else { return };
        let progress_app = handle.clone();
        let _ = db::scan_all(&state.conn, &pricing, move |done, total| {
            let _ = progress_app.emit("scan-progress", serde_json::json!({
                "done": done, "total": total
            }));
        });
    }
    update_tray_title(handle, &state);
    let _ = handle.emit("usage-updated", ());
}

#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    if let Some(q) = app.get_webview_window("quick") {
        let _ = q.hide();
    }
}

/// Background auto-refresh of LiteLLM pricing: on launch (when the local
/// cache is older than 24h) and once a day thereafter. New rates apply to
/// newly scanned usage only — historical costs keep the rates in effect
/// when they were recorded.
/// Keep the signed-in account's archive in step with `auth.json`.
///
/// Codex rotates those tokens on its own schedule. Without this the snapshot
/// only got rewritten when the switcher UI happened to be open, so an archive
/// could sit days behind the live file and fail to restore when switched to.
///
/// Freshness only: no network call, and only the account that is actually
/// signed in. Idle accounts cannot be kept alive this way.
fn spawn_codex_auth_freshness(handle: tauri::AppHandle) {
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(120);
    std::thread::spawn(move || loop {
        std::thread::sleep(INTERVAL);
        let state: tauri::State<AppState> = handle.state();
        let enabled = state
            .settings
            .lock()
            .map(|settings| settings.codex_switch_enabled)
            .unwrap_or(false);
        if !enabled {
            continue;
        }
        codex_switch::accounts::refresh_live_archive(
            &codex_switch::codex_home(),
            &codex_switch::store_dir(&state.cache_dir),
        );
    });
}

fn spawn_pricing_auto_refresh(handle: tauri::AppHandle) {
    const DAY: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
    std::thread::spawn(move || loop {
        let state: tauri::State<AppState> = handle.state();
        let cache_file = state.cache_dir.join("litellm-pricing.json");
        let stale = std::fs::metadata(&cache_file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map_or(true, |age| age > DAY);
        if stale && PricingMap::refresh_online(&state.cache_dir).is_ok() {
            if let Ok(mut pricing) = state.pricing.lock() {
                *pricing = PricingMap::load(Some(state.cache_dir.clone()));
            }
        }
        // Re-check every 6 hours; the mtime gate makes this refresh at
        // most once a day.
        std::thread::sleep(std::time::Duration::from_secs(6 * 3600));
    });
}

#[tauri::command]
fn get_overview(
    state: tauri::State<AppState>,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    cost_mode: Option<String>,
) -> Result<aggregate::Overview, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    aggregate::overview(&conn, since_ms, until_ms, parse_mode(cost_mode))
}

#[tauri::command]
fn get_daily(
    state: tauri::State<AppState>,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    cost_mode: Option<String>,
) -> Result<Vec<aggregate::DailyRow>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    aggregate::daily(&conn, since_ms, until_ms, parse_mode(cost_mode))
}

#[tauri::command]
fn get_hourly(
    state: tauri::State<AppState>,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    cost_mode: Option<String>,
) -> Result<Vec<aggregate::DailyRow>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    aggregate::hourly(&conn, since_ms, until_ms, parse_mode(cost_mode))
}

#[tauri::command]
fn get_models(
    state: tauri::State<AppState>,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    cost_mode: Option<String>,
) -> Result<Vec<aggregate::ModelRow>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    aggregate::models(&conn, since_ms, until_ms, parse_mode(cost_mode))
}

#[tauri::command]
fn get_sessions(
    state: tauri::State<AppState>,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    cost_mode: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<aggregate::SessionRow>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    aggregate::sessions(
        &conn,
        since_ms,
        until_ms,
        parse_mode(cost_mode),
        limit.unwrap_or(200),
    )
}

#[tauri::command]
fn get_projects(
    state: tauri::State<AppState>,
    since_ms: Option<i64>,
    until_ms: Option<i64>,
    cost_mode: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<aggregate::ProjectRow>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    aggregate::projects(
        &conn,
        since_ms,
        until_ms,
        parse_mode(cost_mode),
        limit.unwrap_or(20),
    )
}

#[tauri::command]
fn get_blocks(
    state: tauri::State<AppState>,
    since_ms: Option<i64>,
    cost_mode: Option<String>,
) -> Result<Vec<aggregate::Block>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    aggregate::blocks(&conn, since_ms, parse_mode(cost_mode), 5.0)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceInfo {
    agent: String,
    dirs: Vec<String>,
    file_count: usize,
}

#[tauri::command]
fn get_sources() -> Vec<SourceInfo> {
    adapters::ALL
        .iter()
        .map(|a| SourceInfo {
            agent: a.agent.to_string(),
            dirs: (a.data_dirs)()
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            file_count: (a.collect_files)().len(),
        })
        .collect()
}

#[tauri::command]
fn get_session_models(
    state: tauri::State<AppState>,
    agent: String,
    session_id: String,
    cost_mode: Option<String>,
) -> Result<Vec<aggregate::ModelRow>, String> {
    let conn = state.conn.lock().map_err(|e| e.to_string())?;
    aggregate::session_models(&conn, &agent, &session_id, parse_mode(cost_mode))
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    // Hidden frameless popover window, shown next to the tray icon on click.
    let quick = WebviewWindowBuilder::new(app, "quick", WebviewUrl::App("index.html".into()))
        .title("TokBar")
        .inner_size(360.0, 460.0)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false);
    // Transparent window + self-drawn rounded corners is a macOS look;
    // WebView2 transparency on Windows composites poorly (artifacts show
    // through), so the window stays opaque there. The HUD-window vibrancy
    // behind the webview gives the panel its frosted-glass material (the
    // radius matches the panel div's rounded-2xl).
    #[cfg(target_os = "macos")]
    let quick = quick.transparent(true).effects(WindowEffectsConfig {
        effects: vec![WindowEffect::HudWindow],
        state: Some(WindowEffectState::Active),
        radius: Some(16.0),
        color: None,
    });
    quick.build()?;

    let show = MenuItem::with_id(app, "show", "打开 TokBar / Open TokBar", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 / Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(tauri::image::Image::from_bytes(TRAY_ICON)?)
        .icon_as_template(cfg!(target_os = "macos"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("quick") {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        // macOS menu bar is at the top, so the panel opens
                        // below the icon; Windows/Linux trays sit at the
                        // bottom, so it opens above (TrayCenter).
                        let pos = if cfg!(target_os = "macos") {
                            Position::TrayBottomCenter
                        } else {
                            Position::TrayCenter
                        };
                        let _ = w.move_window(pos);
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            let conn = db::open(&data_dir.join("tokbar.db")).expect("failed to open database");
            let _ = retention::retry_pending_deletions(&conn);
            let pricing = PricingMap::load(Some(data_dir.clone()));
            let settings = load_settings(&data_dir);
            app.manage(AppState {
                conn: Mutex::new(conn),
                pricing: Mutex::new(pricing),
                cache_dir: data_dir,
                settings: Mutex::new(settings),
                scan_lock: Mutex::new(()),
                inject: Mutex::new(None),
            });
            // Resume the injection if it was left on, so the button is there
            // without the user opening settings first.
            if let Err(error) = start_codex_inject(app.handle()) {
                eprintln!("codex inject autostart skipped: {error}");
            }
            setup_tray(app)?;
            spawn_pricing_auto_refresh(app.handle().clone());
            spawn_codex_auth_freshness(app.handle().clone());
            spawn_usage_watcher(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Menu-bar app behavior: closing the main window hides it,
            // the app keeps running in the tray (quit via tray menu).
            tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                let _ = window.hide();
            }
            // The quick popover hides itself when it loses focus.
            tauri::WindowEvent::Focused(false) if window.label() == "quick" => {
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            refresh_data,
            refresh_pricing,
            show_main_window,
            set_tray_mode,
            get_tray_mode,
            preview_retention,
            cleanup_old_sessions,
            get_session_models,
            get_overview,
            get_daily,
            get_hourly,
            get_models,
            get_sessions,
            get_projects,
            get_blocks,
            get_sources,
            get_feature_flags,
            set_feature_flag,
            codex_switch_state,
            codex_switch_select,
            codex_switch_official,
            codex_provider_create,
            codex_provider_update,
            codex_provider_delete,
            codex_import_accounts,
            codex_account_capture,
            codex_account_add,
            codex_account_update,
            codex_account_delete,
            preview_session_delete,
            delete_session,
            set_codex_app_path,
            codex_inject_restart
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the opt-in: a fresh install, and an upgrade from a
    /// settings.json written before these fields existed, must both land on
    /// "disabled" rather than inheriting a truthy default.
    #[test]
    fn opt_in_features_default_to_off() {
        let fresh = Settings::default();
        let upgraded: Settings = serde_json::from_str(r#"{"trayMode":"tokens"}"#).unwrap();

        assert!(!fresh.codex_switch_enabled);
        assert!(!fresh.session_delete_enabled);
        assert_eq!(upgraded.tray_mode, "tokens");
        assert!(!upgraded.codex_switch_enabled);
        assert!(!upgraded.session_delete_enabled);
    }

    #[test]
    fn enabled_flags_survive_a_save_load_round_trip() {
        let mut settings = Settings::default();
        settings.codex_switch_enabled = true;
        let json = serde_json::to_string(&settings).unwrap();

        let restored: Settings = serde_json::from_str(&json).unwrap();

        assert!(json.contains("codexSwitchEnabled"));
        assert!(restored.codex_switch_enabled);
        assert!(!restored.session_delete_enabled);
    }

    #[test]
    fn tray_usage_keeps_archived_conversation_totals() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE entries (
               date_local TEXT NOT NULL,
               cost_usd REAL,
               calculated_cost REAL NOT NULL,
               total_tokens INTEGER NOT NULL
             );
             CREATE TABLE usage_archive (
               date_local TEXT NOT NULL,
               cost_auto REAL NOT NULL,
               total_tokens INTEGER NOT NULL
             );
             INSERT INTO entries VALUES ('2026-08-23', NULL, 1.25, 1000);
             INSERT INTO usage_archive VALUES ('2026-08-23', 2.75, 2000);
             INSERT INTO usage_archive VALUES ('2026-08-22', 99.0, 99000);",
        )
        .unwrap();

        let (cost, tokens) = tray_usage_for_date(&conn, "2026-08-23").unwrap();

        assert!((cost - 4.0).abs() < f64::EPSILON);
        assert_eq!(tokens, 3000);
    }
}
