//! Put a delete button inside Codex's own sidebar.
//!
//! Codex is an Electron app, and an Electron renderer can only be scripted
//! over the DevTools protocol if the process was *started* with
//! `--remote-debugging-port`. There is no way to attach after the fact, so
//! TokBar has to launch Codex itself. That is the whole reason this module
//! exists and why it sits behind an explicit opt-in.
//!
//! Shape: one supervisor thread quits any running Codex, relaunches it with
//! the debug flag, connects over CDP, installs a binding plus `inject.js`,
//! then pumps binding calls until asked to stop. A page reload drops the
//! connection; the loop simply reattaches.
//!
//! macOS only for now. Windows Codex ships as a Store package and needs a
//! different activation path entirely; `launch_codex` says so rather than
//! failing in some confusing way.

pub mod cdp;
pub mod threads;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

/// Debug port for the Codex renderer. Matches CodexPlusPlus's default, which
/// is deliberate: if both are installed they collide loudly on startup instead
/// of silently fighting over the same renderer.
pub const DEFAULT_DEBUG_PORT: u16 = 9229;

const BINDING_NAME: &str = "__tokbarInjectBinding";
const INJECT_SCRIPT: &str = include_str!("inject.js");

/// How long to wait for Codex to come up and start serving CDP.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(45);
/// How long to wait for a quitting Codex to actually disappear. Relaunching
/// before the old process is gone is what makes Electron's single-instance
/// lock swallow the new one, debug flag and all.
const QUIT_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectStatus {
    /// Supervisor thread is alive.
    pub running: bool,
    /// CDP is connected and the script is installed.
    pub attached: bool,
    pub debug_port: u16,
    pub codex_app_path: String,
    /// Last failure, kept so the settings card can show why nothing happened.
    pub last_error: String,
    /// Codex is up but was not started by us, so it has no debug port.
    pub needs_relaunch: bool,
}

/// Handles one binding call from the page: `(action, payload) -> result`.
pub type ActionHandler = Arc<dyn Fn(&str, &Value) -> Result<Value, String> + Send + Sync>;

