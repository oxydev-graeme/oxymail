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

const SCHEMA: &str = "
create table if not exists accounts (
    id integer primary key,
    name text not null,
    kind text not null,
    root text not null,
    unique (kind, root)
);
create table if not exists folders (
    id integer primary key,
    account_id integer not null references accounts(id),
    name text not null,
    role text,
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
    unique (origin_folder_id, file)
);
create index if not exists messages_folder_date on messages(folder_id, deleted, date desc);
create virtual table if not exists messages_fts using fts5(
    subject, sender, recipients, body, content=''
);
create table if not exists settings (
    key text primary key,
    value text not null
);
";

/// `PRAGMA user_version` for the current schema.
const SCHEMA_VERSION: i64 = 2;

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
    pub name: String,
    pub kind: String,
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
            tx.execute(
                "insert or ignore into accounts (name, kind, root) values (?1, 'archive', ?2)",
                params![account_name, root_str],
            )?;
            let account_id: i64 = tx.query_row(
                "select id from accounts where kind = 'archive' and root = ?1",
                [&root_str],
                |r| r.get(0),
            )?;
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
                        let rel = format!("{sub}/{}", entry.file_name().to_string_lossy());
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
                        let Some(msg) = parser.parse(&raw) else {
                            stats.unreadable += 1;
                            continue;
                        };
                        let from = msg.from().and_then(|a| a.first());
                        let to = join_addresses(msg.to());
                        let body = msg.body_text(0).map(|b| b.into_owned()).unwrap_or_default();
                        let subject = msg.subject().map(str::to_string);
                        let from_name = from.and_then(|a| a.name()).map(str::to_string);
                        let from_addr = from.and_then(|a| a.address()).map(str::to_string);
                        tx.execute(
                            "insert into messages (folder_id, origin_folder_id, file, date, from_name,
                                 from_addr, to_addrs, subject, snippet, flags, attachments, message_id)
                             values (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                            params![
                                folder_id,
                                rel,
                                msg.date().map(|d| d.to_timestamp()).unwrap_or(0),
                                from_name,
                                from_addr,
                                to,
                                subject,
                                snippet(&body),
                                maildir_flags(&rel),
                                msg.attachment_count() as i64,
                                msg.message_id(),
                            ],
                        )?;
                        let id = tx.last_insert_rowid();
                        tx.execute(
                            "insert into messages_fts (rowid, subject, sender, recipients, body)
                             values (?1, ?2, ?3, ?4, ?5)",
                            params![
                                id,
                                subject,
                                format!("{} {}", from_name.unwrap_or_default(), from_addr.unwrap_or_default()),
                                to,
                                truncate(&body, INDEXED_BODY_CHARS),
                            ],
                        )?;
                        stats.added += 1;
                        if stats.added % 2000 == 0 {
                            progress(stats.added);
                        }
                    }
                }
            }
        }
        tx.commit()?;
        Ok(stats)
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let mut stmt = self.db.prepare("select id, name, kind from accounts order by name")?;
        let rows = stmt.query_map([], |r| Ok(Account { id: r.get(0)?, name: r.get(1)?, kind: r.get(2)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn folders(&self, account_id: i64) -> Result<Vec<Folder>> {
        let mut stmt = self.db.prepare(
            "select f.id, f.account_id, f.name, f.role,
                    (select count(*) from messages m where m.folder_id = f.id and m.deleted = 0),
                    (select count(*) from messages m
                     where m.folder_id = f.id and m.deleted = 0 and instr(m.flags, 'S') = 0)
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
            "where m.folder_id = ?1 and m.deleted = 0 order by m.date desc limit ?2 offset ?3",
            params![folder_id, limit, offset],
        )
    }

    /// Full-text search across every account. Plain words are matched as prefixes.
    pub fn search(&self, query: &str, limit: i64) -> Result<Vec<MessageSummary>> {
        let fts = fts_query(query);
        if fts.is_empty() {
            return Ok(Vec::new());
        }
        self.summaries(
            "join (select rowid as hit, rank from messages_fts where messages_fts match ?1) s on s.hit = m.id
             where m.deleted = 0 order by s.rank limit ?2",
            params![fts, limit],
        )
    }

    fn summaries(&self, tail: &str, params: impl rusqlite::Params) -> Result<Vec<MessageSummary>> {
        let sql = format!(
            "select m.id, m.folder_id, m.date, m.from_name, m.from_addr, m.subject, m.snippet, m.flags,
                    m.attachments, f.name, f.role, f.account_id
             from messages m join folders f on f.id = m.folder_id {tail}"
        );
        let mut stmt = self.db.prepare(&sql)?;
        let rows = stmt.query_map(params, |r| {
            let flags: String = r.get(7)?;
            Ok(MessageSummary {
                id: r.get(0)?,
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
            "select a.root, f.name, m.file from messages m
             join folders f on f.id = m.origin_folder_id join accounts a on a.id = f.account_id
             where m.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
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

        Ok(MessageView {
            id,
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

/// Turns free text into an FTS5 query: each word quoted and prefix-matched, all required.
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

        store.set_setting("theme", "dark")?;
        assert_eq!(store.settings()?.get("theme").map(String::as_str), Some("dark"));
        fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
