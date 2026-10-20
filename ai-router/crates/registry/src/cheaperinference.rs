//! Models from Cheaper Inference's `GET /v1/models`.
//!
//! That endpoint requires an API key; the CLI only calls it for a configured
//! `cheaperinference` provider. Prices are already customer-facing USD per
//! million tokens (the standard scope), so they are used as-is. Tiered prices
//! (`input_token_price_threshold` / `above_threshold`) and per-resolution media
//! prices are not applied.
//!
//! The catalog does not carry tool-calling support, so no `tools` capability is
//! inferred. Set it on a model if a specific upstream accepts tools.

use airouter_core::{Modality, ModelConfig, Pricing};
use serde_json::Value;

const LIST_URL: &str = "https://api.cheaperinference.com/v1/models";

pub fn list_url() -> &'static str {
    LIST_URL
}

pub fn models_from_list(body: &Value) -> Vec<ModelConfig> {
    let Some(items) = body.get("data").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    items.iter().filter_map(model_from_entry).collect()
}

fn model_from_entry(entry: &Value) -> Option<ModelConfig> {
    let upstream = entry.get("id").and_then(|v| v.as_str())?.trim().to_string();
    if upstream.is_empty() {
        return None;
    }
    let endpoint = entry.get("endpoint").and_then(|v| v.as_str()).unwrap_or("");
    let modality = modality_for(entry.get("type")?.as_str()?, endpoint)?;
    let pricing = entry
        .get("pricing")
        .map(|p| pricing_for(p, modality))
        .unwrap_or_default();
    let caps = entry.get("capabilities");
    let mut capabilities = Vec::new();
    if modality == Modality::Chat {
        if cap_bool(caps, "streaming") {
            capabilities.push("streaming".into());
        }
        if cap_bool(caps, "vision") {
            capabilities.push("vision".into());
        }
        if cap_bool(caps, "reasoning") {
            capabilities.push("reasoning".into());
        }
    }
    let context = entry
        .get("context_length")
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0);
    Some(ModelConfig {
        id: format!("cheaperinference/{upstream}"),
        provider: "cheaperinference".into(),
        upstream,
        modality,
        enabled: true,
        context,
        capabilities,
        pricing,
        quality: None,
        task_quality: Default::default(),
        latency_ms_prior: None,
    })
}

fn cap_bool(caps: Option<&Value>, key: &str) -> bool {
    caps.and_then(|c| c.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn modality_for(kind: &str, endpoint: &str) -> Option<Modality> {
    match kind {
        "text" if endpoint == "/v1/decisions" => None,
        "text" => Some(Modality::Chat),
        "image" => Some(Modality::Image),
        "video" => Some(Modality::Video),
        _ => None,
    }
}

fn pricing_for(pricing: &Value, modality: Modality) -> Pricing {
    let mut out = Pricing::default();
    match modality {
        Modality::Chat => {
            out.input_per_mtok = price(pricing, "input_per_million");
            out.output_per_mtok = price(pricing, "output_per_million");
            out.cache_read_per_mtok = price(pricing, "cache_read_input_per_million");
            out.cache_write_per_mtok = price(pricing, "cache_write_input_per_million");
        }
        Modality::Image => {
            out.per_image = price(pricing, "media_unit_price");
        }
        Modality::Video => {
            // The catalog prices video per second; airouter's time unit is a minute.
            out.per_minute = price(pricing, "media_unit_price").map(|v| round_price(v * 60.0));
        }
        _ => {}
    }
    out
}

/// Token and media prices arrive as strings, but accept a bare number too.
fn price(pricing: &Value, key: &str) -> Option<f64> {
    let value = pricing.get(key)?;
    let parsed = value
        .as_str()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .or_else(|| value.as_f64())?;
    Some(round_price(parsed))
}

fn round_price(value: f64) -> f64 {
    (value * 1e8).round() / 1e8
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn text_model_prices_and_capabilities() {
        let body = json!({"data": [{
            "id": "claude-sonnet-5.5",
            "type": "text",
            "endpoint": "/v1/chat/completions",
            "context_length": 200000,
            "capabilities": {"streaming": true, "vision": true, "reasoning": true},
            "pricing": {
                "input_per_million": "3.000000",
                "output_per_million": "15.000000",
                "cache_read_input_per_million": "0.300000",
                "cache_write_input_per_million": "3.750000"
            }
        }]});
        let models = models_from_list(&body);
        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.id, "cheaperinference/claude-sonnet-5.5");
        assert_eq!(model.upstream, "claude-sonnet-5.5");
        assert_eq!(model.modality, Modality::Chat);
        assert_eq!(model.context, Some(200_000));
        assert_eq!(model.pricing.input_per_mtok, Some(3.0));
        assert_eq!(model.pricing.output_per_mtok, Some(15.0));
        assert_eq!(model.pricing.cache_read_per_mtok, Some(0.3));
        assert_eq!(model.pricing.cache_write_per_mtok, Some(3.75));
        assert!(!model.capabilities.iter().any(|c| c == "tools"));
        assert!(model.capabilities.iter().any(|c| c == "vision"));
        assert!(model.capabilities.iter().any(|c| c == "reasoning"));
        assert!(model.capabilities.iter().any(|c| c == "streaming"));
    }

    #[test]
    fn image_model_uses_media_unit_price() {
        let body = json!({"data": [{
            "id": "nano-banana",
            "type": "image",
            "endpoint": "/v1/images/generations",
            "capabilities": {"vision": true, "image_generation": true},
            "pricing": {"media_unit": "image", "media_unit_price": "0.030560"}
        }]});
        let model = &models_from_list(&body)[0];
        assert_eq!(model.modality, Modality::Image);
        assert_eq!(model.pricing.per_image, Some(0.03056));
    }

    #[test]
    fn video_prices_are_converted_to_a_minute() {
        let body = json!({"data": [{
            "id": "seedance-2.0",
            "type": "video",
            "endpoint": "/v1/videos/generations",
            "pricing": {"media_unit": "second", "media_unit_price": "0.318070"}
        }]});
        let model = &models_from_list(&body)[0];
        assert_eq!(model.modality, Modality::Video);
        assert_eq!(model.pricing.per_minute, Some(19.0842));
    }

    #[test]
    fn decision_models_are_skipped() {
        let body = json!({"data": [{
            "id": "jev-1.13",
            "type": "text",
            "endpoint": "/v1/decisions",
            "pricing": {"input_per_million": "0.027300", "output_per_million": "0.000000"}
        }]});
        assert!(models_from_list(&body).is_empty());
    }

    #[test]
    fn empty_or_unexpected_bodies_yield_nothing() {
        assert!(models_from_list(&json!({})).is_empty());
        assert!(models_from_list(&json!({"data": []})).is_empty());
        assert!(models_from_list(&json!([{"id": "x"}])).is_empty());
    }
}
