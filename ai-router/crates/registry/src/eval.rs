//! Offline comparison of single-model, always-MoA, and on-demand decisions.
//!
//! This does not call a provider. Quality numbers come from the fixture when
//! the case includes them.

use airouter_core::{
    decide, estimate_moa_cost, MoaConfig, MoaDecisionInput, MoaOverride, Modality, RequestSummary,
    RouteRequest,
};
use serde::{Deserialize, Serialize};

use crate::{local_signals, resolve, Registry};

#[derive(Debug, Clone, Deserialize)]
pub struct EvalCase {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub tokens: Option<u64>,
    #[serde(default)]
    pub has_tools: bool,
    #[serde(default)]
    pub followup: bool,
    #[serde(default)]
    pub single_quality: Option<f64>,
    #[serde(default)]
    pub moa_quality: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalRow {
    pub id: String,
    pub task: String,
    pub difficulty: u8,
    pub single_model: String,
    pub single_cost_usd: f64,
    pub on_demand: String,
    pub on_demand_reason: String,
    pub always_cost_usd: f64,
    pub single_quality: Option<f64>,
    pub moa_quality: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalReport {
    pub profile: String,
    pub cases: Vec<EvalRow>,
    pub on_demand_moa: u64,
    pub avg_single_quality: Option<f64>,
    pub avg_moa_quality: Option<f64>,
    pub avg_single_cost_usd: f64,
    pub avg_always_cost_usd: f64,
    pub avg_on_demand_cost_usd: f64,
}

pub fn evaluate(
    registry: &Registry,
    moa: &MoaConfig,
    profile_name: &str,
    cases: &[EvalCase],
) -> Result<EvalReport, String> {
    let routing = registry
        .routing
        .profiles
        .get(profile_name)
        .ok_or_else(|| format!("unknown routing profile '{profile_name}'"))?;
    let moa_name = routing
        .moa_profile
        .as_deref()
        .ok_or_else(|| format!("profile '{profile_name}' has no moa_profile"))?;
    let moa_profile = moa
        .profiles
        .get(moa_name)
        .ok_or_else(|| format!("moa profile '{moa_name}' is not configured"))?;
    let mut proposer_pricing = Vec::new();
    for proposer in &moa_profile.proposers {
        let model = registry
            .find(&proposer.model)
            .ok_or_else(|| format!("proposer '{}' is not configured", proposer.model))?;
        proposer_pricing.push(model.pricing.clone());
    }
    let aggregator = registry
        .find(&moa_profile.aggregator)
        .ok_or_else(|| format!("aggregator '{}' is not configured", moa_profile.aggregator))?;
    let aggregator_pricing = aggregator.pricing.clone();
    let triggers = routing.moa_triggers.clone().unwrap_or_default();
    let mut rows = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let mut summary = RequestSummary {
            token_estimate: case
                .tokens
                .unwrap_or_else(|| (case.text.len() as u64 / 4).max(1)),
            has_tools: case.has_tools,
            last_user_text: case.text.clone(),
            client_label: "eval".into(),
            ..RequestSummary::default()
        };
        if case.followup {
            summary.token_estimate = summary.token_estimate.max(1);
        }
        let signals = local_signals(&summary);
        let req = RouteRequest {
            model: format!("auto/{profile_name}"),
            modality: Modality::Chat,
            summary: summary.clone(),
            signals: Some(signals.clone()),
            mode_override: None,
        };
        let target = resolve(registry, &req).map_err(|e| e.to_string())?;
        let route = target
            .route
            .as_ref()
            .ok_or("auto did not produce a route")?;
        let selected = airouter_core::SelectedModel {
            id: target.client_model.clone(),
            task_quality: route.selected_quality,
            estimated_cost_usd: route.estimated_cost_usd,
            required_quality: route.required_quality,
            explicit_prior: route.explicit_quality_prior,
        };
        let input = MoaDecisionInput {
            signals: &signals,
            summary: &summary,
            client: "eval",
            override_: MoaOverride::Auto,
            is_followup: case.followup,
            selected: &selected,
            triggers: &triggers,
            profile: moa_profile,
            proposer_pricing: &proposer_pricing,
            aggregator_pricing: &aggregator_pricing,
        };
        let on_demand = decide(&input);
        let always_cost = estimate_moa_cost(
            &proposer_pricing,
            &aggregator_pricing,
            summary.token_estimate,
            moa_profile.proposer_max_tokens,
            moa_profile.aggregator_max_tokens,
        );
        rows.push(EvalRow {
            id: if case.id.is_empty() {
                format!("case-{index}")
            } else {
                case.id.clone()
            },
            task: signals.task.clone(),
            difficulty: signals.difficulty,
            single_model: target.client_model,
            single_cost_usd: route.estimated_cost_usd,
            on_demand: on_demand.header_value().into(),
            on_demand_reason: on_demand.reason().to_string(),
            always_cost_usd: always_cost,
            single_quality: case.single_quality,
            moa_quality: case.moa_quality,
        });
    }
    let on_demand_moa = rows.iter().filter(|row| row.on_demand == "full").count() as u64;
    let avg_single_cost_usd = mean(rows.iter().map(|row| row.single_cost_usd));
    let avg_always_cost_usd = mean(rows.iter().map(|row| row.always_cost_usd));
    let avg_on_demand_cost_usd = mean(rows.iter().map(|row| match row.on_demand.as_str() {
        "full" => row.always_cost_usd,
        _ => row.single_cost_usd,
    }));
    Ok(EvalReport {
        profile: profile_name.to_string(),
        avg_single_quality: mean_opt(rows.iter().filter_map(|row| row.single_quality)),
        avg_moa_quality: mean_opt(rows.iter().filter_map(|row| row.moa_quality)),
        on_demand_moa,
        avg_single_cost_usd,
        avg_always_cost_usd,
        avg_on_demand_cost_usd,
        cases: rows,
    })
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let values: Vec<f64> = values.collect();
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn mean_opt(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<f64> = values.collect();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}
