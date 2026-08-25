//! Account side of the Codex quick switch: cold backups of `auth.json`.
//!
//! Ported from CodexPlusPlus `codex_quick_switch.rs`. Only one ChatGPT login
//! can be live at a time, so switching accounts means swapping `auth.json` for
//! a previously archived copy. Archives live under TokBar's own app data dir
//! (`codex-switch/accounts/`), 0600 on Unix, and never reach the frontend --
//! the UI only ever sees ids, names and models.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::config;
use super::{atomic_write, new_id};

const METADATA_FILE: &str = "metadata.json";
const ACCOUNTS_DIR: &str = "accounts";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountOption {
    pub id: String,
    pub name: String,
    /// Written to the top-level `model` in config.toml when this account is
    /// selected. Older entries may be empty, which means "leave model alone".
    #[serde(default)]
    pub model: String,
}

/// An account whose name is reserved but whose login has not happened yet.
/// No archive exists for it until the user actually signs in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAccount {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectionKind {
    Account,
    Provider,
}

/// Last thing the user picked. Display state only -- never used to decide
/// which account is actually live, that always comes from `auth.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub kind: SelectionKind,
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    #[serde(default)]
    pub accounts: Vec<AccountOption>,
    #[serde(default)]
    pub selection: Option<Selection>,
    #[serde(default)]
    pub pending_account: Option<PendingAccount>,
}

/// Archive the currently logged-in account without touching the live session.
///
/// Not in the upstream project, which always had a launcher able to restart
/// Codex. TokBar cannot, so the first account has to be adoptable without
/// logging the user out.
pub fn capture_current(
    codex_home: &Path,
    store_dir: &Path,
    name: &str,
    model: &str,
) -> Result<AccountOption, String> {
    let name = name.trim();
    let model = model.trim();
    if name.is_empty() {
        return Err("account name is required".to_string());
    }
    let mut metadata = load_metadata(store_dir)?;
    let (_, providers) = config::read_state(codex_home)?;
    validate_unique_name(name, None, &metadata, &providers)?;

    let auth_path = codex_home.join("auth.json");
    let auth_bytes = fs::read(&auth_path)
        .map_err(|e| format!("failed to read {}: {e}", auth_path.display()))?;
    validate_chatgpt_auth(&auth_bytes)?;
    if let Some(existing) = matching_account(store_dir, &metadata.accounts, None, &auth_bytes) {
        return Err(format!(
            "this login is already saved as \"{}\"",
            existing.name
        ));
    }

    let account = AccountOption {
        id: new_id(),
        name: name.to_string(),
        // Fall back to whatever config.toml currently uses, so a later switch
        // back to this account restores the model it was last used with.
        model: if model.is_empty() {
            config::top_level_model(codex_home)
        } else {
            model.to_string()
        },
    };
    write_archive(store_dir, &account.id, &auth_bytes)?;

    metadata.accounts.push(account.clone());
    metadata.selection = Some(Selection {
        kind: SelectionKind::Account,
        id: account.id.clone(),
        name: account.name.clone(),
    });
    if let Err(error) = save_metadata(store_dir, &metadata) {
        let _ = fs::remove_file(archive_path(store_dir, &account.id));
        return Err(error);
    }
    Ok(account)
}

