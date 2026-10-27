//! Economic model selection.
//!
//! Hard filters run first. Each mode then optimizes a different term inside
//! the models that clear the quality floor, cost cap, and latency cap.

use airouter_core::{
    estimate_output_tokens, estimate_text_cost, latency_prior_ms, model_task_quality,
    required_quality, Error, Fallback, Modality, ModelConfig, RouteInfo, RoutingMode,
    RoutingProfile, Signals, Target,
};

use crate::rules::candidate_ids;
use crate::Registry;

struct Candidate<'a> {
    model: &'a ModelConfig,
    quality: f64,
    explicit: bool,
    cost: f64,
    latency_ms: u64,
    latency_defaulted: bool,
}

pub fn select(
    registry: &Registry,
    modality: Modality,
    profile_name: &str,
    profile: &RoutingProfile,
    signals: &Signals,
    summary: &airouter_core::RequestSummary,
    mode: RoutingMode,
) -> Result<Target, Error> {
    let required = required_quality(&signals.task, signals.difficulty, profile);
    let output_tokens = estimate_output_tokens(&signals.task, summary.token_estimate);
    let mut rejected = Vec::new();
    let mut hard: Vec<Candidate> = Vec::new();
    for id in candidate_ids(profile, signals) {
        match crate::rules::admit(registry, &id, modality, profile, signals, summary) {
            Ok(model) => {
                if hard.iter().any(|c| c.model.id == model.id) {
                    continue;
                }
                let cost =
                    estimate_text_cost(&model.pricing, summary.token_estimate, output_tokens);
                if profile.max_cost_usd.is_some_and(|cap| cost > cap) {
                    rejected.push(format!(
                        "{id}: estimated ${cost:.4} over cap ${:.4}",
                        profile.max_cost_usd.unwrap_or(0.0)
                    ));
                    continue;
                }
                let (latency_ms, latency_defaulted) = latency_prior_ms(model);
                if profile.max_latency_ms.is_some_and(|cap| latency_ms > cap) {
                    rejected.push(format!("{id}: latency {latency_ms}ms over cap"));
                    continue;
                }
                let (quality, explicit) = model_task_quality(model, &signals.task);
                hard.push(Candidate {
                    model,
                    quality,
                    explicit,
                    cost,
                    latency_ms,
                    latency_defaulted,
                });
            }
            Err(reason) => rejected.push(reason),
        }
    }

    let gated = signals.confidence < profile.confidence_floor;
    if gated {
        if let Some(safe) = hard.iter().find(|c| c.model.id == profile.safe_model) {
            return Ok(finish(
                profile_name,
                profile,
                signals,
                mode,
                required,
                safe,
                &hard,
                rejected,
                format!(
                    "confidence {:.2} below floor {:.2}; safe model",
                    signals.confidence, profile.confidence_floor
                ),
            ));
        }
    }

    if let Some(winner) = rank(&hard, mode, required, profile) {
        let mut reason = format!(
            "{mode}: required {required:.2}, picked {} ({:.2}), est ${:.4}",
            winner.model.id,
            winner.quality,
            winner.cost,
            mode = mode.as_str()
        );
        if mode == RoutingMode::Speed && winner.latency_defaulted {
            reason.push_str("; latency prior defaulted to 1000ms");
        }
        return Ok(finish(
            profile_name,
            profile,
            signals,
            mode,
            required,
            winner,
            &hard,
            rejected,
            reason,
        ));
    }

    for candidate in &hard {
        if candidate.quality < required {
            rejected.push(format!(
                "{}: quality {:.2} < required {required:.2}",
                candidate.model.id, candidate.quality
            ));
        }
    }

    let relaxed = (required - profile.relax_step).max(0.0);
    if profile.relax_step > 0.0 && relaxed < required {
        if let Some(winner) = rank(&hard, mode, relaxed, profile) {
            let mut reason = format!(
                "{mode}: no model >= {required:.2}; relaxed to {relaxed:.2} and picked {} ({:.2}), est ${:.4}",
                winner.model.id,
                winner.quality,
                winner.cost,
                mode = mode.as_str()
            );
            if let Some(cap) = profile.max_cost_usd {
                reason = format!(
                    "{mode}: no model >= {required:.2} under ${cap:.2}; relaxed to {relaxed:.2} and picked {} ({:.2})",
                    winner.model.id,
                    winner.quality,
                    mode = mode.as_str()
                );
            }
            return Ok(finish(
                profile_name,
                profile,
                signals,
                mode,
                required,
                winner,
                &hard,
                rejected,
                reason,
            ));
        }
    }

    if let Some(safe) = hard.iter().find(|c| c.model.id == profile.safe_model) {
        return Ok(finish(
            profile_name,
            profile,
            signals,
            mode,
            required,
            safe,
            &hard,
            rejected,
            format!(
                "{mode}: no model >= {required:.2}; safe model {}",
                safe.model.id,
                mode = mode.as_str()
            ),
        ));
    }

    let detail = if rejected.is_empty() {
        "no eligible model".to_string()
    } else {
        rejected.join("; ")
    };
    Err(Error::NoRoute(format!("{profile_name}: {detail}")))
}

