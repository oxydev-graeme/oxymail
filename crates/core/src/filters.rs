//! Mail filters: "when the subject contains X, mark as spam" and the like, run on new Inbox
//! mail as it arrives. Mailbird's filters are brought over by the Mailbird import.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::Store;

/// Where a filter looks: `subject`, `from` (name or address), `to` (any recipient) or `any`.
pub const FIELDS: &[&str] = &["subject", "from", "to", "any"];
/// What a filter does: `spam`, `archive` and `trash` move the message; `read` and `star` flag it.
pub const ACTIONS: &[&str] = &["spam", "archive", "trash", "read", "star"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Filter {
    #[serde(default)]
    pub id: i64,
    /// `None` applies to every account.
    pub account_id: Option<i64>,
    pub field: String,
    pub value: String,
    pub action: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// Case-insensitive "contains", where `*` stands for any run of characters, as in Mailbird
/// (`*squirt*`, `Air Watch S`).
pub fn glob_contains(text: &str, pattern: &str) -> bool {
    let text = text.to_lowercase();
    let pattern = pattern.to_lowercase();
    let mut rest = text.as_str();
    let mut any = false;
    for part in pattern.split('*').filter(|p| !p.is_empty()) {
        any = true;
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    any
}

impl Filter {
    /// Whether this filter applies to a message of `account` with these headers.
    fn matches(&self, account: i64, subject: &str, from: &str, to: &str) -> bool {
        self.account_id.is_none_or(|a| a == account)
            && match self.field.as_str() {
                "subject" => glob_contains(subject, &self.value),
                "from" => glob_contains(from, &self.value),
                "to" => glob_contains(to, &self.value),
                _ => [subject, from, to].iter().any(|t| glob_contains(t, &self.value)),
            }
    }
}

/// The columns a filter looks at, for messages joined with their folder.
const FILTER_COLUMNS: &str = "f.account_id, coalesce(m.subject, ''),
    coalesce(m.from_name, '') || ' ' || coalesce(m.from_addr, ''), coalesce(m.to_addrs, '')";

impl Store {
    pub fn filters(&self) -> Result<Vec<Filter>> {
        let mut stmt = self
            .db
            .prepare("select id, account_id, field, value, action, enabled from filters order by sort_index, id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Filter {
                id: r.get(0)?,
                account_id: r.get(1)?,
                field: r.get(2)?,
                value: r.get(3)?,
                action: r.get(4)?,
                enabled: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Adds a filter (`id` 0) or replaces one. Returns its id.
    pub fn save_filter(&self, f: &Filter) -> Result<i64> {
        if !FIELDS.contains(&f.field.as_str()) || !ACTIONS.contains(&f.action.as_str()) {
            bail!("unknown filter {} → {}", f.field, f.action);
        }
        if f.value.trim().trim_matches('*').is_empty() {
            bail!("a filter needs some text to look for");
        }
        if f.id == 0 {
            self.db.execute(
                "insert into filters (account_id, field, value, action, enabled, sort_index)
                 values (?1, ?2, ?3, ?4, ?5, (select coalesce(max(sort_index), 0) + 1 from filters))",
                params![f.account_id, f.field, f.value.trim(), f.action, f.enabled],
            )?;
            Ok(self.db.last_insert_rowid())
        } else {
            self.db.execute(
                "update filters set account_id = ?1, field = ?2, value = ?3, action = ?4, enabled = ?5 where id = ?6",
                params![f.account_id, f.field, f.value.trim(), f.action, f.enabled, f.id],
            )?;
            Ok(f.id)
        }
    }

    pub fn delete_filter(&self, id: i64) -> Result<()> {
        self.db.execute("delete from filters where id = ?1", [id])?;
        Ok(())
    }

    /// Adds filters that aren't there yet (same account, field, text and action). Returns how
    /// many were added.
    pub fn add_filters(&self, filters: &[Filter]) -> Result<usize> {
        let existing = self.filters()?;
        let mut added = 0;
        for f in filters {
            let known = existing.iter().any(|e| {
                e.account_id == f.account_id && e.field == f.field && e.action == f.action && e.value.eq_ignore_ascii_case(f.value.trim())
            });
            if !known {
                self.save_filter(&Filter { id: 0, ..f.clone() })?;
                added += 1;
            }
        }
        Ok(added)
    }

    /// Runs the enabled filters over `ids`: action → messages it applies to. A message is
    /// handled by the first filter that matches it, in list order.
    pub fn filter_matches(&self, ids: &[i64]) -> Result<BTreeMap<String, Vec<i64>>> {
        let filters: Vec<Filter> = self.filters()?.into_iter().filter(|f| f.enabled).collect();
        let mut out: BTreeMap<String, Vec<i64>> = BTreeMap::new();
        if filters.is_empty() {
            return Ok(out);
        }
        let mut stmt = self.db.prepare_cached(&format!(
            "select {FILTER_COLUMNS} from messages m join folders f on f.id = m.folder_id where m.id = ?1 and m.deleted = 0"
        ))?;
        for id in ids {
            let row = stmt.query_row([id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
            });
            let Ok((account, subject, from, to)) = row else { continue };
            if let Some(f) = filters.iter().find(|f| f.matches(account, &subject, &from, &to)) {
                out.entry(f.action.clone()).or_default().push(*id);
            }
        }
        Ok(out)
    }

    /// The mail already in an Inbox that a filter applies to (skipping mail it would change
    /// nothing for: read mail for "Mark as read", starred mail for "Star"), for running a
    /// filter on mail that arrived before it existed.
    pub fn filter_existing(&self, filter_id: i64) -> Result<Vec<i64>> {
        let Some(filter) = self.filters()?.into_iter().find(|f| f.id == filter_id) else { bail!("that filter no longer exists") };
        let skip = match filter.action.as_str() {
            "read" => "and m.unread = 1",
            "star" => "and m.starred = 0",
            _ => "",
        };
        let mut stmt = self.db.prepare(&format!(
            "select m.id, {FILTER_COLUMNS} from messages m join folders f on f.id = m.folder_id
             where f.role = 'inbox' and m.deleted = 0 {skip}"
        ))?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
        })?;
        let mut ids = Vec::new();
        for row in rows {
            let (id, account, subject, from, to) = row?;
            if filter.matches(account, &subject, &from, &to) {
                ids.push(id);
            }
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_like_mailbird() {
        assert!(glob_contains("Get your SmartCam today", "smartcam"));
        assert!(glob_contains("Hot squirting deals", "*squirt*"));
        assert!(glob_contains("***SPAM*** Buy now", "***SPAM***"), "only stars is not an empty pattern here");
        assert!(glob_contains("Air Watch S5 sale", "Air Watch S"));
        assert!(glob_contains("a fat reducer b", "fat*reducer"));
        assert!(!glob_contains("reducer fat", "fat*reducer"));
        assert!(!glob_contains("anything", "**"));
    }

    #[test]
    fn filters_pick_the_first_match_per_message() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("oxymail-filters-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mail = |id: &str, from: &str, subject: &str| {
            format!("From: {from}\r\nTo: me@example.com\r\nMessage-ID: <{id}@x>\r\nSubject: {subject}\r\nDate: Tue, 1 Jul 2025 10:00:00 +0000\r\n\r\nHi\r\n")
        };
        let cur = dir.join("mail/me@example.com/Inbox/cur");
        std::fs::create_dir_all(&cur)?;
        std::fs::write(cur.join(format!("1.M1.oxymail{}", crate::MAILDIR_INFO)), mail("a", "Shop <deals@shop.example>", "SmartCam 50% off"))?;
        std::fs::write(cur.join(format!("2.M2.oxymail{}", crate::MAILDIR_INFO)), mail("b", "Wordfence <wf@site.example>", "[Wordfence Alert] login"))?;
        std::fs::write(cur.join(format!("3.M3.oxymail{}", crate::MAILDIR_INFO)), mail("c", "Friend <f@example.com>", "Lunch?"))?;
        let mut store = Store::open(&dir.join("t.db"))?;
        store.import_archive(&dir.join("mail"), |_| {})?;
        let account = store.accounts()?[0].id;

        let spam = Filter { id: 0, account_id: None, field: "subject".into(), value: "smartcam".into(), action: "spam".into(), enabled: true };
        let wf = Filter { id: 0, account_id: Some(account), field: "from".into(), value: "wordfence".into(), action: "archive".into(), enabled: true };
        let off = Filter { id: 0, account_id: None, field: "any".into(), value: "lunch".into(), action: "trash".into(), enabled: false };
        assert_eq!(store.add_filters(&[spam.clone(), wf, off])?, 3);
        assert_eq!(store.add_filters(&[Filter { value: "SMARTCAM".into(), ..spam }])?, 0, "no duplicates");

        let inbox = store.folders(account)?[0].id;
        let ids: Vec<i64> = store.messages(inbox, 0, 10)?.into_iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), 3);
        let hits = store.filter_matches(&ids)?;
        assert_eq!(hits.get("spam").map(Vec::len), Some(1));
        assert_eq!(hits.get("archive").map(Vec::len), Some(1));
        assert!(!hits.contains_key("trash"), "disabled filters don't run");

        // Run now: the same rule over the Inbox, skipping mail it would change nothing for.
        let saved = store.filters()?;
        let spam_id = saved.iter().find(|f| f.action == "spam").unwrap().id;
        assert_eq!(store.filter_existing(spam_id)?.len(), 1);
        let read = store.save_filter(&Filter { id: 0, account_id: None, field: "any".into(), value: "*".into(), action: "read".into(), enabled: true });
        assert!(read.is_err(), "a filter of only stars matches nothing and is refused");
        let lunch = saved.iter().find(|f| f.action == "trash").unwrap().id;
        assert_eq!(store.filter_existing(lunch)?.len(), 1, "Run now works on a filter that's switched off");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
