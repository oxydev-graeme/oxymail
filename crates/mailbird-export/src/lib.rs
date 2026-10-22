//! Exports a copy of a Mailbird `Store` folder to open formats.
//!
//! Mail goes to Maildir (one tree per account, one folder per Mailbird folder).
//! Every non-secret table is also dumped to JSON Lines as a lossless safety net.
//! The store is opened read-only and never modified.
//!
//! Used by the `oxymail-mailbird-export` command and by the app's "Bring over Mailbird" flow.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::NaiveDateTime;
use flate2::read::DeflateDecoder;
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;
use mail_builder::headers::message_id::MessageId;
use mail_builder::headers::raw::Raw;
use mail_builder::headers::text::Text;
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde_json::{Map, Value, json};

mod extras;

pub use extras::ExtrasStats;

#[derive(Default)]
pub struct Options {
    /// Only export this Mailbird account id
    pub account: Option<i64>,
    /// Stop after this many messages (for test runs)
    pub limit: Option<usize>,
    /// Only write contacts (vCard/CSV) and calendars (.ics); skip mail
    pub extras_only: bool,
}

/// How far an export has got, for a progress display.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct Summary {
    pub accounts: usize,
    pub messages: usize,
    pub missing_bodies: usize,
    pub missing_attachments: usize,
    /// Attachments the system's antivirus wouldn't let us read; marked in their messages.
    pub blocked_attachments: usize,
    /// Mailbird ids of messages that couldn't be written (antivirus, as a rule); see report.json.
    pub unwritable: Vec<i64>,
    pub extras: ExtrasStats,
}

/// True if `dir` is a Mailbird `Store` folder (it holds `Store.db`), rather than an export.
pub fn is_store(dir: &Path) -> bool {
    dir.join("Store.db").is_file()
}

/// Exports the Mailbird store in `store` to `out`. The store is only ever read.
pub fn export(store: &Path, out: &Path, opts: &Options, mut progress: impl FnMut(Progress)) -> Result<Summary> {
    if !is_store(store) {
        bail!("{} isn't a Mailbird Store folder: there's no Store.db in it", store.display());
    }
    if out.starts_with(store) {
        bail!("Choose a folder outside the Mailbird store for the export");
    }
    // An earlier export (it has report.json) may be written over; any other folder must be empty.
    let occupied = fs::read_dir(out).map(|mut d| d.next().is_some()).unwrap_or(false);
    if occupied && !out.join("report.json").is_file() {
        bail!("{} already has files in it. Choose an empty folder for the export.", out.display());
    }
    fs::create_dir_all(out)?;
    restrict_permissions(out)?;

    let opened = open_store(store, out)?;
    let result = (|| {
        let db = &opened.db;
        let extras = extras::export(db, store, out)?;
        let mut summary =
            Summary {
            accounts: 0,
            messages: 0,
            missing_bodies: 0,
            missing_attachments: 0,
            blocked_attachments: 0,
            unwritable: Vec::new(),
            extras,
        };
        if opts.extras_only {
            return Ok(summary);
        }
        dump_raw_tables(db, &out.join("raw"))?;
        let stats = export_mail(db, store, out, opts, &mut progress)?;
        write_report(out, &stats)?;
        summary.accounts = stats.len();
        for s in stats.values() {
            summary.messages += s.messages;
            summary.missing_bodies += s.missing_body;
            summary.missing_attachments += s.missing_attachments;
            summary.blocked_attachments += s.blocked_attachments;
            summary.unwritable.extend(&s.unwritable);
        }
        Ok(summary)
    })();
    drop(opened);
    result
}

/// The open store database, plus the temporary copy it was read from, if one was needed.
struct OpenedStore {
    db: Connection,
    temp: Option<PathBuf>,
}

