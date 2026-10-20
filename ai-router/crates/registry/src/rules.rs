//! Deterministic profile router.
//!
//! Hard filters run before any score. A low-confidence classification uses the
//! profile's safe model. Otherwise candidates are ordered by
//! `quality - lambda * estimated_cost - mu * latency_seconds`.

use airouter_core::{
    auto_profile, Error, Fallback, Modality, ModelConfig, RouteInfo, RouteRequest, RoutingProfile,
    Signals, Target,
};

use crate::Registry;

pub fn local_signals(summary: &airouter_core::RequestSummary) -> Signals {
    let text = summary.last_user_text.to_ascii_lowercase();
    let has_fence = text.contains("```");
    let contains_code = has_fence
        || summary.has_tools
        || text.contains("function ")
        || text.contains("def ")
        || text.contains("bug");
    let needs_tools = summary.has_tools;
    let extraction = summary.needs_json
        || text.contains("extract")
        || text.contains("json")
        || text.contains("parse the");
    let reasoning = text.contains("step by step")
        || text.contains("prove")
        || text.contains("derive")
        || summary.token_estimate > 8_000;
    let writing = text.contains("essay")
        || text.contains("rewrite")
        || text.contains("blog post")
        || text.contains("short story");
    let task = if contains_code {
        "code"
    } else if extraction {
        "extraction"
    } else if reasoning {
        "reasoning"
    } else if writing {
        "writing"
    } else {
        "chat"
    };
    let difficulty = if summary.token_estimate > 8_000 || task == "reasoning" {
        3
    } else if contains_code || summary.token_estimate > 2_000 {
        2
    } else if summary.token_estimate > 400 {
        1
    } else {
        0
    };
    let confidence = if summary.last_user_text.trim().is_empty() && !summary.has_tools {
        0.2
    } else if contains_code || summary.has_tools || summary.has_images {
        0.85
    } else if task != "chat" {
        0.7
    } else {
        0.6
    };
    Signals {
        source: "local".into(),
        task: task.into(),
        difficulty,
        confidence,
        needs_tools,
        needs_reasoning: task == "reasoning",
        contains_code,
    }
}

pub fn resolve_auto(registry: &Registry, req: &RouteRequest) -> Result<Target, Error> {
    let routing = &registry.routing;
    if !routing.enabled {
        return Err(Error::AutoRoutingDisabled);
    }
    let Some(name) = auto_profile(&req.model, &routing.default_profile) else {
        return Err(Error::UnknownProfile(req.model.clone()));
    };
    let Some(profile) = routing.profiles.get(name) else {
        return Err(Error::UnknownProfile(name.to_string()));
    };
    let signals = req
        .signals
        .clone()
        .unwrap_or_else(|| local_signals(&req.summary));
    pick(
        registry,
        req.modality,
        name,
        profile,
        &signals,
        &req.summary,
    )
}

fn pick(
    registry: &Registry,
    modality: Modality,
    profile_name: &str,
    profile: &RoutingProfile,
    signals: &Signals,
    summary: &airouter_core::RequestSummary,
) -> Result<Target, Error> {
    let token_estimate = summary.token_estimate;
    let mut rejected = Vec::new();
    let mut eligible: Vec<&ModelConfig> = Vec::new();
    for id in candidate_ids(profile, signals) {
        match admit(registry, &id, modality, profile, signals, summary) {
            Ok(model) => {
                if eligible.iter().any(|m| m.id == model.id) {
                    continue;
                }
                eligible.push(model);
            }
            Err(reason) => rejected.push(reason),
        }
    }
    if eligible.is_empty() {
        return Err(Error::NoRoute(profile_name.to_string()));
    }
    eligible.sort_by(|a, b| {
        score(b, profile, signals, token_estimate)
            .partial_cmp(&score(a, profile, signals, token_estimate))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let gated = signals.confidence < profile.confidence_floor;
    if gated {
        if let Some(pos) = eligible.iter().position(|m| m.id == profile.safe_model) {
            let safe = eligible.remove(pos);
            eligible.insert(0, safe);
        }
    }
    let chosen = eligible[0];
    let reason = if gated {
        format!(
            "confidence {:.2} below floor {:.2}; safe model",
            signals.confidence, profile.confidence_floor
        )
    } else {
        format!("tier:{} difficulty:{}", signals.task, signals.difficulty)
    };
    let fallbacks = eligible
        .iter()
        .skip(1)
        .map(|m| Fallback {
            provider: m.provider.clone(),
            upstream_model: m.upstream.clone(),
            client_model: m.id.clone(),
            modality: m.modality,
            pricing: m.pricing.clone(),
            capabilities: m.capabilities.clone(),
        })
        .collect();
    Ok(Target {
        provider: chosen.provider.clone(),
        upstream_model: chosen.upstream.clone(),
        client_model: chosen.id.clone(),
        modality: chosen.modality,
        pricing: chosen.pricing.clone(),
        capabilities: chosen.capabilities.clone(),
        route: Some(RouteInfo {
            profile: profile_name.to_string(),
            reason,
            confidence: signals.confidence,
            source: signals.source.clone(),
            task: signals.task.clone(),
            difficulty: signals.difficulty,
            needs_tools: signals.needs_tools,
            needs_reasoning: signals.needs_reasoning,
            contains_code: signals.contains_code,
            rejected,
        }),
        fallbacks,
    })
}

fn candidate_ids(profile: &RoutingProfile, signals: &Signals) -> Vec<String> {
    let mut ids = Vec::new();
    for tier in &profile.tiers {
        let task_ok = tier.task == "*" || tier.task == signals.task;
        let diff_ok =
            signals.difficulty >= tier.min_difficulty && signals.difficulty <= tier.max_difficulty;
        if task_ok && diff_ok {
            ids.extend(tier.models.iter().cloned());
        }
    }
    if !profile.safe_model.is_empty() && !ids.iter().any(|id| id == &profile.safe_model) {
        ids.push(profile.safe_model.clone());
    }
    if ids.is_empty() {
        ids.push(profile.safe_model.clone());
    }
    ids
}

fn admit<'a>(
    registry: &'a Registry,
    id: &str,
    modality: Modality,
    profile: &RoutingProfile,
    signals: &Signals,
    summary: &airouter_core::RequestSummary,
) -> Result<&'a ModelConfig, String> {
    let Some(model) = registry.find(id) else {
        return Err(format!("{id}: not configured"));
    };
    if !model.enabled {
        return Err(format!("{id}: disabled"));
    }
    if model.modality != modality {
        return Err(format!("{id}: modality {}", model.modality));
    }
    if !profile.allow_providers.is_empty()
        && !profile.allow_providers.iter().any(|p| p == &model.provider)
    {
        return Err(format!("{id}: provider not allowed"));
    }
    if let Some(context) = model.context {
        if summary.token_estimate > context {
            return Err(format!("{id}: context {context}"));
        }
    }
    if missing_capability(model, signals.needs_tools, "tools") {
        return Err(format!("{id}: no tools"));
    }
    if missing_capability(model, summary.has_images, "vision") {
        return Err(format!("{id}: no vision"));
    }
    if summary.has_tools && model.upstream == "gpt-6-luna" {
        // Luna's chat endpoint rejects function tools whenever reasoning is on.
        // Hermes often omits `reasoning_effort` or sends it where this summary
        // does not see it, and Luna still reasons by default. Keep it out of
        // tool-using auto routes.
        return Err(format!(
            "{id}: function tools reject reasoning_effort on chat completions"
        ));
    }
    Ok(model)
}

