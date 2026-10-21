//! Account passwords live in the desktop keyring (GNOME Keyring / KWallet via Secret Service),
//! never in OxyMail's database.

use anyhow::{Context, Result};

const SERVICE: &str = "OxyMail";

fn entry(email: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, email).context("opening the system keyring")
}

pub fn save_password(email: &str, password: &str) -> Result<()> {
    entry(email)?.set_password(password).context("saving the password to the system keyring")
}

pub fn password(email: &str) -> Result<String> {
    entry(email)?
        .get_password()
        .with_context(|| format!("no saved password for {email} in the system keyring"))
}

pub fn forget_password(email: &str) -> Result<()> {
    match entry(email)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("removing the password from the system keyring"),
    }
}

fn refresh_entry(email: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &format!("oauth-refresh:{email}")).context("opening the system keyring")
}

pub fn save_refresh_token(email: &str, token: &str) -> Result<()> {
    refresh_entry(email)?.set_password(token).context("saving the sign-in to the system keyring")
}

pub fn refresh_token(email: &str) -> Result<String> {
    refresh_entry(email)?
        .get_password()
        .with_context(|| format!("{email} isn't signed in; sign in again from Settings → Accounts"))
}

pub fn forget_refresh_token(email: &str) -> Result<()> {
    match refresh_entry(email)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).context("removing the sign-in from the system keyring"),
    }
}

/// Client secrets for sign-in providers (Settings → Sign-in).
pub fn save_client_secret(provider: &str, secret: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, &format!("oauth-client:{provider}"))?;
    if secret.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    } else {
        Ok(entry.set_password(secret)?)
    }
}

pub fn client_secret(provider: &str) -> Option<String> {
    keyring::Entry::new(SERVICE, &format!("oauth-client:{provider}")).ok()?.get_password().ok()
}