/// Start adding a second account: archive the current login, then clear
/// `auth.json` so the user can sign in as somebody else.
///
/// Destructive by design -- Codex has to be restarted and the new account
/// signed in before anything works again. `reconcile_pending` picks the new
/// login up on the next state read.
pub fn begin_add(
    codex_home: &Path,
    store_dir: &Path,
    name: &str,
    current_account_name: &str,
    model: &str,
) -> Result<(PendingAccount, AccountOption), String> {
    let target_name = name.trim();
    if target_name.is_empty() {
        return Err("new account name is required".to_string());
    }
    let mut metadata = load_metadata(store_dir)?;
    if metadata.pending_account.is_some() {
        return Err("an account is already waiting for sign-in".to_string());
    }
    let (_, providers) = config::read_state(codex_home)?;
    validate_unique_name(target_name, None, &metadata, &providers)?;

    let auth_path = codex_home.join("auth.json");
    let auth_bytes = fs::read(&auth_path)
        .map_err(|e| format!("failed to read {}: {e}", auth_path.display()))?;
    validate_chatgpt_auth(&auth_bytes)?;

    let selection = valid_selection(metadata.selection.clone(), &metadata.accounts, &providers);
    let previous_metadata = metadata.clone();
    let mut created_archive: Option<PathBuf> = None;
    // The account about to be logged out must be recoverable. Either it is
    // already archived, or the caller has to name it right now.
    let preserved = match matching_account(
        store_dir,
        &metadata.accounts,
        selection.as_ref(),
        &auth_bytes,
    ) {
        Some(account) => account,
        None => {
            let preserved_name = current_account_name.trim();
            if preserved_name.is_empty() {
                return Err("the current login is not saved yet; name it first".to_string());
            }
            if preserved_name.eq_ignore_ascii_case(target_name) {
                return Err("the current and new account names must differ".to_string());
            }
            validate_unique_name(preserved_name, None, &metadata, &providers)?;
            let account = AccountOption {
                id: new_id(),
                name: preserved_name.to_string(),
                model: config::top_level_model(codex_home),
            };
            write_archive(store_dir, &account.id, &auth_bytes)?;
            metadata.accounts.push(account.clone());
            created_archive = Some(archive_path(store_dir, &account.id));
            account
        }
    };
    // Refresh the archive before the live file goes away: tokens rotate, and a
    // stale snapshot would fail to restore later.
    refresh_archive(store_dir, &metadata.accounts, selection.as_ref(), Some(&auth_bytes));

    let pending = PendingAccount {
        id: new_id(),
        name: target_name.to_string(),
        model: model.trim().to_string(),
    };
    metadata.selection = Some(Selection {
        kind: SelectionKind::Account,
        id: preserved.id.clone(),
        name: preserved.name.clone(),
    });
    metadata.pending_account = Some(pending.clone());
    if let Err(error) = save_metadata(store_dir, &metadata) {
        if let Some(path) = created_archive {
            let _ = fs::remove_file(path);
        }
        return Err(error);
    }

    let config_path = codex_home.join("config.toml");
    let old_config = fs::read(&config_path).ok();
    if let Err(error) = config::switch(codex_home, "") {
        let _ = save_metadata(store_dir, &previous_metadata);
        if let Some(path) = created_archive {
            let _ = fs::remove_file(path);
        }
        return Err(format!("could not return to official mode: {error}"));
    }
    if let Err(error) = fs::remove_file(&auth_path) {
        restore_file(&config_path, old_config.as_deref());
        let _ = save_metadata(store_dir, &previous_metadata);
        if let Some(path) = created_archive {
            let _ = fs::remove_file(path);
        }
        return Err(format!("failed to remove {}: {error}", auth_path.display()));
    }
    Ok((pending, preserved))
}

/// Adopt a freshly signed-in account into the pending slot. Called on every
/// state read, so the UI picks the new login up by polling.
pub fn reconcile_pending(
    codex_home: &Path,
    store_dir: &Path,
) -> Result<Option<AccountOption>, String> {
    let mut metadata = load_metadata(store_dir)?;
    let Some(pending) = metadata.pending_account.clone() else {
        return Ok(None);
    };
    let auth_path = codex_home.join("auth.json");
    let auth_bytes = match fs::read(&auth_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to read {}: {error}", auth_path.display())),
    };
    // A half-written or non-ChatGPT auth.json means the sign-in is still in
    // flight: keep waiting instead of archiving something broken.
    if validate_chatgpt_auth(&auth_bytes).is_err() || chatgpt_account_id(&auth_bytes).is_none() {
        return Ok(None);
    }
    // Same identity as before means the user signed back into the old account;
    // that is not the new one we are waiting for.
    if matching_account(store_dir, &metadata.accounts, None, &auth_bytes).is_some() {
        return Ok(None);
    }

    let account = AccountOption {
        id: pending.id,
        name: pending.name,
        model: pending.model,
    };
    write_archive(store_dir, &account.id, &auth_bytes)?;
    metadata.accounts.push(account.clone());
    metadata.selection = Some(Selection {
        kind: SelectionKind::Account,
        id: account.id.clone(),
        name: account.name.clone(),
    });
    metadata.pending_account = None;
    if let Err(error) = save_metadata(store_dir, &metadata) {
        let _ = fs::remove_file(archive_path(store_dir, &account.id));
        return Err(error);
    }
    Ok(Some(account))
}

