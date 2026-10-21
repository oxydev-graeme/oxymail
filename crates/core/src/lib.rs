//! OxyMail core: the local message store, Maildir import and search.
//!
//! Message files stay where they are on disk; the store keeps an index of them in SQLite
//! (headers for listing, an FTS5 table for search) and reads the file again to display it.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use base64::Engine;
use mail_parser::{MessageParser, MimeHeaders};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

mod live;
pub use live::{LiveAccount, MailbirdServer, RemoteFolder, RemoteRef};

const SCHEMA: &str = "
create table if not exists accounts (
    id integer primary key,
    name text not null,
    kind text not null,
    root text not null,
    label text,
    sender_name text,
    color text,
    signature text,
    icon text,
    config text,
    archive_root text,
    sort_index integer not null default 0,
    removed integer not null default 0,
    unique (kind, root)
);
create table if not exists folders (
    id integer primary key,
    account_id integer not null references accounts(id),
    name text not null,
    role text,
    remote_name text,
    dir text,
    uidvalidity integer,
    last_uid integer not null default 0,
    unique (account_id, name)
);
create table if not exists messages (
    id integer primary key,
    folder_id integer not null references folders(id),
    file text not null,
    date integer not null,
    from_name text,
    from_addr text,
    to_addrs text,
    subject text,
    snippet text,
    flags text not null default '',
    attachments integer not null default 0,
    message_id text,
    origin_folder_id integer not null references folders(id),
    deleted integer not null default 0,
    uid integer,
    from_archive integer not null default 0,
    thread_key text,
    snoozed_until integer,
    unique (origin_folder_id, file)
);
create index if not exists messages_folder_date on messages(folder_id, deleted, date desc);
create virtual table if not exists messages_fts using fts5(
    subject, sender, recipients, body, content=''
);
create table if not exists contacts (
    email text primary key,
    name text,
    uses integer not null default 0,
    last_used integer not null default 0,
    in_address_book integer not null default 0,
    blocked integer not null default 0,
    images_allowed integer not null default 0
);
create table if not exists drafts (
    id integer primary key,
    account_id integer,
    subject text,
    recipients text,
    body text not null,
    updated integer not null
);
create table if not exists scheduled (
    id integer primary key,
    account_id integer not null,
    send_at integer not null,
    mail text not null,
    draft text not null,
    summary text,
    error text
);
create table if not exists settings (
    key text primary key,
    value text not null
);
";

/// `PRAGMA user_version` for the current schema.
const SCHEMA_VERSION: i64 = 6;

/// v4 added what live IMAP folders need.
const FOLDER_COLUMNS_V4: &[(&str, &str)] = &[
    ("remote_name", "text"),
    ("dir", "text"),
    ("uidvalidity", "integer"),
    ("last_uid", "integer not null default 0"),
];

fn add_missing_columns(db: &Connection, table: &str, columns: &[(&str, &str)]) -> Result<()> {
    let existing: Vec<String> = db
        .prepare(&format!("select name from pragma_table_info('{table}')"))?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for (column, decl) in columns {
        if !existing.iter().any(|c| c == column) {
            db.execute_batch(&format!("alter table {table} add column {column} {decl}"))?;
        }
    }
    Ok(())
}

/// v3 added per-account settings. Applied to any accounts table that lacks them.
const ACCOUNT_COLUMNS_V3: &[(&str, &str)] = &[
    ("label", "text"),
    ("sender_name", "text"),
    ("color", "text"),
    ("signature", "text"),
    ("icon", "text"),
    ("config", "text"),
    ("sort_index", "integer not null default 0"),
    ("removed", "integer not null default 0"),
];

/// v1 had no `origin_folder_id`/`deleted`, and `unique (folder_id, file)`, which breaks as
/// soon as a message moves. The table is rebuilt; ids are kept so the FTS rows still match.
const MIGRATE_V1_TO_V2: &str = "
create table messages_v2 (
    id integer primary key,
    folder_id integer not null references folders(id),
    file text not null,
    date integer not null,
    from_name text,
    from_addr text,
    to_addrs text,
    subject text,
    snippet text,
    flags text not null default '',
    attachments integer not null default 0,
    message_id text,
    origin_folder_id integer not null references folders(id),
    deleted integer not null default 0,
    unique (origin_folder_id, file)
);
insert into messages_v2
    select id, folder_id, file, date, from_name, from_addr, to_addrs, subject, snippet, flags,
           attachments, message_id, folder_id, 0
    from messages;
drop table messages;
alter table messages_v2 rename to messages;
create index messages_folder_date on messages(folder_id, deleted, date desc);
";

/// How much body text goes into the search index per message.
const INDEXED_BODY_CHARS: usize = 20_000;
const SNIPPET_CHARS: usize = 200;

#[derive(Debug, Serialize)]
pub struct Account {
    pub id: i64,
    /// The mailbox address (or archive folder name); fixed.
    pub name: String,
    pub kind: String,
    /// Optional label shown in the sidebar instead of the address.
    pub label: Option<String>,
    /// Name used in From when sending.
    pub sender_name: Option<String>,
    /// `#rrggbb` colour for the sidebar dot.
    pub color: Option<String>,
    /// HTML signature appended to new messages.
    pub signature: Option<String>,
    /// Small square image as a `data:` URI, shown instead of the colour dot.
    pub icon: Option<String>,
    /// Unread messages outside Sent, Drafts, Spam and Trash.
    pub unread: i64,
    /// True when the account holds imported archive mail (it may also be live).
    pub has_archive: bool,
}

