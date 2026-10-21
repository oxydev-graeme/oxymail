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
    /// Accounts with an Inbox push (IDLE) watcher running.
    watching: Mutex<HashSet<i64>>,
}

/// RFC 2177 asks clients to re-issue IDLE at least every 29 minutes.
const IDLE_WINDOW: Duration = Duration::from_secs(25 * 60);

/// Starts an Inbox push watcher for every live account that doesn't have one. Called at
/// start-up and after each periodic sync, so new accounts are picked up.
pub fn ensure_watchers(app: &AppHandle) {
    let Ok(store) = Store::open(&Store::default_path()) else { return };
    for account in store.live_accounts().unwrap_or_default() {
        let fresh = app.state::<SyncState>().watching.lock().map(|mut w| w.insert(account.id)).unwrap_or(false);
        if fresh {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { watch_inbox(app, account.id).await });
        }
    }
}

/// Keeps an IDLE connection to one account's Inbox and syncs the Inbox when it changes. Stops
/// when the account is removed or the server has no IDLE (the 3-minute sync still covers it).
async fn watch_inbox(app: AppHandle, account_id: i64) {
    let mut failures = 0u32;
    loop {
        let Some(account) = Store::open(&Store::default_path()).ok().and_then(|s| s.live_account(account_id).ok().flatten()) else {
            break;
        };
        let Ok(config) = config_of(&account.config) else { break };
        let outcome = match oxymail_sync::credential(&config).await {
            Ok(secret) => imap::idle_inbox(&config, &secret, IDLE_WINDOW).await,
            Err(e) => Err(e),
        };
        match outcome {
            Ok(true) => {
                failures = 0;
                sync_accounts(app.clone(), Some(account_id)).await;
            }
            Ok(false) => failures = 0,
            Err(e) if format!("{e:#}").contains("doesn't support push") => break,
            Err(_) => {
                // Offline, asleep or signed out: back off, up to 10 minutes between tries.
                failures += 1;
                tokio::time::sleep(Duration::from_secs((30 * 2u64.pow(failures.min(5))).min(600))).await;
            }
        }
    }
    if let Ok(mut watching) = app.state::<SyncState>().watching.lock() {
        watching.remove(&account_id);
    }
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
    password: Option<String>,
    sender_name: Option<String>,
) -> Res<i64> {
    verify(&config, password).await?;
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
    password: Option<String>,
) -> Res<()> {
    verify(&config, password).await?;
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
    verify(&config, password).await?;
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
            let password = oxymail_sync::credential(&config).await.map_err(full)?;
            let status = app.clone();
            let id = account.id;
            imap::sync_account(&mut store, account.id, &config, &password, move |text| {
                let _ = status.emit("sync-status", json!({ "account_id": id, "text": text }));
            })
            .await
            .map_err(full)
        }
        .await;
        if let Ok(report) = &result {
            notify_new_mail(&app, &store, &account.email, report);
        }
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

