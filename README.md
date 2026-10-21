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