impl Drop for OpenedStore {
    fn drop(&mut self) {
        if let Some(dir) = &self.temp {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

/// Opens `Store.db` read-only. `immutable=1` keeps SQLite from touching the folder, but it also
/// ignores the write-ahead log, where Mailbird's most recent changes can still be sitting. So when
/// a non-empty `Store.db-wal` is present, the database and its log are copied next to the export
/// and read from there instead, and the copy is removed afterwards.
fn open_store(store: &Path, out: &Path) -> Result<OpenedStore> {
    let db_path = store.join("Store.db");
    let wal = store.join("Store.db-wal");
    if fs::metadata(&wal).map(|m| m.len() > 0).unwrap_or(false) {
        let temp = out.join(".store-copy");
        fs::create_dir_all(&temp)?;
        fs::copy(&db_path, temp.join("Store.db")).context("copying Store.db")?;
        fs::copy(&wal, temp.join("Store.db-wal")).context("copying Store.db-wal")?;
        let db = Connection::open_with_flags(temp.join("Store.db"), OpenFlags::SQLITE_OPEN_READ_WRITE)
            .with_context(|| format!("opening the copy of {}", db_path.display()))?;
        return Ok(OpenedStore { db, temp: Some(temp) });
    }
    let uri = format!("file:{}?mode=ro&immutable=1", uri_path(&db_path));
    let db = Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI)
        .with_context(|| format!("opening {}", db_path.display()))?;
    Ok(OpenedStore { db, temp: None })
}

/// A path written for a SQLite `file:` URI: forward slashes, a leading `/` before a Windows drive
/// letter, and the characters URIs reserve percent-encoded.
fn uri_path(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('\\', "/");
    if s.as_bytes().get(1) == Some(&b':') {
        s.insert(0, '/');
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '?' | '#' | '%' | ' ' => out.push_str(&format!("%{:02X}", c as u32)),
            _ => out.push(c),
        }
    }
    out
}

// Folders.Identity values
const INBOX: i64 = 1;
const STARRED: i64 = 2;
const DRAFTS: i64 = 3;
const SENT: i64 = 4;
const ALL_MAIL: i64 = 5;
const IMPORTANT: i64 = 6;
const SPAM: i64 = 7;
const TRASH: i64 = 8;
const ARCHIVED: i64 = 9;
const SNOOZED: i64 = 10;
const SCHEDULED: i64 = 11;

// Messages_Contacts.Type values
const FROM: i64 = 0;
const REPLY_TO: i64 = 1;
const TO: i64 = 2;
const CC: i64 = 3;
const BCC: i64 = 4;

/// Columns that hold credentials or sync tokens; never written out.
const SECRET_COLUMNS: &[&str] = &[
    "EncryptedPassword",
    "AccessToken",
    "RefreshToken",
    "SyncToken",
    "AclSyncToken",
    "ExchangeSyncToken",
];
/// Tables skipped in the raw dump: secrets, search index, and bodies (exported as mail).
const SKIP_TABLES: &[&str] = &["OAuth2Credentials", "MessageBodies"];

struct Folder {
    identity: i64,
    name: String,
}

struct Attachment {
    id: i64,
    file_name: Option<String>,
    content_type: Option<String>,
    content_id: Option<String>,
    inline: bool,
    size: i64,
}

#[derive(Default)]
struct AccountStats {
    messages: usize,
    missing_body: usize,
    truncated_body: usize,
    attachments: usize,
    missing_attachments: usize,
    bad_dates: usize,
    lossy_utf8: usize,
    blocked_attachments: usize,
    unwritable: Vec<i64>,
    folders: BTreeMap<String, usize>,
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_: &Path) -> Result<()> {
    Ok(())
}

/// Dumps every table (bar secrets, FTS and bodies) to `<table>.jsonl`. Blobs are skipped.
fn dump_raw_tables(db: &Connection, dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    let tables: Vec<String> = db
        .prepare("select name from sqlite_master where type='table' order by name")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    for table in tables {
        if table.starts_with("FTS_") || SKIP_TABLES.contains(&table.as_str()) {
            continue;
        }
        let mut stmt = db.prepare(&format!("select * from \"{table}\""))?;
        let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
        let mut out = std::io::BufWriter::new(fs::File::create(dir.join(format!("{table}.jsonl")))?);
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let mut obj = Map::new();
            for (i, col) in columns.iter().enumerate() {
                if SECRET_COLUMNS.contains(&col.as_str()) {
                    continue;
                }
                let value = match row.get_ref(i)? {
                    ValueRef::Null | ValueRef::Blob(_) => Value::Null,
                    ValueRef::Integer(n) => json!(n),
                    ValueRef::Real(f) => json!(f),
                    ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                };
                obj.insert(col.clone(), value);
            }
            serde_json::to_writer(&mut out, &obj)?;
            out.write_all(b"\n")?;
        }
        out.flush()?;
    }
    Ok(())
}

