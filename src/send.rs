use crate::config::SmtpIdentityConfig;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendEnvelope {
    pub from: String,
    pub host: String,
    pub port: u16,
    pub tls_mode: Option<String>,
}

pub fn build_envelope(identity: &SmtpIdentityConfig) -> AppResult<SendEnvelope> {
    if identity.host.trim().is_empty() {
        return Err(AppError::Send("smtp host must not be empty".to_string()));
    }
    if identity.email_address.trim().is_empty() {
        return Err(AppError::Send(
            "smtp email_address must not be empty".to_string(),
        ));
    }

    Ok(SendEnvelope {
        from: identity.email_address.clone(),
        host: identity.host.clone(),
        port: identity.port,
        tls_mode: identity.tls_mode.clone(),
    })
}

pub fn send_draft(
    identity: &SmtpIdentityConfig,
    _to: &[String],
    _subject: &str,
    _body_text: &str,
) -> AppResult<()> {
    let _ = build_envelope(identity)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::SmtpIdentityConfig;

    use super::build_envelope;

    #[test]
    fn builds_transport_from_identity() {
        let identity = SmtpIdentityConfig {
            name: "default".to_string(),
            email_address: "ash@example.com".to_string(),
            host: "smtp.example.com".to_string(),
            port: 465,
            username: Some("ash@example.com".to_string()),
            password_env: Some("MAILFOR_SMTP_PASSWORD".to_string()),
            tls_mode: Some("tls".to_string()),
            default: Some(true),
        };

        let envelope = build_envelope(&identity).expect("build envelope");
        assert_eq!(envelope.host, "smtp.example.com");
        assert_eq!(envelope.port, 465);
        assert_eq!(envelope.from, "ash@example.com");
    }
}
