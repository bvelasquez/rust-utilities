//! Optional classifiers that run before the synchronous rules router.
//!
//! A timeout or error fails open to local features. Hosted calls (Jev) run only
//! when the profile lists the client in `hosted_signal_clients`.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use airouter_config::{load_secrets, resolve_secret, secrets_path};
use airouter_core::{auto_profile, RequestSummary, SignalKind, Signals};
use airouter_registry::local_signals;

use crate::AppState;

struct CacheEntry {
    at: Instant,
    signals: Signals,
}

/// A classifier that may fail. Callers time it out and fall back to local features.
pub trait SignalProvider: Send + Sync {
    fn classify<'a>(
        &'a self,
        summary: &'a RequestSummary,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Signals, String>> + Send + 'a>>;
}

pub struct SignalCache {
    entries: HashMap<u64, CacheEntry>,
}

impl SignalCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn get(&mut self, key: u64) -> Option<Signals> {
        let ttl = Duration::from_secs(60);
        match self.entries.get(&key) {
            Some(entry) if entry.at.elapsed() < ttl => Some(entry.signals.clone()),
            Some(_) => {
                self.entries.remove(&key);
                None
            }
            None => None,
        }
    }

    fn insert(&mut self, key: u64, signals: Signals) {
        if self.entries.len() > 256 {
            self.entries.clear();
        }
        self.entries.insert(
            key,
            CacheEntry {
                at: Instant::now(),
                signals,
            },
        );
    }
}

pub async fn extract(state: &AppState, model: &str, summary: &RequestSummary) -> Signals {
    let local = local_signals(summary);
    let (kind, timeout_ms, profile_opt_in, jev_key, jev_url, jev_model, examples) = {
        let rt = state.read();
        let routing = &rt.registry.routing;
        let profile_name = auto_profile(model, &routing.default_profile);
        let opted_in = profile_name
            .and_then(|name| routing.profiles.get(name))
            .is_some_and(|profile| {
                profile
                    .hosted_signal_clients
                    .iter()
                    .any(|label| label == &summary.client_label)
            });
        (
            routing.signals.provider,
            routing.signals.timeout_ms,
            opted_in,
            routing.signals.jev.api_key.clone(),
            routing.signals.jev.base_url.clone(),
            routing.signals.jev.model.clone(),
            routing.signals.embedding.examples.clone(),
        )
    };

    let cache_key = cache_key(model, summary);
    if let Some(hit) = state
        .signals()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(cache_key)
    {
        return hit;
    }

    let mut signals = match kind {
        SignalKind::Embedding => {
            let provider = EmbeddingProvider { examples };
            classify_with_timeout(&provider, summary, timeout_ms)
                .await
                .unwrap_or(local)
        }
        SignalKind::Jev if profile_opt_in => {
            let provider = JevProvider {
                state,
                timeout_ms,
                key_spec: jev_key,
                base_url: jev_url,
                model: jev_model,
            };
            match classify_with_timeout(&provider, summary, timeout_ms).await {
                Ok(remote) => remote,
                Err(err) => {
                    tracing::warn!(%err, "jev signal failed open to local features");
                    local
                }
            }
        }
        SignalKind::Jev | SignalKind::None => local,
    };
    if signals.source.is_empty() {
        signals.source = "local".into();
    }
    state
        .signals()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(cache_key, signals.clone());
    signals
}

fn cache_key(model: &str, summary: &RequestSummary) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    model.hash(&mut hasher);
    summary.client_label.hash(&mut hasher);
    summary.has_tools.hash(&mut hasher);
    summary.has_images.hash(&mut hasher);
    summary.needs_json.hash(&mut hasher);
    summary.token_estimate.hash(&mut hasher);
    summary.last_user_text.hash(&mut hasher);
    hasher.finish()
}

async fn classify_with_timeout(
    provider: &impl SignalProvider,
    summary: &RequestSummary,
    timeout_ms: u64,
) -> Result<Signals, String> {
    tokio::time::timeout(
        Duration::from_millis(timeout_ms.max(50)),
        provider.classify(summary),
    )
    .await
    .map_err(|_| "signal provider timed out".to_string())?
}