fn export_mail(
    db: &Connection,
    store: &Path,
    out: &Path,
    opts: &Options,
    progress: &mut impl FnMut(Progress),
) -> Result<BTreeMap<String, AccountStats>> {
    let mut total: usize = db.query_row(
        "select count(*) from Messages where (?1 is null or AccountId = ?1)",
        [opts.account],
        |r| r.get::<_, i64>(0),
    )? as usize;
    if let Some(limit) = opts.limit {
        total = total.min(limit);
    }
    progress(Progress { done: 0, total });

    let accounts: HashMap<i64, (i64, String)> = db
        .prepare("select Id, Identity, Username from Accounts")?
        .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
        .collect::<Result<_, _>>()?;

    let folders: HashMap<i64, Folder> = db
        .prepare("select Id, Identity, Name from Folders")?
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                Folder { identity: r.get(1)?, name: r.get(2)? },
            ))
        })?
        .collect::<Result<_, _>>()?;

    let mut memberships: HashMap<i64, Vec<i64>> = HashMap::new();
    for row in db
        .prepare("select MessageId, FolderId from Folders_Messages")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
    {
        let (message, folder) = row?;
        memberships.entry(message).or_default().push(folder);
    }

    let mut contacts: HashMap<i64, Vec<(i64, String, String)>> = HashMap::new();
    for row in db
        .prepare("select MessageId, Type, Email, Name from Messages_Contacts order by Id")?
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                r.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ))
        })?
    {
        let (message, kind, email, name) = row?;
        contacts.entry(message).or_default().push((kind, email, name));
    }

    let mut references: HashMap<i64, Vec<String>> = HashMap::new();
    for row in db
        .prepare("select MessageId, MailMessageId from MessageReferences order by Id")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)))?
    {
        if let (message, Some(id)) = row? {
            references.entry(message).or_default().push(id);
        }
    }

    let mut attachments: HashMap<i64, Vec<Attachment>> = HashMap::new();
    for row in db
        .prepare(
            "select Id, MessageId, SuggestedFileName, ContentType, ContentId, IsContentIdInBody, Size
             from Attachments order by Id",
        )?
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(1)?,
                Attachment {
                    id: r.get(0)?,
                    file_name: r.get(2)?,
                    content_type: r.get(3)?,
                    content_id: r.get(4)?,
                    inline: r.get::<_, Option<bool>>(5)?.unwrap_or(false),
                    size: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
                },
            ))
        })?
    {
        let (message, attachment) = row?;
        attachments.entry(message).or_default().push(attachment);
    }

    let mut body_stmt = db.prepare("select Data, IsHtml from MessageBodies where Id = ?")?;
    let mut msg_stmt = db.prepare(
        "select Id, AccountId, Subject, ReceivedAt_UTC, IsRead, MailMessageId,
                InReplyTo_MailMessageId, MessageBodyId, ThreadId, ListUnsubscribe
         from Messages where (?1 is null or AccountId = ?1) order by AccountId, Id",
    )?;
    let mut rows = msg_stmt.query([opts.account])?;

    let mut stats: BTreeMap<String, AccountStats> = BTreeMap::new();
    let mut done = 0usize;
    while let Some(r) = rows.next()? {
        if opts.limit.is_some_and(|l| done >= l) {
            break;
        }
        let id: i64 = r.get(0)?;
        let account_id: i64 = r.get(1)?;
        let subject: Option<String> = r.get(2)?;
        let received: Option<String> = r.get(3)?;
        let is_read: bool = r.get::<_, Option<bool>>(4)?.unwrap_or(false);
        let message_id: Option<String> = r.get(5)?;
        let in_reply_to: Option<String> = r.get(6)?;
        let body_id: Option<i64> = r.get(7)?;
        let thread_id: Option<String> = r.get(8)?;
        let list_unsubscribe: Option<String> = r.get(9)?;

        let (account_identity, username) = accounts
            .get(&account_id)
            .cloned()
            .unwrap_or((0, format!("account-{account_id}")));
        let st = stats.entry(username.clone()).or_default();
        st.messages += 1;

        let my_folders: Vec<&Folder> = memberships
            .get(&id)
            .map(|ids| ids.iter().filter_map(|f| folders.get(f)).collect())
            .unwrap_or_default();
        let has = |identity: i64| my_folders.iter().any(|f| f.identity == identity);

        let mut builder = MessageBuilder::new();

        let timestamp = received.as_deref().and_then(parse_date);
        if timestamp.is_none() {
            st.bad_dates += 1;
        }
        builder = builder.date(timestamp.unwrap_or(0));

        let message_id = message_id.unwrap_or_else(|| format!("mailbird-{id}@oxymail.local"));
        builder = builder.message_id(strip_brackets(&message_id));
        if let Some(parent) = in_reply_to {
            builder = builder.in_reply_to(strip_brackets(&parent));
        }
        if let Some(refs) = references.get(&id) {
            let refs: Vec<String> = refs.iter().map(|r| strip_brackets(r)).collect();
            builder = builder.references(MessageId::from(refs));
        }

        let people = contacts.get(&id).map(Vec::as_slice).unwrap_or(&[]);
        for (kind, set) in [(FROM, 0), (REPLY_TO, 1), (TO, 2), (CC, 3), (BCC, 4)] {
            let list: Vec<Address> = people
                .iter()
                .filter(|(k, email, _)| *k == kind && !email.is_empty())
                .map(|(_, email, name)| {
                    let name = (!name.is_empty() && name != email).then(|| name.clone());
                    Address::new_address(name, email.clone())
                })
                .collect();
            if list.is_empty() {
                continue;
            }
            let addr = Address::new_list(list);
            builder = match set {
                0 => builder.from(addr),
                1 => builder.reply_to(addr),
                2 => builder.to(addr),
                3 => builder.cc(addr),
                _ => builder.bcc(addr),
            };
        }

        builder = builder.subject(subject.unwrap_or_default());

        let body = match body_id {
            Some(bid) => body_stmt
                .query_row([bid], |b| Ok((b.get::<_, Option<Vec<u8>>>(0)?, b.get::<_, Option<bool>>(1)?)))
                .ok(),
            None => None,
        };
        match body {
            Some((Some(data), is_html)) => {
                let (raw, complete) = inflate(&data);
                if !complete {
                    st.truncated_body += 1;
                    builder = builder.header("X-OxyMail-Truncated", Raw::new("body"));
                }
                let text = match String::from_utf8(raw) {
                    Ok(s) => s,
                    Err(e) => {
                        st.lossy_utf8 += 1;
                        String::from_utf8_lossy(e.as_bytes()).into_owned()
                    }
                };
                builder = if is_html.unwrap_or(false) {
                    builder.html_body(text)
                } else {
                    builder.text_body(text)
                };
            }
            _ => {
                st.missing_body += 1;
                builder = builder
                    .header("X-OxyMail-Missing", Raw::new("body"))
                    .text_body(
                        "[OxyMail: Mailbird never downloaded the body of this message. \
                         It may still be on the mail server.]",
                    );
            }
        }

        for att in attachments.get(&id).map(Vec::as_slice).unwrap_or(&[]) {
            st.attachments += 1;
            match read_attachment(store, att.id)? {
                Attached::Blocked(reason) => {
                    st.blocked_attachments += 1;
                    let name = att.file_name.clone().unwrap_or_else(|| format!("attachment-{}", att.id));
                    builder = builder.header(
                        "X-OxyMail-Blocked-Attachment",
                        Text::new(format!("{name} ({} bytes): {reason}", att.size)),
                    );
                }
                Attached::Found(disk_name, data) => {
                    let ct = att.content_type.clone().unwrap_or_else(|| "application/octet-stream".into());
                    let name = att.file_name.clone().unwrap_or(disk_name);
                    builder = match (&att.content_id, att.inline) {
                        (Some(cid), true) => builder.inline(ct, strip_brackets(cid), data),
                        _ => builder.attachment(ct, name, data),
                    };
                }
                Attached::Missing => {
                    st.missing_attachments += 1;
                    let name = att.file_name.clone().unwrap_or_else(|| format!("attachment-{}", att.id));
                    builder = builder.header(
                        "X-OxyMail-Missing-Attachment",
                        Text::new(format!("{name} ({} bytes)", att.size)),
                    );
                }
            }
        }

        if let Some(unsub) = list_unsubscribe {
            builder = builder.header("List-Unsubscribe", Raw::new(unsub));
        }
        if account_identity == 1 {
            if let Some(thread) = thread_id {
                builder = builder.header("X-GM-THRID", Raw::new(thread));
            }
        }
        let labels: Vec<&str> = my_folders.iter().map(|f| f.name.as_str()).collect();
        if !labels.is_empty() {
            builder = builder.header("X-OxyMail-Folders", Text::new(labels.join(", ")));
        }
        builder = builder.header("X-OxyMail-Mailbird-Id", Raw::new(id.to_string()));

        let bytes = builder.write_to_vec()?;

        let mut flags = String::new();
        if has(DRAFTS) {
            flags.push('D');
        }
        if has(STARRED) {
            flags.push('F');
        }
        if is_read {
            flags.push('S');
        }
        // Windows forbids ':' in file names; Maildir tools use ';' there instead.
        let info = if cfg!(windows) { ";2," } else { ":2," };
        let file_name = format!("{}.M{id}.oxymail{info}{flags}", timestamp.unwrap_or(0).max(0));

        let account_dir = out.join("mail").join(sanitize(&username));
        let mut written: Option<PathBuf> = None;
        for folder in target_folders(&my_folders, account_identity == 1) {
            let dir = account_dir.join(sanitize(&folder));
            for sub in ["cur", "new", "tmp"] {
                fs::create_dir_all(dir.join(sub))?;
            }
            let path = dir.join("cur").join(&file_name);
            let result = match &written {
                Some(first) => fs::hard_link(first, &path).or_else(|_| fs::write(&path, &bytes)),
                None => fs::write(&path, &bytes),
            };
            match result {
                Ok(()) => {
                    written.get_or_insert(path);
                    *st.folders.entry(folder).or_default() += 1;
                }
                // A full disk stops the export; anything else is this one message (on Windows,
                // usually Defender refusing a file with a flagged attachment), so it's recorded
                // in the report and the export carries on.
                Err(e) if e.kind() == std::io::ErrorKind::StorageFull => return Err(e.into()),
                Err(_) => {
                    let _ = fs::remove_file(&path);
                    if written.is_none() {
                        st.unwritable.push(id);
                        break;
                    }
                }
            }
        }

        done += 1;
        if done % 500 == 0 {
            progress(Progress { done, total });
        }
    }
    progress(Progress { done, total });
    Ok(stats)
}