pub struct Supervisor {
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<InjectStatus>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Supervisor {
    pub fn status(&self) -> InjectStatus {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        if let Ok(mut status) = self.status.lock() {
            status.running = false;
            status.attached = false;
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// What the supervisor is allowed to do to Codex on its first pass. Whatever
/// it is, it applies **once**: afterwards the supervisor only ever attaches, so
/// quitting Codex keeps it quit instead of being dragged back open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchIntent {
    /// Never start Codex. Used at TokBar startup.
    AttachOnly,
    /// Start Codex only if nothing is serving CDP. Used when the feature is
    /// switched on: a Codex already running with a debug port just gets
    /// attached to, one without gets restarted.
    LaunchIfDown,
    /// Quit any running Codex and start it fresh, even if it is already
    /// serving CDP. This is what the "relaunch" button means -- without it the
    /// button silently does nothing whenever Codex is already up.
    Restart,
}

pub fn start(
    app_path: PathBuf,
    debug_port: u16,
    intent: LaunchIntent,
    handler: ActionHandler,
) -> Supervisor {
    let stop = Arc::new(AtomicBool::new(false));
    let status = Arc::new(Mutex::new(InjectStatus {
        running: true,
        debug_port,
        codex_app_path: app_path.to_string_lossy().to_string(),
        ..Default::default()
    }));
    let thread_stop = Arc::clone(&stop);
    let thread_status = Arc::clone(&status);
    let handle = std::thread::Builder::new()
        .name("codex-inject".to_string())
        .spawn(move || {
            supervise(app_path, debug_port, intent, handler, thread_stop, thread_status);
        })
        .ok();
    Supervisor {
        stop,
        status,
        handle,
    }
}

fn set_error(status: &Arc<Mutex<InjectStatus>>, message: String) {
    if let Ok(mut status) = status.lock() {
        status.attached = false;
        status.last_error = message;
    }
}

/// What the supervisor should do on this pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopAction {
    /// CDP is up: connect and install the script.
    Attach,
    /// Codex is down and we still hold the one-shot permit.
    Launch,
    /// Codex is down and we do not. Sit still -- relaunching here is what
    /// makes Codex impossible to quit.
    Wait,
}

fn next_action(listening: bool, intent: LaunchIntent) -> LoopAction {
    match intent {
        // A restart is unconditional: the point is to replace the running
        // instance, not to check whether one is there.
        LaunchIntent::Restart => LoopAction::Launch,
        _ if listening => LoopAction::Attach,
        LaunchIntent::LaunchIfDown => LoopAction::Launch,
        LaunchIntent::AttachOnly => LoopAction::Wait,
    }
}

fn set_waiting(status: &Arc<Mutex<InjectStatus>>) {
    if let Ok(mut status) = status.lock() {
        status.attached = false;
        status.needs_relaunch = true;
    }
}

fn supervise(
    app_path: PathBuf,
    debug_port: u16,
    intent: LaunchIntent,
    handler: ActionHandler,
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<InjectStatus>>,
) {
    let mut intent = intent;
    while !stop.load(Ordering::SeqCst) {
        let action = next_action(cdp::is_listening(debug_port), intent);
        // Spend the intent before acting on it, so a failed launch cannot turn
        // into a loop that keeps reopening Codex.
        intent = LaunchIntent::AttachOnly;
        match action {
            LoopAction::Wait => {
                set_waiting(&status);
                sleep_interruptible(Duration::from_secs(3), &stop);
                continue;
            }
            LoopAction::Launch => {
                if let Err(error) = ensure_codex_running(&app_path, debug_port, &stop) {
                    set_error(&status, error);
                    sleep_interruptible(Duration::from_secs(5), &stop);
                    continue;
                }
            }
            LoopAction::Attach => {}
        }
        match attach(debug_port, &handler, &stop, &status) {
            Ok(()) => {}
            Err(error) => set_error(&status, error),
        }
        if let Ok(mut status) = status.lock() {
            status.attached = false;
        }
        // A page reload or a navigation kills the target; reattaching after a
        // short pause is the normal path, not an error.
        sleep_interruptible(Duration::from_secs(2), &stop);
    }
    if let Ok(mut status) = status.lock() {
        status.running = false;
        status.attached = false;
    }
}

/// Quit a debug-less Codex, then launch one with the port open. Only called
/// while holding the one-shot launch permit.
fn ensure_codex_running(
    app_path: &std::path::Path,
    debug_port: u16,
    stop: &Arc<AtomicBool>,
) -> Result<(), String> {
    if macos_app_running(app_path) {
        quit_codex(app_path)?;
    }
    launch_codex(app_path, debug_port)?;

    let deadline = Instant::now() + LAUNCH_TIMEOUT;
    while Instant::now() < deadline {
        if stop.load(Ordering::SeqCst) {
            return Err("cancelled".to_string());
        }
        if cdp::is_listening(debug_port) {
            return Ok(());
        }
        sleep_interruptible(Duration::from_millis(500), stop);
    }
    Err(format!(
        "Codex did not open a debug port on {debug_port} within {}s",
        LAUNCH_TIMEOUT.as_secs()
    ))
}

fn attach(
    debug_port: u16,
    handler: &ActionHandler,
    stop: &Arc<AtomicBool>,
    status: &Arc<Mutex<InjectStatus>>,
) -> Result<(), String> {
    let targets = cdp::list_targets(debug_port)?;
    let target = cdp::pick_page_target(&targets).ok_or("no injectable Codex page found")?;
    let websocket_url = target
        .web_socket_debugger_url
        .clone()
        .ok_or("page target has no debugger url")?;
    let mut session = cdp::CdpSession::connect(&websocket_url)?;
    let mut bindings = Vec::new();

    session.send("Runtime.enable", json!({}), &mut bindings)?;
    // Remove first: a reattach onto a surviving context would otherwise fail
    // with "binding already exists".
    let _ = session.send(
        "Runtime.removeBinding",
        json!({ "name": BINDING_NAME }),
        &mut bindings,
    );
    session.send(
        "Runtime.addBinding",
        json!({ "name": BINDING_NAME }),
        &mut bindings,
    )?;

    let source = format!("{}\n{INJECT_SCRIPT}", bridge_script());
    // Both: the first covers future navigations, the second the page that is
    // already loaded right now.
    session.send(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": source }),
        &mut bindings,
    )?;
    session.send(
        "Runtime.evaluate",
        json!({ "expression": source, "awaitPromise": false, "returnByValue": true }),
        &mut bindings,
    )?;

    if let Ok(mut status) = status.lock() {
        status.attached = true;
        status.last_error.clear();
        status.needs_relaunch = false;
    }

    while !stop.load(Ordering::SeqCst) {
        session.read(&mut bindings)?;
        for call in bindings.drain(..) {
            dispatch(&mut session, handler, &call)?;
        }
    }
    session.close();
    Ok(())
}

/// Page-side half of the bridge: a promise-returning `__tokbarInvoke` on top
/// of the one-way binding, resolved by an evaluate from Rust.
fn bridge_script() -> String {
    format!(
        r#"
(() => {{
  window.__tokbarInjectSeq = window.__tokbarInjectSeq || 0;
  window.__tokbarInjectCallbacks = window.__tokbarInjectCallbacks || new Map();
  window.__tokbarInjectResolve = (id, result) => {{
    const resolve = window.__tokbarInjectCallbacks.get(id);
    if (!resolve) return;
    window.__tokbarInjectCallbacks.delete(id);
    resolve(result);
  }};
  // A binding whose CDP client has gone away still exists as a function and
  // silently swallows the call, so every request needs a deadline -- otherwise
  // quitting TokBar leaves buttons that hang forever instead of failing.
  window.__tokbarInvokeTimeoutMs = 10000;
  window.__tokbarInvoke = (action, payload) => new Promise((resolve) => {{
    if (typeof window.{BINDING_NAME} !== "function") {{
      resolve({{ status: "failed", code: "bridge_down" }});
      return;
    }}
    const id = String(++window.__tokbarInjectSeq);
    const timer = setTimeout(() => {{
      window.__tokbarInjectCallbacks.delete(id);
      resolve({{ status: "failed", code: "bridge_down" }});
    }}, window.__tokbarInvokeTimeoutMs);
    window.__tokbarInjectCallbacks.set(id, (result) => {{
      clearTimeout(timer);
      resolve(result);
    }});
    window.{BINDING_NAME}(JSON.stringify({{ id, action, payload }}));
  }});
}})();
"#
    )
}

fn dispatch(
    session: &mut cdp::CdpSession,
    handler: &ActionHandler,
    call: &Value,
) -> Result<(), String> {
    let params = call.get("params").unwrap_or(&Value::Null);
    if params.get("name").and_then(Value::as_str) != Some(BINDING_NAME) {
        return Ok(());
    }
    let Some(raw) = params.get("payload").and_then(Value::as_str) else {
        return Ok(());
    };
    let request: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
    let id = request
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let action = request.get("action").and_then(Value::as_str).unwrap_or("");
    let payload = request.get("payload").cloned().unwrap_or(Value::Null);

    // A failing action must still resolve the page's promise, or the UI hangs
    // on a spinner forever.
    let result = handler(action, &payload).unwrap_or_else(|error| {
        json!({ "status": "failed", "message": error })
    });
    let expression = format!(
        "window.__tokbarInjectResolve({}, {})",
        serde_json::to_string(&id).unwrap_or_else(|_| "\"\"".to_string()),
        serde_json::to_string(&result).unwrap_or_else(|_| "null".to_string()),
    );
    let mut ignored = Vec::new();
    session.send(
        "Runtime.evaluate",
        json!({ "expression": expression, "returnByValue": true }),
        &mut ignored,
    )?;
    Ok(())
}

/// Standard install locations, newest naming first. An explicit override in
/// settings wins over all of them.
pub fn resolve_app_path(override_path: &str) -> Result<PathBuf, String> {
    let trimmed = override_path.trim();
    if !trimmed.is_empty() {
        let path = PathBuf::from(trimmed);
        return if path.exists() {
            Ok(path)
        } else {
            Err(format!("Codex app not found at {trimmed}"))
        };
    }
    let mut candidates = vec![
        PathBuf::from("/Applications/ChatGPT.app"),
        PathBuf::from("/Applications/Codex.app"),
    ];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Applications/ChatGPT.app"));
        candidates.push(home.join("Applications/Codex.app"));
    }
    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| "could not find ChatGPT.app or Codex.app".to_string())
}

