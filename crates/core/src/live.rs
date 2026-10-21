//! Storage for live (IMAP) accounts. Downloaded messages are cached as Maildir files under the
//! account's root, one directory per server folder, and indexed like archive mail.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use mail_parser::MessageParser;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::{Store, index_message, sanitize_component};

#[derive(Debug, Clone, Serialize)]
pub struct LiveAccount {
    pub id: i64,
    pub email: String,
    /// JSON connection settings, owned by the sync crate.
    pub config: String,
    pub root: PathBuf,
}

/// What Mailbird had configured for an account (passwords were never exported).
#[derive(Debug, Clone, Serialize)]
pub struct MailbirdServer {
    /// `imap`, `pop3` or `exchange`.
    pub kind: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub username: String,
    pub oauth: bool,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub smtp_tls: Option<bool>,
    pub smtp_username: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemoteRef {
    pub message_id: i64,
    pub account_id: i64,
    pub folder_id: i64,
    pub remote_name: String,
    pub uid: u32,
    pub role: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemoteFolder {
    pub id: i64,
    pub remote_name: String,
    pub uidvalidity: Option<u32>,
    pub last_uid: u32,
}

impl Store {
    /// Adds a live account whose cache lives in `<cache_root>/<email>`. Returns its id.
    pub fn add_live_account(&self, email: &str, config: &str, cache_root: &Path) -> Result<i64> {
        let root = cache_root.join(sanitize_component(email));
        fs::create_dir_all(&root)?;
        self.db.execute(
            "insert into accounts (name, kind, root, config, sort_index)
             values (?1, 'imap', ?2, ?3, (select coalesce(max(sort_index), -1) + 1 from accounts))
             on conflict (kind, root) do update set config = excluded.config, removed = 0",
            params![email, root.to_string_lossy(), config],
        )?;
        Ok(self.db.query_row(
            "select id from accounts where kind = 'imap' and root = ?1",
            [root.to_string_lossy()],
            |r| r.get(0),
        )?)
    }

    pub fn live_accounts(&self) -> Result<Vec<LiveAccount>> {
        let mut stmt = self.db.prepare(
            "select id, name, config, root from accounts where kind = 'imap' and removed = 0 order by sort_index",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(LiveAccount {
                id: r.get(0)?,
                email: r.get(1)?,
                config: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                root: PathBuf::from(r.get::<_, String>(3)?),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Replaces a live account's connection settings (JSON owned by the sync crate).
    pub fn set_live_config(&self, id: i64, config: &str) -> Result<()> {
        self.db.execute("update accounts set config = ?1 where id = ?2 and kind = 'imap'", params![config, id])?;
        Ok(())
    }

    /// Turns an imported archive account into a live one, keeping all its archive mail.
    pub fn connect_archive(&self, account_id: i64, config: &str, cache_root: &Path) -> Result<()> {
        let email: String = self.db.query_row("select name from accounts where id = ?1", [account_id], |r| r.get(0))?;
        let root = cache_root.join(sanitize_component(&email));
        let taken: bool = self.db.query_row(
            "select count(*) > 0 from accounts where kind = 'imap' and root = ?1 and id != ?2 and removed = 0",
            params![root.to_string_lossy(), account_id],
            |r| r.get(0),
        )?;
        if taken {
            anyhow::bail!("{email} is already added as a separate live account. Remove that one first.");
        }
        // A removed live account for the same address would hold the cache path; free it.
        self.db.execute(
            "update accounts set root = root || '.removed.' || id where kind = 'imap' and root = ?1 and id != ?2",
            params![root.to_string_lossy(), account_id],
        )?;
        fs::create_dir_all(&root)?;
        self.db.execute(
            "update accounts set kind = 'imap', config = ?1, root = ?2 where id = ?3",
            params![config, root.to_string_lossy(), account_id],
        )?;
        Ok(())
    }

    /// The server settings Mailbird used for an imported account, read from the export's
    /// `raw/Accounts.jsonl` and `raw/SenderIdentities.jsonl`.
    pub fn mailbird_server(&self, account_id: i64) -> Result<Option<MailbirdServer>> {
        let (email, archive_root): (String, Option<String>) = self.db.query_row(
            "select name, archive_root from accounts where id = ?1",
            [account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let Some(raw) = archive_root.as_deref().and_then(|r| Path::new(r).parent()?.parent().map(|p| p.join("raw"))) else {
            return Ok(None);
        };
        let rows = |name: &str| -> Vec<serde_json::Value> {
            fs::read_to_string(raw.join(name))
                .map(|t| t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
                .unwrap_or_default()
        };
        let text = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
        let num = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_i64());
        let Some(account) = rows("Accounts.jsonl").into_iter().find(|a| text(a, "Username").as_deref() == Some(email.as_str())) else {
            return Ok(None);
        };
        let identity = rows("SenderIdentities.jsonl").into_iter().find(|i| num(i, "AccountId") == num(&account, "Id"));
        Ok(Some(MailbirdServer {
            kind: match num(&account, "Server_Type") {
                Some(0) => "imap",
                Some(1) => "pop3",
                Some(2) => "exchange",
                _ => "unknown",
            }
            .to_string(),
            host: text(&account, "Server_Host").unwrap_or_default(),
            port: num(&account, "Server_Port").unwrap_or(0) as u16,
            tls: num(&account, "Server_Protocol") == Some(1),
            username: text(&account, "Username").unwrap_or_default(),
            oauth: account.get("OAuth2CredentialsId").is_some_and(|v| !v.is_null()),
            smtp_host: identity.as_ref().and_then(|i| text(i, "Server_Host")),
            smtp_port: identity.as_ref().and_then(|i| num(i, "Server_Port")).map(|p| p as u16),
            smtp_tls: identity.as_ref().and_then(|i| num(i, "Server_Protocol")).map(|p| p == 1),
            smtp_username: identity.as_ref().and_then(|i| text(i, "Username")),
        }))
    }

    pub fn live_account(&self, id: i64) -> Result<Option<LiveAccount>> {
        Ok(self.live_accounts()?.into_iter().find(|a| a.id == id))
    }

    /// Finds or creates the local folder for a server folder.
    pub fn remote_folder(&self, account_id: i64, remote_name: &str, display: &str, role: Option<&str>) -> Result<RemoteFolder> {
        let existing: Option<i64> = self
            .db
            .query_row(
                "select id from folders where account_id = ?1 and remote_name = ?2",
                params![account_id, remote_name],
                |r| r.get(0),
            )
            .optional()?;
        // An archive folder of the same role (or name) becomes this server folder, so imported
        // and new mail share one Inbox, one Sent and so on.
        let adopt: Option<i64> = if existing.is_none() {
            self.db
                .query_row(
                    "select id from folders where account_id = ?1 and remote_name is null
                       and ((?2 is not null and role = ?2) or name = ?3)
                     order by (role is ?2) desc, id limit 1",
                    params![account_id, role, display],
                    |r| r.get(0),
                )
                .optional()?
        } else {
            None
        };
        let id = match existing.or(adopt) {
            Some(id) => {
                self.db.execute(
                    "update folders set remote_name = ?1, dir = coalesce(dir, ?2), role = coalesce(role, ?3) where id = ?4",
                    params![remote_name, sanitize_component(remote_name), role, id],
                )?;
                id
            }
            None => {
                // A display name can clash (e.g. two "Sent" folders); keep the remote name then.
                let clash: bool = self.db.query_row(
                    "select count(*) > 0 from folders where account_id = ?1 and name = ?2",
                    params![account_id, display],
                    |r| r.get(0),
                )?;
                let name = if clash { remote_name } else { display };
                self.db.execute(
                    "insert into folders (account_id, name, role, remote_name, dir) values (?1, ?2, ?3, ?4, ?5)",
                    params![account_id, name, role, remote_name, sanitize_component(remote_name)],
                )?;
                self.db.last_insert_rowid()
            }
        };
        Ok(self.db.query_row(
            "select id, remote_name, uidvalidity, last_uid from folders where id = ?1",
            [id],
            |r| Ok(RemoteFolder { id: r.get(0)?, remote_name: r.get(1)?, uidvalidity: r.get(2)?, last_uid: r.get(3)? }),
        )?)
    }

    /// The server reset the folder's UIDs: drop what we cached and start again. Archive mail
    /// stays; it only loses its link to the old UIDs.
    pub fn reset_remote_folder(&self, folder_id: i64, uidvalidity: u32) -> Result<()> {
        self.db.execute(
            "update messages set deleted = 1 where origin_folder_id = ?1 and from_archive = 0",
            [folder_id],
        )?;
        self.db.execute("update messages set uid = null where origin_folder_id = ?1 and from_archive = 1", [folder_id])?;
        self.db.execute(
            "update folders set uidvalidity = ?1, last_uid = 0 where id = ?2",
            params![uidvalidity, folder_id],
        )?;
        Ok(())
    }

    /// Writes a downloaded message into the folder's cache and indexes it. Already-known UIDs
    /// are skipped. Returns true if the message was new.
    pub fn store_remote_message(&mut self, folder_id: i64, uid: u32, flags: &str, raw: &[u8]) -> Result<bool> {
        let known: bool = self.db.query_row(
            "select count(*) > 0 from messages where origin_folder_id = ?1 and uid = ?2",
            params![folder_id, uid],
            |r| r.get(0),
        )?;
        if known {
            return Ok(false);
        }
        // Already in the imported archive? Link it to this UID instead of storing it twice.
        if let Some(message_id) = MessageParser::default().parse_headers(raw).and_then(|m| m.message_id().map(str::to_string)) {
            let archived: Option<i64> = self
                .db
                .query_row(
                    "select id from messages where folder_id = ?1 and message_id = ?2 and uid is null
                       and from_archive = 1 and deleted = 0 limit 1",
                    params![folder_id, message_id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = archived {
                self.db.execute("update messages set uid = ?1, flags = ?2 where id = ?3", params![uid, flags, id])?;
                self.db.execute("update folders set last_uid = max(last_uid, ?1) where id = ?2", params![uid, folder_id])?;
                return Ok(false);
            }
        }
        let (root, dir): (String, String) = self.db.query_row(
            "select a.root, f.dir from folders f join accounts a on a.id = f.account_id where f.id = ?1",
            [folder_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let folder_dir = Path::new(&root).join(&dir);
        for sub in ["cur", "new", "tmp"] {
            fs::create_dir_all(folder_dir.join(sub))?;
        }
        let rel = format!("cur/{uid}.U{uid}.oxymail{}{flags}", crate::MAILDIR_INFO);
        fs::write(folder_dir.join(&rel), raw)?;
        let tx = self.db.transaction()?;
        let added = index_message(&tx, &MessageParser::default(), folder_id, &rel, raw, Some(uid))?.is_some();
        tx.execute("update folders set last_uid = max(last_uid, ?1) where id = ?2", params![uid, folder_id])?;
        tx.commit()?;
        Ok(added)
    }

    /// Lowest and highest UID cached for a folder, for refreshing flags.
    pub fn cached_uid_range(&self, folder_id: i64) -> Result<Option<(u32, u32)>> {
        Ok(self
            .db
            .query_row(
                "select min(uid), max(uid) from messages where origin_folder_id = ?1 and uid is not null and deleted = 0",
                [folder_id],
                |r| Ok((r.get::<_, Option<u32>>(0)?, r.get::<_, Option<u32>>(1)?)),
            )
            .map(|(a, b)| a.zip(b))?)
    }

    /// Applies flags read from the server (`S`, `F`, `D`, `R` in Maildir terms).
    pub fn set_remote_flags(&mut self, folder_id: i64, flags: &[(u32, String)]) -> Result<()> {
        let tx = self.db.transaction()?;
        for (uid, f) in flags {
            tx.execute(
                "update messages set flags = ?1 where origin_folder_id = ?2 and uid = ?3",
                params![f, folder_id, uid],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Hides cached messages whose UIDs are no longer on the server (deleted or moved elsewhere).
    pub fn forget_missing_uids(&mut self, folder_id: i64, low: u32, present: &[u32]) -> Result<usize> {
        let present: std::collections::HashSet<u32> = present.iter().copied().collect();
        let cached: Vec<(i64, u32, bool)> = self
            .db
            .prepare("select id, uid, from_archive from messages where origin_folder_id = ?1 and uid >= ?2 and deleted = 0")?
            .query_map(params![folder_id, low], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        let tx = self.db.transaction()?;
        let mut gone = 0;
        for (id, uid, from_archive) in cached {
            if !present.contains(&uid) {
                if from_archive {
                    // Archive mail is never hidden because the server let it go.
                    tx.execute("update messages set uid = null where id = ?1", [id])?;
                } else {
                    tx.execute("update messages set deleted = 1 where id = ?1", [id])?;
                    gone += 1;
                }
            }
        }
        tx.commit()?;
        Ok(gone)
    }

    /// Server location `(account_id, remote folder, uid)` of each message that lives on a server.
    pub fn remote_refs(&self, ids: &[i64]) -> Result<Vec<(i64, String, u32)>> {
        let mut out = Vec::new();
        for id in ids {
            let found: Option<(i64, Option<String>, Option<u32>)> = self
                .db
                .query_row(
                    "select f.account_id, f.remote_name, m.uid from messages m
                     join folders f on f.id = m.origin_folder_id join accounts a on a.id = f.account_id
                     where m.id = ?1 and a.kind = 'imap'",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            if let Some((account, Some(folder), Some(uid))) = found {
                out.push((account, folder, uid));
            }
        }
        Ok(out)
    }

/// Where a live message sits on its server.
    pub fn remote_ref(&self, id: i64) -> Result<Option<RemoteRef>> {
        Ok(self
            .db
            .query_row(
                "select m.id, f.account_id, f.id, f.remote_name, m.uid, f.role from messages m
                 join folders f on f.id = m.origin_folder_id join accounts a on a.id = f.account_id
                 where m.id = ?1 and a.kind = 'imap' and m.uid is not null and f.remote_name is not null",
                [id],
                |r| {
                    Ok(RemoteRef {
                        message_id: r.get(0)?,
                        account_id: r.get(1)?,
                        folder_id: r.get(2)?,
                        remote_name: r.get(3)?,
                        uid: r.get(4)?,
                        role: r.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    /// The local folder mirroring a server folder, if it has been synced.
    pub fn folder_by_remote(&self, account_id: i64, remote_name: &str) -> Result<Option<i64>> {
        Ok(self
            .db
            .query_row(
                "select id from folders where account_id = ?1 and remote_name = ?2",
                params![account_id, remote_name],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn folder_remote_name(&self, folder_id: i64) -> Result<Option<String>> {
        Ok(self.db.query_row("select remote_name from folders where id = ?1", [folder_id], |r| r.get(0))?)
    }

    /// After messages moved on the server: archive mail follows to `target` (unlinked from its
    /// old UID, to be re-linked when the target syncs); downloaded copies are dropped and come
    /// back from the target folder on the next sync.
    pub fn apply_remote_move(&mut self, ids: &[i64], target: Option<i64>) -> Result<()> {
        let tx = self.db.transaction()?;
        for id in ids {
            let from_archive: bool = tx.query_row("select from_archive from messages where id = ?1", [id], |r| r.get(0))?;
            match (from_archive, target) {
                (true, Some(folder)) => {
                    tx.execute("update messages set folder_id = ?1, uid = null where id = ?2", params![folder, id])?;
                }
                _ => {
                    tx.execute("update messages set deleted = 1 where id = ?1", [id])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Messages can be re-created by a move (new UID, new row). Maps ids that are now hidden
    /// to the visible copy with the same Message-ID in the same account.
    pub fn resolve_current(&self, ids: &[i64]) -> Result<Vec<i64>> {
        let mut out = Vec::new();
        for id in ids {
            let (deleted, message_id, account): (bool, Option<String>, i64) = self.db.query_row(
                "select m.deleted, m.message_id, f.account_id from messages m join folders f on f.id = m.folder_id where m.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let current = match (deleted, message_id) {
                (true, Some(mid)) => self
                    .db
                    .query_row(
                        "select m.id from messages m join folders f on f.id = m.folder_id
                         where f.account_id = ?1 and m.message_id = ?2 and m.deleted = 0
                         order by m.uid is null, m.id desc limit 1",
                        params![account, mid],
                        |r| r.get(0),
                    )
                    .optional()?
                    .unwrap_or(*id),
                _ => *id,
            };
            out.push(current);
        }
        Ok(out)
    }

    pub fn set_folder_uidvalidity(&self, folder_id: i64, uidvalidity: u32) -> Result<()> {
        self.db.execute("update folders set uidvalidity = ?1 where id = ?2", params![uidvalidity, folder_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(id: &str, subject: &str) -> String {
        format!("From: A <a@example.com>\r\nMessage-ID: <{id}>\r\nSubject: {subject}\r\nDate: Tue, 1 Jul 2025 10:00:00 +0000\r\n\r\nBody of {subject}\r\n")
    }

    #[test]
    fn connecting_an_archive_keeps_its_mail_and_links_duplicates() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("oxymail-live-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let inbox = dir.join("export/mail/me@example.com/Inbox/cur");
        fs::create_dir_all(&inbox)?;
        fs::write(inbox.join(format!("1.M1.oxymail{}S", crate::MAILDIR_INFO)), mail("old@x", "Old archived"))?;
        fs::write(inbox.join(format!("2.M2.oxymail{}S", crate::MAILDIR_INFO)), mail("both@x", "On server too"))?;

        let mut store = Store::open(&dir.join("t.db"))?;
        store.import_archive(&dir.join("export/mail"), |_| {})?;
        let account = store.accounts()?[0].id;
        store.connect_archive(account, "{}", &dir.join("cache"))?;
        assert_eq!(store.accounts()?[0].kind, "imap");
        assert!(store.accounts()?[0].has_archive);

        // The server's INBOX adopts the archive Inbox.
        let folder = store.remote_folder(account, "INBOX", "Inbox", Some("inbox"))?;
        assert_eq!(store.folders(account)?.len(), 1, "no second Inbox");

        // A message already in the archive is linked, not stored again; a new one is stored.
        assert!(!store.store_remote_message(folder.id, 10, "S", mail("both@x", "On server too").as_bytes())?);
        assert!(store.store_remote_message(folder.id, 11, "", mail("new@x", "Brand new").as_bytes())?);
        assert_eq!(store.messages(folder.id, 0, 50)?.len(), 3);

        // The server drops both: the archived one stays visible, the downloaded one goes.
        store.forget_missing_uids(folder.id, 1, &[])?;
        let subjects: Vec<String> = store.messages(folder.id, 0, 50)?.into_iter().filter_map(|m| m.subject).collect();
        assert!(subjects.contains(&"On server too".to_string()) && subjects.contains(&"Old archived".to_string()));
        assert!(!subjects.contains(&"Brand new".to_string()));

        // Archive mail still opens, and re-importing the archive adds nothing.
        let old = store.search("archived", 5)?[0].id;
        assert_eq!(store.message(old)?.subject.as_deref(), Some("Old archived"));
        assert_eq!(store.import_archive(&dir.join("export/mail"), |_| {})?.added, 0);
        assert_eq!(store.accounts()?.len(), 1, "re-import must not create a second account");
        fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
