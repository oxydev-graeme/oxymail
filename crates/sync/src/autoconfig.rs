//! Finds IMAP and SMTP settings for an email address, the way Thunderbird does:
//! the provider's own autoconfig file, then Mozilla's ISP database (by domain, then by the
//! domain's MX host), then common host names.

use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Security {
    /// TLS from the first byte (IMAP 993, SMTP 465).
    Tls,
    /// Plain connection upgraded with STARTTLS (IMAP 143, SMTP 587).
    StartTls,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    pub host: String,
    pub port: u16,
    pub security: Security,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountConfig {
    pub email: String,
    pub username: String,
    pub imap: Server,
    pub smtp: Server,
    /// Where the settings came from, shown to the user.
    pub source: String,
    /// Set when the provider supports OAuth sign-in (`google`, `microsoft`).
    pub oauth: Option<String>,
    /// How to sign in: `None` for a password, or the provider id when signed in with OAuth.
    #[serde(default)]
    pub auth: Option<String>,
}

pub async fn discover(email: &str) -> Result<AccountConfig> {
    let email = email.trim().to_lowercase();
    let Some((_, domain)) = email.split_once('@') else {
        anyhow::bail!("that doesn't look like an email address");
    };
    let http = reqwest::Client::builder().timeout(Duration::from_secs(8)).build()?;

    let candidates = [
        (format!("https://autoconfig.{domain}/mail/config-v1.1.xml?emailaddress={email}"), "the provider's autoconfig"),
        (format!("https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml?emailaddress={email}"), "the provider's autoconfig"),
        (format!("https://autoconfig.thunderbird.net/v1.1/{domain}"), "Mozilla's ISP database"),
    ];
    for (url, source) in &candidates {
        if let Some(config) = fetch(&http, url, &email, source).await {
            return Ok(config);
        }
    }

    // Domains hosted by Google Workspace or Microsoft 365 are found through their MX host.
    if let Some(mx_domain) = mx_base_domain(domain).await {
        if mx_domain != domain {
            let url = format!("https://autoconfig.thunderbird.net/v1.1/{mx_domain}");
            if let Some(mut config) = fetch(&http, &url, &email, "Mozilla's ISP database (via MX)").await {
                config.username = email.clone();
                return Ok(config);
            }
        }
    }

    Ok(guess(&email, domain))
}

async fn fetch(http: &reqwest::Client, url: &str, email: &str, source: &str) -> Option<AccountConfig> {
    let response = http.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let xml = response.text().await.ok()?;
    parse(&xml, email, source)
}

/// Parses a Thunderbird `clientConfig` document, preferring TLS over STARTTLS.
pub fn parse(xml: &str, email: &str, source: &str) -> Option<AccountConfig> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let (local, domain) = email.split_once('@')?;
    let fill = |s: &str| {
        s.replace("%EMAILADDRESS%", email)
            .replace("%EMAILLOCALPART%", local)
            .replace("%EMAILDOMAIN%", domain)
    };

    let pick = |tag: &str, kind: &str| -> Option<(Server, String, bool)> {
        let mut best: Option<(Server, String, bool, u8)> = None;
        for node in doc.descendants().filter(|n| n.has_tag_name(tag) && n.attribute("type") == Some(kind)) {
            let text = |name: &str| node.children().find(|c| c.has_tag_name(name)).and_then(|c| c.text()).map(str::trim);
            let security = match text("socketType")? {
                "SSL" => Security::Tls,
                "STARTTLS" => Security::StartTls,
                _ => continue,
            };
            let oauth = node
                .children()
                .filter(|c| c.has_tag_name("authentication"))
                .any(|c| c.text().map(str::trim) == Some("OAuth2"));
            let server = Server { host: fill(text("hostname")?), port: text("port")?.parse().ok()?, security };
            let rank = if security == Security::Tls { 2 } else { 1 };
            if best.as_ref().is_none_or(|b| rank > b.3) {
                best = Some((server, fill(text("username").unwrap_or("%EMAILADDRESS%")), oauth, rank));
            }
        }
        best.map(|(s, u, o, _)| (s, u, o))
    };

    let (imap, username, imap_oauth) = pick("incomingServer", "imap")?;
    let (smtp, _, _) = pick("outgoingServer", "smtp")?;
    let oauth = imap_oauth.then(|| oauth_provider(&imap.host)).flatten();
    Some(AccountConfig { email: email.to_string(), username, imap, smtp, source: source.to_string(), oauth, auth: None })
}

fn oauth_provider(host: &str) -> Option<String> {
    if host.ends_with("gmail.com") || host.ends_with("googlemail.com") {
        Some("google".into())
    } else if host.contains("office365.com") || host.contains("outlook.com") {
        Some("microsoft".into())
    } else {
        None
    }
}

/// The registrable part of the domain's first MX host, e.g. `google.com` for
/// `aspmx.l.google.com`.
async fn mx_base_domain(domain: &str) -> Option<String> {
    let resolver = hickory_resolver::Resolver::builder_tokio().ok()?.build().ok()?;
    let lookup = resolver.mx_lookup(domain).await.ok()?;
    let host = lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            hickory_resolver::proto::rr::RData::MX(mx) => Some((mx.preference, mx.exchange.to_utf8())),
            _ => None,
        })
        .min()?
        .1;
    let labels: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    (labels.len() >= 2).then(|| labels[labels.len() - 2..].join("."))
}

/// Last resort: the usual `mail.` host with standard TLS ports. The user can correct it.
fn guess(email: &str, domain: &str) -> AccountConfig {
    let host = format!("mail.{domain}");
    AccountConfig {
        email: email.to_string(),
        username: email.to_string(),
        imap: Server { host: host.clone(), port: 993, security: Security::Tls },
        smtp: Server { host, port: 465, security: Security::Tls },
        source: "a guess (please check)".to_string(),
        oauth: None,
        auth: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_thunderbird_config_preferring_tls() {
        let xml = r#"<clientConfig version="1.1"><emailProvider id="example.com">
          <incomingServer type="imap"><hostname>imap.example.com</hostname><port>143</port>
            <socketType>STARTTLS</socketType><username>%EMAILLOCALPART%</username></incomingServer>
          <incomingServer type="imap"><hostname>imap.example.com</hostname><port>993</port>
            <socketType>SSL</socketType><username>%EMAILADDRESS%</username></incomingServer>
          <outgoingServer type="smtp"><hostname>smtp.example.com</hostname><port>587</port>
            <socketType>STARTTLS</socketType></outgoingServer>
        </emailProvider></clientConfig>"#;
        let c = parse(xml, "me@example.com", "test").unwrap();
        assert_eq!(c.imap, Server { host: "imap.example.com".into(), port: 993, security: Security::Tls });
        assert_eq!(c.username, "me@example.com");
        assert_eq!(c.smtp.port, 587);
        assert_eq!(c.oauth, None);
    }
}