/// Picks the Maildir folders a message is written to. Virtual folders (Starred, Important,
/// Snoozed, Scheduled) become flags or headers. For Gmail, All Mail is only used when the
/// message has no other real folder, and is then called "Archive".
fn target_folders(folders: &[&Folder], is_gmail: bool) -> Vec<String> {
    let virtual_folder = |f: &&&Folder| {
        matches!(f.identity, STARRED | IMPORTANT | SNOOZED | SCHEDULED)
            || (is_gmail && f.identity == ALL_MAIL)
    };
    let mut names: Vec<String> = folders
        .iter()
        .filter(|f| !virtual_folder(f))
        .map(|f| match f.identity {
            INBOX => "Inbox".to_string(),
            SENT => "Sent".to_string(),
            DRAFTS => "Drafts".to_string(),
            SPAM => "Spam".to_string(),
            TRASH => "Trash".to_string(),
            ARCHIVED => "Archive".to_string(),
            _ => f.name.clone(),
        })
        .collect();
    names.sort();
    names.dedup();
    if names.is_empty() {
        names.push(if folders.is_empty() { "Unfiled" } else { "Archive" }.to_string());
    }
    names
}

enum Attached {
    Found(String, Vec<u8>),
    Missing,
    /// On disk but unreadable, usually because antivirus blocks it. Holds the reason.
    Blocked(String),
}