fn app_display_name(app_path: &std::path::Path) -> String {
    app_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("ChatGPT")
        .to_string()
}

#[cfg(target_os = "macos")]
fn macos_app_running(app_path: &std::path::Path) -> bool {
    let script = format!(
        r#"application "{}" is running"#,
        app_display_name(app_path).replace('"', "\\\"")
    );
    std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()
        .is_some_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).trim().eq_ignore_ascii_case("true")
        })
}

#[cfg(not(target_os = "macos"))]
fn macos_app_running(_app_path: &std::path::Path) -> bool {
    false
}

/// Ask Codex to quit, then wait until the process is really gone.
///
/// The waiting is the point. CodexPlusPlus relaunches straight after the quit
/// request and regularly ends up with a Codex that never opened its debug
/// port, because Electron's single-instance lock hands the launch to the
/// still-dying old process.
#[cfg(target_os = "macos")]
fn quit_codex(app_path: &std::path::Path) -> Result<(), String> {
    let name = app_display_name(app_path);
    let script = format!(r#"tell application "{}" to quit"#, name.replace('"', "\\\""));
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status();
    let deadline = Instant::now() + QUIT_TIMEOUT;
    while Instant::now() < deadline {
        if !macos_app_running(app_path) {
            // LaunchServices needs a beat after the process exits before a new
            // instance registers cleanly.
            std::thread::sleep(Duration::from_millis(400));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(format!(
        "{name} did not quit within {}s; close it manually and retry",
        QUIT_TIMEOUT.as_secs()
    ))
}

#[cfg(not(target_os = "macos"))]
fn quit_codex(_app_path: &std::path::Path) -> Result<(), String> {
    Err(unsupported_platform())
}

/// `open -n <bundle>` and not `open -a <name>`: with several Codex builds
/// sharing a bundle id, LaunchServices can resolve the name to the wrong one
/// and the debug flag lands nowhere. No `-W`, so nothing has to babysit the
/// child process.
#[cfg(target_os = "macos")]
fn launch_codex(app_path: &std::path::Path, debug_port: u16) -> Result<(), String> {
    std::process::Command::new("open")
        .arg("-n")
        .arg(app_path)
        .arg("--args")
        .arg(format!("--remote-debugging-port={debug_port}"))
        .arg(format!(
            "--remote-allow-origins=http://127.0.0.1:{debug_port}"
        ))
        .status()
        .map_err(|e| format!("failed to launch {}: {e}", app_path.display()))
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(format!("launching {} failed", app_path.display()))
            }
        })
}

