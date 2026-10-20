//! Auto-routing profiles, request summaries, and the signals a router branches on.
//!
//! `RulesRouter` stays synchronous. The gateway fills [`Signals`] before resolve
//! so a hosted classifier never runs inside the registry crate.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Local facts the gateway can see without calling a model.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestSummary {
    pub token_estimate: u64,
    pub has_tools: bool,
    pub has_images: bool,
    pub needs_json: bool,
    pub stream: bool,
    /// Last user turn, already trimmed. Not written to the usage log.
    pub last_user_text: String,
    pub client_label: String,
    /// Top-level `reasoning_effort` from the chat body, when the client sent one.
    pub reasoning_effort: Option<String>,
}

/// Classification used by profile policy. `confidence` is 0–1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signals {
    pub source: String,
    pub task: String,
    pub difficulty: u8,
    pub confidence: f64,
    pub needs_tools: bool,
    pub needs_reasoning: bool,
    pub contains_code: bool,
}

impl Default for Signals {
    fn default() -> Self {
        Self {
            source: "local".into(),
            task: "chat".into(),
            difficulty: 0,
            confidence: 0.5,
            needs_tools: false,
            needs_reasoning: false,
            contains_code: false,
        }
    }
}

/// What `auto` decided, attached to the target and the usage row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteInfo {
    pub profile: String,
    pub reason: String,
    pub confidence: f64,
    pub source: String,
    pub task: String,
    pub difficulty: u8,
    pub needs_tools: bool,
    pub needs_reasoning: bool,
    pub contains_code: bool,
    pub rejected: Vec<String>,
}

impl RouteInfo {
    pub fn signals_json(&self) -> String {
        serde_json::json!({
            "source": self.source,
            "task": self.task,
            "difficulty": self.difficulty,
            "confidence": self.confidence,
            "needs_tools": self.needs_tools,
            "needs_reasoning": self.needs_reasoning,
            "contains_code": self.contains_code,
        })
        .to_string()
    }

    pub fn rejected_json(&self) -> String {
        serde_json::to_string(&self.rejected).unwrap_or_else(|_| "[]".into())
    }
}

/// Next model to try after a retryable upstream failure. Streaming responses
/// that have already started are not failed over.
#[derive(Debug, Clone)]
pub struct Fallback {
    pub provider: String,
    pub upstream_model: String,
    pub client_model: String,
    pub modality: crate::Modality,
    pub pricing: crate::Pricing,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    None,
    Jev,
    Embedding,
}

impl Default for SignalKind {
    fn default() -> Self {
        Self::None
    }
}

/// Hosted TypeSafe classifier. Off unless a profile lists opted-in clients.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JevSettings {
    /// `env:NAME` or `file:PATH`. Never a logged literal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default = "default_jev_url")]
    pub base_url: String,
    #[serde(default = "default_jev_model")]
    pub model: String,
}

impl Default for JevSettings {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: default_jev_url(),
            model: default_jev_model(),
        }
    }
}

fn default_jev_url() -> String {
    "https://api.typesafe.ai".into()
}

fn default_jev_model() -> String {
    "jev-latest".into()
}

/// Local similarity against example utterances. No network call.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct EmbeddingSettings {
    /// Reserved for a future local embedding model id. Unused by the lexical scorer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Task name to example phrases.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub examples: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SignalSettings {
    #[serde(default)]
    pub provider: SignalKind,
    #[serde(default = "default_signal_timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub jev: JevSettings,
    #[serde(default)]
    pub embedding: EmbeddingSettings,
}

impl Default for SignalSettings {
    fn default() -> Self {
        Self {
            provider: SignalKind::None,
            timeout_ms: default_signal_timeout(),
            jev: JevSettings::default(),
            embedding: EmbeddingSettings::default(),
        }
    }
}

fn default_signal_timeout() -> u64 {
    400
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RoutingTier {
    /// Task this tier matches. `*` matches every task.
    #[serde(default = "default_star")]
    pub task: String,
    #[serde(default)]
    pub min_difficulty: u8,
    #[serde(default = "default_max_difficulty")]
    pub max_difficulty: u8,
    pub models: Vec<String>,
}

fn default_star() -> String {
    "*".into()
}

fn default_max_difficulty() -> u8 {
    4
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RoutingProfile {
    /// Used when classification confidence is below [`Self::confidence_floor`].
    pub safe_model: String,
    #[serde(default)]
    pub tiers: Vec<RoutingTier>,
    #[serde(default = "default_floor")]
    pub confidence_floor: f64,
    /// Weight on estimated USD cost in the score `quality - lambda*cost - mu*latency_seconds`.
    #[serde(default = "default_lambda")]
    pub lambda: f64,
    /// Weight on the model's latency prior, in seconds.
    #[serde(default = "default_mu")]
    pub mu: f64,
    /// Empty means every configured provider is eligible.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_providers: Vec<String>,
    /// Client labels allowed to send prompt text to a hosted signal provider.
    /// Empty means hosted signals stay off for this profile.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosted_signal_clients: Vec<String>,
}

fn default_floor() -> f64 {
    0.5
}

fn default_lambda() -> f64 {
    10.0
}

fn default_mu() -> f64 {
    0.02
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RoutingConfig {
    #[serde(default = "default_routing_enabled")]
    pub enabled: bool,
    /// Profile used for a bare `auto` model name.
    #[serde(default = "default_profile_name")]
    pub default_profile: String,
    #[serde(default)]
    pub profiles: BTreeMap<String, RoutingProfile>,
    #[serde(default)]
    pub signals: SignalSettings,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            enabled: default_routing_enabled(),
            default_profile: default_profile_name(),
            profiles: BTreeMap::new(),
            signals: SignalSettings::default(),
        }
    }
}

fn default_routing_enabled() -> bool {
    true
}

fn default_profile_name() -> String {
    "default".into()
}

/// `auto` or `auto/<profile>`. Nested suffixes are rejected by the caller.
pub fn auto_profile<'a>(model: &'a str, default_profile: &'a str) -> Option<&'a str> {
    if model == "auto" {
        Some(default_profile)
    } else if let Some(rest) = model.strip_prefix("auto/") {
        if rest.is_empty() || rest.contains('/') {
            None
        } else {
            Some(rest)
        }
    } else {
        None
    }
}
