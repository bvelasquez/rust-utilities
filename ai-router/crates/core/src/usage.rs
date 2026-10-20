use crate::modality::Modality;
use crate::pricing::Pricing;
use crate::routing::RouteInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Token and unit counts used for cost. `known` is false when the upstream
/// omitted usage entirely (as opposed to a real zero).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub known: bool,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cache_write_tokens: u64,
    pub images: u64,
    pub characters: u64,
    pub audio_seconds: f64,
}

impl TokenUsage {
    pub fn from_openai_usage(usage: &Value) -> Self {
        let prompt = json_u64(usage.get("prompt_tokens"));
        let completion = json_u64(usage.get("completion_tokens"));
        // Some providers only send input_tokens / output_tokens (OpenAI-ish variants).
        let input = prompt.or_else(|| json_u64(usage.get("input_tokens")));
        let output = completion.or_else(|| json_u64(usage.get("output_tokens")));
        let cached = usage
            .get("prompt_tokens_details")
            .and_then(|d| json_u64(d.get("cached_tokens")))
            .or_else(|| json_u64(usage.get("cache_read_input_tokens")))
            .or_else(|| json_u64(usage.get("cached_tokens")))
            .unwrap_or(0);
        let cache_write = json_u64(usage.get("cache_creation_input_tokens")).unwrap_or(0);
        if input.is_none() && output.is_none() && cached == 0 && cache_write == 0 {
            return Self::default();
        }
        Self {
            known: true,
            input_tokens: input.unwrap_or(0),
            output_tokens: output.unwrap_or(0),
            cached_tokens: cached,
            cache_write_tokens: cache_write,
            ..Self::default()
        }
    }

    /// Pull usage off an OpenAI chat completion JSON object.
    pub fn from_openai_chat(body: &Value) -> Self {
        match body.get("usage") {
            Some(u) if !u.is_null() => Self::from_openai_usage(u),
            _ => Self::default(),
        }
    }

    pub fn merge_from(&mut self, other: &TokenUsage) {
        if !other.known && other.images == 0 && other.characters == 0 && other.audio_seconds == 0.0
        {
            return;
        }
        if other.known {
            self.known = true;
            self.input_tokens = other.input_tokens;
            self.output_tokens = other.output_tokens;
            self.cached_tokens = other.cached_tokens;
            self.cache_write_tokens = other.cache_write_tokens;
        }
        if other.images > 0 {
            self.images = other.images;
        }
        if other.characters > 0 {
            self.characters = other.characters;
        }
        if other.audio_seconds > 0.0 {
            self.audio_seconds = other.audio_seconds;
        }
    }
}

fn json_u64(v: Option<&Value>) -> Option<u64> {
    match v {
        Some(Value::Number(n)) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        _ => None,
    }
}

/// Context the gateway already knows before the upstream responds.
#[derive(Debug, Clone)]
pub struct UsageMeta {
    pub request_id: String,
    pub client_label: String,
    pub requested_model: String,
    pub provider: String,
    pub upstream_model: String,
    pub modality: Modality,
    pub endpoint: String,
    pub pricing: Pricing,
    pub route: Option<RouteInfo>,
}

pub fn compute_cost(pricing: &Pricing, usage: &TokenUsage, modality: Modality) -> f64 {
    let per_mtok = |tokens: u64, price: f64| (tokens as f64) * price / 1_000_000.0;
    let input_price = pricing.input_per_mtok.unwrap_or(0.0);
    let output_price = pricing.output_per_mtok.unwrap_or(0.0);
    let cache_read_price = pricing.cache_read_per_mtok.unwrap_or(input_price);
    let cache_write_price = pricing.cache_write_per_mtok.unwrap_or(input_price);

    let mut cost = 0.0;
    match modality {
        Modality::Embedding => {
            let price = pricing
                .embedding_per_mtok
                .or(pricing.input_per_mtok)
                .unwrap_or(0.0);
            cost += per_mtok(usage.input_tokens, price);
        }
        Modality::Image => {
            cost += usage.images as f64 * pricing.per_image.unwrap_or(0.0);
        }
        Modality::AudioTts => {
            cost += (usage.characters as f64) / 1000.0 * pricing.per_1k_chars.unwrap_or(0.0);
        }
        Modality::AudioStt => {
            cost += (usage.audio_seconds / 60.0) * pricing.per_minute.unwrap_or(0.0);
        }
        Modality::Chat | Modality::Video => {
            let cached = usage.cached_tokens.min(usage.input_tokens);
            let uncached = usage.input_tokens.saturating_sub(cached);
            cost += per_mtok(uncached, input_price);
            cost += per_mtok(cached, cache_read_price);
            cost += per_mtok(usage.cache_write_tokens, cache_write_price);
            cost += per_mtok(usage.output_tokens, output_price);
        }
    }
    cost
}

