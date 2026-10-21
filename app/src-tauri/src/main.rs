// Hides the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;

use oxymail_core::{Account, Folder, ImportStats, MessageSummary, MessageView, Store};
use tauri::State;

struct AppState(Mutex<Store>);

/// Commands return errors as strings so the UI can show them.
type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
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
fn import_archive(state: State<AppState>, path: String) -> Res<ImportStats> {
    let path = match path.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    };
    state.0.lock().map_err(err)?.import_archive(&path, |_| {}).map_err(err)
}

fn main() {
    let store = Store::open(&Store::default_path()).expect("opening the OxyMail store");
    tauri::Builder::default()
        .manage(AppState(Mutex::new(store)))
        .invoke_handler(tauri::generate_handler![
            accounts,
            folders,
            messages,
            search,
            message,
            save_attachment,
            import_archive
        ])
        .run(tauri::generate_context!())
        .expect("running OxyMail");
}
