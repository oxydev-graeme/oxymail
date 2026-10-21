//! OAuth 2.0 sign-in for Google and Microsoft mail, as a desktop app: the system browser opens
//! the provider's consent page and the answer comes back to a one-off listener on 127.0.0.1
//! (RFC 8252), protected with PKCE. Refresh tokens live in the keyring; access tokens are
//! cached in memory and renewed shortly before they expire.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::secrets;

/// How long the user has to finish signing in in the browser.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    auth_url: &'static str,
    token_url: &'static str,
    scope: &'static str,
}

pub const GOOGLE: Provider = Provider {
    id: "google",
    name: "Google",
    auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    scope: "https://mail.google.com/ openid email",
};

pub const MICROSOFT: Provider = Provider {
    id: "microsoft",
    name: "Microsoft",
    auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
    token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
    scope: "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access openid email",
};

pub fn provider(id: &str) -> Result<&'static Provider> {
    match id {
        "google" => Ok(&GOOGLE),
        "microsoft" => Ok(&MICROSOFT),
        _ => bail!("unknown sign-in provider {id}"),
    }
}

/// The app's own registration with a provider. Microsoft public clients have no secret.
#[derive(Clone, Debug)]
pub struct Client {
    pub id: String,
    pub secret: Option<String>,
}

static CLIENTS: LazyLock<Mutex<HashMap<String, Client>>> = LazyLock::new(Default::default);
static ACCESS: LazyLock<Mutex<HashMap<String, (String, Instant)>>> = LazyLock::new(Default::default);

/// Registers (or clears, with `None`) the client used for a provider. Called by the app at
/// start-up and whenever the user changes it in Settings.
pub fn set_client(provider: &str, client: Option<Client>) {
    if let Ok(mut clients) = CLIENTS.lock() {
        match client {
            Some(c) if !c.id.trim().is_empty() => clients.insert(provider.to_string(), c),
            _ => clients.remove(provider),
        };
    }
}

pub fn has_client(provider: &str) -> bool {
    CLIENTS.lock().map(|c| c.contains_key(provider)).unwrap_or(false)
}

fn client(provider: &Provider) -> Result<Client> {
    CLIENTS
        .lock()
        .ok()
        .and_then(|c| c.get(provider.id).cloned())
        .ok_or_else(|| anyhow!("{} sign-in isn't set up yet. Add a client ID in Settings → Sign-in.", provider.name))
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    error_description: Option<String>,
}

/// Runs the browser sign-in for `email` and stores the refresh token. Returns the address the
/// provider confirmed, which must match `email`.
pub async fn sign_in(provider_id: &str, email: &str) -> Result<String> {
    let provider = provider(provider_id)?;
    let client = client(provider)?;

    let listener = TcpListener::bind("127.0.0.1:0").await.context("opening a local port for the sign-in")?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let verifier = random_token(48)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_token(24)?;

    let mut url = reqwest::Url::parse(provider.auth_url)?;
    url.query_pairs_mut()
        .append_pair("client_id", &client.id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", provider.scope)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("login_hint", email)
        // Google only issues a refresh token with offline access and fresh consent.
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    open_browser(url.as_str())?;

    let code = tokio::time::timeout(SIGN_IN_TIMEOUT, wait_for_code(&listener, &state))
        .await
        .map_err(|_| anyhow!("sign-in timed out; nothing came back from the browser within 5 minutes"))??;

    let mut form = vec![
        ("client_id", client.id.clone()),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code".to_string()),
        ("redirect_uri", redirect),
    ];
    if let Some(secret) = &client.secret {
        form.push(("client_secret", secret.clone()));
    }
    let tokens = token_request(provider, &form).await?;
    let refresh = tokens
        .refresh_token
        .clone()
        .ok_or_else(|| anyhow!("{} didn't return a refresh token", provider.name))?;
    let confirmed = tokens.id_token.as_deref().and_then(id_token_email).unwrap_or_else(|| email.to_string());
    if !confirmed.eq_ignore_ascii_case(email) {
        bail!("you signed in as {confirmed}, but this account is {email}. Sign in with {email} instead.");
    }
    secrets::save_refresh_token(email, &refresh)?;
    cache_access(email, &tokens);
    Ok(confirmed)
}

/// A current access token for `email`, renewing it with the stored refresh token if needed.
pub async fn access_token(provider_id: &str, email: &str) -> Result<String> {
    if let Some((token, expires)) = ACCESS.lock().ok().and_then(|a| a.get(email).cloned()) {
        if Instant::now() + Duration::from_secs(60) < expires {
            return Ok(token);
        }
    }
    let provider = provider(provider_id)?;
    let client = client(provider)?;
    let refresh = secrets::refresh_token(email)?;
    let mut form = vec![
        ("client_id", client.id.clone()),
        ("refresh_token", refresh),
        ("grant_type", "refresh_token".to_string()),
    ];
    if let Some(secret) = &client.secret {
        form.push(("client_secret", secret.clone()));
    }
    let tokens = token_request(provider, &form)
        .await
        .with_context(|| format!("renewing {} sign-in for {email}; signing in again may fix it", provider.name))?;
    // Microsoft rotates refresh tokens; keep the newest one.
    if let Some(new_refresh) = &tokens.refresh_token {
        secrets::save_refresh_token(email, new_refresh)?;
    }
    cache_access(email, &tokens);
    Ok(tokens.access_token)
}

pub fn forget(email: &str) {
    if let Ok(mut access) = ACCESS.lock() {
        access.remove(email);
    }
    let _ = secrets::forget_refresh_token(email);
}

fn cache_access(email: &str, tokens: &TokenResponse) {
    let lifetime = Duration::from_secs(tokens.expires_in.unwrap_or(3600));
    if let Ok(mut access) = ACCESS.lock() {
        access.insert(email.to_string(), (tokens.access_token.clone(), Instant::now() + lifetime));
    }
}

async fn token_request(provider: &Provider, form: &[(&str, String)]) -> Result<TokenResponse> {
    let response = reqwest::Client::new()
        .post(provider.token_url)
        .form(form)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .with_context(|| format!("contacting {}", provider.name))?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        let detail = serde_json::from_str::<TokenError>(&body)
            .map(|e| format!("{}{}", e.error, e.error_description.map(|d| format!(": {d}")).unwrap_or_default()))
            .unwrap_or(body);
        bail!("{} refused the sign-in ({detail})", provider.name);
    }
    Ok(serde_json::from_str(&body)?)
}

