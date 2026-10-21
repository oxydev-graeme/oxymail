//! Saves, reads back and deletes a throwaway keyring entry.
fn main() -> anyhow::Result<()> {
    let who = "keyring-check@oxymail.invalid";
    oxymail_sync::secrets::save_password(who, "test-secret")?;
    let back = oxymail_sync::secrets::password(who)?;
    oxymail_sync::secrets::forget_password(who)?;
    println!("keyring round trip: {}", if back == "test-secret" { "ok" } else { "MISMATCH" });
    println!("after delete: {}", if oxymail_sync::secrets::password(who).is_err() { "gone" } else { "STILL THERE" });
    Ok(())
}
