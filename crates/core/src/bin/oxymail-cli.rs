//! Command-line access to the OxyMail store, for importing and checking it without the app.

use std::path::PathBuf;

use anyhow::{Result, bail};
use oxymail_core::Store;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let db = std::env::var_os("OXYMAIL_DB").map(PathBuf::from).unwrap_or_else(Store::default_path);
    let mut store = Store::open(&db)?;

    match args.first().map(String::as_str) {
        Some("import") => {
            let Some(root) = args.get(1) else { bail!("usage: oxymail-cli import <maildir-root>") };
            let stats = store.import_archive(root.as_ref(), |n| eprintln!("{n} indexed"))?;
            println!("{}", serde_json::to_string_pretty(&stats)?);
        }
        Some("backfill-threads") => {
            let n = store.backfill_threads(|n| eprintln!("{n} done"))?;
            println!("{n} messages given a conversation key");
        }
        Some("thread") => {
            let Some(id) = args.get(1).and_then(|s| s.parse().ok()) else { bail!("usage: oxymail-cli thread <id>") };
            for m in store.thread(id)? {
                println!("{:8}  {:30.30}  {}", m.id, m.from_name.or(m.from_addr).unwrap_or_default(), m.subject.unwrap_or_default());
            }
        }
        Some("accounts") => {
            for account in store.accounts()? {
                println!("{:4}  {}", account.id, account.name);
                for f in store.folders(account.id)? {
                    println!("      {:5}  {:30} {:6} total {:6} unread", f.id, f.name, f.total, f.unread);
                }
            }
        }
        Some("search") => {
            let query = args[1..].join(" ");
            for m in store.search(&query, 20)? {
                println!(
                    "{:8}  {:30.30}  {}",
                    m.id,
                    m.from_name.or(m.from_addr).unwrap_or_default(),
                    m.subject.unwrap_or_default()
                );
            }
        }
        Some("show") => {
            let Some(id) = args.get(1).and_then(|s| s.parse().ok()) else { bail!("usage: oxymail-cli show <id>") };
            let view = store.message(id)?;
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "subject": view.subject, "from": view.from, "to": view.to, "cc": view.cc,
                "date": view.date, "html_chars": view.html.as_ref().map(|h| h.len()),
                "text_chars": view.text.as_ref().map(|t| t.len()),
                "attachments": view.attachments, "notices": view.notices, "from_addr": view.from_addr, "has_remote_content": view.has_remote_content, "images_allowed": view.images_allowed, "unsubscribe": view.unsubscribe,
            }))?);
        }
        _ => bail!("usage: oxymail-cli <import|accounts|search|show> ..."),
    }
    Ok(())
}
