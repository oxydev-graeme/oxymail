//! The unread count on the taskbar or dock icon, and new-mail notifications that go away once
//! the mail is read. Each platform has its own badge: macOS has a dock badge, Linux docks and
//! panels read the Unity launcher entry signal over D-Bus, and Windows has no badge at all, so
//! a small numbered icon is drawn over the taskbar button instead.
//!
//! Some Linux panels (Zorin Taskbar, Dash to Panel) also count an app's notifications that are
//! still in the message tray, so a notification for mail since read on a phone would keep a
//! number on the icon; Windows keeps them in the Action Center. On both, notifications are
//! therefore sent here, where their ids are kept, and withdrawn once every message they
//! announced is read. (macOS's Notification Center can't be cleared this way yet.)

use oxymail_core::Store;
use tauri::AppHandle;

/// Sets the badge to the unread Inbox count and withdraws notifications for mail now read.
/// Cheap enough to run after every sync, every flag change and on the 30-second tick, which
/// also restores a badge a panel cleared when the window was focused.
pub fn refresh(app: &AppHandle, store: &Store) {
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    withdraw_read(store);
    // "Show the unread count on the taskbar and tray icon", on unless turned off.
    let shown = store.settings().ok().and_then(|s| s.get("unread_badge").cloned()).as_deref() != Some("false");
    let count = if shown { store.inbox_unread().unwrap_or(0) } else { 0 };
    set_count(app, count);
    crate::desktop::set_tray_count(app, count);
}

/// A desktop notification for new mail; `ids` are the messages it announces. `sound` plays
/// the system's new-mail sound with it.
pub fn notify_new_mail(app: &AppHandle, title: String, body: String, ids: Vec<i64>, sound: bool) {
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        tauri::async_runtime::spawn(linux::notify(title, body, ids, sound));
    }
    #[cfg(target_os = "windows")]
    {
        let app_id = WINDOWS_APP_ID.get_or_init(|| crate::windows_shell::toast_app_id(&app.config().identifier));
        let tag = NEXT_TAG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if crate::windows_shell::show_toast(app_id, &tag.to_string(), &title, &body, sound).is_ok() {
            remember(tag, ids);
        }
    }
    #[cfg(target_os = "macos")]
    {
        use tauri_plugin_notification::NotificationExt;
        let _ = ids;
        let mut builder = app.notification().builder().title(title).body(body);
        if sound {
            builder = builder.sound("Default");
        }
        let _ = builder.show();
    }
}

/// Notifications on screen or in the tray / Action Center: their id and the messages each
/// announces.
#[cfg(any(target_os = "linux", target_os = "windows"))]
static SHOWN: std::sync::Mutex<Vec<(u32, Vec<i64>)>> = std::sync::Mutex::new(Vec::new());

/// Windows toast tags, numbered from the start of this run.
#[cfg(target_os = "windows")]
static NEXT_TAG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn remember(id: u32, ids: Vec<i64>) {
    if let Ok(mut shown) = SHOWN.lock() {
        shown.push((id, ids));
    }
}

/// Withdraws every notification whose messages have all been read (here or anywhere else).
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn withdraw_read(store: &Store) {
    let read: Vec<u32> = {
        let Ok(mut shown) = SHOWN.lock() else { return };
        let mut read = Vec::new();
        shown.retain(|(id, ids)| {
            let unread = store.any_unread(ids).unwrap_or(true);
            if !unread {
                read.push(*id);
            }
            unread
        });
        read
    };
    if read.is_empty() {
        return;
    }
    #[cfg(target_os = "linux")]
    tauri::async_runtime::spawn(linux::close(read));
    #[cfg(target_os = "windows")]
    if let Some(app_id) = WINDOWS_APP_ID.get() {
        for tag in read {
            // Harmless for one the user already dismissed.
            let _ = crate::windows_shell::remove_toast(app_id, &tag.to_string());
        }
    }
}

/// Set on the first toast, for withdrawing them later.
#[cfg(target_os = "windows")]
static WINDOWS_APP_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();

#[cfg(target_os = "linux")]
fn set_count(_app: &AppHandle, count: i64) {
    tauri::async_runtime::spawn(async move {
        let _ = linux::set_count(count.max(0)).await;
    });
}