/// Restore an archived `auth.json` and return to official mode.
/// `changed` is false only when the target is already live.
pub fn switch(
    codex_home: &Path,
    store_dir: &Path,
    account_id: &str,
) -> Result<(AccountOption, bool), String> {
    let mut metadata = load_metadata(store_dir)?;
    let account = metadata
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .cloned()
        .ok_or("account no longer exists")?;
    let path = archive_path(store_dir, &account.id);
    let auth_bytes =
        fs::read(&path).map_err(|e| format!("failed to read backup {}: {e}", path.display()))?;
    validate_chatgpt_auth(&auth_bytes)?;

    let auth_path = codex_home.join("auth.json");
    let old_auth = fs::read(&auth_path).ok();
    let (previous_provider, providers) = config::read_state(codex_home)?;
    let selection = valid_selection(metadata.selection.clone(), &metadata.accounts, &providers);
    // Already-selected is only a no-op in official mode with a matching live
    // identity. Coming back from a provider always has to rewrite config.toml.
    let already_selected = metadata.selection.as_ref().is_some_and(|selection| {
        selection.kind == SelectionKind::Account && selection.id == account.id
    });
    if previous_provider.is_empty()
        && already_selected
        && old_auth
            .as_deref()
            .and_then(chatgpt_account_id)
            .zip(chatgpt_account_id(&auth_bytes))
            .is_some_and(|(live, archived)| live == archived)
    {
        return Ok((account, false));
    }

    refresh_archive(store_dir, &metadata.accounts, selection.as_ref(), old_auth.as_deref());
    write_private_file(&auth_path, &auth_bytes)?;
    if let Err(error) = config::switch(codex_home, "") {
        restore_auth(&auth_path, old_auth.as_deref());
        return Err(format!("failed to write official config: {error}"));
    }
    if let Err(error) = config::set_top_level_model_in_home(codex_home, &account.model) {
        restore_auth(&auth_path, old_auth.as_deref());
        let _ = config::switch(codex_home, &previous_provider);
        return Err(format!("failed to sync the top-level model: {error}"));
    }

    let previous_metadata = metadata.clone();
    metadata.selection = Some(Selection {
        kind: SelectionKind::Account,
        id: account.id.clone(),
        name: account.name.clone(),
    });
    if let Err(error) = save_metadata(store_dir, &metadata) {
        restore_auth(&auth_path, old_auth.as_deref());
        let _ = config::switch(codex_home, &previous_provider);
        let _ = save_metadata(store_dir, &previous_metadata);
        return Err(error);
    }
    Ok((account, true))
}

