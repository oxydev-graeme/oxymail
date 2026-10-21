# OxyMail privacy policy

*Last updated: 21 October 2026*

OxyMail is a desktop email client that runs on your own computer. This policy explains what it does with your data. It's also published at <https://getoxymail.com/privacy>.

## Where your mail lives

- **On your computer:** OxyMail downloads your email, contacts and calendars to your computer, and stores them in the application's data folder:
  - Windows: `%APPDATA%`
  - macOS: `~/Library/Application Support`
  - Linux: `~/.local/share`
- **Nowhere else:** there is no OxyMail server. Your messages are never sent to the OxyMail developers or to anyone else.
- **Passwords and sign-in tokens:** these are kept in your operating system's secure storage (Windows Credential Manager, macOS Keychain or the Linux Secret Service), not in OxyMail's files.

## Google account data

When you choose **Sign in with Google**, OxyMail asks Google for permission to access your Gmail (`https://mail.google.com/`) and your email address.

- **What it's used for:** OxyMail uses this access only to show, search, organise and send your email from inside the app on your computer.
  - It reads your mail over IMAP.
  - It sends mail over SMTP.
  - It applies the actions you take (read, star, archive, move, delete) on Google's servers.
  - It uses your email address to check that you signed in to the right account.
- **Who it's shared with:** nobody. OxyMail does not transfer your Google user data to the developers or to any third party. It doesn't use your data for advertising, and doesn't sell it.
- **How data travels:** directly between your computer and Google.

**OxyMail's use and transfer of information received from Google APIs adheres to the [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including the Limited Use requirements.**

## Other connections OxyMail makes

| What | Why |
| --- | --- |
| **Your mail providers' servers** | To send and receive your mail. |
| **Mozilla's ISP database and your provider's autoconfig service** | When you add an account: to find its server settings. Only your email domain is sent. |
| **GitHub** | To check for updates, if automatic update checks are on (Settings → About). No personal data is sent. |
| **Remote images in email** | Blocked by default. Only loaded when you choose **Load images** for a message or sender. |

OxyMail has no analytics, telemetry or tracking.

## Removing your data

- **Signing an account out:** removing an account in OxyMail deletes its saved password or sign-in token.
- **Revoking Google access:** you can revoke OxyMail's access to your Google account at any time, at <https://myaccount.google.com/permissions>.
- **Removing everything:** uninstall OxyMail and delete its data folder.

## Contact

For questions about this policy, email **hello@oxyaccounting.co.za** or open an issue at <https://github.com/oxydev-graeme/oxymail/issues>.
