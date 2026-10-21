# OxyMail

A Linux-first desktop email client, built in Rust.

> **Status:** early discovery. There is no usable code yet.

## Goals

- Run well on Linux (developed on Zorin OS), with Windows and macOS later
- Multiple accounts: IMAP/SMTP, Gmail and Microsoft 365 via OAuth2
- A unified inbox, offline reading and fast full-text search
- Keep mail in open formats (Maildir, vCard) so it never gets locked in
- Import from Mailbird's local data store

## Stack

| Layer | Choice |
| --- | --- |
| Engine (sync, storage, search, import) | Rust |
| App shell | [Tauri](https://tauri.app) |
| UI | TypeScript (Svelte or SolidJS) |

Main crates: `async-imap` / `imap-codec`, `lettre`, `mail-parser`, `mail-builder`, `oauth2`, `rusqlite`, `tantivy`.

## Roadmap

1. **Mailbird exporter:** read a copy of Mailbird's local store and export mail to Maildir/mbox, contacts to vCard, and signatures and settings to plain files
2. **Engine:** account model, IMAP sync, local store
3. **App:** message list and reader in a Tauri shell
4. Compose and send, OAuth providers, search

## Clean-room notice

OxyMail is an independent project and is not affiliated with Mailbird. It contains no Mailbird code. The importer reads data files the user already owns, so they can move that data to another app.

## Licence

GPL-3.0. See [LICENSE](LICENSE).

## Installing

Download the latest release from [Releases](https://github.com/oxydev-graeme/oxymail/releases/latest):

- **AppImage:** `chmod +x OxyMail_*.AppImage` and run it. It needs no installation.
- **Debian/Ubuntu/Zorin:** `sudo apt install ./OxyMail_*_amd64.deb`

Both update themselves. OxyMail checks GitHub when it starts (Settings → About can turn this off) and installs only releases signed with the project's key.

## Releasing

`scripts/release.sh 0.2.1` bumps the version, tags and pushes. GitHub Actions (`.github/workflows/release.yml`) then builds the .deb and AppImage, signs them with the `TAURI_SIGNING_PRIVATE_KEY` secret and publishes them with `latest.json`, which installed copies read to update.

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
