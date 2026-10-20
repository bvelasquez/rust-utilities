use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Prices in USD. Token prices are per million tokens.
/// Missing token prices mean "unknown" and contribute 0, except cache tokens
/// which fall back to the matching input price so cached traffic is not free by accident.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Pricing {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_per_mtok: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_image: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_1k_chars: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_minute: Option<f64>,
}
