use std::env;

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};
use once_cell::sync::Lazy;

static MAILER: Lazy<AsyncSmtpTransport<Tokio1Executor>> = Lazy::new(|| {
    let smtp_user = "info@vate.world".to_string();
    let smtp_pass = env::var("SMTP_PASSWORD").expect("SMTP_PASSWORD must be set");

    let creds = Credentials::new(smtp_user, smtp_pass);

    AsyncSmtpTransport::<Tokio1Executor>::starttls_relay("mail.vate.world")
        .expect("Geçersiz SMTP host")
        .port(587)
        .credentials(creds)
        .build()
});

pub async fn send_email(
    email_to: &str,
    subject: &str,
    content: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let email = Message::builder()
        .from("Vate <info@vate.world>".parse()?)
        .to(email_to.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(content)?;

    MAILER.send(email).await?;

    Ok(())
}
