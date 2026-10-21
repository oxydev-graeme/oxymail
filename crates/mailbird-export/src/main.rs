//! Exports a copy of a Mailbird `Store` folder to open formats.
//!
//! Mail goes to Maildir (one tree per account, one folder per Mailbird folder).
//! Every non-secret table is also dumped to JSON Lines as a lossless safety net.
//! The store is opened read-only and never modified.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use clap::Parser;
use flate2::read::DeflateDecoder;
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;
use mail_builder::headers::message_id::MessageId;
use mail_builder::headers::raw::Raw;
use mail_builder::headers::text::Text;
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde_json::{Map, Value, json};

mod extras;

#[derive(Parser)]
#[command(about = "Export a Mailbird store to Maildir and JSON")]
struct Args {
    /// Folder containing Store.db and the A/ attachments folder
    #[arg(long)]
    store: PathBuf,
    /// Output folder (created if missing)
    #[arg(long)]
    out: PathBuf,
    /// Only export this Mailbird account id
    #[arg(long)]
    account: Option<i64>,
    /// Stop after this many messages (for test runs)
    #[arg(long)]
    limit: Option<usize>,
    /// Only write contacts (vCard/CSV) and calendars (.ics); skip mail
    #[arg(long)]
    extras_only: bool,
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
    folders: BTreeMap<String, usize>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let db_path = args.store.join("Store.db");
    let db = Connection::open_with_flags(
        format!("file:{}?mode=ro&immutable=1", db_path.display()),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .with_context(|| format!("opening {}", db_path.display()))?;

    fs::create_dir_all(&args.out)?;
    restrict_permissions(&args.out)?;

    let extras = extras::export(&db, &args.store, &args.out)?;
    eprintln!(
        "contacts: {} in the address book, {} collected addresses; calendars: {} with {} events",
        extras.contacts, extras.collected_addresses, extras.calendars, extras.events
    );
    if args.extras_only {
        return Ok(());
    }
    dump_raw_tables(&db, &args.out.join("raw"))?;
    let stats = export_mail(&db, &args)?;
    write_report(&args.out, &stats)?;
    Ok(())
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
    eprintln!("raw tables dumped to {}", dir.display());
    Ok(())
}

fn export_mail(db: &Connection, args: &Args) -> Result<BTreeMap<String, AccountStats>> {
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
    let mut rows = msg_stmt.query([args.account])?;

    let mut stats: BTreeMap<String, AccountStats> = BTreeMap::new();
    let mut done = 0usize;
    while let Some(r) = rows.next()? {
        if args.limit.is_some_and(|l| done >= l) {
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
            match read_attachment(&args.store, att.id)? {
                Some((disk_name, data)) => {
                    let ct = att.content_type.clone().unwrap_or_else(|| "application/octet-stream".into());
                    let name = att.file_name.clone().unwrap_or(disk_name);
                    builder = match (&att.content_id, att.inline) {
                        (Some(cid), true) => builder.inline(ct, strip_brackets(cid), data),
                        _ => builder.attachment(ct, name, data),
                    };
                }
                None => {
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

        let account_dir = args.out.join("mail").join(sanitize(&username));
        let mut written: Option<PathBuf> = None;
        for folder in target_folders(&my_folders, account_identity == 1) {
            let dir = account_dir.join(sanitize(&folder));
            for sub in ["cur", "new", "tmp"] {
                fs::create_dir_all(dir.join(sub))?;
            }
            let path = dir.join("cur").join(&file_name);
            match &written {
                None => {
                    fs::write(&path, &bytes)?;
                    written = Some(path);
                }
                Some(first) => {
                    if fs::hard_link(first, &path).is_err() {
                        fs::write(&path, &bytes)?;
                    }
                }
            }
            *st.folders.entry(folder).or_default() += 1;
        }

        done += 1;
        if done % 5000 == 0 {
            eprintln!("{done} messages exported");
        }
    }
    eprintln!("{done} messages exported (finished)");
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

fn read_attachment(store: &Path, id: i64) -> Result<Option<(String, Vec<u8>)>> {
    let dir = store.join("A").join(id.to_string());
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(None);
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            return Ok(Some((name, fs::read(entry.path())?)));
        }
    }
    Ok(None)
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
        .map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() { "_".to_string() } else { cleaned }
}

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
        },
        "accounts": report,
    });
    fs::write(out.join("report.json"), serde_json::to_string_pretty(&report)?)?;
    eprintln!("report written to {}", out.join("report.json").display());
    Ok(())
}
