// Hides the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use oxymail_core::{Account, AccountUpdate, Folder, ImportStats, MessageSummary, MessageView, Store};
use tauri::{AppHandle, Emitter, Manager, State};

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
    let dir = dirs::download_dir().or_else(|| dirs::home_dir().map(|h| h.join("Downloads"))).unwrap_or_default();
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
    open::that_detached(target).map_err(err)
}

#[tauri::command]
fn snooze(app: AppHandle, state: State<AppState>, ids: Vec<i64>, until: Option<i64>) -> Res<()> {
    let store = state.0.lock().map_err(err)?;
    store.snooze(&ids, until).map_err(err)?;
    let _ = app.emit("scheduled-changed", ());
    Ok(())
}

#[tauri::command]
fn snoozed(state: State<AppState>) -> Res<Vec<MessageSummary>> {
    state.0.lock().map_err(err)?.snoozed().map_err(err)
}

#[tauri::command]
fn scheduled(state: State<AppState>) -> Res<Vec<oxymail_core::Scheduled>> {
    state.0.lock().map_err(err)?.scheduled().map_err(err)
}

/// Cancels a scheduled message and returns its Compose state for editing.
#[tauri::command]
fn unschedule(state: State<AppState>, id: i64) -> Res<String> {
    let store = state.0.lock().map_err(err)?;
    let draft = store.scheduled().map_err(err)?.into_iter().find(|s| s.id == id).map(|s| s.draft).ok_or("already sent")?;
    store.unschedule(id).map_err(err)?;
    Ok(draft)
}

// ---- Calendar and contacts -------------------------------------------------

#[tauri::command]
fn calendars(state: State<AppState>) -> Res<Vec<oxymail_core::Calendar>> {
    state.0.lock().map_err(err)?.calendars().map_err(err)
}

#[tauri::command]
fn create_calendar(state: State<AppState>, name: String, color: Option<String>) -> Res<i64> {
    state.0.lock().map_err(err)?.create_calendar(&name, color.as_deref()).map_err(err)
}

#[tauri::command]
fn update_calendar(state: State<AppState>, id: i64, name: String, color: String, visible: bool) -> Res<()> {
    state.0.lock().map_err(err)?.update_calendar(id, &name, &color, visible).map_err(err)
}

#[tauri::command]
fn delete_calendar(state: State<AppState>, id: i64) -> Res<()> {
    state.0.lock().map_err(err)?.delete_calendar(id).map_err(err)
}

#[tauri::command]
fn occurrences(state: State<AppState>, from: i64, to: i64) -> Res<Vec<oxymail_core::Occurrence>> {
    state.0.lock().map_err(err)?.occurrences(from, to).map_err(err)
}

#[tauri::command]
fn event(state: State<AppState>, id: i64) -> Res<oxymail_core::Event> {
    state.0.lock().map_err(err)?.event(id).map_err(err)
}