#[cfg(target_os = "windows")]
fn set_count(app: &AppHandle, count: i64) {
    use std::sync::atomic::{AtomicI64, Ordering};
    use tauri::Manager;
    // Only redrawn on a change, so the taskbar button doesn't flicker on every tick.
    static SHOWN: AtomicI64 = AtomicI64::new(-1);
    if SHOWN.swap(count, Ordering::Relaxed) == count {
        return;
    }
    let Some(window) = app.get_webview_window("main") else { return };
    let icon = (count > 0).then(|| tauri::image::Image::new_owned(overlay(count), SIZE, SIZE));
    let _ = window.set_overlay_icon(icon);
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn set_count(app: &AppHandle, count: i64) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_badge_count((count > 0).then_some(count));
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use zbus::zvariant::Value;

    /// Must match the installed `oxymail.desktop`, which is how the panel finds our icon.
    const DESKTOP_ID: &str = "oxymail";
    const APP_URI: &str = "application://oxymail.desktop";
    const PATH: &str = "/com/canonical/unity/launcherentry/oxymail";
    const NOTIFICATIONS: (&str, &str, &str) =
        ("org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications");

    /// Kept open for the life of the app: docks drop the badge when the sender disconnects.
    static CONNECTION: Mutex<Option<zbus::Connection>> = Mutex::new(None);
    async fn connection() -> zbus::Result<zbus::Connection> {
        if let Some(c) = CONNECTION.lock().ok().and_then(|c| c.clone()) {
            return Ok(c);
        }
        let c = zbus::Connection::session().await?;
        if let Ok(mut slot) = CONNECTION.lock() {
            *slot = Some(c.clone());
        }
        Ok(c)
    }

    pub async fn set_count(count: i64) -> zbus::Result<()> {
        let mut properties: HashMap<&str, Value> = HashMap::new();
        properties.insert("count", Value::from(count));
        properties.insert("count-visible", Value::from(count > 0));
        connection()
            .await?
            .emit_signal(None::<&str>, PATH, "com.canonical.Unity.LauncherEntry", "Update", &(APP_URI, properties))
            .await
    }

    pub async fn notify(title: String, body: String, ids: Vec<i64>, sound: bool) {
        let Ok(connection) = connection().await else { return };
        let mut hints: HashMap<&str, Value> = HashMap::new();
        // Ties the notification to our launcher, so the panel counts it against our icon.
        hints.insert("desktop-entry", Value::from(DESKTOP_ID));
        // The sound theme's new-mail sound, or none at all.
        if sound {
            hints.insert("sound-name", Value::from("message-new-email"));
        } else {
            hints.insert("suppress-sound", Value::from(true));
        }
        hints.insert("category", Value::from("email.arrived"));
        let (destination, path, interface) = NOTIFICATIONS;
        let reply = connection
            .call_method(
                Some(destination),
                path,
                Some(interface),
                "Notify",
                &("OxyMail", 0u32, DESKTOP_ID, title.as_str(), body.as_str(), Vec::<&str>::new(), hints, -1i32),
            )
            .await;
        let Ok(id) = reply.and_then(|r| r.body().deserialize::<u32>()) else { return };
        super::remember(id, ids);
    }

    pub async fn close(ids: Vec<u32>) {
        let Ok(connection) = connection().await else { return };
        let (destination, path, interface) = NOTIFICATIONS;
        for id in ids {
            // Harmless for one the user already dismissed.
            let _ = connection.call_method(Some(destination), path, Some(interface), "CloseNotification", &(id,)).await;
        }
    }
}

#[cfg(target_os = "windows")]
const SIZE: u32 = 32;

/// A red disc with the count in white, as RGBA. Over 99 reads "99+".
#[cfg(target_os = "windows")]
fn overlay(count: i64) -> Vec<u8> {
    const RED: [u8; 3] = [0xc0, 0x35, 0x2b];
    let size = SIZE as usize;
    let mut px = vec![0u8; size * size * 4];

    // The disc, with a one-pixel soft edge so it doesn't look jagged when scaled down.
    let centre = size as f32 / 2.0;
    let radius = centre;
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f32 + 0.5 - centre).powi(2) + (y as f32 + 0.5 - centre).powi(2)).sqrt();
            let alpha = (radius - d).clamp(0.0, 1.0);
            let i = (y * size + x) * 4;
            px[i..i + 3].copy_from_slice(&RED);
            px[i + 3] = (alpha * 255.0) as u8;
        }
    }

    let text = if count > 99 { "99+".to_string() } else { count.to_string() };
    // Bigger digits when there are fewer of them.
    let scale = match text.len() {
        1 => 4,
        2 => 3,
        _ => 2,
    };
    let glyph_w = 3 * scale;
    let gap = scale;
    let width = text.len() * glyph_w + (text.len() - 1) * gap;
    let height = 5 * scale;
    let left = (size - width) / 2;
    let top = (size - height) / 2;
    for (n, ch) in text.chars().enumerate() {
        let rows = glyph(ch);
        let x0 = left + n * (glyph_w + gap);
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let (x, y) = (x0 + col * scale + dx, top + row * scale + dy);
                        let i = (y * size + x) * 4;
                        px[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
        }
    }
    px
}

/// A 3×5 pixel font: each row is three bits, left to right.
#[cfg(target_os = "windows")]
fn glyph(ch: char) -> [u8; 5] {
    match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        _ => [0; 5],
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn overlay_fits_every_count() {
        for count in [1, 9, 10, 99, 100, 12_345] {
            assert_eq!(overlay(count).len(), (SIZE * SIZE * 4) as usize);
        }
    }
}
