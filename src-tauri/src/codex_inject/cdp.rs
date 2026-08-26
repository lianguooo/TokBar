//! Minimal Chrome DevTools Protocol client.
//!
//! Deliberately blocking (`tungstenite`, not `tokio-tungstenite`): the whole
//! injection supervisor is one dedicated thread, so an async runtime would buy
//! nothing and add two dependency trees. Only what the injection needs is
//! implemented -- list targets, send commands, pump `Runtime.bindingCalled`.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

const HTTP_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_TIMEOUT: Duration = Duration::from_millis(100);
const READ_TIMEOUT: Duration = Duration::from_millis(500);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Deserialize)]
pub struct Target {
    #[serde(rename = "type", default)]
    pub target_type: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, rename = "webSocketDebuggerUrl")]
    pub web_socket_debugger_url: Option<String>,
}

impl Target {
    fn is_injectable_page(&self) -> bool {
        self.target_type == "page"
            && self
                .web_socket_debugger_url
                .as_deref()
                .is_some_and(|url| !url.is_empty())
    }

    /// Codex desktop serves its UI from `app://-`; older builds and the web
    /// wrapper show up as a chatgpt.com page instead.
    fn is_codex_page(&self) -> bool {
        let url = self.url.trim().to_ascii_lowercase();
        let title = self.title.trim().to_ascii_lowercase();
        url == "app://-"
            || url == "app://-/"
            || url.starts_with("app://-/index.html")
            || format!("{title} {url}").contains("codex")
            || (title == "chatgpt"
                && (url.starts_with("https://chatgpt.com")
                    || url.starts_with("https://chat.openai.com")))
    }

    /// Higher is more likely to be the window that owns the sidebar.
    fn main_window_score(&self) -> i32 {
        let url = self.url.trim().to_ascii_lowercase();
        let mut score = 0;
        if self.is_codex_page() {
            score += 100;
        }
        // A query string means a sub-route: overlays, pickers, popovers.
        if url.contains('?') {
            score -= 50;
        }
        if url.contains("overlay") || url.contains("popup") {
            score -= 50;
        }
        score
    }
}

/// True when the loopback port accepts a connection.
///
/// The caller only uses this as a cheap readiness probe before `list_targets`
/// performs the real CDP validation. Keeping the probe at the TCP layer avoids
/// proxy-aware HTTP clients stalling the Tauri command thread on Windows when
/// Codex is not running.
pub fn is_listening(port: u16) -> bool {
    [
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
    ]
    .into_iter()
    .any(|address| TcpStream::connect_timeout(&address, PROBE_TIMEOUT).is_ok())
}

pub fn list_targets(port: u16) -> Result<Vec<Target>, String> {
    let mut errors = Vec::new();
    for host in ["127.0.0.1", "[::1]"] {
        let url = format!("http://{host}:{port}/json");
        match ureq::get(&url)
            .timeout(HTTP_TIMEOUT)
            .call()
            .map_err(|e| e.to_string())
            .and_then(|response| {
                response
                    .into_json::<Vec<Target>>()
                    .map_err(|e| e.to_string())
            }) {
            Ok(targets) => return Ok(targets),
            Err(error) => errors.push(format!("{url}: {error}")),
        }
    }
    Err(format!("no CDP endpoint on port {port} ({})", errors.join("; ")))
}

/// Pick the main Codex window.
///
/// Codex exposes more than one page target: alongside `app://-/index.html`
/// there are transient overlays such as
/// `app://-/index.html?initialRoute=%2Favatar-overlay`, and `/json` does not
/// order them predictably. Injecting into an overlay silently produces no
/// delete buttons, so score instead of taking the first match.
pub fn pick_page_target(targets: &[Target]) -> Option<&Target> {
    targets
        .iter()
        .filter(|target| target.is_injectable_page())
        .max_by_key(|target| target.main_window_score())
}

pub struct CdpSession {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl CdpSession {
    pub fn connect(websocket_url: &str) -> Result<Self, String> {
        let (socket, _) =
            tungstenite::connect(websocket_url).map_err(|e| format!("CDP connect failed: {e}"))?;
        // A read timeout is what makes the pump interruptible: without it a
        // quiet page would block the thread past any stop request.
        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream
                .set_read_timeout(Some(READ_TIMEOUT))
                .map_err(|e| format!("failed to set CDP read timeout: {e}"))?;
        }
        Ok(Self { socket, next_id: 1 })
    }

    /// Send a command and wait for its matching response, ignoring events that
    /// arrive in between -- except binding calls, which are returned to the
    /// caller so none are dropped while a command is in flight.
    pub fn send(
        &mut self,
        method: &str,
        params: Value,
        pending_bindings: &mut Vec<Value>,
    ) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({ "id": id, "method": method, "params": params });
        self.socket
            .send(Message::Text(payload.to_string().into()))
            .map_err(|e| format!("failed to send {method}: {e}"))?;

        let deadline = std::time::Instant::now() + COMMAND_TIMEOUT;
        loop {
            if std::time::Instant::now() > deadline {
                return Err(format!("timed out waiting for {method}"));
            }
            let Some(message) = self.read(pending_bindings)? else {
                continue;
            };
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(format!("{method} failed: {error}"));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// Read one frame. `Ok(None)` means the read timed out with nothing
    /// pending, which is the normal idle case, not an error.
    pub fn read(&mut self, pending_bindings: &mut Vec<Value>) -> Result<Option<Value>, String> {
        let message = match self.socket.read() {
            Ok(message) => message,
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(None)
            }
            Err(error) => return Err(format!("CDP read failed: {error}")),
        };
        let Message::Text(text) = message else {
            return Ok(None);
        };
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("failed to parse CDP message: {e}"))?;
        if value.get("method").and_then(Value::as_str) == Some("Runtime.bindingCalled") {
            pending_bindings.push(value.clone());
        }
        Ok(Some(value))
    }

    pub fn close(&mut self) {
        let _ = self.socket.close(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_an_open_loopback_port_without_an_http_request() {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        assert!(is_listening(port));
    }

    fn page(url: &str) -> Target {
        Target {
            target_type: "page".to_string(),
            title: "Codex".to_string(),
            url: url.to_string(),
            web_socket_debugger_url: Some("ws://127.0.0.1:9229/x".to_string()),
        }
    }

    /// Seen on a live install: the overlay was listed first, and injecting
    /// there installs nothing because it has no sidebar.
    #[test]
    fn prefers_the_main_window_over_an_overlay_listed_first() {
        let targets = vec![
            page("app://-/index.html?initialRoute=%2Favatar-overlay"),
            page("app://-/index.html"),
        ];

        let picked = pick_page_target(&targets).unwrap();

        assert_eq!(picked.url, "app://-/index.html");
    }

    #[test]
    fn falls_back_to_any_injectable_page() {
        let mut other = page("https://example.test/");
        other.title = "Something else".to_string();

        let targets = [other];
        let picked = pick_page_target(&targets).unwrap();

        assert_eq!(picked.url, "https://example.test/");
    }

    #[test]
    fn ignores_non_page_targets() {
        let mut worker = page("app://-/index.html");
        worker.target_type = "service_worker".to_string();

        let targets = [worker];
        assert!(pick_page_target(&targets).is_none());
    }
}