/// A desktop notification for new Inbox mail, unless turned off in Settings.
fn notify_new_mail(app: &AppHandle, store: &Store, account: &str, report: &imap::SyncReport) {
    use tauri_plugin_notification::NotificationExt;
    if report.new_inbox == 0 || store.settings().ok().and_then(|s| s.get("notifications").cloned()).as_deref() == Some("false") {
        return;
    }
    let (title, body) = match report.new_inbox_preview.as_slice() {
        [(from, subject)] if report.new_inbox == 1 => (from.clone(), subject.clone()),
        previews => (
            format!("{} new messages · {account}", report.new_inbox),
            previews.iter().map(|(f, s)| format!("{f}: {s}")).collect::<Vec<_>>().join("\n"),
        ),
    };
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Sends a message from a live account, files it in Sent, then syncs that account.
#[tauri::command]
pub async fn send_message(app: AppHandle, account_id: i64, mail: Outgoing) -> Res<String> {
    deliver(&app, account_id, mail).await
}

/// Builds and sends a message, files it in Sent and syncs the account. Used by Send, the
/// undo-send timer and scheduled sending; opens its own database connection.
pub async fn deliver(app: &AppHandle, account_id: i64, mut mail: Outgoing) -> Res<String> {
    let (config, sender_name) = {
        let store = Store::open(&Store::default_path()).map_err(err)?;
        if let Some(original) = mail.forward_from {
            use base64::Engine;
            for index in mail.forward_attachments.clone() {
                let (name, data) = store.attachment(original, index).map_err(err)?;
                let content_type = store
                    .message(original)
                    .ok()
                    .and_then(|v| v.attachments.into_iter().find(|a| a.index == index).map(|a| a.content_type))
                    .unwrap_or_else(|| "application/octet-stream".into());
                mail.attachments.push(oxymail_sync::smtp::OutgoingAttachment {
                    name,
                    content_type,
                    data: base64::engine::general_purpose::STANDARD.encode(data),
                });
            }
            mail.forward_attachments.clear();
            mail.forward_from = None;
        }
        let live = store.live_account(account_id).map_err(err)?.ok_or("only live accounts can send")?;
        let sender = store.accounts().map_err(err)?.into_iter().find(|a| a.id == account_id).and_then(|a| a.sender_name);
        (config_of(&live.config)?, sender)
    };
    let password = oxymail_sync::credential(&config).await.map_err(full)?;
    let raw = smtp::build(sender_name.as_deref(), &config.email, &mail).map_err(full)?;
    smtp::send(&config, &password, &mail, &raw).await.map_err(full)?;
    if let Ok(store) = Store::open(&Store::default_path()) {
        let everyone: Vec<String> = mail.to.iter().chain(&mail.cc).chain(&mail.bcc).cloned().collect();
        let _ = store.note_sent_to(&everyone);
    }

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

/// Queues a message for later. `mail` is the outgoing message, `draft` the Compose state.
#[tauri::command]
pub fn schedule_send(state: State<'_, AppState>, account_id: i64, send_at: i64, mail: Outgoing, draft: String) -> Res<i64> {
    let summary = format!("{} — {}", mail.to.first().cloned().unwrap_or_default(), mail.subject);
    let json = serde_json::to_string(&mail).map_err(err)?;
    state.0.lock().map_err(err)?.schedule(account_id, send_at, &json, &draft, &summary).map_err(err)
}

/// Runs every 30 seconds: brings back snoozed mail and sends scheduled messages that are due.
pub async fn tick(app: &AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let Ok(mut store) = Store::open(&Store::default_path()) else { return };

    if let Ok(woke) = store.wake_snoozed(now) {
        let accounts: HashSet<i64> = woke.iter().map(|m| m.account_id).collect();
        for account_id in accounts {
            let _ = app.emit("mail-changed", json!({ "account_id": account_id }));
        }
        if let Some(first) = woke.first() {
            let title = if woke.len() == 1 { "Snoozed message is back".to_string() } else { format!("{} snoozed messages are back", woke.len()) };
            let body = format!("{}: {}", first.from_name.clone().or(first.from_addr.clone()).unwrap_or_default(), first.subject.clone().unwrap_or_default());
            let _ = app.notification().builder().title(title).body(body).show();
        }
    }

    for item in store.scheduled().unwrap_or_default().into_iter().filter(|s| s.send_at <= now && s.error.is_none()) {
        let Ok(mail) = serde_json::from_str::<Outgoing>(&item.mail) else {
            let _ = store.set_schedule_error(item.id, Some("the saved message couldn't be read"));
            continue;
        };
        match deliver(app, item.account_id, mail).await {
            Ok(_) => {
                let _ = store.unschedule(item.id);
                let _ = app.notification().builder().title("Scheduled message sent").body(item.summary.clone().unwrap_or_default()).show();
            }
            Err(e) => {
                let _ = store.set_schedule_error(item.id, Some(&e));
            }
        }
        let _ = app.emit("scheduled-changed", ());
    }
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
                let password = oxymail_sync::credential(&config).await.map_err(full)?;
                imap::store_flag(&config, &password, &folder, &uids, flag, on).await.map_err(full)
            }
            .await;
            if let Err(error) = result {
                let _ = app.emit("sync-error", json!({ "account_id": account, "error": error }));
            }
        }
    });
}

