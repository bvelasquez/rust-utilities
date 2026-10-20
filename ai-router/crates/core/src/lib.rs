//! Shared types for the personal AI router.
//!
//! A client names a model (`provider/model` or an alias) and the gateway
//! dispatches to that provider. `auto` and `auto/<profile>` are resolved by
//! the rules router when a routing profile is configured.

mod error;
mod modality;
mod model;
mod pricing;
mod routing;
mod usage;

pub use error::Error;
pub use modality::Modality;
pub use model::ModelConfig;
pub use pricing::Pricing;
pub use routing::{
    auto_profile, EmbeddingSettings, Fallback, JevSettings, RequestSummary, RouteInfo,
    RoutingConfig, RoutingProfile, RoutingTier, SignalKind, SignalSettings, Signals,
};
pub use usage::{compute_cost, SseUsageParser, TokenUsage, UsageMeta};

/// What the caller is asking the router to do. Handlers set this from the endpoint.
#[derive(Debug, Clone)]
pub struct RouteRequest {
    pub model: String,
    pub modality: Modality,
    pub summary: RequestSummary,
    /// Filled by the gateway before resolve. `None` means the router derives local signals.
    pub signals: Option<Signals>,
}

impl RouteRequest {
    pub fn explicit(model: impl Into<String>, modality: Modality) -> Self {
        Self {
            model: model.into(),
            modality,
            summary: RequestSummary::default(),
            signals: None,
        }
    }
}

/// A resolved upstream call.
#[derive(Debug, Clone)]
pub struct Target {
    pub provider: String,
    pub upstream_model: String,
    pub client_model: String,
    pub modality: Modality,
    pub pricing: Pricing,
    pub capabilities: Vec<String>,
    pub route: Option<RouteInfo>,
    pub fallbacks: Vec<Fallback>,
}

/// Finished (or abandoned) request, stored and shown in the live log.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RequestRecord {
    pub id: String,
    pub ts: i64,
    pub client_label: String,
    pub requested_model: String,
    pub provider: String,
    pub upstream_model: String,
    pub modality: String,
    pub status: u16,
    pub error: Option<String>,
    pub latency_ms: u64,
    pub ttft_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub cost_usd: f64,
    pub endpoint: String,
    /// Set when the request was resolved through `auto` or `auto/<profile>`.
    #[serde(default)]
    pub route_profile: String,
    #[serde(default)]
    pub route_reason: String,
    #[serde(default)]
    pub route_confidence: Option<f64>,
    #[serde(default)]
    pub route_signals: String,
    #[serde(default)]
    pub route_rejected: String,
    /// `1` thumbs up, `-1` thumbs down, absent when the client has not rated the call.
    #[serde(default)]
    pub feedback: Option<i8>,
}

/// Dashboard payload shared by the admin API and the TUI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DashboardSnapshot {
    pub listen: String,
    pub admin_listen: String,
    pub uptime_secs: u64,
    pub requests_total: u64,
    pub requests_last_minute: u64,
    pub error_rate: f64,
    pub spend_today_usd: f64,
    pub spend_month_usd: f64,
    pub providers: Vec<ProviderView>,
    pub models: Vec<ModelView>,
    pub recent: Vec<RequestRecord>,
    pub usage_by_model: Vec<UsageRow>,
    pub usage_by_day: Vec<UsageRow>,
    pub config_path: String,
    pub config_loaded_at: i64,
    pub config_warnings: Vec<String>,
    pub config_error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderView {
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub enabled: bool,
    pub key_present: bool,
    pub last_status: Option<u16>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelView {
    pub id: String,
    pub provider: String,
    pub upstream: String,
    pub modality: String,
    pub enabled: bool,
    pub context: Option<u64>,
    pub capabilities: Vec<String>,
    pub input_per_mtok: Option<f64>,
    pub output_per_mtok: Option<f64>,
    pub per_image: Option<f64>,
    pub per_1k_chars: Option<f64>,
    pub per_minute: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UsageRow {
    pub key: String,
    pub requests: u64,
    pub errors: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// OpenAI-style error body.
pub fn openai_error_body(error_type: &str, message: &str, code: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "error": {
            "message": message,
            "type": error_type,
            "code": code,
        }
    })
}

/// Best-effort removal of credential-looking substrings from error text.
pub fn redact(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find("Bearer ") {
        out.push_str(&rest[..idx]);
        out.push_str("Bearer [redacted]");
        let after = &rest[idx + "Bearer ".len()..];
        let skip = after
            .find(|c: char| c.is_whitespace())
            .unwrap_or(after.len());
        rest = &after[skip..];
    }
    out.push_str(rest);
    if out.len() > 2000 {
        out.truncate(2000);
        out.push_str("…");
    }
    out
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