/// Empty capability lists mean "unspecified", so they stay eligible.
fn missing_capability(model: &ModelConfig, needed: bool, name: &str) -> bool {
    needed && !model.capabilities.is_empty() && !model.capabilities.iter().any(|c| c == name)
}

fn score(model: &ModelConfig, profile: &RoutingProfile, signals: &Signals, tokens: u64) -> f64 {
    let quality = model
        .task_quality
        .get(&signals.task)
        .or_else(|| model.task_quality.get("*"))
        .copied()
        .or(model.quality)
        .unwrap_or(0.5);
    let input_price = model.pricing.input_per_mtok.unwrap_or(0.0);
    let output_price = model.pricing.output_per_mtok.unwrap_or(0.0);
    let est_out = (tokens / 4).max(32);
    let cost = (tokens as f64 * input_price + est_out as f64 * output_price) / 1_000_000.0;
    let latency_s = model.latency_ms_prior.unwrap_or(1_000) as f64 / 1_000.0;
    quality - profile.lambda * cost - profile.mu * latency_s
}

/// Re-score logged auto requests against the current profiles.
pub fn replay(registry: &Registry, rows: &[ReplayRow]) -> ReplayReport {
    let mut report = ReplayReport::default();
    let mut compared = 0u64;
    let mut agreed = 0u64;
    let mut strong = 0u64;
    for row in rows {
        if !row.requested_model.starts_with("auto") || row.status >= 400 {
            continue;
        }
        report.auto_requests += 1;
        report.recorded_cost += row.cost_usd;
        match row.feedback {
            Some(v) if v > 0 => report.feedback_up += 1,
            Some(v) if v < 0 => report.feedback_down += 1,
            _ => {}
        }
        let Ok(signals) = serde_json::from_str::<Signals>(&row.route_signals) else {
            continue;
        };
        let req = RouteRequest {
            model: row.requested_model.clone(),
            modality: Modality::Chat,
            summary: airouter_core::RequestSummary {
                token_estimate: row.input_tokens.max(1),
                ..airouter_core::RequestSummary::default()
            },
            signals: Some(signals),
        };
        let Ok(target) = resolve_auto(registry, &req) else {
            continue;
        };
        compared += 1;
        if target.upstream_model == row.upstream_model {
            agreed += 1;
        }
        if let Some(profile) = registry.routing.profiles.get(&row.route_profile) {
            if let Some(safe) = registry.find(&profile.safe_model) {
                if row.upstream_model == safe.upstream {
                    strong += 1;
                }
                let usage = airouter_core::TokenUsage {
                    known: true,
                    input_tokens: row.input_tokens,
                    output_tokens: row.output_tokens,
                    ..airouter_core::TokenUsage::default()
                };
                report.all_strong_cost +=
                    airouter_core::compute_cost(&safe.pricing, &usage, Modality::Chat);
            }
        }
    }
    if compared > 0 {
        report.strong_share = strong as f64 / compared as f64;
        report.agreement = agreed as f64 / compared as f64;
    }
    report.compared = compared;
    report
}

#[derive(Debug, Clone)]
pub struct ReplayRow {
    pub requested_model: String,
    pub upstream_model: String,
    pub status: u16,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub route_signals: String,
    pub route_profile: String,
    pub feedback: Option<i8>,
}

#[derive(Debug, Clone, Default)]
pub struct ReplayReport {
    pub auto_requests: u64,
    pub compared: u64,
    pub recorded_cost: f64,
    pub all_strong_cost: f64,
    /// Share of replayed calls whose winner is the profile safe model.
    pub strong_share: f64,
    /// Share of replayed calls that still pick the logged upstream model.
    pub agreement: f64,
    pub feedback_up: u64,
    pub feedback_down: u64,
}