/// Editable account fields. `None` clears a field.
#[derive(Debug, serde::Deserialize)]
pub struct AccountUpdate {
    pub label: Option<String>,
    pub sender_name: Option<String>,
    pub color: Option<String>,
    pub signature: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Folder {
    pub id: i64,
    pub account_id: i64,
    pub name: String,
    pub role: Option<String>,
    pub total: i64,
    pub unread: i64,
}

#[derive(Debug, Serialize)]
pub struct MessageSummary {
    pub id: i64,
    /// Unix time a snoozed message comes back; `None` when not snoozed.
    pub snoozed_until: Option<i64>,
    pub folder_id: i64,
    pub folder_name: String,
    pub folder_role: Option<String>,
    pub account_id: i64,
    pub date: i64,
    pub from_name: Option<String>,
    pub from_addr: Option<String>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub unread: bool,
    pub starred: bool,
    pub attachments: i64,
}

#[derive(Debug, Serialize)]
pub struct AttachmentInfo {
    pub index: usize,
    pub name: String,
    pub content_type: String,
    pub size: usize,
}

#[derive(Debug, Serialize)]
pub struct MessageView {
    pub id: i64,
    pub account_id: i64,
    pub message_id: Option<String>,
    pub references: Vec<String>,
    pub reply_to: Vec<String>,
    pub subject: Option<String>,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub date: i64,
    /// HTML with `cid:` images inlined as data URIs. Remote content is left for the UI to block.
    pub html: Option<String>,
    pub text: Option<String>,
    pub attachments: Vec<AttachmentInfo>,
    /// OxyMail notes added at export time, such as a body Mailbird never downloaded.
    pub notices: Vec<String>,
    pub from_addr: Option<String>,
    /// The HTML loads images or styles from the web (blocked unless allowed).
    pub has_remote_content: bool,
    /// Remote content from this sender is allowed (Settings or "Always for this sender").
    pub images_allowed: bool,
    /// Unsubscribe targets from List-Unsubscribe: https: and mailto: URLs.
    pub unsubscribe: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Scheduled {
    pub id: i64,
    pub account_id: i64,
    pub send_at: i64,
    pub mail: String,
    pub draft: String,
    pub summary: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Draft {
    pub id: i64,
    pub account_id: Option<i64>,
    pub subject: Option<String>,
    pub recipients: Option<String>,
    /// The Compose window's state, as JSON.
    pub body: String,
    pub updated: i64,
}

#[derive(Debug, Serialize)]
pub struct ContactSuggestion {
    pub email: String,
    pub name: Option<String>,
}

/// Sender and subject of a raw message, for notifications.
pub fn peek_sender_subject(raw: &[u8]) -> (String, String) {
    let Some(msg) = MessageParser::default().parse_headers(raw) else { return Default::default() };
    let from = msg.from().and_then(|a| a.first());
    let sender = from.and_then(|a| a.name().or(a.address())).unwrap_or_default().to_string();
    (sender, msg.subject().unwrap_or_default().to_string())
}

#[derive(Debug, Default, Serialize)]
pub struct ImportStats {
    pub folders: usize,
    pub added: usize,
    pub skipped: usize,
    pub unreadable: usize,
}

pub struct Store {
    db: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        db.pragma_update(None, "journal_mode", "wal")?;
        db.busy_timeout(std::time::Duration::from_secs(10))?;
        migrate(&db)?;
        db.pragma_update(None, "foreign_keys", "on")?;
        Ok(Self { db })
    }

    /// Default database location: `~/.local/share/oxymail/oxymail.db`.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("oxymail").join("oxymail.db")
    }

