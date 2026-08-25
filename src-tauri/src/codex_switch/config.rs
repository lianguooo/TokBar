//! Provider side of the Codex quick switch: everything that lives in
//! `$CODEX_HOME/config.toml`.
//!
//! Ported from CodexPlusPlus `codex_provider.rs`, with `anyhow` swapped for
//! TokBar's `Result<T, String>` convention and the `reqwest` URL parser
//! replaced by a small scheme/host check so no new HTTP dependency is pulled
//! in. Edits go through `toml_edit` so unrelated user config (comments,
//! ordering, MCP servers, …) survives untouched.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Serialize;
use toml_edit::{value, DocumentMut, Item, Table};

use super::{atomic_write, new_id};

/// Sidecar that remembers the official (no-provider) top-level model while a
/// provider is active, so switching back restores what the user had.
const OFFICIAL_MODEL_BACKUP: &str = ".tokbar-official-model.bak";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOption {
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// Echoed back so the edit form can prefill it. Never leaves the machine:
    /// it only travels over Tauri's in-process IPC.
    pub experimental_bearer_token: String,
    /// Model written to the top-level `model` key when this provider is selected.
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSwitchResult {
    pub current_provider: String,
    pub changed: bool,
}

/// Read the current provider id (empty = official mode) and every configured
/// provider. A missing or empty config.toml is treated as official mode with
/// no providers, not as an error.
pub fn read_state(home: &Path) -> Result<(String, Vec<ProviderOption>), String> {
    let (_, document) = read_document(home)?;
    Ok((current_provider(&document), options(&document)))
}

pub fn create(
    home: &Path,
    name: &str,
    base_url: &str,
    bearer_token: &str,
    model: &str,
) -> Result<ProviderOption, String> {
    let fields = validate_fields(name, base_url, bearer_token, model)?;
    let (config_path, mut document) = read_document(home)?;
    if options(&document)
        .iter()
        .any(|provider| provider.name.eq_ignore_ascii_case(fields.name))
    {
        return Err(format!("provider name \"{}\" already exists", fields.name));
    }

    // Internal id is decoupled from the display name so user input can never
    // leak spaces or dots into the TOML key path.
    let provider_id = format!("provider-{}", new_id());
    let mut table = Table::new();
    table["name"] = value(fields.name);
    table["base_url"] = value(fields.base_url);
    table["experimental_bearer_token"] = value(fields.bearer_token);
    table["model"] = value(fields.model);
    let providers = document
        .as_table_mut()
        .entry("model_providers")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or("config.toml: model_providers must be a table")?;
    providers.insert(&provider_id, Item::Table(table));

    // Creating a provider selects it, mirroring the upstream behaviour.
    backup_official_model(home, &document);
    document["model_provider"] = value(&provider_id);
    set_top_level_model(&mut document, fields.model);
    write_document(&config_path, &document)?;

    options(&document)
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| "provider was written but cannot be read back".to_string())
}

pub fn update(
    home: &Path,
    provider_id: &str,
    name: &str,
    base_url: &str,
    bearer_token: &str,
    model: &str,
) -> Result<(ProviderOption, bool), String> {
    let fields = validate_fields(name, base_url, bearer_token, model)?;
    let (config_path, mut document) = read_document(home)?;
    let existing = options(&document);
    let previous = existing
        .iter()
        .find(|provider| provider.id == provider_id)
        .cloned()
        .ok_or("provider no longer exists")?;
    if existing
        .iter()
        .any(|p| p.id != provider_id && p.name.eq_ignore_ascii_case(fields.name))
    {
        return Err(format!("provider name \"{}\" already exists", fields.name));
    }
    let changed = previous.name != fields.name
        || previous.base_url != fields.base_url
        || previous.experimental_bearer_token != fields.bearer_token
        || previous.model != fields.model;
    if !changed {
        return Ok((previous, false));
    }

    let selected = current_provider(&document);
    let providers = document
        .get_mut("model_providers")
        .and_then(Item::as_table_mut)
        .ok_or("config.toml: model_providers must be a table")?;
    let item = providers.get_mut(provider_id).ok_or("provider no longer exists")?;
    // Both a standard table and a legacy inline table are accepted; only the
    // four fields the form owns are overwritten, extras are left alone.
    if let Some(table) = item.as_table_mut() {
        table["name"] = value(fields.name);
        table["base_url"] = value(fields.base_url);
        table["experimental_bearer_token"] = value(fields.bearer_token);
        table["model"] = value(fields.model);
    } else if let Some(table) = item.as_inline_table_mut() {
        table.insert("name", fields.name.into());
        table.insert("base_url", fields.base_url.into());
        table.insert("experimental_bearer_token", fields.bearer_token.into());
        table.insert("model", fields.model.into());
    } else {
        return Err("provider entry must be a table".to_string());
    }
    // Editing the live provider keeps the top-level model in sync.
    if selected == provider_id {
        set_top_level_model(&mut document, fields.model);
    }
    write_document(&config_path, &document)?;

    let provider = options(&document)
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| "provider was written but cannot be read back".to_string())?;
    Ok((provider, true))
}