/// Rename an account and/or change its model. Metadata only -- archives are
/// never read or rewritten here.
pub fn update(
    codex_home: &Path,
    store_dir: &Path,
    account_id: &str,
    name: &str,
    model: &str,
) -> Result<(AccountOption, bool), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("account name is required".to_string());
    }
    let model = model.trim();
    let mut metadata = load_metadata(store_dir)?;
    let (current_provider, providers) = config::read_state(codex_home)?;
    validate_unique_name(name, Some(account_id), &metadata, &providers)?;

    // A pending account has no archive yet, but its label is still editable.
    if let Some(pending) = metadata
        .pending_account
        .as_mut()
        .filter(|pending| pending.id == account_id)
    {
        if pending.name == name && pending.model == model {
            let account = AccountOption {
                id: pending.id.clone(),
                name: pending.name.clone(),
                model: pending.model.clone(),
            };
            return Ok((account, false));
        }
        pending.name = name.to_string();
        pending.model = model.to_string();
        let account = AccountOption {
            id: pending.id.clone(),
            name: pending.name.clone(),
            model: pending.model.clone(),
        };
        save_metadata(store_dir, &metadata)?;
        return Ok((account, true));
    }

    let index = metadata
        .accounts
        .iter()
        .position(|account| account.id == account_id)
        .ok_or("account no longer exists")?;
    if metadata.accounts[index].name == name && metadata.accounts[index].model == model {
        return Ok((metadata.accounts[index].clone(), false));
    }
    metadata.accounts[index].name = name.to_string();
    metadata.accounts[index].model = model.to_string();
    let account = metadata.accounts[index].clone();
    if let Some(selection) = metadata
        .selection
        .as_mut()
        .filter(|s| s.kind == SelectionKind::Account && s.id == account.id)
    {
        selection.name = account.name.clone();
    }
    save_metadata(store_dir, &metadata)?;

    // Editing the account that is live in official mode syncs config.toml so
    // the new model takes effect on the next Codex start.
    let is_live = current_provider.is_empty()
        && fs::read(codex_home.join("auth.json")).ok().is_some_and(|auth| {
            matching_account(store_dir, &metadata.accounts, None, &auth)
                .is_some_and(|matched| matched.id == account.id)
        });
    if is_live {
        config::set_top_level_model_in_home(codex_home, &account.model)?;
    }
    Ok((account, true))
}

/// Delete an archived account. The live login is refused in both official and
/// provider mode: `auth.json` identity is independent of the request channel,
/// and dropping its only backup would make the account unrecoverable.
pub fn delete(
    codex_home: &Path,
    store_dir: &Path,
    account_id: &str,
) -> Result<AccountOption, String> {
    let mut metadata = load_metadata(store_dir)?;
    if metadata
        .pending_account
        .as_ref()
        .is_some_and(|pending| pending.id == account_id)
    {
        let pending = metadata.pending_account.take().expect("checked above");
        save_metadata(store_dir, &metadata)?;
        return Ok(AccountOption {
            id: pending.id,
            name: pending.name,
            model: pending.model,
        });
    }

    let (_, providers) = config::read_state(codex_home)?;
    let selection = valid_selection(metadata.selection.clone(), &metadata.accounts, &providers);
    let live_id = live_account_id(codex_home, store_dir, &metadata.accounts, selection.as_ref());
    if live_id == account_id {
        return Err("the signed-in account cannot be deleted; switch away first".to_string());
    }
    if metadata.pending_account.is_some()
        && selection
            .as_ref()
            .is_some_and(|s| s.kind == SelectionKind::Account && s.id == account_id)
    {
        return Err("this account is protecting a pending sign-in".to_string());
    }

    let index = metadata
        .accounts
        .iter()
        .position(|account| account.id == account_id)
        .ok_or("account no longer exists")?;
    let account = metadata.accounts[index].clone();
    let path = archive_path(store_dir, &account.id);
    let archived = fs::read(&path).unwrap_or_default();
    let previous_metadata = metadata.clone();
    metadata.accounts.remove(index);
    if metadata
        .selection
        .as_ref()
        .is_some_and(|s| s.kind == SelectionKind::Account && s.id == account.id)
    {
        metadata.selection = None;
    }
    save_metadata(store_dir, &metadata)?;
    if path.exists() {
        if let Err(error) = fs::remove_file(&path) {
            let _ = save_metadata(store_dir, &previous_metadata);
            if !archived.is_empty() {
                let _ = write_private_file(&path, &archived);
            }
            return Err(format!("failed to delete backup {}: {error}", path.display()));
        }
    }
    Ok(account)
}

/// Result of pulling archives out of another tool's store.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub imported: Vec<AccountOption>,
    /// Already present here, matched by ChatGPT account id.
    pub skipped_existing: usize,
    /// Unreadable, or not a signed-in ChatGPT auth.json.
    pub skipped_invalid: usize,
}