#[cfg(not(target_os = "macos"))]
fn launch_codex(_app_path: &std::path::Path, _debug_port: u16) -> Result<(), String> {
    Err(unsupported_platform())
}

#[cfg(not(target_os = "macos"))]
fn unsupported_platform() -> String {
    "the in-Codex delete button is macOS-only for now".to_string()
}

fn sleep_interruptible(total: Duration, stop: &Arc<AtomicBool>) {
    let step = Duration::from_millis(200);
    let mut waited = Duration::ZERO;
    while waited < total {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(step);
        waited += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_script_exposes_the_binding_by_name() {
        let script = bridge_script();

        assert!(script.contains(BINDING_NAME), "{script}");
        assert!(script.contains("__tokbarInvoke"));
        assert!(script.contains("__tokbarInjectResolve"));
    }

    /// A failing handler still has to resolve the page promise, otherwise the
    /// delete button spins forever.
    #[test]
    fn inject_script_and_bridge_are_wired_together() {
        assert!(INJECT_SCRIPT.contains("__tokbarInvoke"));
        assert!(INJECT_SCRIPT.contains("data-app-action-sidebar-thread-id"));
        assert!(INJECT_SCRIPT.contains("delete_thread"));
        assert!(INJECT_SCRIPT.contains("undo_delete"));
    }

    #[test]
    fn optimistic_new_threads_are_not_offered_a_delete_button() {
        assert!(INJECT_SCRIPT.contains("isPersistedLocalThread"));
        assert!(INJECT_SCRIPT.contains("local:client-new-thread:"));
    }

    /// Closing TokBar leaves the binding defined but unanswered, so the page
    /// needs its own deadline and a teardown for the buttons it can no longer
    /// serve. Without this a click just hangs forever.
    #[test]
    fn the_bridge_gives_up_when_nothing_answers() {
        let bridge = bridge_script();

        assert!(bridge.contains("setTimeout"), "{bridge}");
        assert!(bridge.contains("bridge_down"), "{bridge}");
        // The page side has to act on it, not just receive it.
        assert!(INJECT_SCRIPT.contains("bridge_down"));
        assert!(INJECT_SCRIPT.contains("markBridgeDown"));
        // And come back when TokBar reconnects and re-evaluates the script.
        assert!(INJECT_SCRIPT.contains("revive"));
    }

    /// Quitting Codex used to bring it straight back, because the loop treated
    /// "not listening" as "launch it" on every pass.
    #[test]
    fn a_quit_codex_stays_quit_once_the_intent_is_spent() {
        assert_eq!(
            next_action(false, LaunchIntent::LaunchIfDown),
            LoopAction::Launch
        );

        // Every pass after the first runs as AttachOnly.
        assert_eq!(next_action(false, LaunchIntent::AttachOnly), LoopAction::Wait);
        assert_eq!(next_action(false, LaunchIntent::AttachOnly), LoopAction::Wait);
    }

    /// The relaunch button did nothing whenever Codex was already up: the loop
    /// saw a live debug port and just reattached.
    #[test]
    fn a_restart_relaunches_even_when_codex_is_already_serving_cdp() {
        assert_eq!(next_action(true, LaunchIntent::Restart), LoopAction::Launch);
        assert_eq!(next_action(false, LaunchIntent::Restart), LoopAction::Launch);
    }

    #[test]
    fn a_running_codex_is_attached_to_rather_than_restarted() {
        assert_eq!(next_action(true, LaunchIntent::LaunchIfDown), LoopAction::Attach);
        assert_eq!(next_action(true, LaunchIntent::AttachOnly), LoopAction::Attach);
    }

    #[test]
    fn an_explicit_app_path_must_exist() {
        let error = resolve_app_path("/nope/Missing.app").unwrap_err();

        assert!(error.contains("not found"), "{error}");
    }
}
