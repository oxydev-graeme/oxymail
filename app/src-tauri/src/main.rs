// Hides the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use oxymail_core::{Account, AccountUpdate, Folder, ImportStats, MessageSummary, MessageView, Store};
use tauri::{AppHandle, Emitter, State};

mod live;

pub(crate) struct AppState(Mutex<Store>);

/// Commands return errors as strings so the UI can show them.
pub(crate) type Res<T> = Result<T, String>;

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn accounts(state: State<AppState>) -> Res<Vec<Account>> {
    state.0.lock().map_err(err)?.accounts().map_err(err)
}

#[tauri::command]
fn folders(state: State<AppState>, account_id: i64) -> Res<Vec<Folder>> {
    state.0.lock().map_err(err)?.folders(account_id).map_err(err)
}

#[tauri::command]
fn messages(state: State<AppState>, folder_id: i64, offset: i64, limit: i64) -> Res<Vec<MessageSummary>> {
    state.0.lock().map_err(err)?.messages(folder_id, offset, limit).map_err(err)
}

#[tauri::command]
fn search(state: State<AppState>, query: String) -> Res<Vec<MessageSummary>> {
    state.0.lock().map_err(err)?.search(&query, 200).map_err(err)
}

#[tauri::command]
fn message(state: State<AppState>, id: i64) -> Res<MessageView> {
    state.0.lock().map_err(err)?.message(id).map_err(err)
}

/// Saves an attachment to ~/Downloads (without overwriting) and returns the path.
#[tauri::command]
fn save_attachment(state: State<AppState>, id: i64, index: usize) -> Res<String> {
    let (name, data) = state.0.lock().map_err(err)?.attachment(id, index).map_err(err)?;
    let dir = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join("Downloads");
    std::fs::create_dir_all(&dir).map_err(err)?;
    let safe: String = name.chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect();
    let mut path = dir.join(&safe);
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("({n}) {safe}"));
        n += 1;
    }
    std::fs::write(&path, data).map_err(err)?;
    Ok(path.display().to_string())
}

#[tauri::command]
fn set_flag(app: AppHandle, state: State<AppState>, ids: Vec<i64>, flag: char, on: bool) -> Res<()> {
    let mut store = state.0.lock().map_err(err)?;
    store.set_flag(&ids, flag, on).map_err(err)?;
    live::push_flags(&app, &store, &ids, flag, on);
    Ok(())
}

#[tauri::command]
fn settings(state: State<AppState>) -> Res<BTreeMap<String, String>> {
    state.0.lock().map_err(err)?.settings().map_err(err)
}

#[tauri::command]
fn set_setting(state: State<AppState>, key: String, value: String) -> Res<()> {
    state.0.lock().map_err(err)?.set_setting(&key, &value).map_err(err)
}

#[tauri::command]
fn reorder_accounts(state: State<AppState>, ids: Vec<i64>) -> Res<()> {
    state.0.lock().map_err(err)?.reorder_accounts(&ids).map_err(err)
}

#[tauri::command]
fn update_account(state: State<AppState>, id: i64, update: AccountUpdate) -> Res<()> {
    state.0.lock().map_err(err)?.update_account(id, &update).map_err(err)
}

#[tauri::command]
fn remove_account(state: State<AppState>, id: i64) -> Res<()> {
    let store = state.0.lock().map_err(err)?;
    live::forget_password(&store, id);
    store.remove_account(id).map_err(err)
}

#[tauri::command]
fn mark_folder_read(app: AppHandle, state: State<AppState>, folder_id: i64) -> Res<Vec<i64>> {
    let mut store = state.0.lock().map_err(err)?;
    let ids = store.mark_folder_read(folder_id).map_err(err)?;
    live::push_flags(&app, &store, &ids, 'S', true);
    Ok(ids)
}

#[tauri::command]
fn mark_account_read(app: AppHandle, state: State<AppState>, account_id: i64) -> Res<Vec<i64>> {
    let mut store = state.0.lock().map_err(err)?;
    let ids = store.mark_account_read(account_id).map_err(err)?;
    live::push_flags(&app, &store, &ids, 'S', true);
    Ok(ids)
}

#[tauri::command]
fn suggest_contacts(state: State<AppState>, query: String) -> Res<Vec<oxymail_core::ContactSuggestion>> {
    state.0.lock().map_err(err)?.suggest_contacts(&query, 8).map_err(err)
}

/// Messages from every account's folders with one role, for the "All inboxes" view.
#[tauri::command]
fn messages_by_role(state: State<AppState>, role: String, offset: i64, limit: i64) -> Res<Vec<MessageSummary>> {
    state.0.lock().map_err(err)?.messages_by_role(&role, offset, limit).map_err(err)
}