/// Delete a provider. The selected one is refused: removing it would leave
/// `model_provider` pointing at a missing table and break Codex on next start.
pub fn delete(home: &Path, provider_id: &str) -> Result<ProviderOption, String> {
    let (config_path, mut document) = read_document(home)?;
    if current_provider(&document) == provider_id {
        return Err("the active provider cannot be deleted; switch away first".to_string());
    }
    let provider = options(&document)
        .into_iter()
        .find(|provider| provider.id == provider_id)
        .ok_or("provider no longer exists")?;
    let providers = document
        .get_mut("model_providers")
        .and_then(Item::as_table_mut)
        .ok_or("config.toml: model_providers must be a table")?;
    if providers.remove(provider_id).is_none() {
        return Err("provider no longer exists".to_string());
    }
    write_document(&config_path, &document)?;
    Ok(provider)
}

/// Point `model_provider` at `provider` (empty string = official mode) and keep
/// the top-level `model` consistent with that choice.
pub fn switch(home: &Path, provider: &str) -> Result<ProviderSwitchResult, String> {
    let (config_path, mut document) = read_document(home)?;
    let previous = current_provider(&document);
    if !provider.is_empty()
        && !options(&document)
            .iter()
            .any(|option| option.id == provider)
    {
        return Err(format!("provider \"{provider}\" is not in config.toml"));
    }
    if previous == provider {
        return Ok(ProviderSwitchResult {
            current_provider: previous,
            changed: false,
        });
    }

    if provider.is_empty() {
        document.as_table_mut().remove("model_provider");
        restore_official_model(home, &mut document);
    } else {
        backup_official_model(home, &document);
        document["model_provider"] = value(provider);
        let model = options(&document)
            .into_iter()
            .find(|option| option.id == provider)
            .map(|option| option.model)
            .unwrap_or_default();
        if !model.is_empty() {
            set_top_level_model(&mut document, &model);
        }
    }
    write_document(&config_path, &document)?;
    Ok(ProviderSwitchResult {
        current_provider: current_provider(&document),
        changed: true,
    })
}

/// Current top-level `model`, or an empty string when unset.
pub fn top_level_model(home: &Path) -> String {
    read_document(home)
        .ok()
        .and_then(|(_, document)| top_model(&document))
        .unwrap_or_default()
}

/// Write the top-level `model`. An empty model is a no-op, so callers can pass
/// through an account with no recorded model without clobbering config.
pub fn set_top_level_model_in_home(home: &Path, model: &str) -> Result<(), String> {
    let model = model.trim();
    if model.is_empty() {
        return Ok(());
    }
    let (config_path, mut document) = read_document(home)?;
    set_top_level_model(&mut document, model);
    write_document(&config_path, &document)
}

struct Fields<'a> {
    name: &'a str,
    base_url: &'a str,
    bearer_token: &'a str,
    model: &'a str,
}

fn validate_fields<'a>(
    name: &'a str,
    base_url: &'a str,
    bearer_token: &'a str,
    model: &'a str,
) -> Result<Fields<'a>, String> {
    let fields = Fields {
        name: name.trim(),
        base_url: base_url.trim(),
        bearer_token: bearer_token.trim(),
        model: model.trim(),
    };
    if fields.name.is_empty() {
        return Err("provider name is required".to_string());
    }
    if fields.base_url.is_empty() {
        return Err("base_url is required".to_string());
    }
    if fields.bearer_token.is_empty() {
        return Err("experimental_bearer_token is required".to_string());
    }
    if fields.model.is_empty() {
        return Err("model is required".to_string());
    }
    validate_base_url(fields.base_url)?;
    Ok(fields)
}

