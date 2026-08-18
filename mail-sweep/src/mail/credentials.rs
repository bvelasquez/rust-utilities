/// IMAP/SMTP login material (app password or OAuth access token).
#[derive(Debug, Clone)]
pub enum MailCredentials {
    Password(String),
    OAuthAccessToken(String),
}

impl MailCredentials {
    pub fn password(value: String) -> Self {
        Self::Password(value)
    }

    pub fn oauth(access_token: String) -> Self {
        Self::OAuthAccessToken(access_token)
    }

    pub fn is_oauth(&self) -> bool {
        matches!(self, Self::OAuthAccessToken(_))
    }

    pub fn secret(&self) -> &str {
        match self {
            Self::Password(p) | Self::OAuthAccessToken(p) => p.as_str(),
        }
    }
}