#[tauri::command]
fn save_event(state: State<AppState>, event: oxymail_core::Event) -> Res<i64> {
    state.0.lock().map_err(err)?.save_event(&event).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn delete_event(state: State<AppState>, id: i64) -> Res<()> {
    state.0.lock().map_err(err)?.delete_event(id).map_err(err)
}

#[tauri::command]
fn people(state: State<AppState>, query: String) -> Res<Vec<oxymail_core::PersonSummary>> {
    state.0.lock().map_err(err)?.people(&query).map_err(err)
}

#[tauri::command]
fn person(state: State<AppState>, id: i64) -> Res<oxymail_core::Person> {
    state.0.lock().map_err(err)?.person(id).map_err(err)
}

#[tauri::command]
fn save_person(state: State<AppState>, person: oxymail_core::Person) -> Res<i64> {
    state.0.lock().map_err(err)?.save_person(&person).map_err(err)
}

#[tauri::command]
fn delete_person(state: State<AppState>, id: i64) -> Res<()> {
    state.0.lock().map_err(err)?.delete_person(id).map_err(err)
}

/// Imports .ics/.vcf files, or every such file under a folder.
#[tauri::command]
fn import_pim(state: State<AppState>, paths: Vec<String>) -> Res<oxymail_core::PimImport> {
    let mut store = state.0.lock().map_err(err)?;
    let mut total = oxymail_core::PimImport::default();
    for path in paths {
        let path = expand_home(&path);
        if path.is_dir() {
            let s = store.import_pim_folder(&path).map_err(err)?;
            total.calendars += s.calendars;
            total.events += s.events;
            total.people += s.people;
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ics")) {
            total.events += store.import_ics(&path).map_err(err)?.1;
            total.calendars += 1;
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("vcf")) {
            total.people += store.import_vcf(&path).map_err(err)?;
        } else {
            return Err(format!("{} isn't a calendar (.ics) or contacts (.vcf) file", path.display()));
        }
    }
    Ok(total)
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    }
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
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    };
    state.0.lock().map_err(err)?.import_archive(&path, |_| {}).map_err(err)
}

/// True if the folder is a Mailbird `Store` (it holds Store.db) rather than an export.
#[tauri::command]
fn is_mailbird_store(path: String) -> bool {
    oxymail_mailbird_export::is_store(&expand_home(&path))
}

/// Where the export goes unless the user picks somewhere else.
#[tauri::command]
fn default_export_folder() -> String {
    dirs::home_dir().unwrap_or_default().join("MailbirdExport").display().to_string()
}

#[derive(Clone, serde::Serialize)]
struct MailbirdProgress {
    /// "export", "import" or "contacts"
    stage: &'static str,
    done: usize,
    /// 0 when the total isn't known
    total: usize,
}

#[derive(serde::Serialize)]
struct MailbirdResult {
    export: oxymail_mailbird_export::Summary,
    mail: ImportStats,
    pim: oxymail_core::PimImport,
}

/// Exports a copy of a Mailbird store to open formats in `out`, then imports the mail, contacts
/// and calendars from it. The store is only read. Progress arrives as `mailbird-progress` events.
#[tauri::command]
async fn migrate_mailbird(app: AppHandle, store: String, out: String) -> Res<MailbirdResult> {
    let store = expand_home(&store);
    let out = expand_home(&out);
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |stage, done, total| {
            let _ = app.emit("mailbird-progress", MailbirdProgress { stage, done, total });
        };
        let options = oxymail_mailbird_export::Options::default();
        let export = oxymail_mailbird_export::export(&store, &out, &options, |p| emit("export", p.done, p.total))
            .map_err(|e| format!("{e:#}"))?;

        let state = app.state::<AppState>();
        let mut db = state.0.lock().map_err(err)?;
        emit("import", 0, 0);
        let mail = db.import_archive(&out.join("mail"), |n| emit("import", n, 0)).map_err(|e| format!("{e:#}"))?;
        emit("contacts", 0, 0);
        let pim = db.import_pim_folder(&out).map_err(|e| format!("{e:#}"))?;
        Ok(MailbirdResult { export, mail, pim })
    })
    .await
    .map_err(err)?
}

fn main() {
    oxymail_sync::init();
    let store = Store::open(&Store::default_path()).expect("opening the OxyMail store");
    live::load_oauth_clients(&store);
    tauri::Builder::default()
        .manage(AppState(Mutex::new(store)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
                let ticker = handle.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        live::tick(&ticker).await;
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    }
                });
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
            is_mailbird_store,
            default_export_folder,
            migrate_mailbird,
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
            calendars,
            create_calendar,
            update_calendar,
            delete_calendar,
            occurrences,
            event,
            save_event,
            delete_event,
            people,
            person,
            save_person,
            delete_person,
            import_pim,
            snooze,
            snoozed,
            scheduled,
            unschedule,
            live::schedule_send,
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