/// Incremental parser for OpenAI SSE. Safe to feed arbitrary chunk boundaries.
#[derive(Debug, Default)]
pub struct SseUsageParser {
    buf: String,
}

impl SseUsageParser {
    pub fn push(&mut self, text: &str, usage: &mut TokenUsage) -> bool {
        self.buf.push_str(text);
        let mut saw_content = false;
        while let Some(i) = self.buf.find('\n') {
            let line = self.buf[..i].trim().to_string();
            self.buf.drain(..=i);
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(data) {
                    if let Some(u) = v.get("usage") {
                        usage.merge_from(&TokenUsage::from_openai_usage(u));
                    }
                    if chunk_has_content(&v) {
                        saw_content = true;
                    }
                }
            }
        }
        saw_content
    }
}

fn chunk_has_content(v: &Value) -> bool {
    let Some(choices) = v.get("choices").and_then(|c| c.as_array()) else {
        return false;
    };
    let Some(delta) = choices.first().and_then(|c| c.get("delta")) else {
        return false;
    };
    if delta
        .get("content")
        .and_then(|c| c.as_str())
        .is_some_and(|s| !s.is_empty())
    {
        return true;
    }
    if delta
        .get("tool_calls")
        .and_then(|c| c.as_array())
        .is_some_and(|a| !a.is_empty())
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_cost_splits_cache_from_prompt() {
        let pricing = Pricing {
            input_per_mtok: Some(3.0),
            output_per_mtok: Some(15.0),
            cache_read_per_mtok: Some(0.3),
            ..Pricing::default()
        };
        let usage = TokenUsage {
            known: true,
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cached_tokens: 250_000,
            ..TokenUsage::default()
        };
        let cost = compute_cost(&pricing, &usage, Modality::Chat);
        // 750k * 3 + 250k * 0.3 + 1M * 15 = 2.25 + 0.075 + 15
        assert!((cost - 17.325).abs() < 1e-9, "{cost}");
    }

    #[test]
    fn image_audio_and_embedding_units() {
        let pricing = Pricing {
            embedding_per_mtok: Some(0.02),
            per_image: Some(0.04),
            per_1k_chars: Some(0.015),
            per_minute: Some(0.006),
            ..Pricing::default()
        };
        let embed = TokenUsage {
            known: true,
            input_tokens: 500_000,
            ..TokenUsage::default()
        };
        assert!((compute_cost(&pricing, &embed, Modality::Embedding) - 0.01).abs() < 1e-12);

        let image = TokenUsage {
            images: 2,
            ..TokenUsage::default()
        };
        assert!((compute_cost(&pricing, &image, Modality::Image) - 0.08).abs() < 1e-12);

        let tts = TokenUsage {
            characters: 2000,
            ..TokenUsage::default()
        };
        assert!((compute_cost(&pricing, &tts, Modality::AudioTts) - 0.03).abs() < 1e-12);

        let stt = TokenUsage {
            audio_seconds: 90.0,
            ..TokenUsage::default()
        };
        assert!((compute_cost(&pricing, &stt, Modality::AudioStt) - 0.009).abs() < 1e-12);
    }

    #[test]
    fn sse_parser_reads_usage_across_chunks() {
        let mut parser = SseUsageParser::default();
        let mut usage = TokenUsage::default();
        let first = "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n";
        assert!(!parser.push(&first[..8], &mut usage));
        assert!(parser.push(&first[8..], &mut usage));
        assert!(!usage.known);
        let more =
            "data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n";
        parser.push(more, &mut usage);
        assert!(usage.known);
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 2);
    }
}
