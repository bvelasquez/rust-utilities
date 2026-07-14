use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Openrouter,
    Anthropic,
    Openai,
    Cursor,
}

impl Provider {
    pub fn all() -> [Provider; 4] {
        [
            Provider::Openrouter,
            Provider::Anthropic,
            Provider::Openai,
            Provider::Cursor,
        ]
    }

    pub fn docs_url(self) -> &'static str {
        match self {
            Provider::Openrouter => "https://openrouter.ai/docs/api/api-reference/analytics",
            Provider::Anthropic => {
                "https://platform.claude.com/docs/en/manage-claude/usage-cost-api"
            }
            Provider::Openai => "https://developers.openai.com/api/docs/guides/admin-apis",
            Provider::Cursor => "https://cursor.com/docs/account/teams/admin-api",
        }
    }

    pub fn key_hint(self) -> &'static str {
        match self {
            Provider::Openrouter => "management key (not regular inference key)",
            Provider::Anthropic => "admin key (sk-ant-admin01-...)",
            Provider::Openai => "organization admin API key",
            Provider::Cursor => "Cursor Admin API key (Teams/Enterprise; dashboard → Settings → Admin API)",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provider::Openrouter => write!(f, "openrouter"),
            Provider::Anthropic => write!(f, "anthropic"),
            Provider::Openai => write!(f, "openai"),
            Provider::Cursor => write!(f, "cursor"),
        }
    }
}

impl FromStr for Provider {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openrouter" | "or" => Ok(Provider::Openrouter),
            "anthropic" | "claude" => Ok(Provider::Anthropic),
            "openai" | "oa" => Ok(Provider::Openai),
            "cursor" => Ok(Provider::Cursor),
            other => anyhow::bail!("unknown provider: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageBucket {
    pub provider: Provider,
    pub bucket_start: DateTime<Utc>,
    pub granularity: String,
    pub model: Option<String>,
    pub cost_usd: f64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub request_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderTestResult {
    pub provider: Provider,
    pub ok: bool,
    pub message: String,
    pub key_type_hint: String,
    pub docs_url: String,
}
