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
        let id = match existing {
            Some(id) => {
                self.db.execute(
                    "update folders set name = ?1, role = coalesce(?2, role) where id = ?3",
                    params![display, role, id],
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

    /// The server reset the folder's UIDs: drop what we cached and start again.
    pub fn reset_remote_folder(&self, folder_id: i64, uidvalidity: u32) -> Result<()> {
        self.db.execute("update messages set deleted = 1 where origin_folder_id = ?1", [folder_id])?;
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
        let (root, dir): (String, String) = self.db.query_row(
            "select a.root, f.dir from folders f join accounts a on a.id = f.account_id where f.id = ?1",
            [folder_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let folder_dir = Path::new(&root).join(&dir);
        for sub in ["cur", "new", "tmp"] {
            fs::create_dir_all(folder_dir.join(sub))?;
        }
        let rel = format!("cur/{uid}.U{uid}.oxymail:2,{flags}");
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
        let cached: Vec<(i64, u32)> = self
            .db
            .prepare("select id, uid from messages where origin_folder_id = ?1 and uid >= ?2 and deleted = 0")?
            .query_map(params![folder_id, low], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let tx = self.db.transaction()?;
        let mut gone = 0;
        for (id, uid) in cached {
            if !present.contains(&uid) {
                tx.execute("update messages set deleted = 1 where id = ?1", [id])?;
                gone += 1;
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

    pub fn set_folder_uidvalidity(&self, folder_id: i64, uidvalidity: u32) -> Result<()> {
        self.db.execute("update folders set uidvalidity = ?1 where id = ?2", params![uidvalidity, folder_id])?;
        Ok(())
    }
}
