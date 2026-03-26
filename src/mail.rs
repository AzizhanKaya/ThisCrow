#![cfg(feature = "mail")]

use crate::DOMAIN;
use anyhow::Result;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};
use once_cell::sync::Lazy;
use std::env;

static MAILER: Lazy<AsyncSmtpTransport<Tokio1Executor>> = Lazy::new(|| {
    let smtp_user = format!("info@{}", *DOMAIN);
    let smtp_pass = env::var("SMTP_PASSWORD").expect("SMTP_PASSWORD must be set");

    let creds = Credentials::new(smtp_user, smtp_pass);

    AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&format!("mail.{}", *DOMAIN))
        .expect("Geçersiz SMTP host")
        .port(587)
        .credentials(creds)
        .build()
});

pub async fn send_email(email_to: &str, subject: &str, content: String) -> Result<()> {
    let email = Message::builder()
        .from(format!("ThisCrow <info@{}>", *DOMAIN).parse()?)
        .to(email_to.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(content)?;

    MAILER.send(email).await?;

    Ok(())
}
