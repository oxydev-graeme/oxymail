//! IMAP download. Read-only towards the server: messages are fetched with `BODY.PEEK[]` so
//! downloading never marks anything as read, and nothing is moved, flagged or deleted.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_imap::types::{Flag, NameAttribute};
use async_imap::{Client, Session};
use futures::TryStreamExt;
use oxymail_core::Store;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore, pki_types::ServerName};

use crate::autoconfig::{AccountConfig, Security, Server};

pub type ImapSession = Session<TlsStream<TcpStream>>;

/// How many of the newest messages to download per folder on the first sync.
pub const INITIAL_MESSAGES_PER_FOLDER: u32 = 1000;
const FETCH_BATCH: usize = 50;
const NETWORK_TIMEOUT: Duration = Duration::from_secs(30);

pub fn tls_connector() -> TlsConnector {
    let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    // Name the crypto provider: both ring (via lettre) and aws-lc (via reqwest) are linked in.
    let provider = Arc::new(tokio_rustls::rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

async fn tls(server: &Server, tcp: TcpStream) -> Result<TlsStream<TcpStream>> {
    let name = ServerName::try_from(server.host.clone()).context("invalid server name")?;
    Ok(tls_connector().connect(name, tcp).await.with_context(|| format!("TLS handshake with {}", server.host))?)
}

/// Connects and signs in. `password` is the account password, or the OAuth access token when
/// `config.auth` is set.
pub async fn connect(config: &AccountConfig, password: &str) -> Result<ImapSession> {
    let server = &config.imap;
    let tcp = timeout(NETWORK_TIMEOUT, TcpStream::connect((server.host.as_str(), server.port)))
        .await
        .map_err(|_| anyhow!("timed out connecting to {}:{}", server.host, server.port))?
        .with_context(|| format!("connecting to {}:{}", server.host, server.port))?;

    let stream = match server.security {
        Security::Tls => tls(server, tcp).await?,
        Security::StartTls => {
            let mut client = Client::new(tcp);
            client.read_response().await?.context("server closed the connection")?;
            client.run_command_and_check_ok("STARTTLS", None).await.context("STARTTLS refused")?;
            tls(server, client.into_inner()).await?
        }
    };

    let mut client = Client::new(stream);
    if server.security == Security::Tls {
        client.read_response().await?.context("server closed the connection")?;
    }
    let session = if config.auth.is_some() {
        let auth = XOAuth2 { response: crate::oauth::xoauth2(&config.username, password), sent: false };
        timeout(NETWORK_TIMEOUT, client.authenticate("XOAUTH2", auth))
            .await
            .map_err(|_| anyhow!("timed out signing in"))?
            .map_err(|(e, _)| anyhow!("sign-in was refused: {e}"))?
    } else {
        timeout(NETWORK_TIMEOUT, client.login(&config.username, password))
            .await
            .map_err(|_| anyhow!("timed out logging in"))?
            .map_err(|(e, _)| anyhow!("login failed: {e}"))?
    };
    Ok(session)
}

/// XOAUTH2 sends its token once; if the server answers with an error challenge, an empty
/// reply ends the exchange so the server reports the failure.
struct XOAuth2 {
    response: String,
    sent: bool,
}

impl async_imap::Authenticator for XOAuth2 {
    type Response = String;
    fn process(&mut self, _challenge: &[u8]) -> String {
        if std::mem::replace(&mut self.sent, true) { String::new() } else { self.response.clone() }
    }
}

pub struct FolderInfo {
    pub remote_name: String,
    pub display: String,
    pub role: Option<&'static str>,
}

/// Lists selectable folders with their roles (from SPECIAL-USE attributes, then names).
pub async fn folders(session: &mut ImapSession) -> Result<Vec<FolderInfo>> {
    let names: Vec<_> = session.list(Some(""), Some("*")).await?.try_collect().await?;
    let mut out = Vec::new();
    for name in &names {
        if name.attributes().iter().any(|a| matches!(a, NameAttribute::NoSelect)) {
            continue;
        }
        let remote = name.name().to_string();
        let decoded = decode_modified_utf7(&remote);
        let delimiter = name.delimiter().unwrap_or("/");
        let leaf = decoded.rsplit(delimiter).next().unwrap_or(&decoded).to_string();
        let role = name
            .attributes()
            .iter()
            .find_map(|a| match a {
                NameAttribute::Sent => Some("sent"),
                NameAttribute::Drafts => Some("drafts"),
                NameAttribute::Junk => Some("spam"),
                NameAttribute::Trash => Some("trash"),
                NameAttribute::Archive => Some("archive"),
                NameAttribute::All => Some("all"),
                NameAttribute::Flagged => Some("flagged"),
                _ => None,
            })
            .or_else(|| role_from_name(&remote, &leaf));
        // Gmail's "[Gmail]/..." prefix is noise; nested folders keep their full path.
        let display = if decoded.starts_with("[Gmail]") || decoded.starts_with("[Google Mail]") {
            leaf.clone()
        } else if role == Some("inbox") {
            "Inbox".to_string()
        } else {
            decoded.replace(delimiter, " / ")
        };
        out.push(FolderInfo { remote_name: remote, display, role });
    }
    Ok(out)
}

fn role_from_name(remote: &str, leaf: &str) -> Option<&'static str> {
    if remote.eq_ignore_ascii_case("INBOX") {
        return Some("inbox");
    }
    match leaf.to_ascii_lowercase().as_str() {
        "sent" | "sent items" | "sent mail" | "sent messages" => Some("sent"),
        "drafts" | "draft" => Some("drafts"),
        "spam" | "junk" | "junk email" | "junk e-mail" => Some("spam"),
        "trash" | "deleted" | "deleted items" | "deleted messages" | "bin" => Some("trash"),
        "archive" | "archives" => Some("archive"),
        _ => None,
    }
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct SyncReport {
    pub folders: usize,
    pub new_messages: usize,
    pub removed: usize,
    pub errors: Vec<String>,
}

/// Downloads new messages and refreshes flags for every folder of a live account.
/// `progress` receives a short status line as each folder is processed.
pub async fn sync_account(
    store: &mut Store,
    account_id: i64,
    config: &AccountConfig,
    password: &str,
    mut progress: impl FnMut(String),
) -> Result<SyncReport> {
    let mut session = connect(config, password).await?;
    let mut report = SyncReport::default();
    let list = folders(&mut session).await?;
    for info in &list {
        // Gmail's All Mail and Starred duplicate every other folder; skip them.
        if matches!(info.role, Some("all") | Some("flagged")) {
            continue;
        }
        progress(format!("{}: {}", config.email, info.display));
        let role = info.role.filter(|r| *r != "all" && *r != "flagged");
        match sync_folder(store, &mut session, account_id, info, role).await {
            Ok((new, removed)) => {
                report.folders += 1;
                report.new_messages += new;
                report.removed += removed;
            }
            Err(e) => report.errors.push(format!("{}: {e:#}", info.display)),
        }
    }
    let _ = session.logout().await;
    Ok(report)
}

async fn sync_folder(
    store: &mut Store,
    session: &mut ImapSession,
    account_id: i64,
    info: &FolderInfo,
    role: Option<&str>,
) -> Result<(usize, usize)> {
    let folder = store.remote_folder(account_id, &info.remote_name, &info.display, role)?;
    // EXAMINE opens the folder read-only, so the server can't clear \Recent or similar.
    let mailbox = session.examine(&info.remote_name).await?;
    let uidvalidity = mailbox.uid_validity.unwrap_or(0);
    match folder.uidvalidity {
        Some(v) if v != uidvalidity => store.reset_remote_folder(folder.id, uidvalidity)?,
        None => store.set_folder_uidvalidity(folder.id, uidvalidity)?,
        _ => {}
    }
    let last_uid = if folder.uidvalidity == Some(uidvalidity) { folder.last_uid } else { 0 };
    if mailbox.exists == 0 {
        let removed = match store.cached_uid_range(folder.id)? {
            Some((low, _)) => store.forget_missing_uids(folder.id, low, &[])?,
            None => 0,
        };
        return Ok((0, removed));
    }

    // Which UIDs to download.
    let wanted: Vec<u32> = if last_uid == 0 {
        let first = mailbox.exists.saturating_sub(INITIAL_MESSAGES_PER_FOLDER) + 1;
        let fetches: Vec<_> = session.fetch(format!("{first}:*"), "UID").await?.try_collect().await?;
        fetches.iter().filter_map(|f| f.uid).collect()
    } else {
        let fetches: Vec<_> = session.uid_fetch(format!("{}:*", last_uid + 1), "UID").await?.try_collect().await?;
        fetches.iter().filter_map(|f| f.uid).filter(|u| *u > last_uid).collect()
    };

    let mut new = 0;
    for batch in wanted.chunks(FETCH_BATCH) {
        let set = batch.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let fetches: Vec<_> = session.uid_fetch(&set, "(UID FLAGS BODY.PEEK[])").await?.try_collect().await?;
        for fetch in &fetches {
            let (Some(uid), Some(body)) = (fetch.uid, fetch.body()) else { continue };
            if store.store_remote_message(folder.id, uid, &maildir_flags(fetch.flags()), body)? {
                new += 1;
            }
        }
    }

    // Refresh flags and notice deletions for everything we hold.
    let mut removed = 0;
    if let Some((low, high)) = store.cached_uid_range(folder.id)? {
        let fetches: Vec<_> = session.uid_fetch(format!("{low}:{high}"), "(UID FLAGS)").await?.try_collect().await?;
        let flags: Vec<(u32, String)> =
            fetches.iter().filter_map(|f| f.uid.map(|u| (u, maildir_flags(f.flags())))).collect();
        let present: Vec<u32> = flags.iter().map(|(u, _)| *u).collect();
        store.set_remote_flags(folder.id, &flags)?;
        removed = store.forget_missing_uids(folder.id, low, &present)?;
    }
    Ok((new, removed))
}

/// IMAP flags to Maildir info letters, in the order Maildir requires.
fn maildir_flags<'a>(flags: impl Iterator<Item = Flag<'a>>) -> String {
    let mut out: Vec<char> = flags
        .filter_map(|f| match f {
            Flag::Draft => Some('D'),
            Flag::Flagged => Some('F'),
            Flag::Answered => Some('R'),
            Flag::Seen => Some('S'),
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out.into_iter().collect()
}

/// Decodes IMAP's modified UTF-7 folder names (RFC 3501 §5.1.3), e.g. `Entw&APw-rfe` → `Entwürfe`.
pub fn decode_modified_utf7(input: &str) -> String {
    use base64::Engine;
    let mut out = String::new();
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let chunk = &after[..end];
        if chunk.is_empty() {
            out.push('&');
        } else {
            let b64 = chunk.replace(',', "/");
            match base64::engine::general_purpose::STANDARD_NO_PAD.decode(b64.trim_end_matches('=')) {
                Ok(bytes) => {
                    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                    out.push_str(&String::from_utf16_lossy(&units));
                }
                Err(_) => out.push_str(&rest[start..start + 2 + end]),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Appends a sent message to the account's Sent folder (not needed for Gmail, which files
/// sent mail itself).
pub async fn append_sent(config: &AccountConfig, password: &str, raw: &[u8]) -> Result<()> {
    let mut session = connect(config, password).await?;
    let sent = folders(&mut session)
        .await?
        .into_iter()
        .find(|f| f.role == Some("sent"))
        .ok_or_else(|| anyhow!("no Sent folder on the server"))?;
    session.append(&sent.remote_name, Some("(\\Seen)"), None, raw).await?;
    let _ = session.logout().await;
    Ok(())
}

/// Sets or clears `\\Seen` (`S`) or `\\Flagged` (`F`) on the server. The only write OxyMail
/// makes to live folders so far; it never moves or deletes.
pub async fn store_flag(config: &AccountConfig, password: &str, folder: &str, uids: &[u32], flag: char, on: bool) -> Result<()> {
    let imap_flag = match flag {
        'S' => "\\Seen",
        'F' => "\\Flagged",
        _ => bail!("unsupported flag {flag}"),
    };
    if uids.is_empty() {
        return Ok(());
    }
    let mut session = connect(config, password).await?;
    session.select(folder).await?;
    let set = uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let sign = if on { '+' } else { '-' };
    let _: Vec<_> = session.uid_store(&set, format!("{sign}FLAGS.SILENT ({imap_flag})")).await?.try_collect().await?;
    let _ = session.logout().await;
    Ok(())
}

/// Checks that we can log in, for the add-account form.
pub async fn test_login(config: &AccountConfig, password: &str) -> Result<()> {
    let mut session = connect(config, password).await?;
    let list = folders(&mut session).await?;
    if list.is_empty() {
        bail!("logged in, but the server lists no folders");
    }
    let _ = session.logout().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_modified_utf7() {
        assert_eq!(decode_modified_utf7("INBOX"), "INBOX");
        assert_eq!(decode_modified_utf7("Entw&APw-rfe"), "Entwürfe");
        assert_eq!(decode_modified_utf7("A &- B"), "A & B");
        assert_eq!(decode_modified_utf7("&ZeVnLIqe-"), "日本語");
    }
}
