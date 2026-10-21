//! Commands for live (IMAP/SMTP) accounts: setup, background sync, sending and server flags.
//! Sync runs on its own database connection so the UI stays responsive.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use oxymail_core::{AccountUpdate, Store};
use oxymail_sync::autoconfig::{self, AccountConfig};
use oxymail_sync::smtp::Outgoing;
use oxymail_sync::{imap, secrets, smtp};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{AppState, Res, err};

/// How often every live account is checked for new mail.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(180);

#[derive(Default)]
pub struct SyncState {
    running: Mutex<HashSet<i64>>,
}

/// Error text including the cause chain ("login failed: … : …").
fn full(e: anyhow::Error) -> String {
    format!("{e:#}")
}

fn cache_root() -> PathBuf {
    Store::default_path().parent().map(|p| p.join("mail")).unwrap_or_else(|| PathBuf::from("mail"))
}

fn config_of(json: &str) -> Res<AccountConfig> {
    serde_json::from_str(json).map_err(|e| format!("account settings are unreadable: {e}"))
}

#[tauri::command]
pub async fn discover_account(email: String) -> Res<AccountConfig> {
    autoconfig::discover(&email).await.map_err(full)
}

/// Tests IMAP and SMTP with the password, saves it to the keyring, adds the account and starts
/// the first sync.
#[tauri::command]
pub async fn add_account(
    app: AppHandle,
    state: State<'_, AppState>,
    config: AccountConfig,
    password: String,
    sender_name: Option<String>,
) -> Res<i64> {
    imap::test_login(&config, &password).await.map_err(|e| format!("Incoming mail (IMAP): {}", full(e)))?;
    smtp::test_login(&config, &password).await.map_err(|e| format!("Outgoing mail (SMTP): {}", full(e)))?;
    secrets::save_password(&config.email, &password).map_err(full)?;
    let json = serde_json::to_string(&config).map_err(err)?;
    let id = {
        let store = state.0.lock().map_err(err)?;
        let id = store.add_live_account(&config.email, &json, &cache_root()).map_err(err)?;
        if sender_name.as_deref().is_some_and(|n| !n.trim().is_empty()) {
            store
                .update_account(id, &AccountUpdate { label: None, sender_name, color: None, signature: None, icon: None })
                .map_err(err)?;
        }
        id
    };
    let handle = app.clone();
    tauri::async_runtime::spawn(async move { sync_accounts(handle, Some(id)).await });
    Ok(id)
}

/// Suggested settings for connecting an imported account: the provider's published settings,
/// overridden by what Mailbird was using. POP3 accounts are moved to IMAP on the same server.
#[tauri::command]
pub async fn connect_suggestion(state: State<'_, AppState>, account_id: i64) -> Res<AccountConfig> {
    use oxymail_sync::autoconfig::{Security, Server};
    let (email, mailbird) = {
        let store = state.0.lock().map_err(err)?;
        let email = store.accounts().map_err(err)?.into_iter().find(|a| a.id == account_id).ok_or("no such account")?.name;
        (email, store.mailbird_server(account_id).map_err(err)?)
    };
    let mut config = autoconfig::discover(&email).await.map_err(full)?;
    let Some(mb) = mailbird else { return Ok(config) };
    let security = |tls: bool| if tls { Security::Tls } else { Security::StartTls };
    match mb.kind.as_str() {
        "exchange" => return Err("This was an Exchange account in Mailbird. OxyMail can't connect to Exchange yet.".into()),
        "imap" => config.imap = Server { host: mb.host.clone(), port: mb.port, security: security(mb.tls) },
        "pop3" => {
            config.imap = Server { host: mb.host.clone(), port: 993, security: Security::Tls };
            config.source = "your Mailbird settings (POP3 there; OxyMail uses IMAP on the same server)".into();
        }
        _ => {}
    }
    if let (Some(host), Some(port)) = (mb.smtp_host.clone(), mb.smtp_port) {
        config.smtp = Server { host, port, security: security(mb.smtp_tls.unwrap_or(true)) };
    }
    if !mb.username.is_empty() {
        config.username = mb.username.clone();
    }
    if mb.kind == "imap" {
        config.source = "your Mailbird settings".into();
    }
    Ok(config)
}

/// Connects an imported archive account to its server: it becomes live and keeps its archive.
#[tauri::command]
pub async fn connect_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: i64,
    config: AccountConfig,
    password: String,
) -> Res<()> {
    imap::test_login(&config, &password).await.map_err(|e| format!("Incoming mail (IMAP): {}", full(e)))?;
    smtp::test_login(&config, &password).await.map_err(|e| format!("Outgoing mail (SMTP): {}", full(e)))?;
    secrets::save_password(&config.email, &password).map_err(full)?;
    let json = serde_json::to_string(&config).map_err(err)?;
    state.0.lock().map_err(err)?.connect_archive(account_id, &json, &cache_root()).map_err(err)?;
    tauri::async_runtime::spawn(async move { sync_accounts(app, Some(account_id)).await });
    Ok(())
}

/// A live account's current server settings, for the edit form.
#[tauri::command]
pub fn live_config(state: State<'_, AppState>, account_id: i64) -> Res<AccountConfig> {
    let store = state.0.lock().map_err(err)?;
    let live = store.live_account(account_id).map_err(err)?.ok_or("not a live account")?;
    config_of(&live.config)
}

