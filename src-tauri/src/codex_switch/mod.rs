//! Codex account / provider quick switch.
//!
//! Ported from CodexPlusPlus (`codex_provider.rs` + `codex_quick_switch.rs`).
//! Two independent axes end up in one menu:
//!
//! * **provider** -- `model_provider` in `$CODEX_HOME/config.toml`, i.e. which
//!   upstream endpoint requests go to. Empty means official ChatGPT.
//! * **account**  -- which ChatGPT login `auth.json` holds. Only one can be
//!   live, so the others are kept as cold backups under TokBar's data dir.
//!
//! They are genuinely independent: selecting a provider rewrites config.toml
//! and does not touch `auth.json`, so the signed-in identity survives. The
//! upstream project conflated "this account is signed in" with "this account
//! is the current switch target", which made the account row unclickable in
//! provider mode and left no way back. Here the two are separate fields --
//! `live_account_id` (identity, set in both modes) and `current_account_id`
//! (switch target, only set in official mode) -- and only the latter drives
//! the disabled state in the UI.
//!
//! TokBar has no launcher, so nothing here restarts Codex. Every write lands
//! on disk and takes effect the next time Codex starts; the UI says so.

pub mod accounts;
pub mod config;

use std::path::{Path, PathBuf};

use serde::Serialize;

use accounts::{AccountOption, PendingAccount, Selection, SelectionKind};
use config::ProviderOption;

/// Everything the switcher UI needs in one round trip.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickSwitchState {
    pub accounts: Vec<AccountOption>,
    pub providers: Vec<ProviderOption>,
    /// Selected provider id; empty string means official mode.
    pub current_provider: String,
    /// Convenience mirror of `current_provider.is_empty()`.
    pub official_mode: bool,
    /// Account whose identity `auth.json` currently holds. Stays set while a
    /// provider is selected -- switching providers does not sign anyone out.
    pub live_account_id: String,
    /// Account that is the current switch target, i.e. the row to disable.
    /// Empty whenever a provider is active, so the account is always clickable
    /// as the way back to official mode.
    pub current_account_id: String,
    pub pending_account: Option<PendingAccount>,
    /// A ChatGPT login exists but no archive matches it, so it cannot be
    /// switched back to yet. The UI offers "save current account" for this.
    pub requires_current_account_name: bool,
    pub selection: Option<Selection>,
    /// Label for a compact trigger: the provider name in provider mode, the
    /// account name otherwise.
    pub display_name: String,
    /// Resolved `$CODEX_HOME`, shown so the user can confirm which install is
    /// being edited.
    pub codex_home: String,
    /// Set when a pending sign-in was adopted during this read, so the UI can
    /// confirm it once.
    pub captured_account: Option<AccountOption>,
    /// Accounts in CodexPlusPlus's store that are not already saved here.
    pub importable_accounts: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchResult {
    pub state: QuickSwitchState,
    /// False when the config already matched, so nothing was written and no
    /// Codex restart is needed.
    pub changed: bool,
    pub message: String,
}

/// Primary Codex home. `codex_homes()` in the adapter may return several
/// (multi-account clones); config edits always target the first, which is the
/// one `$CODEX_HOME` or `~/.codex` resolves to.
pub fn codex_home() -> PathBuf {
    crate::adapters::codex::primary_home()
}

/// CodexPlusPlus's quick-switch store, the one place a user is likely to
/// already have Codex account archives. Read only, and only on request.
pub fn codexplusplus_store() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".codex-session-delete")
        .join("quick-switch")
}

/// Archive + metadata root, inside TokBar's own app data dir.
pub fn store_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join("codex-switch")
}

/// Read the whole switcher state, adopting a pending sign-in if one completed.
pub fn state(cache_dir: &Path) -> Result<QuickSwitchState, String> {
    let home = codex_home();
    let store = store_dir(cache_dir);
    let captured = accounts::reconcile_pending(&home, &store)?;
    let mut state = read_state(&home, &store)?;
    state.captured_account = captured;
    Ok(state)
}

