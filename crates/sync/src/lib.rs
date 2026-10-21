//! OxyMail sync: account discovery, IMAP download, SMTP sending and credential storage.

pub mod autoconfig;
pub mod imap;
pub mod secrets;
pub mod smtp;

/// Call once at startup. Both ring and aws-lc are linked in (via lettre and reqwest), so rustls
/// needs telling which one to use by default.
pub fn init() {
    let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
}