/// Tests and saves new server settings, and a new password if one is given, then syncs.
#[tauri::command]
pub async fn update_server_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: i64,
    config: AccountConfig,
    password: Option<String>,
) -> Res<()> {
    let password = match password.filter(|p| !p.is_empty()) {
        Some(p) => p,
        None => secrets::password(&config.email).map_err(full)?,
    };
    imap::test_login(&config, &password).await.map_err(|e| format!("Incoming mail (IMAP): {}", full(e)))?;
    smtp::test_login(&config, &password).await.map_err(|e| format!("Outgoing mail (SMTP): {}", full(e)))?;
    secrets::save_password(&config.email, &password).map_err(full)?;
    let json = serde_json::to_string(&config).map_err(err)?;
    state.0.lock().map_err(err)?.set_live_config(account_id, &json).map_err(err)?;
    tauri::async_runtime::spawn(async move { sync_accounts(app, Some(account_id)).await });
    Ok(())
}

#[tauri::command]
pub async fn sync_now(app: AppHandle, account_id: Option<i64>) -> Res<()> {
    tauri::async_runtime::spawn(async move { sync_accounts(app, account_id).await });
    Ok(())
}

/// Syncs every live account (or just one), one at a time, reporting through events:
/// `sync-status` while working and `sync-done` with a report or an error for each account.
pub async fn sync_accounts(app: AppHandle, only: Option<i64>) {
    let Ok(mut store) = Store::open(&Store::default_path()) else { return };
    let Ok(accounts) = store.live_accounts() else { return };
    for account in accounts.into_iter().filter(|a| only.is_none_or(|id| id == a.id)) {
        let sync = app.state::<SyncState>();
        if !sync.running.lock().map(|mut r| r.insert(account.id)).unwrap_or(false) {
            continue;
        }
        let result = async {
            let config = config_of(&account.config)?;
            let password = secrets::password(&config.email).map_err(full)?;
            let status = app.clone();
            let id = account.id;
            imap::sync_account(&mut store, account.id, &config, &password, move |text| {
                let _ = status.emit("sync-status", json!({ "account_id": id, "text": text }));
            })
            .await
            .map_err(full)
        }
        .await;
        let payload = match result {
            Ok(report) => json!({ "account_id": account.id, "report": report }),
            Err(error) => json!({ "account_id": account.id, "error": error }),
        };
        let _ = app.emit("sync-done", payload);
        if let Ok(mut running) = app.state::<SyncState>().running.lock() {
            running.remove(&account.id);
        }
    }
}

/// Sends a message from a live account, files it in Sent, then syncs that account.
#[tauri::command]
pub async fn send_message(app: AppHandle, state: State<'_, AppState>, account_id: i64, mail: Outgoing) -> Res<String> {
    let (config, sender_name) = {
        let store = state.0.lock().map_err(err)?;
        let live = store.live_account(account_id).map_err(err)?.ok_or("only live accounts can send")?;
        let sender = store
            .accounts()
            .map_err(err)?
            .into_iter()
            .find(|a| a.id == account_id)
            .and_then(|a| a.sender_name);
        (config_of(&live.config)?, sender)
    };
    let password = secrets::password(&config.email).map_err(full)?;
    let raw = smtp::build(sender_name.as_deref(), &config.email, &mail).map_err(full)?;
    smtp::send(&config, &password, &mail, &raw).await.map_err(full)?;

    // Gmail files sent mail by itself; everyone else needs an APPEND.
    let mut note = "Sent".to_string();
    if !config.smtp.host.ends_with("gmail.com") {
        if let Err(e) = imap::append_sent(&config, &password, &raw).await {
            note = format!("Sent, but it couldn't be saved to the Sent folder: {}", full(e));
        }
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move { sync_accounts(handle, Some(account_id)).await });
    Ok(note)
}

/// Mirrors read/starred changes to the server for messages in live accounts. Runs in the
/// background; failures arrive as `sync-error` events.
pub fn push_flags(app: &AppHandle, store: &Store, ids: &[i64], flag: char, on: bool) {
    let Ok(refs) = store.remote_refs(ids) else { return };
    if refs.is_empty() {
        return;
    }
    let mut groups: BTreeMap<(i64, String), Vec<u32>> = BTreeMap::new();
    for (account, folder, uid) in refs {
        groups.entry((account, folder)).or_default().push(uid);
    }
    let configs: BTreeMap<i64, String> = store
        .live_accounts()
        .unwrap_or_default()
        .into_iter()
        .map(|a| (a.id, a.config))
        .collect();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for ((account, folder), uids) in groups {
            let result = async {
                let config = config_of(configs.get(&account).ok_or("account missing")?)?;
                let password = secrets::password(&config.email).map_err(full)?;
                imap::store_flag(&config, &password, &folder, &uids, flag, on).await.map_err(full)
            }
            .await;
            if let Err(error) = result {
                let _ = app.emit("sync-error", json!({ "account_id": account, "error": error }));
            }
        }
    });
}

/// Refuses moves and deletes on live mail until two-way sync lands, so the index never
/// disagrees with the server.
pub fn refuse_live(store: &Store, ids: &[i64]) -> Res<()> {
    if !store.remote_refs(ids).map_err(err)?.is_empty() {
        return Err("Archiving, deleting and moving live mail arrive in the next update. \
                    For now these work on archive accounts only."
            .into());
    }
    Ok(())
}

pub fn forget_password(store: &Store, account_id: i64) {
    if let Ok(Some(live)) = store.live_account(account_id) {
        let _ = secrets::forget_password(&live.email);
    }
}