fn read_state(home: &Path, store: &Path) -> Result<QuickSwitchState, String> {
    let metadata = accounts::load_metadata(store)?;
    let (current_provider, providers) = config::read_state(home)?;
    let selection = accounts::valid_selection(
        metadata.selection.clone(),
        &metadata.accounts,
        &providers,
    );
    let official_mode = current_provider.is_empty();

    // Keep the signed-in account's archive current, in provider mode too.
    // Upstream only does this in official mode, but a provider switch never
    // touches auth.json -- Codex keeps rotating those tokens either way, and a
    // snapshot left behind is one that fails to restore later.
    let live = std::fs::read(home.join("auth.json")).ok();
    accounts::refresh_archive(store, &metadata.accounts, selection.as_ref(), live.as_deref());

    let live_account_id =
        accounts::live_account_id(home, store, &metadata.accounts, selection.as_ref());
    let current_account_id = if official_mode {
        live_account_id.clone()
    } else {
        String::new()
    };
    let display_name = providers
        .iter()
        .find(|provider| provider.id == current_provider)
        .map(|provider| provider.name.clone())
        .or_else(|| {
            metadata
                .accounts
                .iter()
                .find(|account| account.id == live_account_id)
                .map(|account| account.name.clone())
        })
        .or_else(|| selection.as_ref().map(|selection| selection.name.clone()))
        .unwrap_or_else(|| "ChatGPT".to_string());

    let metadata_accounts = metadata.accounts.clone();
    Ok(QuickSwitchState {
        accounts: metadata.accounts,
        providers,
        current_provider,
        official_mode,
        requires_current_account_name: accounts::has_live_auth(home) && live_account_id.is_empty(),
        live_account_id,
        current_account_id,
        pending_account: metadata.pending_account,
        selection,
        display_name,
        codex_home: home.to_string_lossy().to_string(),
        captured_account: None,
        importable_accounts: accounts::importable_count(
            &codexplusplus_store(),
            store,
            &metadata_accounts,
        ),
    })
}

/// Switch to an account (restores its auth.json, returns to official mode) or
/// to a provider (rewrites `model_provider` only).
pub fn select(cache_dir: &Path, kind: &str, id: &str) -> Result<SwitchResult, String> {
    let home = codex_home();
    let store = store_dir(cache_dir);
    let (changed, message) = match kind {
        "account" => {
            let (account, changed) = accounts::switch(&home, &store, id)?;
            (changed, format!("Switched to account {}", account.name))
        }
        "provider" => {
            let (_, providers) = config::read_state(&home)?;
            let provider = providers
                .iter()
                .find(|provider| provider.id == id)
                .cloned()
                .ok_or("provider no longer exists")?;
            let result = config::switch(&home, id)?;
            accounts::record_selection(
                &store,
                Selection {
                    kind: SelectionKind::Provider,
                    id: provider.id.clone(),
                    name: provider.name.clone(),
                },
            )?;
            (result.changed, format!("Switched to provider {}", provider.name))
        }
        other => return Err(format!("unsupported switch target \"{other}\"")),
    };
    Ok(SwitchResult {
        state: state(cache_dir)?,
        changed,
        message,
    })
}

/// Return to official ChatGPT without restoring a specific archive. Useful
/// when the live login has no archive yet.
pub fn select_official(cache_dir: &Path) -> Result<SwitchResult, String> {
    let home = codex_home();
    let result = config::switch(&home, "")?;
    Ok(SwitchResult {
        state: state(cache_dir)?,
        changed: result.changed,
        message: "Switched to official ChatGPT".to_string(),
    })
}

pub fn create_provider(
    cache_dir: &Path,
    name: &str,
    base_url: &str,
    bearer_token: &str,
    model: &str,
) -> Result<SwitchResult, String> {
    let home = codex_home();
    let store = store_dir(cache_dir);
    let provider = config::create(&home, name, base_url, bearer_token, model)?;
    // Creating selects it, so the recorded selection has to follow.
    accounts::record_selection(
        &store,
        Selection {
            kind: SelectionKind::Provider,
            id: provider.id.clone(),
            name: provider.name.clone(),
        },
    )?;
    Ok(SwitchResult {
        state: state(cache_dir)?,
        changed: true,
        message: format!("Added provider {}", provider.name),
    })
}

pub fn update_provider(
    cache_dir: &Path,
    provider_id: &str,
    name: &str,
    base_url: &str,
    bearer_token: &str,
    model: &str,
) -> Result<SwitchResult, String> {
    let home = codex_home();
    let (provider, changed) =
        config::update(&home, provider_id, name, base_url, bearer_token, model)?;
    Ok(SwitchResult {
        state: state(cache_dir)?,
        changed,
        message: format!("Updated provider {}", provider.name),
    })
}