struct EmbeddingProvider {
    examples: std::collections::BTreeMap<String, Vec<String>>,
}

impl SignalProvider for EmbeddingProvider {
    fn classify<'a>(
        &'a self,
        summary: &'a RequestSummary,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Signals, String>> + Send + 'a>>
    {
        Box::pin(async move {
            lexical_signals(summary, &self.examples).ok_or_else(|| "no embedding match".to_string())
        })
    }
}

struct JevProvider<'a> {
    state: &'a AppState,
    timeout_ms: u64,
    key_spec: Option<String>,
    base_url: String,
    model: String,
}

impl SignalProvider for JevProvider<'_> {
    fn classify<'a>(
        &'a self,
        summary: &'a RequestSummary,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Signals, String>> + Send + 'a>>
    {
        Box::pin(async move {
            jev_signals(
                self.state,
                summary,
                self.timeout_ms,
                self.key_spec.as_deref(),
                &self.base_url,
                &self.model,
            )
            .await
        })
    }
}

fn lexical_signals(
    summary: &RequestSummary,
    examples: &std::collections::BTreeMap<String, Vec<String>>,
) -> Option<Signals> {
    if examples.is_empty() || summary.last_user_text.trim().is_empty() {
        return None;
    }
    let query = tokens(&summary.last_user_text);
    if query.is_empty() {
        return None;
    }
    let mut ranked: Vec<(String, f64)> = examples
        .iter()
        .map(|(task, samples)| {
            let best = samples
                .iter()
                .map(|sample| cosine(&query, &tokens(sample)))
                .fold(0.0_f64, f64::max);
            (task.clone(), best)
        })
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let (task, best) = ranked.first()?.clone();
    let second = ranked.get(1).map(|(_, score)| *score).unwrap_or(0.0);
    if best <= 0.0 {
        return None;
    }
    let mut base = local_signals(summary);
    base.source = "embedding".into();
    base.task = task;
    base.needs_reasoning = base.task == "reasoning";
    base.contains_code = base.contains_code || base.task == "code";
    let margin = if best + second == 0.0 {
        0.5
    } else {
        best / (best + second)
    };
    base.confidence = margin.clamp(0.5, 0.95);
    Some(base)
}

fn tokens(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(|w| w.to_ascii_lowercase())
        .collect()
}

fn cosine(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let dot = a.intersection(b).count() as f64;
    dot / ((a.len() * b.len()) as f64).sqrt()
}