#[tauri::command]
fn unread_by_role(state: State<AppState>, role: String) -> Res<i64> {
    state.0.lock().map_err(err)?.unread_by_role(&role).map_err(err)
}

#[tauri::command]
fn set_images_allowed(state: State<AppState>, email: String, allowed: bool) -> Res<()> {
    state.0.lock().map_err(err)?.set_images_allowed(&email, allowed).map_err(err)
}

/// Opens a web link in the system browser. Only http(s) URLs are accepted.
#[tauri::command]
fn open_url(url: String) -> Res<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only web links can be opened".into());
    }
    open_external(&url)
}

/// Writes an attachment to a private temporary folder and opens it with the default app.
#[tauri::command]
fn open_attachment(state: State<AppState>, id: i64, index: usize) -> Res<String> {
    let (name, data) = state.0.lock().map_err(err)?.attachment(id, index).map_err(err)?;
    let dir = std::env::temp_dir().join(format!("oxymail-{}", std::process::id())).join(format!("{id}-{index}"));
    std::fs::create_dir_all(&dir).map_err(err)?;
    let safe: String = name.chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect();
    let path = dir.join(if safe.is_empty() { "attachment".to_string() } else { safe });
    std::fs::write(&path, data).map_err(err)?;
    open_external(&path.to_string_lossy())?;
    Ok(path.display().to_string())
}

fn open_external(target: &str) -> Res<()> {
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    std::process::Command::new(program).arg(target).spawn().map(|_| ()).map_err(err)
}

#[tauri::command]
fn thread(state: State<AppState>, id: i64) -> Res<Vec<MessageSummary>> {
    state.0.lock().map_err(err)?.thread(id).map_err(err)
}

#[tauri::command]
fn save_draft(state: State<AppState>, id: Option<i64>, account_id: Option<i64>, subject: String, recipients: String, body: String) -> Res<i64> {
    state.0.lock().map_err(err)?.save_draft(id, account_id, &subject, &recipients, &body).map_err(err)
}

#[tauri::command]
fn drafts(state: State<AppState>) -> Res<Vec<oxymail_core::Draft>> {
    state.0.lock().map_err(err)?.drafts().map_err(err)
}

#[tauri::command]
fn delete_draft(state: State<AppState>, id: i64) -> Res<()> {
    state.0.lock().map_err(err)?.delete_draft(id).map_err(err)
}

#[tauri::command]
fn data_location() -> String {
    Store::default_path().display().to_string()
}

#[tauri::command]
fn import_archive(state: State<AppState>, path: String) -> Res<ImportStats> {
    let path = match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    };
    state.0.lock().map_err(err)?.import_archive(&path, |_| {}).map_err(err)
}

fn main() {
    oxymail_sync::init();
    let store = Store::open(&Store::default_path()).expect("opening the OxyMail store");
    live::load_oauth_clients(&store);
    tauri::Builder::default()
        .manage(AppState(Mutex::new(store)))
        .plugin(tauri_plugin_notification::init())
        .manage(live::SyncState::default())
        .setup(|app| {
            // The main window is built here rather than in tauri.conf.json so links clicked in an
            // email open in the browser (or, for mailto:, in Compose) instead of a new webview.
            let handle = app.handle().clone();
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                .title("OxyMail")
                .inner_size(1360.0, 860.0)
                .min_inner_size(900.0, 560.0)
                .decorations(false)
                .on_new_window(move |url, _| {
                    match url.scheme() {
                        "http" | "https" => {
                            let _ = open_external(url.as_str());
                        }
                        "mailto" => {
                            let _ = handle.emit("compose-mailto", url.as_str());
                        }
                        _ => {}
                    }
                    tauri::webview::NewWindowResponse::Deny
                })
                .build()?;

            // Check every live account at start-up and then every few minutes.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    live::sync_accounts(handle.clone(), None).await;
                    live::ensure_watchers(&handle);
                    tokio::time::sleep(live::SYNC_INTERVAL).await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            accounts,
            folders,
            messages,
            search,
            message,
            save_attachment,
            import_archive,
            set_flag,
            live::move_to,
            live::move_to_folder,
            live::delete,
            live::restore,
            settings,
            set_setting,
            data_location,
            reorder_accounts,
            update_account,
            remove_account,
            mark_folder_read,
            mark_account_read,
            live::discover_account,
            live::add_account,
            live::sync_now,
            live::send_message,
            live::live_config,
            live::update_server_settings,
            live::connect_suggestion,
            live::connect_archive,
            suggest_contacts,
            thread,
            save_draft,
            drafts,
            delete_draft,
            set_images_allowed,
            open_url,
            open_attachment,
            messages_by_role,
            unread_by_role,
            live::oauth_sign_in,
            live::oauth_clients,
            live::set_oauth_client
        ])
        .run(tauri::generate_context!())
        .expect("running OxyMail");
}