pub fn delete_provider(cache_dir: &Path, provider_id: &str) -> Result<SwitchResult, String> {
    let provider = config::delete(&codex_home(), provider_id)?;
    Ok(SwitchResult {
        state: state(cache_dir)?,
        changed: true,
        message: format!("Deleted provider {}", provider.name),
    })
}

/// Pull account archives across from CodexPlusPlus.
pub fn import_accounts(cache_dir: &Path) -> Result<SwitchResult, String> {
    let outcome = accounts::import_from(
        &codexplusplus_store(),
        &codex_home(),
        &store_dir(cache_dir),
    )?;
    let names = outcome
        .imported
        .iter()
        .map(|account| account.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let message = if outcome.imported.is_empty() {
        format!(
            "Nothing to import ({} already here, {} unreadable)",
            outcome.skipped_existing, outcome.skipped_invalid
        )
    } else {
        format!("Imported {names}")
    };
    Ok(SwitchResult {
        state: state(cache_dir)?,
        changed: !outcome.imported.is_empty(),
        message,
    })
}

/// Adopt the current login without signing out.
pub fn capture_account(cache_dir: &Path, name: &str, model: &str) -> Result<SwitchResult, String> {
    let account = accounts::capture_current(&codex_home(), &store_dir(cache_dir), name, model)?;
    Ok(SwitchResult {
        state: state(cache_dir)?,
        changed: true,
        message: format!("Saved account {}", account.name),
    })
}

/// Begin adding another account. Signs the current one out, so the caller must
/// have confirmed first.
pub fn begin_add_account(
    cache_dir: &Path,
    name: &str,
    current_account_name: &str,
    model: &str,
) -> Result<SwitchResult, String> {
    let (pending, _) = accounts::begin_add(
        &codex_home(),
        &store_dir(cache_dir),
        name,
        current_account_name,
        model,
    )?;
    Ok(SwitchResult {
        state: state(cache_dir)?,
        changed: true,
        message: format!("Sign in as {} in Codex to finish", pending.name),
    })
}

pub fn update_account(
    cache_dir: &Path,
    account_id: &str,
    name: &str,
    model: &str,
) -> Result<SwitchResult, String> {
    let (account, changed) = accounts::update(
        &codex_home(),
        &store_dir(cache_dir),
        account_id,
        name,
        model,
    )?;
    Ok(SwitchResult {
        state: state(cache_dir)?,
        changed,
        message: format!("Updated account {}", account.name),
    })
}

pub fn delete_account(cache_dir: &Path, account_id: &str) -> Result<SwitchResult, String> {
    let account = accounts::delete(&codex_home(), &store_dir(cache_dir), account_id)?;
    Ok(SwitchResult {
        state: state(cache_dir)?,
        changed: true,
        message: format!("Deleted account {}", account.name),
    })
}

/// Write via a temp file in the same directory, then rename, so a crash can
/// never leave a half-written config.toml or auth.json behind.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut temp_path = path.to_path_buf();
    let extension = match path.extension().and_then(|value| value.to_str()) {
        Some(extension) => format!("{extension}.tmp"),
        None => "tmp".to_string(),
    };
    temp_path.set_extension(extension);
    std::fs::write(&temp_path, bytes)?;
    std::fs::rename(&temp_path, path)
}