fn read_attachment(store: &Path, id: i64) -> Result<Attached> {
    let dir = store.join("A").join(id.to_string());
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Attached::Missing);
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            return Ok(match fs::read(entry.path()) {
                Ok(data) => Attached::Found(name, data),
                Err(e) => Attached::Blocked(e.to_string()),
            });
        }
    }
    Ok(Attached::Missing)
}

/// Inflates a raw DEFLATE body. Empty data is an empty body. On a damaged stream, returns
/// whatever decoded before the damage and `false`.
fn inflate(data: &[u8]) -> (Vec<u8>, bool) {
    use std::io::Read;
    if data.is_empty() {
        return (Vec::new(), true);
    }
    let mut decoder = DeflateDecoder::new(data);
    let mut out = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match decoder.read(&mut chunk) {
            Ok(0) => return (out, true),
            Ok(n) => out.extend_from_slice(&chunk[..n]),
            Err(_) => return (out, false),
        }
    }
}

/// Mailbird dates look like `2025-09-07 06:50:50Z` or `2020-11-09 09:16:16.8964887Z` (UTC).
fn parse_date(s: &str) -> Option<i64> {
    NaiveDateTime::parse_from_str(s.trim_end_matches('Z'), "%Y-%m-%d %H:%M:%S%.f")
        .ok()
        .map(|d| d.and_utc().timestamp())
}

fn strip_brackets(id: &str) -> String {
    id.trim().trim_start_matches('<').trim_end_matches('>').to_string()
}

