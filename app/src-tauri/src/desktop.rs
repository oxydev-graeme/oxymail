//! Living alongside the desktop: the tray icon, closing to the tray, starting when the computer
//! does, and `mailto:` links other apps hand to OxyMail.

use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::{AppState, Res, err};

/// Passed by the start-up entry, so a start with the computer can stay in the tray.
pub const AUTOSTART_ARG: &str = "--autostart";
pub const TRAY_ID: &str = "main";

/// `mailto:` links from the command line, held until the UI is ready to open them.
static PENDING_MAILTO: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn mailto_args(args: &[String]) -> Vec<String> {
    args.iter().filter(|a| a.get(..7).is_some_and(|s| s.eq_ignore_ascii_case("mailto:"))).cloned().collect()
}

/// Links from OxyMail's own command line, read once the UI has loaded.
pub fn queue_mailto(links: Vec<String>) {
    if let Ok(mut pending) = PENDING_MAILTO.lock() {
        pending.extend(links);
    }
}

/// Set once the UI has asked for pending links, after which new ones are sent to it directly.
static UI_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
pub fn take_mailto() -> Vec<String> {
    UI_READY.store(true, std::sync::atomic::Ordering::Relaxed);
    PENDING_MAILTO.lock().map(|mut p| std::mem::take(&mut *p)).unwrap_or_default()
}

/// Another launch of OxyMail (the launcher clicked again, a `mailto:` link) while this one
/// runs: bring this window forward and open any link here instead.
pub fn second_instance(app: &AppHandle, args: Vec<String>) {
    show_main(app);
    for link in mailto_args(&args) {
        let _ = app.emit("compose-mailto", link);
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// A setting that is on unless turned off.
pub fn setting_on(app: &AppHandle, key: &str, default: bool) -> bool {
    let value = app.state::<AppState>().0.lock().ok().and_then(|s| s.settings().ok()).and_then(|s| s.get(key).cloned());
    match value.as_deref() {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

/// Closing the window keeps OxyMail running in the tray (checking mail, notifying) unless
/// "Quit OxyMail when the window is closed" is on.
pub fn close_requested(window: &tauri::Window, api: &tauri::CloseRequestApi) {
    if window.label() == "main" && !setting_on(window.app_handle(), "quit_on_close", false) {
        api.prevent_close();
        let _ = window.hide();
    }
}

pub fn setup_tray(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open OxyMail", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check", "Check for new mail", true, None::<&str>)?;
    let compose = MenuItem::with_id(app, "compose", "New message", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit OxyMail", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &compose, &check, &separator, &quit])?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("OxyMail")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "compose" => {
                show_main(app);
                let _ = app.emit("compose-new", ());
            }
            "check" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { crate::live::sync_accounts(app, None).await });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Linux panels only ever open the menu; elsewhere a left click opens the window.
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(TrayMenu(vec![open, compose, check, quit]));
    // See `TrayMenu`: give the panel a reason to fetch the labels once it has read the menu.
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        republish_tray_menu(&handle);
    });
    Ok(())
}

/// The tray menu's items, kept so their labels can be published again.
///
/// GNOME's AppIndicator extension (Ubuntu's and Zorin's) can show this menu with blank labels:
/// the menu is announced twice as the icon appears, the second announcement cancels the label
/// fetch queued by the first, and the extension never asks again. Reporting a label change
/// later marks the menu stale, so the extension fetches every label the next time it opens.
pub struct TrayMenu(Vec<MenuItem<tauri::Wry>>);

