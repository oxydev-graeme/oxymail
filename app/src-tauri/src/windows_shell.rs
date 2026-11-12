//! Windows: new-mail toasts that can be taken back out of the Action Center once the mail is
//! read, and registering OxyMail as an email app so Windows offers it for `mailto:` links.
//!
//! Kept free of the rest of the app (plain values in, plain values out) so it can be
//! type-checked for Windows from any machine.

use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::core::HSTRING;
use windows_registry::CURRENT_USER;

/// All of OxyMail's new-mail toasts share a group, each with its own tag.
const GROUP: &str = "new-mail";

/// The app id Windows shows toasts under. The installer's Start-menu shortcut carries the app
/// identifier; a development build has no shortcut, and Windows drops toasts from an id it
/// doesn't know, so those borrow PowerShell's (as the notification plugin does).
pub fn toast_app_id(identifier: &str) -> String {
    let dev = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
        .is_some_and(|dir| dir.ends_with("target\\debug") || dir.ends_with("target\\release"));
    if dev {
        r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe".into()
    } else {
        identifier.into()
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Shows a toast tagged `tag`, with Windows' new-mail sound or none at all.
pub fn show_toast(app_id: &str, tag: &str, title: &str, body: &str, sound: bool) -> windows::core::Result<()> {
    let audio = if sound { r#"<audio src="ms-winsoundevent:Notification.Mail"/>"# } else { r#"<audio silent="true"/>"# };
    let xml = format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual>{audio}</toast>"#,
        escape(title),
        escape(body)
    );
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    toast.SetTag(&HSTRING::from(tag))?;
    toast.SetGroup(&HSTRING::from(GROUP))?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))?.Show(&toast)
}

/// Takes a toast off the screen and out of the Action Center. Harmless if the user already
/// dismissed it.
pub fn remove_toast(app_id: &str, tag: &str) -> windows::core::Result<()> {
    ToastNotificationManager::History()?.RemoveGroupedTagWithId(&HSTRING::from(tag), &HSTRING::from(GROUP), &HSTRING::from(app_id))
}

const PROG_ID: &str = "OxyMail.mailto";
const CLIENT_KEY: &str = r"Software\Clients\Mail\OxyMail";
const CAPABILITIES: &str = r"Software\Clients\Mail\OxyMail\Capabilities";

/// Tells Windows (for this user, no administrator rights needed) that OxyMail is an email app
/// that opens `mailto:` links, so it's listed in Settings → Default apps.
pub fn register_mail_client(exe: &str) -> windows::core::Result<()> {
    let open = format!("\"{exe}\" \"%1\"");
    let prog = CURRENT_USER.create(format!(r"Software\Classes\{PROG_ID}"))?;
    prog.set_string("", "Email link")?;
    CURRENT_USER.create(format!(r"Software\Classes\{PROG_ID}\DefaultIcon"))?.set_string("", format!("\"{exe}\",0"))?;
    CURRENT_USER.create(format!(r"Software\Classes\{PROG_ID}\shell\open\command"))?.set_string("", &open)?;

    CURRENT_USER.create(CLIENT_KEY)?.set_string("", "OxyMail")?;
    CURRENT_USER.create(format!(r"{CLIENT_KEY}\shell\open\command"))?.set_string("", format!("\"{exe}\""))?;
    let caps = CURRENT_USER.create(CAPABILITIES)?;
    caps.set_string("ApplicationName", "OxyMail")?;
    caps.set_string("ApplicationDescription", "Email, calendar and contacts")?;
    CURRENT_USER.create(format!(r"{CAPABILITIES}\URLAssociations"))?.set_string("mailto", PROG_ID)?;
    CURRENT_USER.create(r"Software\RegisteredApplications")?.set_string("OxyMail", CAPABILITIES)?;
    Ok(())
}

/// Whether OxyMail registered itself before (so an update can refresh the program path).
pub fn mail_client_registered() -> bool {
    CURRENT_USER.open(CLIENT_KEY).is_ok()
}

/// Whether the user picked OxyMail for `mailto:` links.
pub fn is_default_mail_app() -> bool {
    CURRENT_USER
        .open(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\mailto\UserChoice")
        .and_then(|key| key.get_string("ProgId"))
        .is_ok_and(|prog| prog.eq_ignore_ascii_case(PROG_ID))
}

/// Windows only lets the user change a default app, so this opens Settings at OxyMail's page
/// (Windows 11) or at Default apps (Windows 10), where they choose it for email links.
pub const DEFAULT_APPS_PAGE: &str = "ms-settings:defaultapps?registeredAppUser=OxyMail";