/// What an action asks of each message.
enum Action {
    /// To the account folder with this role (`inbox`, `archive`, `trash`).
    Role(&'static str),
    /// To a specific folder (undo, or moving to a chosen folder).
    Folder(i64),
    /// To Trash, or permanently when already in Trash.
    Delete,
    /// Back to where it was imported from, or the Inbox.
    Restore,
}

#[tauri::command]
pub async fn move_to(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>, role: String) -> Res<()> {
    let role = match role.as_str() {
        "inbox" => "inbox",
        "archive" => "archive",
        _ => return Err(format!("cannot move to {role}")),
    };
    act(&app, &state, ids, Action::Role(role)).await
}

#[tauri::command]
pub async fn move_to_folder(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>, folder_id: i64) -> Res<()> {
    act(&app, &state, ids, Action::Folder(folder_id)).await
}

#[tauri::command]
pub async fn delete(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>) -> Res<()> {
    act(&app, &state, ids, Action::Delete).await
}

#[tauri::command]
pub async fn restore(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>) -> Res<()> {
    act(&app, &state, ids, Action::Restore).await
}

/// Applies an action. Archive-only messages change in the index; messages on a server are
/// moved or deleted there first, then the affected folders are re-synced so the index matches.
async fn act(app: &AppHandle, state: &State<'_, AppState>, ids: Vec<i64>, action: Action) -> Res<()> {
    let (local, remote) = {
        let store = state.0.lock().map_err(err)?;
        let ids = store.resolve_current(&ids).map_err(err)?;
        let mut local = Vec::new();
        let mut remote: BTreeMap<(i64, String), Vec<oxymail_core::RemoteRef>> = BTreeMap::new();
        for id in ids {
            match store.remote_ref(id).map_err(err)? {
                Some(r) => remote.entry((r.account_id, r.remote_name.clone())).or_default().push(r),
                None => local.push(id),
            }
        }
        (local, remote)
    };

    if !local.is_empty() {
        let mut store = state.0.lock().map_err(err)?;
        match &action {
            Action::Role(role) => store.move_to_role(&local, role),
            Action::Folder(folder) => store.move_to_folder(&local, *folder),
            Action::Delete => store.delete(&local),
            Action::Restore => store.restore(&local),
        }
        .map_err(err)?;
    }

    for ((account_id, source), refs) in remote {
        let config = {
            let store = state.0.lock().map_err(err)?;
            config_of(&store.live_account(account_id).map_err(err)?.ok_or("account missing")?.config)?
        };
        let secret = oxymail_sync::credential(&config).await.map_err(full)?;
        let uids: Vec<u32> = refs.iter().map(|r| r.uid).collect();
        let ids: Vec<i64> = refs.iter().map(|r| r.message_id).collect();
        let in_trash = refs.first().and_then(|r| r.role.as_deref()) == Some("trash");

        let mut session = imap::connect(&config, &secret).await.map_err(full)?;
        let target: Option<String> = match &action {
            Action::Delete if in_trash => {
                imap::delete_messages(&mut session, &source, &uids).await.map_err(full)?;
                None
            }
            _ => {
                let target = match &action {
                    Action::Role(role) => imap::folder_for_role(&mut session, role).await,
                    Action::Delete => imap::folder_for_role(&mut session, "trash").await,
                    Action::Restore => imap::folder_for_role(&mut session, "inbox").await,
                    Action::Folder(folder) => {
                        let name = state.0.lock().map_err(err)?.folder_remote_name(*folder).map_err(err)?;
                        name.ok_or_else(|| anyhow::anyhow!("that folder only exists in the archive, not on the server"))
                    }
                }
                .map_err(full)?;
                imap::move_messages(&mut session, &source, &uids, &target).await.map_err(full)?;
                Some(target)
            }
        };
        let _ = session.logout().await;

        {
            let mut store = state.0.lock().map_err(err)?;
            let target_folder = match &target {
                Some(name) => store.folder_by_remote(account_id, name).map_err(err)?,
                None => None,
            };
            store.apply_remote_move(&ids, target_folder).map_err(err)?;
        }
        // Bring the moved copies in now, so the target folder and Undo see them straight away.
        let mut folders = vec![source.clone()];
        folders.extend(target);
        if let Ok(mut store) = Store::open(&Store::default_path()) {
            let _ = imap::sync_folders(&mut store, account_id, &config, &secret, Some(&folders), |_| {}).await;
        }
        let _ = app.emit("mail-changed", json!({ "account_id": account_id }));
    }
    Ok(())
}

pub fn forget_password(store: &Store, account_id: i64) {
    if let Ok(Some(live)) = store.live_account(account_id) {
        let _ = secrets::forget_password(&live.email);
        oxymail_sync::oauth::forget(&live.email);
    }
}

/// Checks both servers accept the account's credentials. With OAuth the stored sign-in is
/// used; otherwise the given password (or the saved one when blank), which is then saved.
async fn verify(config: &AccountConfig, password: Option<String>) -> Res<()> {
    let secret = if config.auth.is_some() {
        oxymail_sync::credential(config).await.map_err(full)?
    } else {
        match password.filter(|p| !p.is_empty()) {
            Some(p) => p,
            None => secrets::password(&config.email).map_err(full)?,
        }
    };
    imap::test_login(config, &secret).await.map_err(|e| format!("Incoming mail (IMAP): {}", full(e)))?;
    smtp::test_login(config, &secret).await.map_err(|e| format!("Outgoing mail (SMTP): {}", full(e)))?;
    if config.auth.is_none() {
        secrets::save_password(&config.email, &secret).map_err(full)?;
    }
    Ok(())
}

/// Opens the browser to sign in to Google or Microsoft for `email`.
#[tauri::command]
pub async fn oauth_sign_in(provider: String, email: String) -> Res<()> {
    oxymail_sync::oauth::sign_in(&provider, &email).await.map(|_| ()).map_err(full)
}

#[derive(serde::Serialize)]
pub struct OAuthClientInfo {
    client_id: String,
    has_secret: bool,
}

const PROVIDERS: [&str; 2] = ["google", "microsoft"];

/// The sign-in clients set in Settings, without their secrets.
#[tauri::command]
pub fn oauth_clients(state: State<'_, AppState>) -> Res<BTreeMap<String, OAuthClientInfo>> {
    let settings = state.0.lock().map_err(err)?.settings().map_err(err)?;
    Ok(PROVIDERS
        .iter()
        .map(|p| {
            let configured = settings.get(&format!("oauth_{p}_client_id")).cloned().unwrap_or_default();
            let built_in = built_in_client(p);
            let client_id = if configured.is_empty() { built_in.as_ref().map(|c| c.id.clone()).unwrap_or_default() } else { configured.clone() };
            let has_secret = secrets::client_secret(p).is_some() || (configured.is_empty() && built_in.is_some_and(|c| c.secret.is_some()));
            (p.to_string(), OAuthClientInfo { client_id, has_secret })
        })
        .collect())
}

/// Saves a provider's client ID (settings) and secret (keyring; `None` keeps the current one).
#[tauri::command]
pub fn set_oauth_client(state: State<'_, AppState>, provider: String, client_id: String, client_secret: Option<String>) -> Res<()> {
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(format!("unknown provider {provider}"));
    }
    if let Some(secret) = client_secret {
        secrets::save_client_secret(&provider, secret.trim()).map_err(full)?;
    }
    let store = state.0.lock().map_err(err)?;
    store.set_setting(&format!("oauth_{provider}_client_id"), client_id.trim()).map_err(err)?;
    load_oauth_client(&store, &provider);
    Ok(())
}