/// Waits for the browser to hit the redirect URL, answers it with a short page, and returns
/// the authorisation code. Requests without our `state` (stray favicon fetches) are ignored.
async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String> {
    loop {
        let (mut socket, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = socket.read(&mut buf).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]);
        let path = request.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("/");
        let url = reqwest::Url::parse(&format!("http://127.0.0.1{path}"))?;
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
        if query.get("state").map(String::as_str) != Some(state) {
            let _ = socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            continue;
        }
        let (title, text, result) = match (query.get("code"), query.get("error")) {
            (Some(code), _) => ("Signed in", "You're signed in. You can close this tab and go back to OxyMail.", Ok(code.clone())),
            (_, Some(error)) => ("Sign-in cancelled", "Sign-in didn't complete. You can close this tab.", Err(anyhow!("sign-in was cancelled ({error})"))),
            _ => ("Sign-in failed", "Something went wrong. You can close this tab.", Err(anyhow!("the browser returned no code"))),
        };
        let page = format!(
            "<!doctype html><meta charset=utf-8><title>OxyMail · {title}</title>\
             <body style=\"font:16px system-ui;display:grid;place-items:center;height:90vh;color:#17202b\">\
             <div style=\"text-align:center\"><h2 style=\"color:#0f5e73\">{title}</h2><p>{text}</p></div>"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
        return result;
    }
}

/// The `email` claim from an ID token. The token came straight from the provider's token
/// endpoint over TLS, so its payload is read without checking the signature.
fn id_token_email(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let json: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    json.get("email").and_then(|e| e.as_str()).map(str::to_string)
}

fn random_token(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| anyhow!("no randomness available: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

fn open_browser(url: &str) -> Result<()> {
    open::that_detached(url).context("opening the web browser")
}

/// The XOAUTH2 SASL string for IMAP and SMTP.
pub fn xoauth2(user: &str, access_token: &str) -> String {
    format!("user={user}\x01auth=Bearer {access_token}\x01\x01")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_email_claim_from_an_id_token() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"email":"me@example.com","sub":"1"}"#);
        assert_eq!(id_token_email(&format!("h.{payload}.s")).as_deref(), Some("me@example.com"));
    }

    #[tokio::test]
    async fn the_redirect_listener_returns_the_code_and_ignores_strays() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let browser = tokio::spawn(async move {
            let get = |path: &'static str| async move {
                let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
                s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes()).await?;
                let mut out = String::new();
                s.read_to_string(&mut out).await?;
                anyhow::Ok(out)
            };
            let stray = get("/favicon.ico").await?;
            let forged = get("/?state=wrong&code=evil").await?;
            let real = get("/?state=s3cret&code=the-code&scope=x").await?;
            anyhow::Ok((stray, forged, real))
        });
        assert_eq!(wait_for_code(&listener, "s3cret").await?, "the-code");
        let (stray, forged, real) = browser.await??;
        assert!(stray.starts_with("HTTP/1.1 404") && forged.starts_with("HTTP/1.1 404"));
        assert!(real.contains("You're signed in"));
        Ok(())
    }

    #[test]
    fn pkce_tokens_are_url_safe() {
        let t = random_token(48).unwrap();
        assert!(t.len() >= 43 && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }
}
