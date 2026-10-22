//! Command-line front end for the Mailbird exporter. The app runs the same export from
//! its welcome screen and Settings → Import.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use oxymail_mailbird_export::{Options, export};

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

fn main() -> Result<()> {
    let args = Args::parse();
    let opts = Options { account: args.account, limit: args.limit, extras_only: args.extras_only };
    let summary = export(&args.store, &args.out, &opts, |p| {
        if p.done % 5000 == 0 || p.done == p.total {
            eprintln!("{} of {} messages exported", p.done, p.total);
        }
    })?;
    let x = &summary.extras;
    eprintln!(
        "contacts: {} in the address book, {} collected addresses; calendars: {} with {} events",
        x.contacts, x.collected_addresses, x.calendars, x.events
    );
    eprintln!(
        "{} messages from {} accounts ({} missing bodies, {} missing attachments); report in {}",
        summary.messages,
        summary.accounts,
        summary.missing_bodies,
        summary.missing_attachments,
        args.out.join("report.json").display()
    );
    if summary.blocked_attachments > 0 || !summary.unwritable.is_empty() {
        eprintln!(
            "antivirus blocked {} attachments (marked in their messages) and {} messages (listed in report.json)",
            summary.blocked_attachments,
            summary.unwritable.len()
        );
    }
    Ok(())
}
