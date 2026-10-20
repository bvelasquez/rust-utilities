//! Prices from DeepInfra's public `GET /models/list`.
//!
//! Token prices there are cents per token. Cache fields are multipliers of the
//! input price. Priority and flex tiers are not applied; these are standard rates.

use airouter_core::{Modality, ModelConfig, Pricing};
use serde_json::Value;

const LIST_URL: &str = "https://api.deepinfra.com/models/list";

pub fn list_url() -> &'static str {
    LIST_URL
}

pub fn models_from_list(body: &Value) -> Vec<ModelConfig> {
    let Some(items) = body.as_array() else {
        return Vec::new();
    };
    items.iter().filter_map(model_from_entry).collect()
}

fn model_from_entry(entry: &Value) -> Option<ModelConfig> {
    if entry.get("deprecated").and_then(|v| v.as_i64()).is_some() {
        return None;
    }
    if entry.get("private").and_then(|v| v.as_i64()).unwrap_or(0) != 0 {
        return None;
    }
    let upstream = entry.get("model_name")?.as_str()?.to_string();
    if upstream.is_empty() {
        return None;
    }
    let kind = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let modality = modality_for(kind)?;
    let pricing = pricing_for(entry.get("pricing")?, modality)?;
    let tags = entry
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|tags| {
            tags.iter()
                .filter_map(|t| t.as_str())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut capabilities = Vec::new();
    if modality == Modality::Chat {
        capabilities.push("streaming".into());
        if tags.iter().any(|t| t == "tools") {
            capabilities.push("tools".into());
        }
        if tags.iter().any(|t| t == "multimodal") {
            capabilities.push("vision".into());
        }
        let reasoning = tags
            .iter()
            .any(|t| t == "reasoning" || t == "can-disable-reasoning")
            && !tags.iter().any(|t| t == "non-reasoning");
        if reasoning {
            capabilities.push("reasoning".into());
        }
    }
    let context = entry
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0);
    Some(ModelConfig {
        id: format!("deepinfra/{upstream}"),
        provider: "deepinfra".into(),
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

fn modality_for(kind: &str) -> Option<Modality> {
    match kind {
        "text-generation" => Some(Modality::Chat),
        "embeddings" => Some(Modality::Embedding),
        "text-to-image" => Some(Modality::Image),
        "text-to-speech" => Some(Modality::AudioTts),
        "automatic-speech-recognition" => Some(Modality::AudioStt),
        "text-to-video" | "text-to-music" => Some(Modality::Video),
        _ => None,
    }
}

fn pricing_for(pricing: &Value, modality: Modality) -> Option<Pricing> {
    let kind = pricing.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let mut out = Pricing::default();
    match kind {
        "tokens" => {
            let input = num(pricing, "cents_per_input_token").map(cents_per_token_to_per_mtok)?;
            let output = num(pricing, "cents_per_output_token").map(cents_per_token_to_per_mtok)?;
            out.input_per_mtok = Some(input);
            out.output_per_mtok = Some(output);
            if let Some(rate) = num(pricing, "rate_per_input_token_cached") {
                out.cache_read_per_mtok = Some(round_price(input * rate));
            }
            let write_rate = num(pricing, "rate_per_input_token_cache_write").or_else(|| {
                pricing
                    .get("rate_per_explicit_cache_write_token")
                    .and_then(|v| v.get("5m"))
                    .and_then(|v| v.as_f64())
            });
            if let Some(rate) = write_rate {
                out.cache_write_per_mtok = Some(round_price(input * rate));
            }
        }
        "input_tokens" => {
            let input = num(pricing, "cents_per_input_token").map(cents_per_token_to_per_mtok)?;
            if modality == Modality::Embedding {
                out.embedding_per_mtok = Some(input);
            } else {
                out.input_per_mtok = Some(input);
            }
        }
        "image_units" => {
            out.per_image =
                num(pricing, "cents_per_image_unit").map(|cents| round_price(cents / 100.0));
        }
        "input_character_length" => {
            out.per_1k_chars = num(pricing, "cents_per_input_chars")
                .map(|cents| round_price(cents * 1_000.0 / 100.0));
        }
        "time" => {
            out.per_minute =
                num(pricing, "cents_per_sec").map(|cents| round_price(cents * 60.0 / 100.0));
        }
        "input_length" => {
            out.per_minute =
                num(pricing, "cents_per_input_sec").map(|cents| round_price(cents * 60.0 / 100.0));
        }
        "output_length" => {
            out.per_minute =
                num(pricing, "cents_per_output_sec").map(|cents| round_price(cents * 60.0 / 100.0));
        }
        _ => return None,
    }
    Some(out)
}

fn num(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(|v| v.as_f64())
}

/// DeepInfra bills `cents_per_*_token`. One million tokens is `cents * 10_000` dollars.
fn cents_per_token_to_per_mtok(cents_per_token: f64) -> f64 {
    round_price(cents_per_token * 10_000.0)
}

fn round_price(value: f64) -> f64 {
    (value * 1e8).round() / 1e8
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn token_prices_are_dollars_per_million_and_cache_is_a_rate() {
        let body = json!([{
            "model_name": "deepseek-ai/DeepSeek-V4-Flash",
            "type": "text-generation",
            "max_tokens": 1000000,
            "tags": ["tools", "multimodal"],
            "pricing": {
                "type": "tokens",
                "cents_per_input_token": 0.000014,
                "cents_per_output_token": 0.000028,
                "rate_per_input_token_cached": 0.1,
                "rate_per_explicit_cache_write_token": {"5m": 1.25}
            }
        }]);
        let models = models_from_list(&body);
        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.id, "deepinfra/deepseek-ai/DeepSeek-V4-Flash");
        assert_eq!(model.pricing.input_per_mtok, Some(0.14));
        assert_eq!(model.pricing.output_per_mtok, Some(0.28));
        assert_eq!(model.pricing.cache_read_per_mtok, Some(0.014));
        assert_eq!(model.pricing.cache_write_per_mtok, Some(0.175));
        assert_eq!(model.context, Some(1_000_000));
        assert!(model.capabilities.iter().any(|c| c == "tools"));
        assert!(model.capabilities.iter().any(|c| c == "vision"));
    }

    #[test]
    fn reasoning_tag_becomes_capability() {
        let reasoning = json!([{
            "model_name": "deepseek-ai/DeepSeek-R1",
            "type": "text-generation",
            "tags": ["tools", "reasoning"],
            "pricing": {"type": "tokens", "cents_per_input_token": 0.0001, "cents_per_output_token": 0.0001}
        }]);
        let model = &models_from_list(&reasoning)[0];
        assert!(model.capabilities.iter().any(|c| c == "reasoning"));

        let non_reasoning = json!([{
            "model_name": "deepseek-ai/DeepSeek-V4-Flash",
            "type": "text-generation",
            "tags": ["tools", "reasoning", "non-reasoning"],
            "pricing": {"type": "tokens", "cents_per_input_token": 0.0001, "cents_per_output_token": 0.0001}
        }]);
        let model = &models_from_list(&non_reasoning)[0];
        assert!(!model.capabilities.iter().any(|c| c == "reasoning"));
    }

    #[test]
    fn deprecated_models_are_omitted() {
        let body = json!([{
            "model_name": "old/model",
            "type": "text-generation",
            "deprecated": 1700000000,
            "pricing": {"type": "tokens", "cents_per_input_token": 0.0001, "cents_per_output_token": 0.0001}
        }]);
        assert!(models_from_list(&body).is_empty());
    }
}
