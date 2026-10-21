//! `cargo run -p oxymail-sync --example probe -- host port` — TLS-connects and prints the
//! server greeting, without logging in.
use tokio::io::AsyncReadExt;
use tokio_rustls::rustls::pki_types::ServerName;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (host, port) = (args[0].clone(), args[1].parse::<u16>()?);
    let tcp = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
    let mut tls = oxymail_sync::imap::tls_connector().connect(ServerName::try_from(host.clone())?, tcp).await?;
    let mut buf = [0u8; 512];
    let n = tls.read(&mut buf).await?;
    println!("{host}:{port} TLS ok, greeting: {}", String::from_utf8_lossy(&buf[..n]).trim());
    Ok(())
}