/// Scheme + host check without pulling in a URL crate: enough to stop the
/// obvious typos that would otherwise land in config.toml and break Codex.
fn validate_base_url(raw: &str) -> Result<(), String> {
    let lower = raw.to_ascii_lowercase();
    let after_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .ok_or("base_url must start with http:// or https://")?;
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = match host_port.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => host_port.split(':').next().unwrap_or_default(),
    };
    if host.is_empty() || host.contains(' ') {
        return Err("base_url must contain a host name".to_string());
    }
    Ok(())
}

fn write_document(config_path: &Path, document: &DocumentMut) -> Result<(), String> {
    let mut text = document.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    atomic_write(config_path, text.as_bytes())
        .map_err(|e| format!("failed to write {}: {e}", config_path.display()))
}

fn top_model(document: &DocumentMut) -> Option<String> {
    document
        .get("model")
        .and_then(Item::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
}

fn set_top_level_model(document: &mut DocumentMut, model: &str) {
    if top_model(document).as_deref() != Some(model) {
        document["model"] = value(model);
    }
}

/// Snapshot the official model the first time we leave official mode. An
/// existing backup is never overwritten, so the restore always returns the
/// model the user had before any provider was selected.
fn backup_official_model(home: &Path, document: &DocumentMut) {
    if !current_provider(document).is_empty() {
        return;
    }
    let backup_path = home.join(OFFICIAL_MODEL_BACKUP);
    if backup_path.exists() {
        return;
    }
    let Some(model) = top_model(document) else {
        return;
    };
    let _ = std::fs::write(&backup_path, model);
}

fn restore_official_model(home: &Path, document: &mut DocumentMut) {
    let backup_path = home.join(OFFICIAL_MODEL_BACKUP);
    let Ok(backup) = std::fs::read_to_string(&backup_path) else {
        return;
    };
    let model = backup.trim();
    if !model.is_empty() {
        set_top_level_model(document, model);
    }
    // Clearing it means the next departure from official mode snapshots the
    // then-current model rather than a stale one.
    let _ = std::fs::remove_file(&backup_path);
}

fn read_document(home: &Path) -> Result<(PathBuf, DocumentMut), String> {
    let config_path = home.join("config.toml");
    let contents = match std::fs::read_to_string(&config_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(format!("failed to read {}: {error}", config_path.display()));
        }
    };
    let document = if contents.trim().is_empty() {
        DocumentMut::new()
    } else {
        contents
            .parse::<DocumentMut>()
            .map_err(|e| format!("failed to parse {}: {e}", config_path.display()))?
    };
    Ok((config_path, document))
}

fn current_provider(document: &DocumentMut) -> String {
    document
        .get("model_provider")
        .and_then(Item::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Read a string field from a provider entry, accepting both table and legacy
/// inline-table shapes.
fn field(item: &Item, key: &str) -> Option<String> {
    item.as_table()
        .and_then(|table| table.get(key))
        .and_then(Item::as_str)
        .or_else(|| {
            item.as_inline_table()
                .and_then(|table| table.get(key))
                .and_then(toml_edit::Value::as_str)
        })
        .map(str::to_string)
}

fn options(document: &DocumentMut) -> Vec<ProviderOption> {
    document
        .get("model_providers")
        .and_then(Item::as_table)
        .map(|table| {
            table
                .iter()
                .filter_map(|(id, item)| {
                    if item.as_table().is_none() && item.as_inline_table().is_none() {
                        return None;
                    }
                    let name = field(item, "name")
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| id.to_string());
                    Some(ProviderOption {
                        id: id.to_string(),
                        name,
                        base_url: field(item, "base_url").unwrap_or_default(),
                        experimental_bearer_token: field(item, "experimental_bearer_token")
                            .unwrap_or_default(),
                        model: field(item, "model").unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
