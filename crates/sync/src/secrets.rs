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