/// Reports each tray menu label as changed (briefly with a trailing space, then back), which
/// makes AppIndicator panels that missed the labels fetch them again. Invisible otherwise.
pub fn republish_tray_menu(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    {
        let Some(menu) = app.try_state::<TrayMenu>() else { return };
        let items = menu.0.clone();
        for item in &items {
            if let Ok(text) = item.text() {
                let _ = item.set_text(format!("{text} "));
            }
        }
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            for item in &items {
                if let Ok(text) = item.text() {
                    let _ = item.set_text(text.trim_end());
                }
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = app;
}

/// The unread count beside the tray icon (as a label on Linux panels, in the tooltip
/// everywhere), or just "OxyMail" when there's nothing unread or the count is turned off.
pub fn set_tray_count(app: &AppHandle, count: i64) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    // A changed count is also a cheap moment to re-publish the menu labels (see `TrayMenu`),
    // which repairs a panel that re-read the menu since start-up, after a GNOME restart say.
    static LAST: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);
    if LAST.swap(count, std::sync::atomic::Ordering::Relaxed) != count {
        republish_tray_menu(app);
    }
    let tooltip = match count {
        0 => "OxyMail".to_string(),
        1 => "OxyMail · 1 unread".to_string(),
        n => format!("OxyMail · {n} unread"),
    };
    let _ = tray.set_tooltip(Some(&tooltip));
    #[cfg(target_os = "linux")]
    let _ = tray.set_title((count > 0).then(|| count.to_string()));
}

#[tauri::command]
pub fn autostart(app: AppHandle) -> Res<bool> {
    app.autolaunch().is_enabled().map_err(err)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, on: bool) -> Res<()> {
    let launcher = app.autolaunch();
    if on { launcher.enable() } else { launcher.disable() }.map_err(err)
}

/// Whether this launch should stay in the tray: started with the computer, and "Start
/// minimised" is on (the default, as in Mailbird).
pub fn start_hidden(state: &State<AppState>, args: &[String]) -> bool {
    args.iter().any(|a| a == AUTOSTART_ARG)
        && state.0.lock().ok().and_then(|s| s.settings().ok()).and_then(|s| s.get("start_minimized").cloned()).as_deref()
            != Some("false")
}

/// Whether OxyMail opens `mailto:` links, or `None` where it can't tell.
#[tauri::command]
pub fn default_mail_app(app: AppHandle) -> Option<bool> {
    let _ = &app;
    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("xdg-settings")
            .args(["check", "default-url-scheme-handler", "mailto", "oxymail.desktop"])
            .output()
            .ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim() == "yes")
    }
    #[cfg(target_os = "windows")]
    return Some(crate::windows_shell::is_default_mail_app());
    #[cfg(target_os = "macos")]
    return Some(crate::macos_shell::is_default_mail_app(&app.config().identifier));
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    None
}

/// Makes OxyMail the app for `mailto:` links. Returns `false` when the system only lets the
/// user make that choice (Windows), in which case its settings page is now open for them.
#[tauri::command]
pub fn make_default_mail_app(app: AppHandle) -> Res<bool> {
    let _ = &app;
    #[cfg(target_os = "linux")]
    {
        let status = std::process::Command::new("xdg-settings")
            .args(["set", "default-url-scheme-handler", "mailto", "oxymail.desktop"])
            .status()
            .map_err(err)?;
        if status.success() { Ok(true) } else { Err("the desktop didn't accept the change".into()) }
    }
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().map_err(err)?;
        crate::windows_shell::register_mail_client(&exe.to_string_lossy()).map_err(err)?;
        open::that_detached(crate::windows_shell::DEFAULT_APPS_PAGE).map_err(err)?;
        Ok(false)
    }
    #[cfg(target_os = "macos")]
    {
        crate::macos_shell::make_default_mail_app(&app.config().identifier)?;
        Ok(true)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    Err("not available on this system yet".into())
}

/// Windows: once registered as an email app, keep the registered program path current (an
/// update or a move would otherwise leave email links pointing at the old one).
pub fn refresh_mail_client_registration() {
    #[cfg(target_os = "windows")]
    if crate::windows_shell::mail_client_registered()
        && let Ok(exe) = std::env::current_exe()
    {
        let _ = crate::windows_shell::register_mail_client(&exe.to_string_lossy());
    }
}

/// macOS hands `mailto:` links to the running app as an event, not on the command line.
#[cfg(target_os = "macos")]
pub fn open_urls(app: &AppHandle, urls: Vec<String>) {
    let links = mailto_args(&urls);
    if links.is_empty() {
        return;
    }
    if UI_READY.load(std::sync::atomic::Ordering::Relaxed) {
        show_main(app);
        for link in links {
            let _ = app.emit("compose-mailto", link);
        }
    } else {
        // Launched by the link: the UI collects it once it has loaded.
        queue_mailto(links);
    }
}
