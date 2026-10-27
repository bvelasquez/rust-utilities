//! Auto-routing profiles, request summaries, and the signals a router branches on.
//!
//! `RulesRouter` stays synchronous. The gateway fills [`Signals`] before resolve
//! so a hosted classifier never runs inside the registry crate.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::pricing::Pricing;
use crate::ModelConfig;

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
    /// `budget`, `balanced`, `quality`, `speed`, or empty when the profile uses the legacy score.
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub required_quality: f64,
    #[serde(default)]
    pub selected_quality: f64,
    #[serde(default)]
    pub estimated_cost_usd: f64,
    #[serde(default)]
    pub selection_reason: String,
    /// `single`, `worker`, `full`, or empty when MoA was not considered.
    #[serde(default)]
    pub moa_mode: String,
    /// True when `selected_quality` came from `task_quality` or `quality`, not the 0.5 default.
    #[serde(default)]
    pub explicit_quality_prior: bool,
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

/// How an auto profile turns candidates into a winner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    Budget,
    Balanced,
    Quality,
    Speed,
}

impl RoutingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Budget => "budget",
            Self::Balanced => "balanced",
            Self::Quality => "quality",
            Self::Speed => "speed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "budget" => Some(Self::Budget),
            "balanced" => Some(Self::Balanced),
            "quality" => Some(Self::Quality),
            "speed" => Some(Self::Speed),
            _ => None,
        }
    }
}

/// `single` keeps one model. `moa_on_demand` may fan out after selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoutingStrategy {
    #[default]
    Single,
    MoaOnDemand,
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
    /// Unset keeps the legacy score. Set this to opt into economic selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RoutingMode>,
    /// Per-task quality floor. Missing tasks use the built-in base.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub required_quality: BTreeMap<String, f64>,
    /// Added once per difficulty level. Unset uses the built-in per-task step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty_step: Option<f64>,
    /// How far to lower the quality floor when nothing clears it.
    #[serde(default = "default_relax")]
    pub relax_step: f64,
    /// Hard cap on estimated USD for one selected call. MoA has its own cap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
    /// Hard cap on the latency prior, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_latency_ms: Option<u64>,
    #[serde(default)]
    pub strategy: RoutingStrategy,
    /// Key into `moa.profiles` when [`Self::strategy`] is `moa_on_demand`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moa_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moa_triggers: Option<MoaTriggers>,
}

/// Bootstrap and budget gates for on-demand mixture-of-agents.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MoaTriggers {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_difficulty: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_token_estimate: Option<u64>,
    /// Added to required quality. The single model must clear that higher bar.
    #[serde(default = "default_quality_margin")]
    pub quality_margin: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub high_value_clients: Vec<String>,
    /// When set, MoA is suppressed once classification confidence is above this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_confidence: Option<f64>,
    /// Hard cap on the estimated N+1 cost. Unset means no MoA budget gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
}

impl Default for MoaTriggers {
    fn default() -> Self {
        Self {
            tasks: Vec::new(),
            min_difficulty: None,
            min_token_estimate: None,
            quality_margin: default_quality_margin(),
            high_value_clients: Vec::new(),
            max_confidence: None,
            max_cost_usd: None,
        }
    }
}

fn default_quality_margin() -> f64 {
    0.10
}

impl Default for RoutingProfile {
    fn default() -> Self {
        Self {
            safe_model: String::new(),
            tiers: Vec::new(),
            confidence_floor: default_floor(),
            lambda: default_lambda(),
            mu: default_mu(),
            allow_providers: Vec::new(),
            hosted_signal_clients: Vec::new(),
            mode: None,
            required_quality: BTreeMap::new(),
            difficulty_step: None,
            relax_step: default_relax(),
            max_cost_usd: None,
            max_latency_ms: None,
            strategy: RoutingStrategy::Single,
            moa_profile: None,
            moa_triggers: None,
        }
    }
}

fn default_relax() -> f64 {
    0.05
}

/// Built-in quality floor before difficulty is applied.
pub fn builtin_quality_base(task: &str) -> f64 {
    match task {
        "chat" => 0.35,
        "extraction" => 0.45,
        "writing" => 0.45,
        "code" => 0.50,
        "reasoning" => 0.55,
        _ => 0.40,
    }
}

pub fn builtin_quality_step(task: &str) -> f64 {
    match task {
        "chat" => 0.05,
        "extraction" => 0.08,
        "writing" => 0.07,
        "code" => 0.10,
        "reasoning" => 0.10,
        _ => 0.05,
    }
}

/// `base[task] + step[task] * difficulty`, clamped to 0–1.
pub fn required_quality(task: &str, difficulty: u8, profile: &RoutingProfile) -> f64 {
    let base = profile
        .required_quality
        .get(task)
        .or_else(|| profile.required_quality.get("*"))
        .copied()
        .unwrap_or_else(|| builtin_quality_base(task));
    let step = profile
        .difficulty_step
        .unwrap_or_else(|| builtin_quality_step(task));
    (base + step * f64::from(difficulty)).clamp(0.0, 1.0)
}

/// `(quality, explicit)`. Explicit is false only for the 0.5 default.
pub fn model_task_quality(model: &ModelConfig, task: &str) -> (f64, bool) {
    if let Some(q) = model
        .task_quality
        .get(task)
        .or_else(|| model.task_quality.get("*"))
        .copied()
    {
        return (q, true);
    }
    if let Some(q) = model.quality {
        return (q, true);
    }
    (0.5, false)
}

/// Expected completion length from the task, used instead of `tokens / 4`.
pub fn estimate_output_tokens(task: &str, input_tokens: u64) -> u64 {
    let ratio = match task {
        "extraction" => 0.25,
        "chat" => 0.40,
        "code" => 0.80,
        "reasoning" => 1.0,
        "writing" => 1.25,
        _ => 0.50,
    };
    ((input_tokens as f64) * ratio).round().max(32.0) as u64
}

pub fn estimate_text_cost(pricing: &Pricing, input_tokens: u64, output_tokens: u64) -> f64 {
    let input_price = pricing.input_per_mtok.unwrap_or(0.0);
    let output_price = pricing.output_per_mtok.unwrap_or(0.0);
    (input_tokens as f64 * input_price + output_tokens as f64 * output_price) / 1_000_000.0
}

pub fn latency_prior_ms(model: &ModelConfig) -> (u64, bool) {
    match model.latency_ms_prior {
        Some(ms) => (ms, false),
        None => (1_000, true),
    }
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