/// Adopt account archives from another quick-switch store (CodexPlusPlus).
///
/// Only ever run from an explicit user action: this reads credential files out
/// of a different application's directory.
///
/// Dedup is by ChatGPT `account_id`, not by name or file id -- the same login
/// is routinely stored under different ids in the two tools, and matching on
/// anything else would create a second entry for an account already here.
pub fn import_from(
    source_store: &Path,
    codex_home: &Path,
    store_dir: &Path,
) -> Result<ImportOutcome, String> {
    let source = load_metadata(source_store)?;
    if source.accounts.is_empty() {
        return Err("no accounts found in the source store".to_string());
    }
    let mut metadata = load_metadata(store_dir)?;
    let (_, providers) = config::read_state(codex_home)?;

    let mut outcome = ImportOutcome {
        imported: Vec::new(),
        skipped_existing: 0,
        skipped_invalid: 0,
    };
    for candidate in &source.accounts {
        let bytes = match fs::read(archive_path(source_store, &candidate.id)) {
            Ok(bytes) => bytes,
            Err(_) => {
                outcome.skipped_invalid += 1;
                continue;
            }
        };
        if validate_chatgpt_auth(&bytes).is_err() {
            outcome.skipped_invalid += 1;
            continue;
        }
        if matching_account(store_dir, &metadata.accounts, None, &bytes).is_some() {
            outcome.skipped_existing += 1;
            continue;
        }

        let account = AccountOption {
            id: new_id(),
            name: unique_name(&candidate.name, &metadata, &providers),
            model: candidate.model.clone(),
        };
        if write_archive(store_dir, &account.id, &bytes).is_err() {
            outcome.skipped_invalid += 1;
            continue;
        }
        metadata.accounts.push(account.clone());
        outcome.imported.push(account);
    }

    if !outcome.imported.is_empty() {
        if let Err(error) = save_metadata(store_dir, &metadata) {
            for account in &outcome.imported {
                let _ = fs::remove_file(archive_path(store_dir, &account.id));
            }
            return Err(error);
        }
    }
    Ok(outcome)
}

/// How many accounts in the source store are not already here, matched by
/// ChatGPT account id. Comparing counts instead would be wrong in both
/// directions: extra local accounts would hide a genuine import, and unrelated
/// ones would advertise an import that turns up nothing.
pub fn importable_count(
    source_store: &Path,
    store_dir: &Path,
    accounts: &[AccountOption],
) -> usize {
    load_metadata(source_store)
        .map(|source| {
            source
                .accounts
                .iter()
                .filter(|candidate| {
                    fs::read(archive_path(source_store, &candidate.id))
                        .ok()
                        .filter(|bytes| validate_chatgpt_auth(bytes).is_ok())
                        .is_some_and(|bytes| {
                            matching_account(store_dir, accounts, None, &bytes).is_none()
                        })
                })
                .count()
        })
        .unwrap_or(0)
}

/// Names are shown in one shared menu, so an imported clash gets a suffix
/// rather than being rejected outright.
fn unique_name(
    preferred: &str,
    metadata: &Metadata,
    providers: &[config::ProviderOption],
) -> String {
    let preferred = preferred.trim();
    let base = if preferred.is_empty() { "account" } else { preferred };
    if validate_unique_name(base, None, metadata, providers).is_ok() {
        return base.to_string();
    }
    for suffix in 2..100 {
        let candidate = format!("{base} {suffix}");
        if validate_unique_name(&candidate, None, metadata, providers).is_ok() {
            return candidate;
        }
    }
    format!("{base} {}", new_id())
}

/// Remember what the user last picked, for the button label after a restart.
pub fn record_selection(store_dir: &Path, selection: Selection) -> Result<(), String> {
    let mut metadata = load_metadata(store_dir)?;
    metadata.selection = Some(selection);
    save_metadata(store_dir, &metadata)
}

/// Id of the account whose identity `auth.json` currently holds, or empty.
/// Independent of provider mode: selecting a provider only changes the request
/// channel, the signed-in identity stays the same.
pub fn live_account_id(
    codex_home: &Path,
    store_dir: &Path,
    accounts: &[AccountOption],
    selection: Option<&Selection>,
) -> String {
    fs::read(codex_home.join("auth.json"))
        .ok()
        .filter(|bytes| validate_chatgpt_auth(bytes).is_ok())
        .and_then(|bytes| matching_account(store_dir, accounts, selection, &bytes))
        .map(|account| account.id)
        .unwrap_or_default()
}

