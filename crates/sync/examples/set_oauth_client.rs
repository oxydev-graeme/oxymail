//! Stores a Google client secret in the system keyring exactly as Settings → Sign-in does.
//! `OXYMAIL_GOOGLE_CLIENT_SECRET=… cargo run -p oxymail-sync --example set_oauth_client`
fn main() -> anyhow::Result<()> {
    let secret = std::env::var("OXYMAIL_GOOGLE_CLIENT_SECRET")?;
    oxymail_sync::secrets::save_client_secret("google", &secret)?;
    let back = oxymail_sync::secrets::client_secret("google").unwrap_or_default();
    println!("google client secret stored: {}", back == secret);
    Ok(())
}