fn rank<'a>(
    hard: &'a [Candidate<'a>],
    mode: RoutingMode,
    floor: f64,
    profile: &RoutingProfile,
) -> Option<&'a Candidate<'a>> {
    let mut eligible: Vec<&Candidate> = match mode {
        RoutingMode::Quality => hard.iter().collect(),
        RoutingMode::Budget | RoutingMode::Balanced | RoutingMode::Speed => hard
            .iter()
            .filter(|c| c.quality + f64::EPSILON >= floor)
            .collect(),
    };
    if eligible.is_empty() {
        return None;
    }
    eligible.sort_by(|a, b| compare(a, b, mode, profile));
    eligible.into_iter().next()
}

fn compare(
    a: &Candidate,
    b: &Candidate,
    mode: RoutingMode,
    profile: &RoutingProfile,
) -> std::cmp::Ordering {
    match mode {
        RoutingMode::Budget => a
            .cost
            .total_cmp(&b.cost)
            .then(b.quality.total_cmp(&a.quality))
            .then(a.latency_ms.cmp(&b.latency_ms)),
        RoutingMode::Quality => b
            .quality
            .total_cmp(&a.quality)
            .then(a.cost.total_cmp(&b.cost))
            .then(a.latency_ms.cmp(&b.latency_ms)),
        RoutingMode::Speed => a
            .latency_ms
            .cmp(&b.latency_ms)
            .then(b.quality.total_cmp(&a.quality))
            .then(a.cost.total_cmp(&b.cost)),
        RoutingMode::Balanced => balanced_score(b, profile)
            .total_cmp(&balanced_score(a, profile))
            .then(b.quality.total_cmp(&a.quality))
            .then(a.cost.total_cmp(&b.cost)),
    }
}

fn balanced_score(candidate: &Candidate, profile: &RoutingProfile) -> f64 {
    let latency_s = candidate.latency_ms as f64 / 1_000.0;
    candidate.quality - profile.lambda * candidate.cost - profile.mu * latency_s
}