pub fn has_live_auth(codex_home: &Path) -> bool {
    fs::read(codex_home.join("auth.json"))
        .ok()
        .is_some_and(|bytes| validate_chatgpt_auth(&bytes).is_ok())
}

/// Keep only selections that still point at something that exists.
pub fn valid_selection(
    selection: Option<Selection>,
    accounts: &[AccountOption],
    providers: &[config::ProviderOption],
) -> Option<Selection> {
    selection.filter(|selection| match selection.kind {
        SelectionKind::Account => accounts.iter().any(|a| a.id == selection.id),
        SelectionKind::Provider => providers.iter().any(|p| p.id == selection.id),
    })
}

/// Write the freshest live auth back into its archive so rotated tokens do not
/// leave the snapshot stale. Best effort: a failure just means we retry later.
pub fn refresh_archive(
    store_dir: &Path,
    accounts: &[AccountOption],
    selection: Option<&Selection>,
    auth_bytes: Option<&[u8]>,
) {
    let Some(auth_bytes) = auth_bytes else {
        return;
    };
    if validate_chatgpt_auth(auth_bytes).is_err() || chatgpt_account_id(auth_bytes).is_none() {
        return;
    }
    let Some(account) = matching_account(store_dir, accounts, selection, auth_bytes) else {
        return;
    };
    let path = archive_path(store_dir, &account.id);
    if fs::read(&path).ok().is_some_and(|bytes| bytes == auth_bytes) {
        return;
    }
    let _ = write_private_file(&path, auth_bytes);
}

/// Copy the live `auth.json` into the archive of whichever account it belongs
/// to, if it has drifted. Returns true when something was written.
///
/// This is *freshness*, not a token refresh: no network call happens, and only
/// the signed-in account is touched. Archives of accounts that are not signed
/// in cannot be updated by anyone -- their tokens age until that account is
/// signed into again.
pub fn refresh_live_archive(codex_home: &Path, store_dir: &Path) -> bool {
    let Ok(metadata) = load_metadata(store_dir) else {
        return false;
    };
    if metadata.accounts.is_empty() {
        return false;
    }
    let Ok(live) = fs::read(codex_home.join("auth.json")) else {
        return false;
    };
    let selection = metadata.selection.clone();
    let before = matching_account(store_dir, &metadata.accounts, selection.as_ref(), &live)
        .map(|account| archive_path(store_dir, &account.id))
        .and_then(|path| fs::read(path).ok());
    refresh_archive(store_dir, &metadata.accounts, selection.as_ref(), Some(&live));
    before.is_some_and(|bytes| bytes != live)
}

pub fn load_metadata(store_dir: &Path) -> Result<Metadata, String> {
    let path = store_dir.join(METADATA_FILE);
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|e| format!("failed to parse {}: {e}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Metadata::default()),
        Err(error) => Err(format!("failed to read {}: {error}", path.display())),
    }
}

fn save_metadata(store_dir: &Path, metadata: &Metadata) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(metadata).map_err(|e| e.to_string())?;
    write_private_file(&store_dir.join(METADATA_FILE), &bytes)
}

/// Names are shared between accounts and providers: the switcher shows one
/// list, so a duplicate label would be ambiguous.
fn validate_unique_name(
    name: &str,
    excluded_id: Option<&str>,
    metadata: &Metadata,
    providers: &[config::ProviderOption],
) -> Result<(), String> {
    if metadata
        .accounts
        .iter()
        .any(|a| Some(a.id.as_str()) != excluded_id && a.name.eq_ignore_ascii_case(name))
    {
        return Err(format!("\"{name}\" is already used by an account"));
    }
    if providers
        .iter()
        .any(|p| Some(p.id.as_str()) != excluded_id && p.name.eq_ignore_ascii_case(name))
    {
        return Err(format!("\"{name}\" is already used by a provider"));
    }
    if metadata
        .pending_account
        .as_ref()
        .is_some_and(|p| Some(p.id.as_str()) != excluded_id && p.name.eq_ignore_ascii_case(name))
    {
        return Err(format!("\"{name}\" is waiting for sign-in"));
    }
    Ok(())
}