async fn jev_signals(
    state: &AppState,
    summary: &RequestSummary,
    timeout_ms: u64,
    key_spec: Option<&str>,
    base_url: &str,
    model: &str,
) -> Result<Signals, String> {
    let spec = key_spec.ok_or_else(|| "jev api key is unset".to_string())?;
    let secrets =
        load_secrets(&secrets_path(&state.inner.config_path)).map_err(|e| e.to_string())?;
    let (key, status) = resolve_secret(spec, &secrets);
    let key = key.ok_or_else(|| format!("jev api key does not resolve ({status})"))?;
    let body = jev_body(summary, model);
    let url = format!("{}/v1/systemone", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let response = tokio::time::timeout(Duration::from_millis(timeout_ms.max(50)), async {
        client
            .post(url)
            .bearer_auth(key)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json::<serde_json::Value>()
            .await
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "jev timed out".to_string())??;
    parse_jev_answers(&response, summary)
}

fn jev_body(summary: &RequestSummary, model: &str) -> serde_json::Value {
    let text = trim_chars(&summary.last_user_text, 4_000);
    let state = serde_json::json!({
        "text": text,
        "has_tools": summary.has_tools,
        "has_images": summary.has_images,
        "needs_json": summary.needs_json,
        "token_estimate": summary.token_estimate,
    });
    // Option order is fixed. jev-1.13 is sensitive to Choice order, and a
    // per-request shuffle would make routing non-deterministic.
    serde_json::json!({
        "model": model,
        "state": state,
        "questions": {
            "task": {
                "type": "choice",
                "instructions": "Which single task does `text` ask for? Use the feature fields only as hints.",
                "criteria": {
                    "code": "Writing, editing, debugging, or explaining source code",
                    "extraction": "Pulling structured fields, JSON, or a short factual span out of text",
                    "chat": "A short conversational reply that is not code, extraction, writing, or multi-step reasoning",
                    "writing": "Drafting or rewriting prose such as an essay, email, or story",
                    "reasoning": "A multi-step proof, plan, or derivation that is not primarily code"
                }
            },
            "difficulty": {
                "type": "score",
                "instructions": "How hard is answering `text` for a capable model?",
                "criteria": ["Trivial", "Easy", "Moderate", "Hard", "Needs a strong model"]
            },
            "needs_tools": {
                "type": "noul",
                "instructions": "Does a correct answer require calling a tool or function, beyond text already in state?"
            },
            "needs_reasoning": {
                "type": "noul",
                "instructions": "Does a correct answer require several dependent reasoning steps?"
            },
            "contains_code": {
                "type": "noul",
                "instructions": "Does `text` contain source code or ask for source code?"
            }
        }
    })
}

pub fn parse_jev_answers(
    body: &serde_json::Value,
    summary: &RequestSummary,
) -> Result<Signals, String> {
    let answers = body
        .get("answers")
        .ok_or_else(|| "jev response has no answers".to_string())?;
    let task = answers
        .pointer("/task/choice")
        .and_then(|v| v.as_str())
        .unwrap_or("chat")
        .to_string();
    let confidence = answers
        .pointer("/task/confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let difficulty = answers
        .pointer("/difficulty/score")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .round()
        .clamp(0.0, 4.0) as u8;
    let noul = |key: &str, fallback: bool| {
        answers
            .pointer(&format!("/{key}/noul"))
            .and_then(|v| v.as_f64())
            .map(|p| p >= 0.5)
            .unwrap_or(fallback)
    };
    Ok(Signals {
        source: "jev".into(),
        task,
        difficulty,
        confidence,
        needs_tools: summary.has_tools || noul("needs_tools", false),
        needs_reasoning: noul("needs_reasoning", false),
        contains_code: noul("contains_code", false),
    })
}

fn trim_chars(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i >= max_chars {
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use airouter_core::RequestSummary;
    use serde_json::json;

    #[test]
    fn parses_jev_choice_score_and_noul() {
        let body = json!({
            "model": "jev-1.13.0",
            "answers": {
                "task": {"type": "choice", "choice": "code", "confidence": 0.81},
                "difficulty": {"type": "score", "score": 2.2, "confidence": 0.7},
                "needs_tools": {"type": "noul", "noul": 0.2},
                "needs_reasoning": {"type": "noul", "noul": 0.1},
                "contains_code": {"type": "noul", "noul": 0.9}
            }
        });
        let summary = RequestSummary {
            has_tools: true,
            ..RequestSummary::default()
        };
        let signals = parse_jev_answers(&body, &summary).unwrap();
        assert_eq!(signals.task, "code");
        assert_eq!(signals.difficulty, 2);
        assert!((signals.confidence - 0.81).abs() < 1e-9);
        assert!(signals.needs_tools);
        assert!(signals.contains_code);
        assert_eq!(signals.source, "jev");
    }

    #[test]
    fn lexical_similarity_picks_the_closer_task() {
        let mut examples = std::collections::BTreeMap::new();
        examples.insert(
            "code".into(),
            vec!["fix the rust compile error in this function".into()],
        );
        examples.insert("chat".into(), vec!["what is the weather today".into()]);
        let summary = RequestSummary {
            last_user_text: "please fix the rust compile error".into(),
            token_estimate: 12,
            ..RequestSummary::default()
        };
        let signals = lexical_signals(&summary, &examples).unwrap();
        assert_eq!(signals.task, "code");
        assert_eq!(signals.source, "embedding");
        assert!(signals.confidence >= 0.5);
    }
}