    /// Imports a tree laid out as `<root>/<account>/<folder>/{cur,new}` as read-only archive
    /// accounts. Safe to run again: files already indexed are skipped.
    pub fn import_archive(&mut self, root: &Path, mut progress: impl FnMut(usize)) -> Result<ImportStats> {
        let mut stats = ImportStats::default();
        let parser = MessageParser::default();
        let tx = self.db.transaction()?;
        for account_dir in sorted_dirs(root)? {
            let account_name = file_name(&account_dir);
            let root_str = account_dir.to_string_lossy().to_string();
            // Matched on archive_root, so an archive later connected to its server is still found.
            let existing: Option<i64> = tx
                .query_row("select id from accounts where archive_root = ?1", [&root_str], |r| r.get(0))
                .optional()?;
            let account_id = match existing {
                Some(id) => {
                    tx.execute("update accounts set removed = 0 where id = ?1", [id])?;
                    id
                }
                None => {
                    tx.execute(
                        "insert into accounts (name, kind, root, archive_root, sort_index)
                         values (?1, 'archive', ?2, ?2, (select coalesce(max(sort_index), -1) + 1 from accounts))",
                        params![account_name, root_str],
                    )?;
                    tx.last_insert_rowid()
                }
            };
            for folder_dir in sorted_dirs(&account_dir)? {
                let folder_name = file_name(&folder_dir);
                tx.execute(
                    "insert or ignore into folders (account_id, name, role) values (?1, ?2, ?3)",
                    params![account_id, folder_name, folder_role(&folder_name)],
                )?;
                let folder_id: i64 = tx.query_row(
                    "select id from folders where account_id = ?1 and name = ?2",
                    params![account_id, folder_name],
                    |r| r.get(0),
                )?;
                stats.folders += 1;
                for sub in ["cur", "new"] {
                    let Ok(entries) = fs::read_dir(folder_dir.join(sub)) else { continue };
                    for entry in entries {
                        let entry = entry?;
                        let rel = entry.path().to_string_lossy().to_string();
                        let exists: Option<i64> = tx
                            .query_row(
                                "select id from messages where origin_folder_id = ?1 and file = ?2",
                                params![folder_id, rel],
                                |r| r.get(0),
                            )
                            .optional()?;
                        if exists.is_some() {
                            stats.skipped += 1;
                            continue;
                        }
                        let raw = fs::read(entry.path())?;
                        let Some(id) = index_message(&tx, &parser, folder_id, &rel, &raw, None)? else {
                            stats.unreadable += 1;
                            continue;
                        };
                        tx.execute("update messages set from_archive = 1 where id = ?1", [id])?;
                        stats.added += 1;
                        if stats.added % 2000 == 0 {
                            progress(stats.added);
                        }
                    }
                }
            }
        }
        apply_mailbird_metadata(&tx, root)?;
        tx.commit()?;
        Ok(stats)
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let mut stmt = self.db.prepare(
            "select a.id, a.name, a.kind, a.label, a.sender_name, a.color, a.signature, a.icon,
                    (select count(*) from messages m join folders f on f.id = m.folder_id
                     where f.account_id = a.id and m.deleted = 0 and m.snoozed_until is null and instr(m.flags, 'S') = 0
                       and coalesce(f.role, '') not in ('sent', 'drafts', 'spam', 'trash'))
             , a.archive_root is not null
             from accounts a where a.removed = 0 order by a.sort_index, a.name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Account {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                label: r.get(3)?,
                sender_name: r.get(4)?,
                color: r.get(5)?,
                signature: r.get(6)?,
                icon: r.get(7)?,
                unread: r.get(8)?,
                has_archive: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Sets the sidebar order to the order of `ids`.
    pub fn reorder_accounts(&mut self, ids: &[i64]) -> Result<()> {
        let tx = self.db.transaction()?;
        for (index, id) in ids.iter().enumerate() {
            tx.execute("update accounts set sort_index = ?1 where id = ?2", params![index as i64, id])?;
        }
        tx.execute(
            "insert into settings (key, value) values ('accounts_reordered', 'true')
             on conflict (key) do update set value = excluded.value",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn update_account(&self, id: i64, update: &AccountUpdate) -> Result<()> {
        let blank = |v: &Option<String>| v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        self.db.execute(
            "update accounts set label = ?1, sender_name = ?2, color = ?3, signature = ?4, icon = ?5 where id = ?6",
            params![
                blank(&update.label),
                blank(&update.sender_name),
                blank(&update.color),
                blank(&update.signature),
                blank(&update.icon),
                id
            ],
        )?;
        Ok(())
    }

    /// Removes an account from OxyMail. Its messages disappear from every view and search, but
    /// nothing is deleted: importing the same archive again brings it back, stars and all.
    pub fn remove_account(&self, id: i64) -> Result<()> {
        self.db.execute("update accounts set removed = 1 where id = ?1", [id])?;
        Ok(())
    }

    pub fn folders(&self, account_id: i64) -> Result<Vec<Folder>> {
        let mut stmt = self.db.prepare(
            "select f.id, f.account_id, f.name, f.role,
                    (select count(*) from messages m where m.folder_id = f.id and m.deleted = 0 and m.snoozed_until is null),
                    (select count(*) from messages m
                     where m.folder_id = f.id and m.deleted = 0 and m.snoozed_until is null and instr(m.flags, 'S') = 0)
             from folders f where f.account_id = ?1
             order by case f.role when 'inbox' then 0 when 'drafts' then 1 when 'sent' then 2
                      when 'archive' then 3 when 'spam' then 5 when 'trash' then 6 else 4 end, f.name",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(Folder {
                id: r.get(0)?,
                account_id: r.get(1)?,
                name: r.get(2)?,
                role: r.get(3)?,
                total: r.get(4)?,
                unread: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn messages(&self, folder_id: i64, offset: i64, limit: i64) -> Result<Vec<MessageSummary>> {
        self.summaries(
            "where m.folder_id = ?1 and m.deleted = 0 and m.snoozed_until is null order by m.date desc limit ?2 offset ?3",
            params![folder_id, limit, offset],
        )
    }

    /// Full-text search across every account. Plain words are matched as prefixes.
    /// Searches every account. Plain words match anywhere (as prefixes); filters narrow it:
    /// `from:` `to:` `subject:` `has:attachment` `is:unread|read|starred` `in:<folder>`
    /// `account:<text>` `before:`/`after:YYYY-MM-DD`, and "quoted phrases".
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<MessageSummary>> {
        let q = parse_search(query);
        if q.fts.is_empty() && q.conditions.is_empty() {
            return Ok(Vec::new());
        }
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        let mut tail = String::new();
        if !q.fts.is_empty() {
            values.push(q.fts.into());
            tail.push_str("join (select rowid as hit, rank from messages_fts where messages_fts match ?1) s on s.hit = m.id ");
        }
        tail.push_str("where m.deleted = 0");
        for (sql, value) in q.conditions {
            match value {
                Some(v) => {
                    values.push(v);
                    tail.push_str(&format!(" and {}", sql.replace('?', &format!("?{}", values.len()))));
                }
                None => tail.push_str(&format!(" and {sql}")),
            }
        }
        values.push(limit.into());
        let order = if tail.starts_with("join") { "s.rank" } else { "m.date desc" };
        tail.push_str(&format!(" order by {order} limit ?{}", values.len()));
        self.summaries(&tail, rusqlite::params_from_iter(values))
    }

    fn summaries(&self, tail: &str, params: impl rusqlite::Params) -> Result<Vec<MessageSummary>> {
        let sql = format!(
            "select m.id, m.folder_id, m.date, m.from_name, m.from_addr, m.subject, m.snippet, m.flags,
                    m.attachments, f.name, f.role, f.account_id, m.snoozed_until
             from messages m join folders f on f.id = m.folder_id
             join accounts a on a.id = f.account_id and a.removed = 0 {tail}"
        );
        let mut stmt = self.db.prepare(&sql)?;
        let rows = stmt.query_map(params, |r| {
            let flags: String = r.get(7)?;
            Ok(MessageSummary {
                id: r.get(0)?,
                snoozed_until: r.get(12)?,
                folder_id: r.get(1)?,
                folder_name: r.get(9)?,
                folder_role: r.get(10)?,
                account_id: r.get(11)?,
                date: r.get(2)?,
                from_name: r.get(3)?,
                from_addr: r.get(4)?,
                subject: r.get(5)?,
                snippet: r.get(6)?,
                unread: !flags.contains('S'),
                starred: flags.contains('F'),
                attachments: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn message_path(&self, id: i64) -> Result<PathBuf> {
        let (root, folder, file): (String, String, String) = self.db.query_row(
            "select a.root, coalesce(f.dir, f.name), m.file from messages m
             join folders f on f.id = m.origin_folder_id join accounts a on a.id = f.account_id
             where m.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if Path::new(&file).is_absolute() {
            return Ok(PathBuf::from(file));
        }
        Ok(Path::new(&root).join(folder).join(file))
    }

    pub fn message(&self, id: i64) -> Result<MessageView> {
        let raw = fs::read(self.message_path(id)?)?;
        let Some(msg) = MessageParser::default().parse(&raw) else {
            bail!("message {id} could not be parsed");
        };

        let mut html = msg.body_html(0).map(|h| h.into_owned());
        let has_real_html = msg.html_body_count() > 0;
        if !has_real_html {
            html = None;
        }
        if let Some(h) = html.as_mut() {
            for part in msg.attachments() {
                let Some(cid) = part.content_id() else { continue };
                let ct = part
                    .content_type()
                    .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")))
                    .unwrap_or_else(|| "application/octet-stream".into());
                let data = base64::engine::general_purpose::STANDARD.encode(part.contents());
                let cid = cid.trim_start_matches('<').trim_end_matches('>');
                *h = h.replace(&format!("cid:{cid}"), &format!("data:{ct};base64,{data}"));
            }
        }

        let attachments = msg
            .attachments()
            .enumerate()
            .filter(|(_, p)| !(is_inline(p) && p.content_id().is_some()))
            .map(|(index, p)| AttachmentInfo {
                index,
                name: p.attachment_name().unwrap_or("attachment").to_string(),
                content_type: p
                    .content_type()
                    .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")))
                    .unwrap_or_else(|| "application/octet-stream".into()),
                size: p.contents().len(),
            })
            .collect();

        let mut notices = Vec::new();
        if header_text(&msg, "X-OxyMail-Missing").is_some() {
            notices.push("Mailbird never downloaded the body of this message.".to_string());
        }
        for name in header_texts(&msg, "X-OxyMail-Missing-Attachment") {
            notices.push(format!("Attachment not available: {name}"));
        }

        let account_id: i64 = self.db.query_row(
            "select f.account_id from messages m join folders f on f.id = m.folder_id where m.id = ?1",
            [id],
            |r| r.get(0),
        )?;
        let references = match msg.references() {
            mail_parser::HeaderValue::Text(t) => vec![t.to_string()],
            mail_parser::HeaderValue::TextList(list) => list.iter().map(|t| t.to_string()).collect(),
            _ => Vec::new(),
        };
        let from_addr = msg.from().and_then(|a| a.first()).and_then(|a| a.address()).map(|a| a.to_lowercase());
        let images_allowed = match &from_addr {
            Some(addr) => self
                .db
                .query_row("select images_allowed from contacts where email = ?1", [addr], |r| r.get(0))
                .optional()?
                .unwrap_or(false),
            None => false,
        };
        let has_remote_content = html.as_deref().is_some_and(|h| {
            let h = h.to_ascii_lowercase();
            ["src=\"http", "src='http", "src=http", "url(http", "url('http", "url(\"http", "srcset=\"http"].iter().any(|p| h.contains(p))
        });
        let unsubscribe = msg
            .header_raw("List-Unsubscribe")
            .map(|v| {
                v.split(',')
                    .map(|p| p.trim().trim_start_matches('<').trim_end_matches('>').trim().to_string())
                    .filter(|u| u.starts_with("https://") || u.starts_with("http://") || u.starts_with("mailto:"))
                    .collect()
            })
            .unwrap_or_default();
        Ok(MessageView {
            id,
            account_id,
            from_addr,
            has_remote_content,
            images_allowed,
            unsubscribe,
            message_id: msg.message_id().map(str::to_string),
            references,
            reply_to: address_list(msg.reply_to()),
            subject: msg.subject().map(str::to_string),
            from: address_list(msg.from()),
            to: address_list(msg.to()),
            cc: address_list(msg.cc()),
            date: msg.date().map(|d| d.to_timestamp()).unwrap_or(0),
            text: if has_real_html { None } else { msg.body_text(0).map(|t| t.into_owned()) },
            html,
            attachments,
            notices,
        })
    }

    /// Returns `(file name, bytes)` for attachment `index` of message `id`.
    pub fn attachment(&self, id: i64, index: usize) -> Result<(String, Vec<u8>)> {
        let raw = fs::read(self.message_path(id)?)?;
        let msg = MessageParser::default().parse(&raw).context("message could not be parsed")?;
        let part = msg.attachments().nth(index).context("no such attachment")?;
        Ok((part.attachment_name().unwrap_or("attachment").to_string(), part.contents().to_vec()))
    }

    /// Turns a Maildir flag on or off for each message. Only `S` (seen) and `F` (flagged/starred)
    /// are user-settable. Archive messages change only in the index; their files are untouched.
    pub fn set_flag(&mut self, ids: &[i64], flag: char, on: bool) -> Result<()> {
        if !matches!(flag, 'S' | 'F') {
            bail!("unsupported flag {flag}");
        }
        let tx = self.db.transaction()?;
        for id in ids {
            let flags: String = tx.query_row("select flags from messages where id = ?1", [id], |r| r.get(0))?;
            let mut chars: Vec<char> = flags.chars().filter(|c| *c != flag).collect();
            if on {
                chars.push(flag);
            }
            chars.sort_unstable();
            tx.execute("update messages set flags = ?1 where id = ?2", params![chars.iter().collect::<String>(), id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Marks every unread message in a folder as read and returns their ids (for undo).
    pub fn mark_folder_read(&mut self, folder_id: i64) -> Result<Vec<i64>> {
        self.mark_read_where("m.folder_id = ?1", folder_id)
    }

    /// Marks every unread message in an account as read, except Spam and Trash.
    pub fn mark_account_read(&mut self, account_id: i64) -> Result<Vec<i64>> {
        self.mark_read_where(
            "f.account_id = ?1 and coalesce(f.role, '') not in ('spam', 'trash')",
            account_id,
        )
    }

    fn mark_read_where(&mut self, condition: &str, arg: i64) -> Result<Vec<i64>> {
        let ids: Vec<i64> = self
            .db
            .prepare(&format!(
                "select m.id from messages m join folders f on f.id = m.folder_id
                 where {condition} and m.deleted = 0 and instr(m.flags, 'S') = 0"
            ))?
            .query_map([arg], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        self.set_flag(&ids, 'S', true)?;
        Ok(ids)
    }

    /// Moves messages to their account's folder with this role (`archive`, `trash`, `inbox`),
    /// creating the folder if the account has none.
    pub fn move_to_role(&mut self, ids: &[i64], role: &str) -> Result<()> {
        let tx = self.db.transaction()?;
        for id in ids {
            let account_id = account_of(&tx, *id)?;
            let target = ensure_folder(&tx, account_id, role)?;
            tx.execute("update messages set folder_id = ?1 where id = ?2", params![target, id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Moves messages into a specific folder and un-hides them; used to undo a move or delete.
    pub fn move_to_folder(&mut self, ids: &[i64], folder_id: i64) -> Result<()> {
        let tx = self.db.transaction()?;
        for id in ids {
            tx.execute("update messages set folder_id = ?1, deleted = 0 where id = ?2", params![folder_id, id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Moves messages to Trash. Messages already in Trash are removed from OxyMail; for
    /// archive accounts that only hides them, and the message file stays on disk.
    pub fn delete(&mut self, ids: &[i64]) -> Result<()> {
        let tx = self.db.transaction()?;
        for id in ids {
            let role: Option<String> = tx.query_row(
                "select f.role from messages m join folders f on f.id = m.folder_id where m.id = ?1",
                [id],
                |r| r.get(0),
            )?;
            if role.as_deref() == Some("trash") {
                tx.execute("update messages set deleted = 1 where id = ?1", [id])?;
            } else {
                let target = ensure_folder(&tx, account_of(&tx, *id)?, "trash")?;
                tx.execute("update messages set folder_id = ?1 where id = ?2", params![target, id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Puts messages back in the folder they were imported from, or the Inbox if that was Trash.
    pub fn restore(&mut self, ids: &[i64]) -> Result<()> {
        let tx = self.db.transaction()?;
        for id in ids {
            let (origin, role): (i64, Option<String>) = tx.query_row(
                "select m.origin_folder_id, f.role from messages m join folders f on f.id = m.origin_folder_id
                 where m.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let target = if role.as_deref() == Some("trash") {
                ensure_folder(&tx, account_of(&tx, *id)?, "inbox")?
            } else {
                origin
            };
            tx.execute("update messages set folder_id = ?1, deleted = 0 where id = ?2", params![target, id])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Address suggestions for the compose fields, address-book entries and frequent
    /// correspondents first.
    pub fn suggest_contacts(&self, query: &str, limit: i64) -> Result<Vec<ContactSuggestion>> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.db.prepare(
            "select email, name from contacts
             where blocked = 0 and (email like ?1 || '%' or lower(name) like ?1 || '%'
                    or lower(name) like '% ' || ?1 || '%' or email like '%.' || ?1 || '%')
             order by in_address_book desc, uses desc, last_used desc limit ?2",
        )?;
        let rows = stmt.query_map(params![q, limit], |r| Ok(ContactSuggestion { email: r.get(0)?, name: r.get(1)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The other messages in a message's conversation (same account), oldest first.
    pub fn thread(&self, id: i64) -> Result<Vec<MessageSummary>> {
        let (key, account): (Option<String>, i64) = self.db.query_row(
            "select m.thread_key, f.account_id from messages m join folders f on f.id = m.folder_id where m.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let Some(key) = key else { return Ok(Vec::new()) };
        self.summaries(
            "where m.thread_key = ?1 and f.account_id = ?2 and m.deleted = 0 order by m.date asc limit 100",
            params![key, account],
        )
    }

    /// Fills in conversation keys for messages indexed before threads existed. Returns how
    /// many were updated; `progress` gets a running count.
    pub fn backfill_threads(&mut self, mut progress: impl FnMut(usize)) -> Result<usize> {
        let ids: Vec<i64> = self.db.prepare("select id from messages where thread_key is null")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let parser = MessageParser::default();
        let mut done = 0;
        for chunk in ids.chunks(2000) {
            let tx = self.db.transaction()?;
            for id in chunk {
                let path = {
                    let (root, folder, file): (String, String, String) = tx.query_row(
                        "select a.root, coalesce(f.dir, f.name), m.file from messages m
                         join folders f on f.id = m.origin_folder_id join accounts a on a.id = f.account_id where m.id = ?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )?;
                    if Path::new(&file).is_absolute() { PathBuf::from(file) } else { Path::new(&root).join(folder).join(file) }
                };
                let Ok(raw) = fs::read(&path) else { continue };
                if let Some(msg) = parser.parse_headers(&raw) {
                    tx.execute("update messages set thread_key = ?1 where id = ?2", params![thread_key(&msg), id])?;
                }
                done += 1;
            }
            tx.commit()?;
            progress(done);
        }
        Ok(done)
    }

    /// Remembers whether remote images may load for mail from this sender.
    pub fn set_images_allowed(&self, email: &str, allowed: bool) -> Result<()> {
        note_contact(&self.db, email, None, 0, 0)?;
        self.db.execute("update contacts set images_allowed = ?2 where email = lower(?1)", params![email, allowed])?;
        Ok(())
    }

    /// Counts the people you write to, so they rank higher in suggestions.
    pub fn note_sent_to(&self, addresses: &[String]) -> Result<()> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        for a in addresses {
            let (name, email) = match (a.rfind('<'), a.rfind('>')) {
                (Some(l), Some(r)) if l < r => (Some(a[..l].trim().trim_matches('"')), &a[l + 1..r]),
                _ => (None, a.as_str()),
            };
            note_contact(&self.db, email.trim(), name, 5, now)?;
        }
        Ok(())
    }

    /// Messages across every account's folders with this role (e.g. all Inboxes), newest first.
    pub fn messages_by_role(&self, role: &str, offset: i64, limit: i64) -> Result<Vec<MessageSummary>> {
        self.summaries(
            "where f.role = ?1 and m.deleted = 0 and m.snoozed_until is null order by m.date desc limit ?2 offset ?3",
            params![role, limit, offset],
        )
    }

    /// Unread count across every account's folders with this role.
    pub fn unread_by_role(&self, role: &str) -> Result<i64> {
        Ok(self.db.query_row(
            "select count(*) from messages m join folders f on f.id = m.folder_id
             join accounts a on a.id = f.account_id and a.removed = 0
             where f.role = ?1 and m.deleted = 0 and m.snoozed_until is null and instr(m.flags, 'S') = 0",
            [role],
            |r| r.get(0),
        )?)
    }

    /// Hides messages until `until` (Unix time); `None` brings them back now.
    pub fn snooze(&self, ids: &[i64], until: Option<i64>) -> Result<()> {
        for id in ids {
            self.db.execute("update messages set snoozed_until = ?1 where id = ?2", params![until, id])?;
        }
        Ok(())
    }

    /// Every snoozed message, soonest first.
    pub fn snoozed(&self) -> Result<Vec<MessageSummary>> {
        self.summaries("where m.snoozed_until is not null and m.deleted = 0 order by m.snoozed_until asc limit 500", [])
    }

    /// Brings back messages whose snooze has ended, marked unread. Returns them.
    pub fn wake_snoozed(&mut self, now: i64) -> Result<Vec<MessageSummary>> {
        let due = self.summaries(
            "where m.snoozed_until is not null and m.snoozed_until <= ?1 and m.deleted = 0 order by m.date desc",
            [now],
        )?;
        let ids: Vec<i64> = due.iter().map(|m| m.id).collect();
        self.set_flag(&ids, 'S', false)?;
        for id in &ids {
            self.db.execute("update messages set snoozed_until = null where id = ?1", [id])?;
        }
        Ok(due)
    }

    /// Queues a message to send at `send_at`. `mail` is the outgoing message and `draft` the
    /// Compose state (both JSON), so it can be reopened for editing.
    pub fn schedule(&self, account_id: i64, send_at: i64, mail: &str, draft: &str, summary: &str) -> Result<i64> {
        self.db.execute(
            "insert into scheduled (account_id, send_at, mail, draft, summary) values (?1, ?2, ?3, ?4, ?5)",
            params![account_id, send_at, mail, draft, summary],
        )?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn scheduled(&self) -> Result<Vec<Scheduled>> {
        let mut stmt = self.db.prepare("select id, account_id, send_at, mail, draft, summary, error from scheduled order by send_at")?;
        let rows = stmt.query_map([], |r| {
            Ok(Scheduled {
                id: r.get(0)?,
                account_id: r.get(1)?,
                send_at: r.get(2)?,
                mail: r.get(3)?,
                draft: r.get(4)?,
                summary: r.get(5)?,
                error: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn unschedule(&self, id: i64) -> Result<()> {
        self.db.execute("delete from scheduled where id = ?1", [id])?;
        Ok(())
    }

    pub fn set_schedule_error(&self, id: i64, error: Option<&str>) -> Result<()> {
        self.db.execute("update scheduled set error = ?1 where id = ?2", params![error, id])?;
        Ok(())
    }

    /// Saves an unsent message (the Compose state as JSON). Returns its id.
    pub fn save_draft(&self, id: Option<i64>, account_id: Option<i64>, subject: &str, recipients: &str, body: &str) -> Result<i64> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        match id {
            Some(id) => {
                self.db.execute(
                    "update drafts set account_id = ?1, subject = ?2, recipients = ?3, body = ?4, updated = ?5 where id = ?6",
                    params![account_id, subject, recipients, body, now, id],
                )?;
                Ok(id)
            }
            None => {
                self.db.execute(
                    "insert into drafts (account_id, subject, recipients, body, updated) values (?1, ?2, ?3, ?4, ?5)",
                    params![account_id, subject, recipients, body, now],
                )?;
                Ok(self.db.last_insert_rowid())
            }
        }
    }

    pub fn drafts(&self) -> Result<Vec<Draft>> {
        let mut stmt = self.db.prepare("select id, account_id, subject, recipients, body, updated from drafts order by updated desc")?;
        let rows = stmt.query_map([], |r| {
            Ok(Draft { id: r.get(0)?, account_id: r.get(1)?, subject: r.get(2)?, recipients: r.get(3)?, body: r.get(4)?, updated: r.get(5)? })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn delete_draft(&self, id: i64) -> Result<()> {
        self.db.execute("delete from drafts where id = ?1", [id])?;
        Ok(())
    }

    pub fn settings(&self) -> Result<std::collections::BTreeMap<String, String>> {
        let mut stmt = self.db.prepare("select key, value from settings")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.db.execute(
            "insert into settings (key, value) values (?1, ?2)
             on conflict (key) do update set value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}


/// Parses `raw` and adds it to the index as `folder_id/rel`. Returns `None` if it isn't a
/// readable message.
/// Which conversation a message belongs to: Gmail's thread id when known, else the first
/// message of its References chain, else what it replies to, else itself.
fn thread_key(msg: &mail_parser::Message) -> Option<String> {
    if let Some(thread) = msg.header_raw("X-GM-THRID") {
        return Some(format!("gm:{}", thread.trim()));
    }
    let first_reference = match msg.references() {
        mail_parser::HeaderValue::Text(t) => Some(t.to_string()),
        mail_parser::HeaderValue::TextList(list) => list.first().map(|t| t.to_string()),
        _ => None,
    };
    let parent = match msg.in_reply_to() {
        mail_parser::HeaderValue::Text(t) => Some(t.to_string()),
        mail_parser::HeaderValue::TextList(list) => list.first().map(|t| t.to_string()),
        _ => None,
    };
    first_reference.or(parent).or_else(|| msg.message_id().map(str::to_string))
}

pub(crate) fn index_message(
    db: &Connection,
    parser: &MessageParser,
    folder_id: i64,
    rel: &str,
    raw: &[u8],
    uid: Option<u32>,
) -> Result<Option<i64>> {
    let Some(msg) = parser.parse(raw) else { return Ok(None) };
    let from = msg.from().and_then(|a| a.first());
    let to = join_addresses(msg.to());
    let body = msg.body_text(0).map(|b| b.into_owned()).unwrap_or_default();
    let subject = msg.subject().map(str::to_string);
    let from_name = from.and_then(|a| a.name()).map(str::to_string);
    let from_addr = from.and_then(|a| a.address()).map(str::to_string);
    db.execute(
        "insert into messages (folder_id, origin_folder_id, file, date, from_name, from_addr, to_addrs,
             subject, snippet, flags, attachments, message_id, uid, thread_key)
         values (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            folder_id,
            rel,
            msg.date().map(|d| d.to_timestamp()).unwrap_or(0),
            from_name,
            from_addr,
            to,
            subject,
            snippet(&body),
            maildir_flags(rel),
            msg.attachment_count() as i64,
            msg.message_id(),
            uid,
            thread_key(&msg),
        ],
    )?;
    let id = db.last_insert_rowid();
    if let Some(addr) = from.and_then(|a| a.address()) {
        note_contact(db, addr, from.and_then(|a| a.name()), 1, msg.date().map(|d| d.to_timestamp()).unwrap_or(0))?;
    }
    db.execute(
        "insert into messages_fts (rowid, subject, sender, recipients, body) values (?1, ?2, ?3, ?4, ?5)",
        params![
            id,
            subject,
            format!("{} {}", from_name.unwrap_or_default(), from_addr.unwrap_or_default()),
            to,
            truncate(&body, INDEXED_BODY_CHARS),
        ],
    )?;
    Ok(Some(id))
}

/// Records that an address was seen (`weight` 1 for mail received, more for mail you send).
pub(crate) fn note_contact(db: &Connection, email: &str, name: Option<&str>, weight: i64, when: i64) -> Result<()> {
    if !email.contains('@') {
        return Ok(());
    }
    db.execute(
        "insert into contacts (email, name, uses, last_used) values (lower(?1), ?2, ?3, ?4)
         on conflict (email) do update set
             uses = uses + excluded.uses,
             last_used = max(last_used, excluded.last_used),
             name = coalesce(contacts.name, excluded.name)",
        params![email, name.filter(|n| !n.trim().is_empty() && *n != email), weight, when],
    )?;
    Ok(())
}

fn migrate(db: &Connection) -> Result<()> {
    let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let has_messages: bool = db.query_row(
        "select count(*) > 0 from sqlite_master where type = 'table' and name = 'messages'",
        [],
        |r| r.get(0),
    )?;
    if has_messages && version < 2 {
        let tx = db.unchecked_transaction()?;
        tx.execute_batch(MIGRATE_V1_TO_V2)?;
        tx.commit()?;
    }
    db.execute_batch(SCHEMA)?;
    let existing: Vec<String> = db
        .prepare("select name from pragma_table_info('accounts')")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for (column, decl) in ACCOUNT_COLUMNS_V3 {
        if !existing.iter().any(|c| c == column) {
            db.execute_batch(&format!("alter table accounts add column {column} {decl}"))?;
        }
    }
    add_missing_columns(db, "folders", FOLDER_COLUMNS_V4)?;
    add_missing_columns(
        db,
        "messages",
        &[
            ("uid", "integer"),
            ("from_archive", "integer not null default 0"),
            ("thread_key", "text"),
            ("snoozed_until", "integer"),
        ],
    )?;
    db.execute_batch("create index if not exists messages_thread on messages(thread_key)")?;
    add_missing_columns(db, "accounts", &[("archive_root", "text")])?;
    if db.query_row("select count(*) from sqlite_master where name = 'contacts'", [], |r| r.get::<_, i64>(0))? > 0 {
        add_missing_columns(db, "contacts", &[("images_allowed", "integer not null default 0")])?;
    }
    if version < 5 {
        // v5: an archive account can be connected to its server and become live, so its
        // archive location moves to `archive_root` and archive message paths become absolute
        // (the account's `root` then points at the live cache).
        db.execute_batch(
            "update accounts set archive_root = root where kind = 'archive' and archive_root is null;
             update messages set from_archive = 1,
                 file = (select a.root || '/' || f.name || '/' || messages.file
                         from folders f join accounts a on a.id = f.account_id
                         where f.id = messages.origin_folder_id)
             where file not like '/%'
               and origin_folder_id in (select f.id from folders f join accounts a on a.id = f.account_id
                                        where a.kind = 'archive');",
        )?;
    }
    db.execute_batch("create index if not exists messages_folder_uid on messages(origin_folder_id, uid)")?;
    if version < 6 {
        // v6: the address book for autocomplete starts from everyone who has written to you.
        db.execute_batch(
            "insert into contacts (email, name, uses, last_used)
             select lower(from_addr), max(from_name), count(*), max(date) from messages
             where from_addr like '%@%' group by lower(from_addr)
             on conflict (email) do nothing;",
        )?;
    }
    db.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

fn account_of(db: &Connection, message_id: i64) -> Result<i64> {
    Ok(db.query_row(
        "select f.account_id from messages m join folders f on f.id = m.folder_id where m.id = ?1",
        [message_id],
        |r| r.get(0),
    )?)
}

/// Finds the account's folder with `role`, creating it (named after the role) if missing.
fn ensure_folder(db: &Connection, account_id: i64, role: &str) -> Result<i64> {
    let existing: Option<i64> = db
        .query_row(
            "select id from folders where account_id = ?1 and role = ?2 order by id limit 1",
            params![account_id, role],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let name = match role {
        "inbox" => "Inbox",
        "archive" => "Archive",
        "trash" => "Trash",
        "spam" => "Spam",
        "sent" => "Sent",
        "drafts" => "Drafts",
        _ => bail!("unknown folder role {role}"),
    };
    db.execute(
        "insert into folders (account_id, name, role) values (?1, ?2, ?3)
         on conflict (account_id, name) do update set role = excluded.role",
        params![account_id, name, role],
    )?;
    Ok(db.query_row(
        "select id from folders where account_id = ?1 and name = ?2",
        params![account_id, name],
        |r| r.get(0),
    )?)
}


/// Copies account order, colour, sender name and signature from a Mailbird export's raw dump
/// (`<root>/../raw/Accounts.jsonl` and `SenderIdentities.jsonl`), only into fields still empty.
fn apply_mailbird_metadata(db: &Connection, root: &Path) -> Result<()> {
    let Some(raw) = root.parent().map(|p| p.join("raw")) else { return Ok(()) };
    let read = |name: &str| -> Vec<serde_json::Value> {
        fs::read_to_string(raw.join(name))
            .map(|text| text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
            .unwrap_or_default()
    };
    let accounts = read("Accounts.jsonl");
    if accounts.is_empty() {
        return Ok(());
    }
    let identities = read("SenderIdentities.jsonl");
    import_mailbird_contacts(db, &read("Contacts.jsonl"), &read("UserContacts.jsonl"), &read("UserContactFields.jsonl"))?;
    let text = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).filter(|s| !s.is_empty()).map(str::to_string);
    for account in &accounts {
        let Some(username) = text(account, "Username") else { continue };
        let mailbird_id = account.get("Id").and_then(|x| x.as_i64());
        let identity = identities.iter().find(|i| i.get("AccountId").and_then(|x| x.as_i64()) == mailbird_id);
        let root_str = root.join(&username).to_string_lossy().to_string();
        db.execute(
            "update accounts set
                color = coalesce(color, ?1),
                sender_name = coalesce(sender_name, ?2),
                signature = coalesce(signature, ?3)
             where archive_root = ?4",
            params![
                text(account, "ColorHex"),
                identity.and_then(|i| text(i, "Name")).filter(|n| n != &username),
                identity.and_then(|i| text(i, "Signature")),
                root_str,
            ],
        )?;
        // Mailbird's order only applies until the user reorders accounts themselves.
        let reordered: bool = db
            .query_row("select count(*) > 0 from settings where key = 'accounts_reordered'", [], |r| r.get(0))?;
        if !reordered {
            if let Some(index) = account.get("SortIndex").and_then(|x| x.as_i64()) {
                db.execute(
                    "update accounts set sort_index = ?1 where archive_root = ?2",
                    params![index, root_str],
                )?;
            }
        }
    }
    Ok(())
}

/// Makes a name safe as a single path component.
pub(crate) fn sanitize_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() { "_".to_string() } else { cleaned }
}

/// Mailbird's collected addresses (with their last-contact date) and its address book.
fn import_mailbird_contacts(
    db: &Connection,
    collected: &[serde_json::Value],
    people: &[serde_json::Value],
    fields: &[serde_json::Value],
) -> Result<()> {
    let text = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(unescape_html);
    for c in collected {
        if c.get("IsValidSingleContact").and_then(|x| x.as_i64()) == Some(0) {
            continue;
        }
        let Some(email) = text(c, "Email") else { continue };
        let when = text(c, "LatestMessageAt_UTC").as_deref().and_then(parse_utc).unwrap_or(0);
        note_contact(db, &email, text(c, "Name").as_deref(), 0, when)?;
        if c.get("IsWhitelistedForImages").and_then(|x| x.as_i64()) == Some(1) {
            db.execute("update contacts set images_allowed = 1 where email = lower(?1)", [&email])?;
        }
        if c.get("IsBlocked").and_then(|x| x.as_i64()) == Some(1) {
            db.execute("update contacts set blocked = 1 where email = lower(?1)", [&email])?;
        }
    }
    for f in fields.iter().filter(|f| f.get("Category").and_then(|x| x.as_i64()) == Some(1)) {
        let (Some(email), Some(person)) = (text(f, "Value"), f.get("UserContactId").and_then(|x| x.as_i64())) else { continue };
        let name = people.iter().find(|p| p.get("Id").and_then(|x| x.as_i64()) == Some(person)).map(|p| {
            [text(p, "FirstName"), text(p, "LastName")].into_iter().flatten().collect::<Vec<_>>().join(" ")
        });
        note_contact(db, &email, name.as_deref(), 0, 0)?;
        db.execute(
            "update contacts set in_address_book = 1, name = coalesce(nullif(?2, ''), name) where email = lower(?1)",
            params![email, name],
        )?;
    }
    Ok(())
}

fn unescape_html(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}

/// `2025-05-05 11:17:50Z` (Mailbird's UTC format) to a Unix timestamp.
fn parse_utc(s: &str) -> Option<i64> {
    let s = s.trim_end_matches('Z');
    let (date, time) = s.split_once(' ')?;
    let d: Vec<i64> = date.split('-').filter_map(|x| x.parse().ok()).collect();
    let t: Vec<i64> = time.split(':').filter_map(|x| x.split('.').next()?.parse().ok()).collect();
    if d.len() != 3 || t.len() != 3 {
        return None;
    }
    // Days from civil date (Howard Hinnant's algorithm).
    let (y, m, day) = (if d[1] <= 2 { d[0] - 1 } else { d[0] }, d[1], d[2]);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + t[0] * 3600 + t[1] * 60 + t[2])
}

fn sorted_dirs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    Ok(dirs)
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

fn folder_role(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "inbox" => Some("inbox"),
        "sent" | "sent items" | "sent mail" => Some("sent"),
        "drafts" => Some("drafts"),
        "spam" | "junk" | "junk email" => Some("spam"),
        "trash" | "deleted items" | "bin" => Some("trash"),
        "archive" | "archived" => Some("archive"),
        _ => None,
    }
}

/// Flags from a Maildir name such as `123.M45.oxymail:2,FS`.
fn maildir_flags(file: &str) -> String {
    file.rsplit_once(":2,").map(|(_, f)| f.to_string()).unwrap_or_default()
}

fn join_addresses(addr: Option<&mail_parser::Address>) -> String {
    address_list(addr).join(", ")
}

fn address_list(addr: Option<&mail_parser::Address>) -> Vec<String> {
    let Some(addr) = addr else { return Vec::new() };
    addr.iter()
        .map(|a| match (a.name(), a.address()) {
            (Some(n), Some(e)) if n != e => format!("{n} <{e}>"),
            (_, Some(e)) => e.to_string(),
            (Some(n), None) => n.to_string(),
            (None, None) => String::new(),
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_inline(part: &mail_parser::MessagePart) -> bool {
    part.content_disposition().is_some_and(|d| d.ctype().eq_ignore_ascii_case("inline"))
}

fn header_text(msg: &mail_parser::Message, name: &str) -> Option<String> {
    header_texts(msg, name).into_iter().next()
}

fn header_texts(msg: &mail_parser::Message, name: &str) -> Vec<String> {
    msg.header_values(name)
        .filter_map(|v| v.as_text().map(str::to_string))
        .collect()
}

fn snippet(body: &str) -> String {
    let collapsed: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&collapsed, SNIPPET_CHARS)
}

fn truncate(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => s[..i].to_string(),
        None => s.to_string(),
    }
}

struct SearchQuery {
    fts: String,
    /// SQL conditions; a `?` is replaced with the numbered parameter for its value.
    conditions: Vec<(String, Option<rusqlite::types::Value>)>,
}

/// Splits a search box query into full-text terms and filters.
fn parse_search(input: &str) -> SearchQuery {
    let mut fts: Vec<String> = Vec::new();
    let mut conditions = Vec::new();
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut token = String::new();
        let mut quoted = false;
        while let Some(&c) = chars.peek() {
            if c == '"' {
                quoted = !quoted;
                chars.next();
            } else if c.is_whitespace() && !quoted {
                break;
            } else {
                token.push(c);
                chars.next();
            }
        }
        tokens.push(token);
    }
    let phrase = |text: &str| format!("\"{}\"*", text.replace('"', ""));
    for token in tokens.into_iter().filter(|t| !t.is_empty()) {
        let (key, value) = token.split_once(':').map(|(k, v)| (k.to_lowercase(), v.to_string())).unwrap_or_default();
        let value_lower = value.to_lowercase();
        match key.as_str() {
            "from" if !value.is_empty() => fts.push(format!("sender : {}", phrase(&value))),
            "to" if !value.is_empty() => fts.push(format!("recipients : {}", phrase(&value))),
            "subject" if !value.is_empty() => fts.push(format!("subject : {}", phrase(&value))),
            "has" if value_lower.starts_with("attach") => conditions.push(("m.attachments > 0".into(), None)),
            "is" if value_lower == "unread" => conditions.push(("instr(m.flags, 'S') = 0".into(), None)),
            "is" if value_lower == "read" => conditions.push(("instr(m.flags, 'S') > 0".into(), None)),
            "is" if value_lower == "starred" || value_lower == "flagged" => conditions.push(("instr(m.flags, 'F') > 0".into(), None)),
            "in" if !value.is_empty() => conditions.push(("(f.role = ? or lower(f.name) = ?)".into(), Some(value_lower.clone().into()))),
            "account" if !value.is_empty() => conditions.push(("a.name like '%' || ? || '%'".into(), Some(value_lower.into()))),
            "before" | "after" if parse_utc(&format!("{value} 00:00:00Z")).is_some() => {
                let ts = parse_utc(&format!("{value} 00:00:00Z")).unwrap_or(0);
                let op = if key == "before" { "<" } else { ">=" };
                conditions.push((format!("m.date {op} ?"), Some(ts.into())));
            }
            _ => fts.push(phrase(&token)),
        }
    }
    SearchQuery { fts: fts.join(" "), conditions }
}

/// Turns free text into an FTS5 query: each word quoted and prefix-matched, all required.
#[cfg(test)]
fn fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_come_from_the_maildir_suffix() {
        assert_eq!(maildir_flags("cur/1.M2.oxymail:2,FS"), "FS");
        assert_eq!(maildir_flags("new/1.M2.oxymail"), "");
    }

    #[test]
    fn search_words_are_quoted_prefixes() {
        assert_eq!(fts_query(r#"invoice "acme"#), r#""invoice"* "acme"*"#);
        assert_eq!(fts_query("   "), "");
    }

    #[test]
    fn search_filters_split_into_fts_and_conditions() {
        let q = parse_search(r#"from:monica has:attachment "tax invoice" is:unread before:2025-01-01"#);
        assert_eq!(q.fts, r#"sender : "monica"* "tax invoice"*"#);
        assert_eq!(q.conditions.len(), 3);
        assert!(parse_search("is:starred").fts.is_empty());
    }

    #[test]
    fn parses_mailbird_utc_dates() {
        assert_eq!(parse_utc("1970-01-01 00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2025-05-05 11:17:50Z"), Some(1746443870));
        assert_eq!(parse_utc("2020-11-09 09:16:16.8964887Z"), Some(1604913376));
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("héllo", 2), "hé");
    }

    #[test]
    fn actions_change_the_index_only() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("oxymail-test-{}", std::process::id()));
        let inbox = dir.join("mail/me@example.com/Inbox/cur");
        fs::create_dir_all(&inbox)?;
        let file = inbox.join("1.M1.oxymail:2,S");
        fs::write(&file, "From: A <a@example.com>\r\nSubject: Hi\r\nDate: Tue, 1 Jul 2025 10:00:00 +0000\r\n\r\nHello\r\n")?;
        let mut store = Store::open(&dir.join("test.db"))?;
        store.import_archive(&dir.join("mail"), |_| {})?;
        let id = store.search("hello", 10)?[0].id;

        store.set_flag(&[id], 'F', true)?;
        store.set_flag(&[id], 'S', false)?;
        let m = &store.search("hello", 10)?[0];
        assert!(m.starred && m.unread);

        store.move_to_role(&[id], "archive")?;
        assert_eq!(store.search("hello", 10)?[0].folder_role.as_deref(), Some("archive"));

        store.delete(&[id])?;
        assert_eq!(store.search("hello", 10)?[0].folder_role.as_deref(), Some("trash"));
        store.delete(&[id])?;
        assert!(store.search("hello", 10)?.is_empty());

        store.restore(&[id])?;
        assert_eq!(store.search("hello", 10)?[0].folder_name, "Inbox");
        assert!(file.exists(), "archive file must never be touched");

        // Re-importing must not duplicate a message that has moved.
        store.move_to_role(&[id], "archive")?;
        assert_eq!(store.import_archive(&dir.join("mail"), |_| {})?.added, 0);

        let account = store.accounts()?[0].id;
        store.update_account(account, &AccountUpdate {
            label: Some("Me".into()), sender_name: None, color: Some("#0f5e73".into()), signature: Some("<b>Me</b>".into()),
            icon: None,
        })?;
        store.set_flag(&[id], 'S', false)?;
        assert_eq!(store.accounts()?[0].unread, 1);
        assert_eq!(store.mark_account_read(account)?, vec![id]);
        assert_eq!(store.accounts()?[0].unread, 0);
        assert_eq!(store.accounts()?[0].label.as_deref(), Some("Me"));
        store.remove_account(account)?;
        assert!(store.accounts()?.is_empty() && store.search("hello", 10)?.is_empty());
        store.import_archive(&dir.join("mail"), |_| {})?;
        assert_eq!(store.accounts()?[0].signature.as_deref(), Some("<b>Me</b>"), "re-import revives the account");

        store.snooze(&[id], Some(100))?;
        assert!(store.messages(store.search("hello", 1)?[0].folder_id, 0, 10)?.is_empty(), "snoozed is hidden");
        assert_eq!(store.snoozed()?.len(), 1);
        assert_eq!(store.wake_snoozed(50)?.len(), 0);
        let woke = store.wake_snoozed(200)?;
        assert_eq!(woke.len(), 1);
        assert!(store.search("hello", 1)?[0].unread && store.snoozed()?.is_empty());

        store.set_setting("theme", "dark")?;
        assert_eq!(store.settings()?.get("theme").map(String::as_str), Some("dark"));
        fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
