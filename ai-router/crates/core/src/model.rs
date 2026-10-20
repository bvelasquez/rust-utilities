use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::modality::Modality;
use crate::pricing::Pricing;

/// A model you have chosen to expose, or a row in the built-in catalog.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelConfig {
    pub id: String,
    pub provider: String,
    pub upstream: String,
    #[serde(default = "default_chat")]
    pub modality: Modality,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub pricing: Pricing,
    /// Hand-set quality prior in 0–1, used when [`Self::task_quality`] has no entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<f64>,
    /// Per-task quality prior. Key `*` is the fallback inside this map.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub task_quality: BTreeMap<String, f64>,
    /// Expected latency used by cost-aware scoring. Missing means 1000ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms_prior: Option<u64>,
}

fn default_chat() -> Modality {
    Modality::Chat
}

fn default_true() -> bool {
    true
}