/// Makes a name safe as a single path component.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c == '/' || c == '\\' || c.is_control() || (cfg!(windows) && WINDOWS_FORBIDDEN.contains(c)) { '_' } else { c })
        .collect();
    let mut cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cfg!(windows) {
        // Windows also drops trailing dots and refuses device names such as CON or NUL.
        cleaned = cleaned.trim_end_matches(['.', ' ']).to_string();
        let stem = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
        if WINDOWS_RESERVED.contains(&stem.as_str()) || stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.ends_with(|c: char| c.is_ascii_digit()) {
            cleaned.insert(0, '_');
        }
    }
    if cleaned.is_empty() { "_".to_string() } else { cleaned }
}

const WINDOWS_FORBIDDEN: &str = "<>:\"|?*";
const WINDOWS_RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL"];

fn write_report(out: &Path, stats: &BTreeMap<String, AccountStats>) -> Result<()> {
    let mut report = Map::new();
    let mut totals = AccountStats::default();
    for (account, s) in stats {
        totals.messages += s.messages;
        totals.missing_body += s.missing_body;
        totals.truncated_body += s.truncated_body;
        totals.attachments += s.attachments;
        totals.missing_attachments += s.missing_attachments;
        totals.bad_dates += s.bad_dates;
        totals.lossy_utf8 += s.lossy_utf8;
        totals.blocked_attachments += s.blocked_attachments;
        totals.unwritable.extend(&s.unwritable);
        report.insert(
            account.clone(),
            json!({
                "messages": s.messages,
                "missing_body": s.missing_body,
                "truncated_body": s.truncated_body,
                "attachments": s.attachments,
                "missing_attachments": s.missing_attachments,
                "bad_dates": s.bad_dates,
                "lossy_utf8_bodies": s.lossy_utf8,
                "blocked_attachments": s.blocked_attachments,
                "unwritable_mailbird_ids": s.unwritable,
                "folders": s.folders,
            }),
        );
    }
    let report = json!({
        "totals": {
            "messages": totals.messages,
            "missing_body": totals.missing_body,
            "truncated_body": totals.truncated_body,
            "attachments": totals.attachments,
            "missing_attachments": totals.missing_attachments,
            "bad_dates": totals.bad_dates,
            "lossy_utf8_bodies": totals.lossy_utf8,
            "blocked_attachments": totals.blocked_attachments,
            "unwritable_messages": totals.unwritable.len(),
        },
        "accounts": report,
    });
    fs::write(out.join("report.json"), serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_paths_work_for_windows_drives_and_odd_characters() {
        assert_eq!(uri_path(Path::new(r"C:\Store\Store.db")), "/C:/Store/Store.db");
        assert_eq!(uri_path(Path::new("/home/me/My Store/Store.db")), "/home/me/My%20Store/Store.db");
        assert_eq!(uri_path(Path::new("/tmp/a#b?c%d/Store.db")), "/tmp/a%23b%3Fc%25d/Store.db");
    }

    #[test]
    fn folder_names_are_safe_path_components() {
        assert_eq!(sanitize("Work/Clients"), "Work_Clients");
        assert_eq!(sanitize("..hidden"), "hidden");
        if cfg!(windows) {
            assert_eq!(sanitize("Re: what?"), "Re_ what_");
            assert_eq!(sanitize("con"), "_con");
            assert_eq!(sanitize("COM1.txt"), "_COM1.txt");
            assert_eq!(sanitize("Notes..."), "Notes");
        }
    }

    #[test]
    fn refuses_a_folder_that_is_not_a_store_or_an_export_inside_the_store() {
        let dir = std::env::temp_dir().join(format!("oxymail-export-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let e = export(&dir, &dir.join("out"), &Options::default(), |_| {}).unwrap_err();
        assert!(e.to_string().contains("isn't a Mailbird Store"), "{e}");

        fs::write(dir.join("Store.db"), b"").unwrap();
        assert!(is_store(&dir));
        let e = export(&dir, &dir.join("out"), &Options::default(), |_| {}).unwrap_err();
        assert!(e.to_string().contains("outside the Mailbird store"), "{e}");
        assert!(!dir.join("out").exists());

        let busy = dir.with_extension("busy");
        fs::create_dir_all(&busy).unwrap();
        fs::write(busy.join("notes.txt"), b"mine").unwrap();
        let e = export(&dir, &busy, &Options::default(), |_| {}).unwrap_err();
        assert!(e.to_string().contains("already has files"), "{e}");
        let _ = fs::remove_dir_all(&busy);
        let _ = fs::remove_dir_all(&dir);
    }
}
