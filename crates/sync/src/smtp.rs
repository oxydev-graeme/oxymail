//! Building and sending outgoing mail.

use anyhow::{Context, Result, bail};
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address as MailAddress;
use mail_builder::headers::message_id::MessageId;
use serde::Deserialize;

use crate::autoconfig::{AccountConfig, Security};

#[derive(Debug, Deserialize)]
pub struct OutgoingAttachment {
    pub name: String,
    pub content_type: String,
    /// Base64 file contents, as read by the UI.
    pub data: String,
}

#[derive(Debug, Deserialize)]
pub struct Outgoing {
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub html: String,
    pub text: String,
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<OutgoingAttachment>,
}

/// Builds the RFC 5322 message. Bcc is left out of the headers; it only goes in the envelope.
pub fn build(from_name: Option<&str>, from_email: &str, mail: &Outgoing) -> Result<Vec<u8>> {
    use base64::Engine;
    let addresses = |list: &[String]| -> Vec<MailAddress<'static>> {
        list.iter().map(|a| MailAddress::new_address(None::<String>, a.trim().to_string())).collect()
    };
    let domain = from_email.split('@').nth(1).unwrap_or("localhost");
    let mut builder = MessageBuilder::new()
        .from(MailAddress::new_address(from_name.map(str::to_string), from_email.to_string()))
        .subject(mail.subject.clone())
        .message_id(format!("oxymail.{}.{}@{domain}", std::process::id(), now_nanos()))
        .text_body(mail.text.clone())
        .html_body(mail.html.clone())
        .header("User-Agent", mail_builder::headers::raw::Raw::new("OxyMail"));
    if !mail.to.is_empty() {
        builder = builder.to(MailAddress::new_list(addresses(&mail.to)));
    }
    if !mail.cc.is_empty() {
        builder = builder.cc(MailAddress::new_list(addresses(&mail.cc)));
    }
    if let Some(parent) = &mail.in_reply_to {
        builder = builder.in_reply_to(strip(parent));
    }
    if !mail.references.is_empty() {
        builder = builder.references(MessageId::from(mail.references.iter().map(|r| strip(r)).collect::<Vec<_>>()));
    }
    for a in &mail.attachments {
        let data = base64::engine::general_purpose::STANDARD.decode(&a.data).context("attachment data")?;
        builder = builder.attachment(a.content_type.clone(), a.name.clone(), data);
    }
    Ok(builder.write_to_vec()?)
}

/// Sends `raw` to every To, Cc and Bcc recipient.
pub async fn send(config: &AccountConfig, password: &str, mail: &Outgoing, raw: &[u8]) -> Result<()> {
    let recipients: Vec<Address> = mail
        .to
        .iter()
        .chain(&mail.cc)
        .chain(&mail.bcc)
        .map(|a| a.trim().parse::<Address>().with_context(|| format!("“{a}” isn't a valid address")))
        .collect::<Result<_>>()?;
    if recipients.is_empty() {
        bail!("add at least one recipient");
    }
    let envelope = Envelope::new(Some(config.email.parse()?), recipients)?;
    let builder = match config.smtp.security {
        Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp.host)?,
        Security::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp.host)?,
    };
    let transport: AsyncSmtpTransport<Tokio1Executor> = builder
        .port(config.smtp.port)
        .credentials(Credentials::new(config.username.clone(), password.to_string()))
        .timeout(Some(std::time::Duration::from_secs(60)))
        .build();
    transport.send_raw(&envelope, raw).await.context("the server refused the message")?;
    Ok(())
}

/// Checks the SMTP login without sending anything.
pub async fn test_login(config: &AccountConfig, password: &str) -> Result<()> {
    let builder = match config.smtp.security {
        Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp.host)?,
        Security::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp.host)?,
    };
    let transport: AsyncSmtpTransport<Tokio1Executor> = builder
        .port(config.smtp.port)
        .credentials(Credentials::new(config.username.clone(), password.to_string()))
        .timeout(Some(std::time::Duration::from_secs(30)))
        .build();
    if !transport.test_connection().await.context("SMTP login failed")? {
        bail!("the SMTP server didn't accept the connection");
    }
    Ok(())
}

fn strip(id: &str) -> String {
    id.trim().trim_start_matches('<').trim_end_matches('>').to_string()
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}