/// Hands a provider's client from settings and keyring to the sync crate.
/// Client registrations compiled into this copy (from `oauth.local.env` at install time), used
/// when nothing is set in Settings → Sign-in.
fn built_in_client(provider: &str) -> Option<oxymail_sync::oauth::Client> {
    let (id, secret) = match provider {
        "google" => (option_env!("OXYMAIL_GOOGLE_CLIENT_ID"), option_env!("OXYMAIL_GOOGLE_CLIENT_SECRET")),
        "microsoft" => (option_env!("OXYMAIL_MICROSOFT_CLIENT_ID"), None),
        _ => (None, None),
    };
    id.filter(|i| !i.is_empty())
        .map(|id| oxymail_sync::oauth::Client { id: id.to_string(), secret: secret.filter(|s| !s.is_empty()).map(str::to_string) })
}

pub fn load_oauth_client(store: &Store, provider: &str) {
    let id = store.settings().ok().and_then(|s| s.get(&format!("oauth_{provider}_client_id")).cloned()).unwrap_or_default();
    let client = if id.is_empty() {
        built_in_client(provider)
    } else {
        Some(oxymail_sync::oauth::Client { id, secret: secrets::client_secret(provider) })
    };
    oxymail_sync::oauth::set_client(provider, client);
}

pub fn load_oauth_clients(store: &Store) {
    for p in PROVIDERS {
        load_oauth_client(store, p);
    }
}