/// Opaque id for accounts and providers. Not a UUID crate dependency: a
/// 128-bit value from the OS-seeded hash of time + address entropy is more
/// than enough for local, single-user identifiers.
pub(crate) fn new_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default();
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(nanos);
    let high = hasher.finish();
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(high);
    hasher.write_u64(nanos);
    format!("{high:016x}{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Exercise the layer that takes explicit paths, so nothing here depends
    /// on `$CODEX_HOME` and the tests stay parallel-safe.
    struct Fixture {
        home: PathBuf,
        store: PathBuf,
        _root: TempRoot,
    }

    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "tokbar-codex-switch-{tag}-{}-{}",
                std::process::id(),
                new_id()
            ));
            let home = root.join("codex");
            let store = root.join("store");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&store).unwrap();
            Self {
                home,
                store,
                _root: TempRoot(root),
            }
        }

        fn state(&self) -> QuickSwitchState {
            read_state(&self.home, &self.store).unwrap()
        }

        fn write_auth(&self, account_id: &str) {
            fs::write(self.home.join("auth.json"), chatgpt_auth(account_id)).unwrap();
        }

        /// Bytes currently stored for an account's archive.
        fn archive(&self, account_id: &str) -> Vec<u8> {
            fs::read(
                self.store
                    .join("accounts")
                    .join(format!("{account_id}.auth.json")),
            )
            .unwrap()
        }

        /// Register an account that is not the signed-in one.
        fn add_idle_account(&self, name: &str, auth: &[u8]) -> String {
            let id = new_id();
            fs::write(
                self.store.join("accounts").join(format!("{id}.auth.json")),
                auth,
            )
            .unwrap();
            let path = self.store.join("metadata.json");
            let mut metadata: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            metadata["accounts"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({ "id": id, "name": name, "model": "m" }));
            fs::write(&path, serde_json::to_vec_pretty(&metadata).unwrap()).unwrap();
            id
        }

        fn config(&self) -> String {
            fs::read_to_string(self.home.join("config.toml")).unwrap_or_default()
        }
    }

    fn chatgpt_auth(account_id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": { "account_id": account_id, "access_token": "token-value" }
        }))
        .unwrap()
    }

    /// Same identity, different token values: what a Codex refresh produces.
    fn rotated_auth(account_id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "auth_mode": "chatgpt",
            "last_refresh": "2026-08-23T15:43:00Z",
            "tokens": { "account_id": account_id, "access_token": "rotated-token" }
        }))
        .unwrap()
    }

    fn add_provider(fixture: &Fixture, name: &str) -> config::ProviderOption {
        config::create(
            &fixture.home,
            name,
            "https://example.test/v1",
            "sk-test-token",
            "provider-model",
        )
        .unwrap()
    }

    /// The regression this port exists to avoid: selecting a provider leaves
    /// `auth.json` alone, so the account is still *signed in* -- but it must
    /// not be reported as the current switch target, or the UI disables the
    /// only row that leads back to official mode.
    #[test]
    fn provider_mode_keeps_the_signed_in_account_switchable() {
        let fixture = Fixture::new("provider-mode");
        fixture.write_auth("account-a");
        let account =
            accounts::capture_current(&fixture.home, &fixture.store, "pro", "account-model")
                .unwrap();
        let provider = add_provider(&fixture, "relay");

        let state = fixture.state();

        assert_eq!(state.current_provider, provider.id);
        assert!(!state.official_mode);
        // Identity survives the provider switch...
        assert_eq!(state.live_account_id, account.id);
        // ...but the account is not the current target, so it stays clickable.
        assert_eq!(state.current_account_id, "");
        assert_eq!(state.display_name, "relay");
    }

    #[test]
    fn official_mode_marks_the_signed_in_account_as_current() {
        let fixture = Fixture::new("official-mode");
        fixture.write_auth("account-a");
        let account =
            accounts::capture_current(&fixture.home, &fixture.store, "pro", "account-model")
                .unwrap();

        let state = fixture.state();

        assert!(state.official_mode);
        assert_eq!(state.live_account_id, account.id);
        assert_eq!(state.current_account_id, account.id);
        assert!(!state.requires_current_account_name);
    }

    /// Coming back from a provider always has to rewrite config.toml, so the
    /// switch must report a real change rather than a no-op.
    #[test]
    fn switching_to_an_account_from_provider_mode_returns_to_official() {
        let fixture = Fixture::new("back-to-account");
        fixture.write_auth("account-a");
        let account =
            accounts::capture_current(&fixture.home, &fixture.store, "pro", "account-model")
                .unwrap();
        add_provider(&fixture, "relay");
        assert!(!fixture.state().official_mode);

        let (switched, changed) =
            accounts::switch(&fixture.home, &fixture.store, &account.id).unwrap();

        assert_eq!(switched.id, account.id);
        assert!(changed);
        let state = fixture.state();
        assert!(state.official_mode);
        assert_eq!(state.current_account_id, account.id);
        // The provider entry survives; only the top-level selection is cleared.
        assert_eq!(config::read_state(&fixture.home).unwrap().0, "");
    }

    #[test]
    fn returning_to_official_restores_the_backed_up_model() {
        let fixture = Fixture::new("model-backup");
        fs::write(
            fixture.home.join("config.toml"),
            "model = \"official-model\"\n",
        )
        .unwrap();
        let provider = add_provider(&fixture, "relay");

        assert!(fixture.config().contains("provider-model"));
        config::switch(&fixture.home, "").unwrap();

        assert!(fixture.config().contains("official-model"));
        // The provider entry itself survives the round trip.
        let (current, providers) = config::read_state(&fixture.home).unwrap();
        assert_eq!(current, "");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, provider.id);
    }

    /// The live login is the one archive that cannot be regenerated from disk.
    /// Provider mode does not change that: `auth.json` still holds it.
    #[test]
    fn deleting_the_signed_in_account_is_refused_in_both_modes() {
        let fixture = Fixture::new("delete-live");
        fixture.write_auth("account-a");
        let account =
            accounts::capture_current(&fixture.home, &fixture.store, "pro", "account-model")
                .unwrap();

        let official_error =
            accounts::delete(&fixture.home, &fixture.store, &account.id).unwrap_err();
        add_provider(&fixture, "relay");
        let provider_error =
            accounts::delete(&fixture.home, &fixture.store, &account.id).unwrap_err();

        assert!(official_error.contains("cannot be deleted"), "{official_error}");
        assert!(provider_error.contains("cannot be deleted"), "{provider_error}");
    }

    #[test]
    fn the_selected_provider_cannot_be_deleted() {
        let fixture = Fixture::new("delete-provider");
        let provider = add_provider(&fixture, "relay");

        let error = config::delete(&fixture.home, &provider.id).unwrap_err();

        assert!(error.contains("cannot be deleted"), "{error}");
    }

    /// A login with no archive cannot be switched back to, so the UI has to be
    /// told to offer "save current account" first.
    #[test]
    fn an_unsaved_login_asks_to_be_named() {
        let fixture = Fixture::new("unsaved");
        fixture.write_auth("account-a");

        let state = fixture.state();

        assert!(state.requires_current_account_name);
        assert_eq!(state.live_account_id, "");
        assert!(state.accounts.is_empty());
    }

    #[test]
    fn names_are_unique_across_accounts_and_providers() {
        let fixture = Fixture::new("unique-names");
        fixture.write_auth("account-a");
        add_provider(&fixture, "relay");

        let error =
            accounts::capture_current(&fixture.home, &fixture.store, "relay", "m").unwrap_err();

        assert!(error.contains("already used by a provider"), "{error}");
    }

    #[test]
    fn provider_edits_leave_unrelated_config_untouched() {
        let fixture = Fixture::new("preserve-config");
        fs::write(
            fixture.home.join("config.toml"),
            "# keep me\nmodel = \"official-model\"\n\n[mcp_servers.demo]\ncommand = \"demo\"\n",
        )
        .unwrap();

        add_provider(&fixture, "relay");

        let config = fixture.config();
        assert!(config.contains("# keep me"), "{config}");
        assert!(config.contains("[mcp_servers.demo]"), "{config}");
    }

    /// Write an account into a foreign quick-switch store, the shape
    /// CodexPlusPlus leaves on disk.
    fn write_source_account(source: &Path, id: &str, name: &str, account_id: &str) {
        fs::create_dir_all(source.join("accounts")).unwrap();
        fs::write(
            source.join("accounts").join(format!("{id}.auth.json")),
            chatgpt_auth(account_id),
        )
        .unwrap();
        let path = source.join("metadata.json");
        let mut metadata: serde_json::Value = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| serde_json::json!({ "accounts": [] }));
        metadata["accounts"].as_array_mut().unwrap().push(serde_json::json!({
            "id": id, "name": name, "model": "m",
        }));
        fs::write(&path, serde_json::to_vec_pretty(&metadata).unwrap()).unwrap();
    }


    /// The situation on a real machine: CodexPlusPlus holds `plus` and `pro`,
    /// TokBar already captured the same `plus` login under a different id.
    /// Importing must bring `pro` over and not create a second `plus`.
    #[test]
    fn imports_only_accounts_not_already_present() {
        let fixture = Fixture::new("import");
        let source = fixture.store.parent().unwrap().join("codexplusplus");
        fs::create_dir_all(source.join("accounts")).unwrap();

        // TokBar side: `plus` already captured from the live login.
        fixture.write_auth("shared-login");
        accounts::capture_current(&fixture.home, &fixture.store, "plus", "m").unwrap();

        // Source side: the same login under another id, plus a second account.
        write_source_account(&source, "aaa", "plus", "shared-login");
        write_source_account(&source, "bbb", "pro", "other-login");

        let outcome = accounts::import_from(&source, &fixture.home, &fixture.store).unwrap();

        assert_eq!(outcome.skipped_existing, 1, "the shared login must dedup");
        assert_eq!(outcome.imported.len(), 1);
        assert_eq!(outcome.imported[0].name, "pro");
        let names: Vec<String> = fixture.state().accounts.iter().map(|a| a.name.clone()).collect();
        assert_eq!(names, vec!["plus", "pro"]);
    }

    /// A clashing name must not block the import; the login is what matters.
    #[test]
    fn a_clashing_name_is_suffixed_rather_than_rejected() {
        let fixture = Fixture::new("import-clash");
        let source = fixture.store.parent().unwrap().join("codexplusplus");
        fs::create_dir_all(source.join("accounts")).unwrap();
        fixture.write_auth("live-login");
        accounts::capture_current(&fixture.home, &fixture.store, "pro", "m").unwrap();
        write_source_account(&source, "bbb", "pro", "different-login");

        let outcome = accounts::import_from(&source, &fixture.home, &fixture.store).unwrap();

        assert_eq!(outcome.imported.len(), 1);
        assert_eq!(outcome.imported[0].name, "pro 2");
    }

    #[test]
    fn an_empty_source_store_reports_rather_than_silently_succeeding() {
        let fixture = Fixture::new("import-empty");
        let source = fixture.store.parent().unwrap().join("missing");

        let error = accounts::import_from(&source, &fixture.home, &fixture.store).unwrap_err();

        assert!(error.contains("no accounts"), "{error}");
    }

    /// Freshness follows the *identity*, not the selection: only the archive
    /// whose account_id matches the live auth is rewritten, and the other
    /// accounts are left exactly as they were.
    #[test]
    fn freshness_updates_only_the_signed_in_account() {
        let fixture = Fixture::new("freshness");
        fixture.write_auth("account-a");
        let live = accounts::capture_current(&fixture.home, &fixture.store, "live", "m").unwrap();
        // A second account that is not signed in.
        let idle_bytes = chatgpt_auth("account-b");
        let idle_path = fixture.store.join("accounts");
        fs::create_dir_all(&idle_path).unwrap();
        let idle = fixture.add_idle_account("idle", &idle_bytes);

        // Codex rotates the live tokens.
        let rotated = rotated_auth("account-a");
        fs::write(fixture.home.join("auth.json"), &rotated).unwrap();
        assert!(accounts::refresh_live_archive(&fixture.home, &fixture.store));

        assert_eq!(fixture.archive(&live.id), rotated, "signed-in archive follows");
        assert_eq!(fixture.archive(&idle), idle_bytes, "idle archive untouched");
    }

    /// A provider switch never touches auth.json, so the snapshot has to keep
    /// tracking it there too -- upstream skipped this and let it drift.
    #[test]
    fn freshness_also_applies_while_a_provider_is_selected() {
        let fixture = Fixture::new("freshness-provider");
        fixture.write_auth("account-a");
        let live = accounts::capture_current(&fixture.home, &fixture.store, "live", "m").unwrap();
        add_provider(&fixture, "relay");
        assert!(!fixture.state().official_mode);

        let rotated = rotated_auth("account-a");
        fs::write(fixture.home.join("auth.json"), &rotated).unwrap();
        fixture.state();

        assert_eq!(fixture.archive(&live.id), rotated);
    }

    #[test]
    fn base_url_must_be_http_with_a_host() {
        let fixture = Fixture::new("bad-url");

        let error = config::create(&fixture.home, "relay", "not-a-url", "sk", "m").unwrap_err();
        let host_error = config::create(&fixture.home, "relay", "https://", "sk", "m").unwrap_err();

        assert!(error.contains("http://"), "{error}");
        assert!(host_error.contains("host name"), "{host_error}");
    }
}