fn finish(
    profile_name: &str,
    profile: &RoutingProfile,
    signals: &Signals,
    mode: RoutingMode,
    required: f64,
    chosen: &Candidate,
    hard: &[Candidate],
    rejected: Vec<String>,
    reason: String,
) -> Target {
    let mut rest: Vec<&Candidate> = hard
        .iter()
        .filter(|c| c.model.id != chosen.model.id)
        .collect();
    rest.sort_by(|a, b| compare(a, b, mode, profile));
    let fallbacks = rest
        .into_iter()
        .map(|c| Fallback {
            provider: c.model.provider.clone(),
            upstream_model: c.model.upstream.clone(),
            client_model: c.model.id.clone(),
            modality: c.model.modality,
            pricing: c.model.pricing.clone(),
            capabilities: c.model.capabilities.clone(),
        })
        .collect();
    Target {
        provider: chosen.model.provider.clone(),
        upstream_model: chosen.model.upstream.clone(),
        client_model: chosen.model.id.clone(),
        modality: chosen.model.modality,
        pricing: chosen.model.pricing.clone(),
        capabilities: chosen.model.capabilities.clone(),
        route: Some(RouteInfo {
            profile: profile_name.to_string(),
            reason: reason.clone(),
            confidence: signals.confidence,
            source: signals.source.clone(),
            task: signals.task.clone(),
            difficulty: signals.difficulty,
            needs_tools: signals.needs_tools,
            needs_reasoning: signals.needs_reasoning,
            contains_code: signals.contains_code,
            rejected,
            mode: mode.as_str().into(),
            required_quality: required,
            selected_quality: chosen.quality,
            estimated_cost_usd: chosen.cost,
            selection_reason: reason,
            moa_mode: String::new(),
            explicit_quality_prior: chosen.explicit,
        }),
        fallbacks,
        moa: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use airouter_core::{
        Modality, ModelConfig, Pricing, RequestSummary, RouteRequest, RoutingMode, RoutingProfile,
        RoutingTier, Signals,
    };

    use super::*;
    use crate::Registry;

    fn model(
        id: &str,
        input: f64,
        output: f64,
        quality: Option<f64>,
        task: Option<(&str, f64)>,
        latency: Option<u64>,
    ) -> ModelConfig {
        let mut task_quality = BTreeMap::new();
        if let Some((name, score)) = task {
            task_quality.insert(name.into(), score);
        }
        ModelConfig {
            id: id.into(),
            provider: id.split('/').next().unwrap_or("p").into(),
            upstream: id.rsplit('/').next().unwrap_or(id).into(),
            modality: Modality::Chat,
            enabled: true,
            context: Some(100_000),
            capabilities: vec!["tools".into(), "vision".into()],
            pricing: Pricing {
                input_per_mtok: Some(input),
                output_per_mtok: Some(output),
                ..Pricing::default()
            },
            quality,
            task_quality,
            latency_ms_prior: latency,
        }
    }

    fn make_profile(mode: RoutingMode, safe: &str, models: &[&str]) -> RoutingProfile {
        RoutingProfile {
            safe_model: safe.into(),
            tiers: vec![RoutingTier {
                task: "*".into(),
                min_difficulty: 0,
                max_difficulty: 4,
                models: models.iter().map(|id| (*id).to_string()).collect(),
            }],
            confidence_floor: 0.0,
            mode: Some(mode),
            lambda: 10.0,
            mu: 0.02,
            max_cost_usd: Some(1.0),
            ..RoutingProfile::default()
        }
    }

    fn registry(models: Vec<ModelConfig>, profile: RoutingProfile) -> Registry {
        let mut routing = airouter_core::RoutingConfig::default();
        routing.default_profile = "p".into();
        routing.profiles.insert("p".into(), profile);
        Registry::from_parts(models, BTreeMap::new(), routing)
    }

    fn request(task: &str, difficulty: u8) -> RouteRequest {
        RouteRequest {
            model: "auto/p".into(),
            modality: Modality::Chat,
            summary: RequestSummary {
                token_estimate: 1_000,
                last_user_text: "hello".into(),
                ..RequestSummary::default()
            },
            signals: Some(Signals {
                task: task.into(),
                difficulty,
                confidence: 0.9,
                ..Signals::default()
            }),
            mode_override: None,
        }
    }

    #[test]
    fn budget_picks_the_cheapest_model_above_the_floor() {
        let cheap = model("a/cheap", 0.1, 0.2, None, Some(("code", 0.70)), Some(800));
        let mid = model("a/mid", 1.0, 2.0, None, Some(("code", 0.90)), Some(500));
        let weak = model("a/weak", 0.01, 0.02, None, Some(("code", 0.20)), Some(100));
        let reg = registry(
            vec![cheap, mid, weak],
            make_profile(
                RoutingMode::Budget,
                "a/mid",
                &["a/cheap", "a/mid", "a/weak"],
            ),
        );
        let target = crate::resolve(&reg, &request("code", 1)).unwrap();
        assert_eq!(target.client_model, "a/cheap");
        assert!(target.route.as_ref().unwrap().required_quality > 0.5);
    }

    #[test]
    fn balanced_respects_lambda_and_the_floor() {
        let cheap = model("a/cheap", 0.1, 0.1, None, Some(("code", 0.70)), Some(200));
        let strong = model("a/strong", 0.2, 0.2, None, Some(("code", 0.95)), Some(200));
        let mut profile = make_profile(RoutingMode::Balanced, "a/strong", &["a/cheap", "a/strong"]);
        profile.lambda = 0.0;
        profile.mu = 0.0;
        let reg = registry(vec![cheap, strong], profile);
        let target = crate::resolve(&reg, &request("code", 0)).unwrap();
        assert_eq!(target.client_model, "a/strong");

        let mut costly = make_profile(RoutingMode::Balanced, "a/strong", &["a/cheap", "a/strong"]);
        costly.lambda = 10_000.0;
        let cheap = model("a/cheap", 0.1, 0.1, None, Some(("code", 0.70)), Some(200));
        let strong = model(
            "a/strong",
            50.0,
            50.0,
            None,
            Some(("code", 0.95)),
            Some(200),
        );
        let reg = registry(vec![cheap, strong], costly);
        let target = crate::resolve(&reg, &request("code", 0)).unwrap();
        assert_eq!(target.client_model, "a/cheap");
    }

    #[test]
    fn quality_picks_the_highest_under_the_cap() {
        let cheap = model(
            "a/cheap",
            0.1,
            0.2,
            None,
            Some(("reasoning", 0.60)),
            Some(100),
        );
        let best = model(
            "a/best",
            2.0,
            4.0,
            None,
            Some(("reasoning", 0.95)),
            Some(100),
        );
        let pricey = model(
            "a/pricey",
            500.0,
            500.0,
            None,
            Some(("reasoning", 0.99)),
            Some(100),
        );
        let mut profile = make_profile(
            RoutingMode::Quality,
            "a/best",
            &["a/cheap", "a/best", "a/pricey"],
        );
        profile.max_cost_usd = Some(0.01);
        let reg = registry(vec![cheap, best, pricey], profile);
        let target = crate::resolve(&reg, &request("reasoning", 0)).unwrap();
        assert_eq!(target.client_model, "a/best");
    }

    #[test]
    fn speed_picks_the_lowest_latency_above_the_floor() {
        let fast_weak = model("a/fast", 0.1, 0.1, None, Some(("chat", 0.20)), Some(100));
        let fast_ok = model("a/ok", 0.5, 0.5, None, Some(("chat", 0.50)), Some(200));
        let slow = model("a/slow", 0.1, 0.1, None, Some(("chat", 0.90)), Some(5_000));
        let reg = registry(
            vec![fast_weak, fast_ok, slow],
            make_profile(RoutingMode::Speed, "a/slow", &["a/fast", "a/ok", "a/slow"]),
        );
        let target = crate::resolve(&reg, &request("chat", 0)).unwrap();
        assert_eq!(target.client_model, "a/ok");
    }

    #[test]
    fn missing_priors_fall_back_and_are_reported() {
        let bare = model("a/bare", 0.1, 0.1, None, None, None);
        let marked = model("a/marked", 1.0, 1.0, Some(0.8), None, Some(100));
        let mut profile = make_profile(RoutingMode::Quality, "a/marked", &["a/bare", "a/marked"]);
        profile.max_cost_usd = None;
        let reg = registry(vec![bare, marked], profile);
        let target = crate::resolve(&reg, &request("chat", 0)).unwrap();
        assert_eq!(target.client_model, "a/marked");
        assert!(target.route.as_ref().unwrap().explicit_quality_prior);
        assert!((target.route.as_ref().unwrap().selected_quality - 0.8).abs() < 1e-9);

        let bare = model("a/bare", 0.1, 0.1, None, Some(("chat", 0.6)), None);
        let mut profile = make_profile(RoutingMode::Speed, "a/bare", &["a/bare"]);
        profile.max_cost_usd = None;
        profile.max_latency_ms = None;
        let reg = registry(vec![bare], profile);
        let target = crate::resolve(&reg, &request("chat", 0)).unwrap();
        assert!(target
            .route
            .as_ref()
            .unwrap()
            .reason
            .contains("latency prior defaulted"));
    }

    #[test]
    fn relaxes_once_then_uses_the_safe_model() {
        let low = model("a/low", 0.1, 0.1, None, Some(("code", 0.40)), Some(100));
        let mid = model("a/mid", 0.2, 0.2, None, Some(("code", 0.55)), Some(100));
        let mut profile = make_profile(RoutingMode::Budget, "a/low", &["a/low", "a/mid"]);
        profile.required_quality.insert("code".into(), 0.80);
        profile.difficulty_step = Some(0.0);
        profile.relax_step = 0.30;
        let reg = registry(vec![low.clone(), mid.clone()], profile);
        let target = crate::resolve(&reg, &request("code", 0)).unwrap();
        assert_eq!(target.client_model, "a/mid");
        assert!(target.route.as_ref().unwrap().reason.contains("relaxed"));

        let mut profile = make_profile(RoutingMode::Budget, "a/low", &["a/low", "a/mid"]);
        profile.required_quality.insert("code".into(), 0.90);
        profile.difficulty_step = Some(0.0);
        profile.relax_step = 0.05;
        let reg = registry(vec![low, mid], profile);
        let target = crate::resolve(&reg, &request("code", 0)).unwrap();
        assert_eq!(target.client_model, "a/low");
        assert!(target.route.as_ref().unwrap().reason.contains("safe model"));
    }

    #[test]
    fn required_quality_rises_with_difficulty_and_caps_stay_hard() {
        let model = model("a/m", 0.1, 0.1, None, Some(("reasoning", 0.70)), Some(100));
        let mut profile = make_profile(RoutingMode::Budget, "a/m", &["a/m"]);
        profile.required_quality.insert("reasoning".into(), 0.50);
        profile.difficulty_step = Some(0.10);
        let reg = registry(vec![model.clone()], profile.clone());
        let easy = crate::resolve(&reg, &request("reasoning", 0)).unwrap();
        let hard = crate::resolve(&reg, &request("reasoning", 3)).unwrap();
        assert!(
            hard.route.as_ref().unwrap().required_quality
                > easy.route.as_ref().unwrap().required_quality
        );
        assert!(
            hard.route.as_ref().unwrap().reason.contains("safe model")
                || hard.route.as_ref().unwrap().reason.contains("relaxed")
        );

        let mut profile = profile;
        profile.max_latency_ms = Some(50);
        profile.required_quality.insert("chat".into(), 0.10);
        let reg = registry(vec![model], profile);
        let err = crate::resolve(&reg, &request("chat", 0)).unwrap_err();
        assert!(matches!(err, Error::NoRoute(_)));
    }
}