/// Find the archive matching a live auth blob. The recorded selection breaks
/// ties when the same identity was saved under more than one name.
fn matching_account(
    store_dir: &Path,
    accounts: &[AccountOption],
    selection: Option<&Selection>,
    auth_bytes: &[u8],
) -> Option<AccountOption> {
    let archived_matches = |account: &AccountOption| {
        fs::read(archive_path(store_dir, &account.id))
            .ok()
            .is_some_and(|bytes| same_chatgpt_account(&bytes, auth_bytes))
    };
    if let Some(selected) = selection
        .filter(|selection| selection.kind == SelectionKind::Account)
        .and_then(|selection| accounts.iter().find(|a| a.id == selection.id))
        .filter(|account| archived_matches(account))
    {
        return Some(selected.clone());
    }
    accounts.iter().find(|a| archived_matches(a)).cloned()
}

/// Compare by the stable `account_id` when both sides have one. Without it the
/// only safe test is byte equality: access tokens rotate, so they must never
/// be used to decide identity.
fn same_chatgpt_account(left: &[u8], right: &[u8]) -> bool {
    match (chatgpt_account_id(left), chatgpt_account_id(right)) {
        (Some(left_id), Some(right_id)) => left_id == right_id,
        _ => left == right,
    }
}

fn chatgpt_account_id(auth_bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(auth_bytes).ok()?;
    value
        .get("tokens")
        .and_then(Value::as_object)
        .and_then(|tokens| tokens.get("account_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// Refuse to archive anything that is not a signed-in ChatGPT auth.json, so a
/// half-written or API-key file can never overwrite a good backup.
fn validate_chatgpt_auth(auth_bytes: &[u8]) -> Result<(), String> {
    let value: Value =
        serde_json::from_slice(auth_bytes).map_err(|_| "auth.json is not valid JSON".to_string())?;
    let object = value
        .as_object()
        .ok_or("auth.json must be a JSON object".to_string())?;
    let is_chatgpt = object
        .get("auth_mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode.eq_ignore_ascii_case("chatgpt"));
    let has_token = object
        .get("tokens")
        .and_then(Value::as_object)
        .is_some_and(|tokens| {
            ["access_token", "id_token", "refresh_token"].iter().any(|key| {
                tokens
                    .get(*key)
                    .and_then(Value::as_str)
                    .is_some_and(|token| !token.trim().is_empty())
            })
        });
    if !is_chatgpt || !has_token {
        return Err("auth.json is not a signed-in ChatGPT account".to_string());
    }
    Ok(())
}

/// Write an archive and read it back before the caller commits to it.
fn write_archive(store_dir: &Path, account_id: &str, auth_bytes: &[u8]) -> Result<(), String> {
    let path = archive_path(store_dir, account_id);
    write_private_file(&path, auth_bytes)?;
    let written = fs::read(&path)
        .map_err(|e| format!("failed to verify backup {}: {e}", path.display()))?;
    if written != auth_bytes {
        let _ = fs::remove_file(&path);
        return Err("account backup failed verification".to_string());
    }
    Ok(())
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    atomic_write(path, bytes).map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("failed to lock down {}: {e}", path.display()))?;
    }
    Ok(())
}

fn restore_auth(path: &Path, bytes: Option<&[u8]>) {
    match bytes {
        Some(bytes) => {
            let _ = write_private_file(path, bytes);
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
}

fn restore_file(path: &Path, bytes: Option<&[u8]>) {
    match bytes {
        Some(bytes) => {
            let _ = atomic_write(path, bytes);
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
}

/// Archive paths are derived from the generated id, never from user input.
fn archive_path(store_dir: &Path, account_id: &str) -> PathBuf {
    store_dir
        .join(ACCOUNTS_DIR)
        .join(format!("{account_id}.auth.json"))
}
