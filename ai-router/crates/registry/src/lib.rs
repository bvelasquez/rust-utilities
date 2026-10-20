//! Model catalog, aliases, and routing.
//!
//! [`resolve`] sends `auto` and `auto/<profile>` to [`RulesRouter`]. Every other
//! id is an explicit model or an alias chain.

mod cheaperinference;
mod deepinfra;
mod rules;

pub use cheaperinference::{
    list_url as cheaperinference_list_url, models_from_list as cheaperinference_models_from_list,
};
pub use deepinfra::{
    list_url as deepinfra_list_url, models_from_list as deepinfra_models_from_list,
};

use std::collections::BTreeMap;

use airouter_core::{Error, ModelConfig, RouteRequest, RoutingConfig, Target};

pub use rules::{local_signals, replay, ReplayReport, ReplayRow};

const CATALOG_JSON: &str = include_str!("catalog.json");

#[derive(Debug, Clone)]
pub struct Registry {
    pub models: Vec<ModelConfig>,
    pub aliases: BTreeMap<String, String>,
    pub catalog: Vec<ModelConfig>,
    pub routing: RoutingConfig,
}

impl Registry {
    pub fn from_models(models: Vec<ModelConfig>, aliases: BTreeMap<String, String>) -> Self {
        Self::from_parts(models, aliases, RoutingConfig::default())
    }

    pub fn from_parts(
        models: Vec<ModelConfig>,
        aliases: BTreeMap<String, String>,
        routing: RoutingConfig,
    ) -> Self {
        Self {
            models,
            aliases,
            catalog: builtin_catalog(),
            routing,
        }
    }

    pub fn enabled_models(&self) -> impl Iterator<Item = &ModelConfig> {
        self.models.iter().filter(|m| m.enabled)
    }

    pub fn find(&self, id: &str) -> Option<&ModelConfig> {
        self.models.iter().find(|m| m.id == id)
    }
}

pub fn builtin_catalog() -> Vec<ModelConfig> {
    serde_json::from_str(CATALOG_JSON).expect("built-in catalog is valid JSON")
}

pub fn find_catalog(id: &str) -> Option<ModelConfig> {
    builtin_catalog().into_iter().find(|m| m.id == id)
}

pub fn search_catalog(query: &str) -> Vec<ModelConfig> {
    let q = query.to_ascii_lowercase();
    builtin_catalog()
        .into_iter()
        .filter(|m| {
            m.id.to_ascii_lowercase().contains(&q)
                || m.provider.to_ascii_lowercase().contains(&q)
                || m.upstream.to_ascii_lowercase().contains(&q)
                || m.modality.as_str().contains(&q)
        })
        .collect()
}

/// Extension point. v1 only implements explicit ids and aliases.
pub trait ModelRouter {
    fn resolve(&self, registry: &Registry, req: &RouteRequest) -> Result<Target, Error>;
}

/// Parses `provider/model` ids and one-level (actually few-level) aliases.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExplicitRouter;

impl ModelRouter for ExplicitRouter {
    fn resolve(&self, registry: &Registry, req: &RouteRequest) -> Result<Target, Error> {
        resolve(registry, req)
    }
}

/// Profile router for `auto` and `auto/<profile>`.
#[derive(Debug, Default, Clone, Copy)]
pub struct RulesRouter;

impl ModelRouter for RulesRouter {
    fn resolve(&self, registry: &Registry, req: &RouteRequest) -> Result<Target, Error> {
        rules::resolve_auto(registry, req)
    }
}

pub fn resolve(registry: &Registry, req: &RouteRequest) -> Result<Target, Error> {
    if req.model == "auto" || req.model.starts_with("auto/") {
        return RulesRouter.resolve(registry, req);
    }
    let id = follow_alias(&registry.aliases, &req.model);
    let Some(model) = registry.find(&id) else {
        return Err(Error::ModelNotFound(req.model.clone()));
    };
    if !model.enabled {
        return Err(Error::ModelDisabled(model.id.clone()));
    }
    if model.modality != req.modality {
        return Err(Error::ModalityMismatch {
            model: model.id.clone(),
            have: model.modality.to_string(),
            need: req.modality.to_string(),
        });
    }
    Ok(Target {
        provider: model.provider.clone(),
        upstream_model: model.upstream.clone(),
        client_model: model.id.clone(),
        modality: model.modality,
        pricing: model.pricing.clone(),
        capabilities: model.capabilities.clone(),
        route: None,
        fallbacks: Vec::new(),
    })
}

