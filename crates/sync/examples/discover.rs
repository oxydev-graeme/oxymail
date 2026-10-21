//! `cargo run -p oxymail-sync --example discover -- you@example.com`
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    for email in std::env::args().skip(1) {
        let config = oxymail_sync::autoconfig::discover(&email).await?;
        println!("{}", serde_json::to_string_pretty(&config)?);
    }
    Ok(())
}
