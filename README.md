# OxyMail

*Email with room to breathe.* **[getoxymail.com](https://getoxymail.com)**

An email client with calendar and contacts for **Windows, macOS and Linux**, built in Rust with [Tauri](https://tauri.app).

## Features

- **Accounts:**
  - several IMAP/SMTP accounts, with settings detected for you
  - Google and Microsoft sign-in (OAuth), or app passwords
  - passwords kept in the system's secure storage (Windows Credential Manager, macOS Keychain, Linux Secret Service)
- **Mail:**
  - all inboxes in one view
  - push mail (IMAP IDLE)
  - offline reading
  - full-text search with filters (`from:` `has:attachment` `is:unread` `before:`…)
  - conversations
- **Organising:**
  - archive, delete and move on the server, with Undo
  - select several messages at once, or drag them to folders
  - snooze
  - mark all as read
- **Writing:**
  - rich text, signatures per account, attachments
  - address autocomplete
  - drafts that save themselves
  - undo send, and send later
- **Privacy:** remote images are blocked unless you allow them, message HTML is sandboxed, and there's a one-click unsubscribe.
- **Calendar:** month view and agenda, repeating events, show or hide calendars, `.ics` import.
- **Contacts:** address book with photos, `.vcf` import, Email and Show-emails shortcuts.
- **Mailbird migration:** a lossless exporter, plus importing the archive with its account order, colours, signatures, contacts and calendars.
- **Automatic updates** from signed GitHub releases.

## Layout

| Path | What it is |
| --- | --- |
| `crates/core` | Local store: SQLite index with FTS5 search, Maildir cache, calendar and contacts |
| `crates/sync` | IMAP sync and IDLE, SMTP, autoconfig, OAuth, secure credential storage |
| `crates/mailbird-export` | Exports a Mailbird `Store` folder to Maildir, vCard and iCalendar |
| `app/` | The Tauri app with a Svelte 5 interface |

## Clean-room notice

OxyMail is an independent project and is not affiliated with Mailbird. It contains no Mailbird code. The importer reads data files the user already owns, so they can move that data to another app.

## Licence

GPL-3.0. See [LICENSE](LICENSE).

## Installing

Download the latest release from [Releases](https://github.com/oxydev-graeme/oxymail/releases/latest):

| Platform | Download | Notes |
| --- | --- | --- |
| **Windows** 10/11 | `OxyMail_*_x64-setup.exe` (or `.msi`) | Installs for your user; no administrator rights needed |
| **macOS** 11+ (Apple Silicon and Intel) | `OxyMail_*_universal.dmg` | Drag to Applications. The first time, right-click → Open (the app isn't notarised yet). |
| **Linux** | `.AppImage`, `.deb` or `.rpm` | AppImage: `chmod +x` and run. Debian/Ubuntu/Zorin: `sudo apt install ./OxyMail_*.deb` |

Every install updates itself. OxyMail checks GitHub when it starts (Settings → About can turn this off) and installs only releases signed with the project's key.

## Releasing

`scripts/release.sh 0.2.1` bumps the version, tags and pushes. GitHub Actions (`.github/workflows/release.yml`) then builds on Windows, macOS and Linux in parallel, signs every installer with the `TAURI_SIGNING_PRIVATE_KEY` secret, and publishes them with `latest.json`, which installed copies read to update. `ci.yml` runs the tests on all three platforms on every push.

## Building

```sh
cargo build --release
```

If the repo is on a network share mounted `noexec`, put the build output on a local disk:
`CARGO_TARGET_DIR=~/.cache/oxymail-target cargo build --release`.

## Mailbird export

```sh
oxymail-mailbird-export --store /path/to/copy/of/Mailbird/Store --out ~/MailbirdExport
```

- Always run it against a **copy** of `%LOCALAPPDATA%\Mailbird\Store`. The store is opened read-only either way.
- **Mail:** Maildir output at `mail/<account>/<folder>/cur/`. A message that sits in several folders, such as Gmail labels, is hard-linked rather than copied.
- **Raw tables:** every non-secret table is written to `raw/<table>.jsonl`. Passwords, OAuth tokens and sync tokens are never exported.
- **Report:** `report.json` lists counts per account and folder, along with any missing bodies or attachments.
- **Missing parts:** where a body or attachment was never downloaded by Mailbird, the message is marked with an `X-OxyMail-Missing` or `X-OxyMail-Missing-Attachment` header.