fn follow_alias(aliases: &BTreeMap<String, String>, model: &str) -> String {
    let mut current = model.to_string();
    for _ in 0..4 {
        match aliases.get(&current) {
            Some(next) if next != &current => current = next.clone(),
            _ => break,
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;
    use airouter_core::{Modality, Pricing};

    fn sample() -> Registry {
        let model = ModelConfig {
            id: "anthropic/claude-sonnet-4-5".into(),
            provider: "anthropic".into(),
            upstream: "claude-sonnet-4-5".into(),
            modality: Modality::Chat,
            enabled: true,
            context: Some(200_000),
            capabilities: vec!["tools".into()],
            pricing: Pricing {
                input_per_mtok: Some(3.0),
                output_per_mtok: Some(15.0),
                ..Pricing::default()
            },
            quality: None,
            task_quality: BTreeMap::new(),
            latency_ms_prior: None,
        };
        let mut disabled = model.clone();
        disabled.id = "openai/gpt-4o".into();
        disabled.provider = "openai".into();
        disabled.upstream = "gpt-4o".into();
        disabled.enabled = false;
        let mut embed = model.clone();
        embed.id = "openai/text-embedding-3-small".into();
        embed.modality = Modality::Embedding;
        embed.upstream = "text-embedding-3-small".into();
        let mut aliases = BTreeMap::new();
        aliases.insert("code".into(), "anthropic/claude-sonnet-4-5".into());
        aliases.insert("best".into(), "code".into());
        Registry::from_models(vec![model, disabled, embed], aliases)
    }

    #[test]
    fn alias_chain_and_modality() {
        let reg = sample();
        let t = resolve(&reg, &RouteRequest::explicit("best", Modality::Chat)).unwrap();
        assert_eq!(t.upstream_model, "claude-sonnet-4-5");
        assert_eq!(t.client_model, "anthropic/claude-sonnet-4-5");

        let err = resolve(
            &reg,
            &RouteRequest::explicit("openai/text-embedding-3-small", Modality::Chat),
        )
        .unwrap_err();
        assert!(matches!(err, Error::ModalityMismatch { .. }));

        let err = resolve(
            &reg,
            &RouteRequest::explicit("openai/gpt-4o", Modality::Chat),
        )
        .unwrap_err();
        assert!(matches!(err, Error::ModelDisabled(_)));

        let err =
            resolve(&reg, &RouteRequest::explicit("auto/coding", Modality::Chat)).unwrap_err();
        assert!(matches!(err, Error::UnknownProfile(_)));
    }

    fn chat_model(id: &str, provider: &str, input: f64, tools: bool) -> ModelConfig {
        ModelConfig {
            id: id.into(),
            provider: provider.into(),
            upstream: id.rsplit('/').next().unwrap_or(id).into(),
            modality: Modality::Chat,
            enabled: true,
            context: Some(8_000),
            capabilities: if tools {
                vec!["tools".into(), "vision".into()]
            } else {
                vec!["vision".into()]
            },
            pricing: Pricing {
                input_per_mtok: Some(input),
                output_per_mtok: Some(input * 4.0),
                ..Pricing::default()
            },
            quality: None,
            task_quality: BTreeMap::new(),
            latency_ms_prior: Some(800),
        }
    }

    fn routed() -> Registry {
        use airouter_core::{RoutingProfile, RoutingTier};
        let cheap = chat_model("groq/fast", "groq", 0.1, true);
        let mut strong = chat_model("anthropic/strong", "anthropic", 3.0, true);
        strong.latency_ms_prior = Some(2_000);
        let no_tools = chat_model("local/plain", "local", 0.0, false);
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "cheap".into(),
            RoutingProfile {
                safe_model: strong.id.clone(),
                tiers: vec![
                    RoutingTier {
                        task: "chat".into(),
                        min_difficulty: 0,
                        max_difficulty: 4,
                        models: vec![cheap.id.clone(), strong.id.clone()],
                    },
                    RoutingTier {
                        task: "code".into(),
                        min_difficulty: 0,
                        max_difficulty: 4,
                        models: vec![strong.id.clone(), no_tools.id.clone()],
                    },
                ],
                confidence_floor: 0.5,
                lambda: 10.0,
                mu: 0.02,
                allow_providers: vec![],
                hosted_signal_clients: vec![],
            },
        );
        let mut routing = airouter_core::RoutingConfig::default();
        routing.default_profile = "cheap".into();
        routing.profiles = profiles;
        Registry::from_parts(vec![cheap, strong, no_tools], BTreeMap::new(), routing)
    }

    #[test]
    fn auto_picks_cheaper_chat_model_and_gates_low_confidence() {
        let reg = routed();
        let mut req = RouteRequest::explicit("auto/cheap", Modality::Chat);
        req.summary.last_user_text = "hello there".into();
        req.summary.token_estimate = 8;
        let target = resolve(&reg, &req).unwrap();
        assert_eq!(target.client_model, "groq/fast");
        assert_eq!(target.route.as_ref().unwrap().profile, "cheap");
        assert!(!target.fallbacks.is_empty());

        req.summary.last_user_text.clear();
        let gated = resolve(&reg, &req).unwrap();
        assert_eq!(gated.client_model, "anthropic/strong");
        assert!(gated.route.as_ref().unwrap().reason.contains("safe model"));
    }

    #[test]
    fn auto_drops_models_that_lack_tools_or_context() {
        let reg = routed();
        let mut req = RouteRequest::explicit("auto", Modality::Chat);
        req.summary.has_tools = true;
        req.summary.last_user_text = "fix the bug in ```rust fn main() {}".into();
        req.summary.token_estimate = 200;
        let target = resolve(&reg, &req).unwrap();
        assert_eq!(target.client_model, "anthropic/strong");
        let rejected = &target.route.as_ref().unwrap().rejected;
        assert!(rejected.iter().any(|r| r.contains("no tools")));

        req.summary.token_estimate = 50_000;
        req.summary.has_tools = false;
        req.summary.last_user_text = "hello".into();
        let err = resolve(&reg, &req).unwrap_err();
        assert!(matches!(err, Error::NoRoute(_)));
    }

    #[test]
    fn auto_skips_luna_when_tools_carry_reasoning_effort() {
        use airouter_core::{RoutingProfile, RoutingTier};
        let luna = chat_model("openai/gpt-6-luna", "openai", 0.1, true);
        let flash = chat_model("deepseek/deepseek-flash", "deepseek", 0.3, true);
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "default".into(),
            RoutingProfile {
                safe_model: flash.id.clone(),
                tiers: vec![RoutingTier {
                    task: "*".into(),
                    min_difficulty: 0,
                    max_difficulty: 4,
                    models: vec![luna.id.clone(), flash.id.clone()],
                }],
                confidence_floor: 0.0,
                lambda: 10.0,
                mu: 0.0,
                allow_providers: vec![],
                hosted_signal_clients: vec![],
            },
        );
        let mut routing = airouter_core::RoutingConfig::default();
        routing.default_profile = "default".into();
        routing.profiles = profiles;
        let reg = Registry::from_parts(vec![luna, flash], BTreeMap::new(), routing);
        let mut req = RouteRequest::explicit("auto", Modality::Chat);
        req.summary.has_tools = true;
        req.summary.last_user_text = "fix the bug in ```rust fn main() {}".into();
        req.summary.token_estimate = 200;
        let target = resolve(&reg, &req).unwrap();
        assert_eq!(target.client_model, "deepseek/deepseek-flash");
        assert!(target
            .route
            .as_ref()
            .unwrap()
            .rejected
            .iter()
            .any(|r| r.contains("gpt-6-luna")));
    }

    #[test]
    fn disabled_routing_refuses_auto() {
        let mut reg = routed();
        reg.routing.enabled = false;
        let err = resolve(&reg, &RouteRequest::explicit("auto/cheap", Modality::Chat)).unwrap_err();
        assert!(matches!(err, Error::AutoRoutingDisabled));
    }

    #[test]
    fn priced_chat_tier_prefers_the_cheaper_model() {
        let mini = ModelConfig {
            id: "openai/gpt-4o-mini".into(),
            provider: "openai".into(),
            upstream: "gpt-4o-mini".into(),
            modality: Modality::Chat,
            enabled: true,
            context: None,
            capabilities: vec![],
            pricing: Pricing {
                input_per_mtok: Some(0.15),
                output_per_mtok: Some(0.6),
                ..Pricing::default()
            },
            quality: None,
            task_quality: BTreeMap::new(),
            latency_ms_prior: None,
        };
        let claude = ModelConfig {
            id: "anthropic/claude".into(),
            provider: "anthropic".into(),
            upstream: "claude-sonnet-4-5".into(),
            modality: Modality::Chat,
            enabled: true,
            context: None,
            capabilities: vec![],
            pricing: Pricing {
                input_per_mtok: Some(3.0),
                output_per_mtok: Some(15.0),
                ..Pricing::default()
            },
            quality: None,
            task_quality: BTreeMap::new(),
            latency_ms_prior: None,
        };
        let mut routing = airouter_core::RoutingConfig::default();
        routing.profiles.insert(
            "cheap".into(),
            airouter_core::RoutingProfile {
                safe_model: claude.id.clone(),
                tiers: vec![airouter_core::RoutingTier {
                    task: "chat".into(),
                    min_difficulty: 0,
                    max_difficulty: 4,
                    models: vec![mini.id.clone(), claude.id.clone()],
                }],
                confidence_floor: 0.5,
                lambda: 50.0,
                mu: 0.02,
                allow_providers: vec![],
                hosted_signal_clients: vec![],
            },
        );
        let reg = Registry::from_parts(vec![mini, claude], BTreeMap::new(), routing);
        let mut req = RouteRequest::explicit("auto/cheap", Modality::Chat);
        req.summary.last_user_text = "hello".into();
        req.summary.token_estimate = 1;
        let target = resolve(&reg, &req).unwrap();
        assert_eq!(target.client_model, "openai/gpt-4o-mini");
        assert_eq!(target.fallbacks[0].client_model, "anthropic/claude");
    }

    #[test]
    fn replay_compares_recorded_cost_with_the_safe_model() {
        let reg = routed();
        let signals = airouter_core::Signals {
            source: "local".into(),
            task: "chat".into(),
            difficulty: 0,
            confidence: 0.8,
            needs_tools: false,
            needs_reasoning: false,
            contains_code: false,
        };
        let row = ReplayRow {
            requested_model: "auto/cheap".into(),
            upstream_model: "fast".into(),
            status: 200,
            input_tokens: 1_000,
            output_tokens: 100,
            cost_usd: 0.0002,
            route_signals: serde_json::to_string(&signals).unwrap(),
            route_profile: "cheap".into(),
            feedback: Some(1),
        };
        let report = replay(&reg, &[row]);
        assert_eq!(report.auto_requests, 1);
        assert_eq!(report.compared, 1);
        assert!(report.all_strong_cost > report.recorded_cost);
        assert_eq!(report.feedback_up, 1);
        assert!(report.agreement > 0.0);
    }

    #[test]
    fn catalog_contains_known_models() {
        let hit = find_catalog("groq/llama-3.3-70b-versatile").unwrap();
        assert_eq!(hit.upstream, "llama-3.3-70b-versatile");
        assert!(!search_catalog("whisper").is_empty());
    }
}
