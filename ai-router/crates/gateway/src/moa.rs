//! On-demand mixture-of-agents execution.
//!
//! Proposers are non-streaming. The aggregator is streamed only when the client
//! asked for SSE, and only after the fan-out finishes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use airouter_core::{
    compute_cost, conversation_has_assistant, decide, now_unix, redact, Error, MoaCallReport,
    MoaConfig, MoaDecisionInput, MoaMode, MoaOverride, MoaProfile, MoaReport, Modality, Pricing,
    RequestSummary, RouteRequest, RoutingMode, RoutingStrategy, SelectedModel, Target, TokenUsage,
    UsageMeta,
};
use airouter_providers::{CallContext, ChatResult, Provider, ProviderError};
use airouter_registry::Registry;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures::stream::{self, StreamExt};
use serde_json::{json, Value};

use crate::{
    attach_route, fail_core, json_response, new_id, sse_response, AppState, ClientId, Finish,
};

pub fn overrides(headers: &HeaderMap) -> Result<(Option<RoutingMode>, MoaOverride), String> {
    let mode = match header_value(headers, "x-airouter-mode") {
        None => None,
        Some(value) => Some(RoutingMode::parse(&value).ok_or_else(|| {
            format!("x-airouter-mode must be budget, balanced, quality, or speed")
        })?),
    };
    let moa = match header_value(headers, "x-airouter-moa") {
        None => MoaOverride::Auto,
        Some(value) => MoaOverride::parse(&value)
            .ok_or_else(|| "x-airouter-moa must be force, off, or auto".to_string())?,
    };
    Ok((mode, moa))
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

struct Snap {
    registry: Registry,
    moa: MoaConfig,
    providers: HashMap<String, Arc<dyn Provider>>,
    timeouts: HashMap<String, Duration>,
    max_retries: u32,
    backoff_ms: u64,
}

fn snapshot(state: &AppState) -> Snap {
    let rt = state.read();
    Snap {
        registry: rt.registry.clone(),
        moa: rt.file.moa.clone(),
        providers: rt.providers.clone(),
        timeouts: rt
            .resolved
            .providers
            .iter()
            .map(|(name, provider)| (name.clone(), provider.timeout))
            .collect(),
        max_retries: rt.file.server.retry.max_retries,
        backoff_ms: rt.file.server.retry.backoff_ms,
    }
}

pub async fn route_chat(
    state: AppState,
    client: ClientId,
    override_: MoaOverride,
    body: Value,
    mut target: Target,
    request_id: String,
    started: Instant,
) -> Response {
    let Some(profile_name) = target.route.as_ref().map(|route| route.profile.clone()) else {
        return crate::dispatch_chat(state, client, body, target, request_id, started).await;
    };
    let snap = snapshot(&state);
    let Some(routing) = snap.registry.routing.profiles.get(&profile_name).cloned() else {
        return crate::dispatch_chat(state, client, body, target, request_id, started).await;
    };
    let consider =
        routing.strategy == RoutingStrategy::MoaOnDemand || override_ != MoaOverride::Auto;
    if !consider {
        return crate::dispatch_chat(state, client, body, target, request_id, started).await;
    }
    if !snap.moa.enabled && override_ != MoaOverride::Force {
        note(&mut target, "moa disabled", "single", "");
        return crate::dispatch_chat(state, client, body, target, request_id, started).await;
    }
    if !snap.moa.enabled && override_ == MoaOverride::Force {
        return fail_core(
            &state,
            &client,
            &requested_model(&body, &target),
            Modality::Chat,
            "/v1/chat/completions",
            request_id,
            started,
            Error::Invalid("moa is disabled".into()),
        );
    }
    let Some(moa_name) = routing.moa_profile.clone() else {
        if override_ == MoaOverride::Force || routing.strategy == RoutingStrategy::MoaOnDemand {
            return fail_core(
                &state,
                &client,
                &requested_model(&body, &target),
                Modality::Chat,
                "/v1/chat/completions",
                request_id,
                started,
                Error::Invalid(format!(
                    "routing profile '{profile_name}' has no moa_profile"
                )),
            );
        }
        return crate::dispatch_chat(state, client, body, target, request_id, started).await;
    };
    let Some(moa_profile) = snap.moa.profiles.get(&moa_name).cloned() else {
        return fail_core(
            &state,
            &client,
            &requested_model(&body, &target),
            Modality::Chat,
            "/v1/chat/completions",
            request_id,
            started,
            Error::Invalid(format!("moa profile '{moa_name}' is not configured")),
        );
    };
    let signals = target
        .route
        .as_ref()
        .map(|route| airouter_core::Signals {
            source: route.source.clone(),
            task: route.task.clone(),
            difficulty: route.difficulty,
            confidence: route.confidence,
            needs_tools: route.needs_tools,
            needs_reasoning: route.needs_reasoning,
            contains_code: route.contains_code,
        })
        .unwrap_or_default();
    let summary = summarize_for_policy(&body, &client.0, &target);
    let selected = selected_from(&target);
    let triggers = routing.moa_triggers.clone().unwrap_or_default();
    let proposer_pricing = match pricing_list(&snap.registry, &moa_profile) {
        Ok(prices) => prices,
        Err(err) => {
            return fail_core(
                &state,
                &client,
                &requested_model(&body, &target),
                Modality::Chat,
                "/v1/chat/completions",
                request_id,
                started,
                err,
            )
        }
    };
    let aggregator_pricing = proposer_pricing.aggregator.clone();
    let decision = decide(&MoaDecisionInput {
        signals: &signals,
        summary: &summary,
        client: &client.0,
        override_,
        is_followup: is_followup(&body),
        selected: &selected,
        triggers: &triggers,
        profile: &moa_profile,
        proposer_pricing: &proposer_pricing.proposers,
        aggregator_pricing: &aggregator_pricing,
    });
    match decision {
        MoaMode::Single { reason } => {
            note(&mut target, &reason, "single", &moa_name);
            crate::dispatch_chat(state, client, body, target, request_id, started).await
        }
        MoaMode::Worker { reason } => {
            note(&mut target, &reason, "worker", &moa_name);
            match retarget(&snap.registry, &mut target, moa_profile.worker()) {
                Ok(()) => {
                    crate::dispatch_chat(state, client, body, target, request_id, started).await
                }
                Err(err) => fail_core(
                    &state,
                    &client,
                    &requested_model(&body, &target),
                    Modality::Chat,
                    "/v1/chat/completions",
                    request_id,
                    started,
                    err,
                ),
            }
        }
        MoaMode::Moa {
            reason,
            estimated_cost_usd,
        } => {
            note(&mut target, &reason, "full", &moa_name);
            if let Some(route) = target.route.as_mut() {
                route.estimated_cost_usd = estimated_cost_usd;
            }
            execute(
                state,
                client,
                snap,
                body,
                target,
                moa_name,
                moa_profile,
                estimated_cost_usd,
                request_id,
                started,
            )
            .await
        }
    }
}

struct PriceList {
    proposers: Vec<Pricing>,
    aggregator: Pricing,
}

fn pricing_list(registry: &Registry, profile: &MoaProfile) -> Result<PriceList, Error> {
    let mut proposers = Vec::new();
    for proposer in &profile.proposers {
        proposers.push(lookup(registry, &proposer.model)?.pricing.clone());
    }
    let aggregator = lookup(registry, &profile.aggregator)?.pricing.clone();
    Ok(PriceList {
        proposers,
        aggregator,
    })
}

fn lookup<'a>(registry: &'a Registry, id: &str) -> Result<&'a airouter_core::ModelConfig, Error> {
    if id == "auto" || id.starts_with("auto/") || id == "moa" || id.starts_with("moa/") {
        return Err(Error::Invalid(format!(
            "moa model '{id}' cannot be auto or moa"
        )));
    }
    let model = registry
        .find(id)
        .ok_or_else(|| Error::ModelNotFound(id.to_string()))?;
    if !model.enabled {
        return Err(Error::ModelDisabled(id.to_string()));
    }
    if model.modality != Modality::Chat {
        return Err(Error::ModalityMismatch {
            model: id.to_string(),
            have: model.modality.to_string(),
            need: Modality::Chat.to_string(),
        });
    }
    Ok(model)
}

fn selected_from(target: &Target) -> SelectedModel {
    let route = target.route.as_ref();
    SelectedModel {
        id: target.client_model.clone(),
        task_quality: route.map(|r| r.selected_quality).unwrap_or(0.5),
        estimated_cost_usd: route.map(|r| r.estimated_cost_usd).unwrap_or(0.0),
        required_quality: route.map(|r| r.required_quality).unwrap_or(0.0),
        explicit_prior: route.is_some_and(|r| r.explicit_quality_prior),
    }
}

fn summarize_for_policy(body: &Value, client: &str, target: &Target) -> RequestSummary {
    let mut summary = crate::summarize(body, client);
    if let Some(route) = &target.route {
        if summary.client_label.is_empty() {
            summary.client_label = client.to_string();
        }
        let _ = route;
    }
    summary
}

fn is_followup(body: &Value) -> bool {
    body.get("messages")
        .and_then(|m| m.as_array())
        .is_some_and(|messages| conversation_has_assistant(messages))
}

fn note(target: &mut Target, reason: &str, mode: &str, profile: &str) {
    if let Some(route) = target.route.as_mut() {
        route.moa_mode = mode.to_string();
        if route.reason.is_empty() {
            route.reason = reason.to_string();
        } else if !route.reason.contains(reason) {
            route.reason = format!("{}; {reason}", route.reason);
        }
        route.selection_reason = route.reason.clone();
    }
    target.moa = Some(MoaReport {
        mode: mode.to_string(),
        profile: profile.to_string(),
        reason: reason.to_string(),
        estimated_cost_usd: target
            .route
            .as_ref()
            .map(|r| r.estimated_cost_usd)
            .unwrap_or(0.0),
        proposers: Vec::new(),
        aggregator: String::new(),
        quorum: 0,
        degraded: false,
        degrade_reason: String::new(),
    });
}

fn retarget(registry: &Registry, target: &mut Target, model_id: &str) -> Result<(), Error> {
    let model = lookup(registry, model_id)?;
    target.provider = model.provider.clone();
    target.upstream_model = model.upstream.clone();
    target.client_model = model.id.clone();
    target.modality = model.modality;
    target.pricing = model.pricing.clone();
    target.capabilities = model.capabilities.clone();
    target.fallbacks.clear();
    Ok(())
}

struct Attempt {
    client_model: String,
    provider: String,
    upstream_model: String,
    pricing: Pricing,
    status: u16,
    latency_ms: u64,
    cost: f64,
    text: String,
    usage: TokenUsage,
    error: Option<String>,
}

impl Attempt {
    fn ok(&self) -> bool {
        self.status < 400 && !self.text.trim().is_empty()
    }

    fn report(&self) -> MoaCallReport {
        MoaCallReport {
            model: self.client_model.clone(),
            status: self.status,
            latency_ms: self.latency_ms,
            cost_usd: self.cost,
        }
    }
}

async fn execute(
    state: AppState,
    client: ClientId,
    snap: Snap,
    body: Value,
    mut target: Target,
    moa_name: String,
    profile: MoaProfile,
    estimated_cost_usd: f64,
    request_id: String,
    started: Instant,
) -> Response {
    if let Err(message) = providers_allowed(&snap.registry, &profile) {
        return fail_core(
            &state,
            &client,
            &requested_model(&body, &target),
            Modality::Chat,
            "/v1/chat/completions",
            request_id,
            started,
            Error::Invalid(message),
        );
    }
    let proposer_targets = match resolve_ids(
        &snap.registry,
        &profile
            .proposers
            .iter()
            .map(|p| p.model.clone())
            .collect::<Vec<_>>(),
    ) {
        Ok(t) => t,
        Err(err) => {
            return fail_core(
                &state,
                &client,
                &requested_model(&body, &target),
                Modality::Chat,
                "/v1/chat/completions",
                request_id,
                started,
                err,
            )
        }
    };
    let deadline = Duration::from_millis(profile.overall_timeout_ms);
    let mut attempts = fan_out(
        &snap,
        &proposer_targets,
        &body,
        &profile.proposer_prompt,
        profile.proposer_max_tokens,
        profile.allow_tools,
        profile.max_concurrency,
        deadline,
    )
    .await;
    if attempts.iter().filter(|a| a.ok()).count() < profile.quorum as usize {
        let retries = retry_targets(&snap.registry, &profile, &proposer_targets, &attempts);
        if !retries.is_empty() {
            let more = fan_out(
                &snap,
                &retries,
                &body,
                &profile.proposer_prompt,
                profile.proposer_max_tokens,
                profile.allow_tools,
                profile.max_concurrency,
                deadline,
            )
            .await;
            for attempt in more {
                if let Some(slot) = attempts.iter_mut().find(|existing| {
                    !existing.ok() && existing.client_model == attempt.client_model
                }) {
                    *slot = attempt;
                } else {
                    attempts.push(attempt);
                }
            }
        }
    }
    record_children(
        &state,
        &client,
        &body,
        &target,
        &request_id,
        &attempts,
        "proposer",
    );
    if attempts.iter().filter(|a| a.ok()).count() < profile.quorum as usize {
        return degrade_worker(
            state,
            client,
            snap,
            body,
            target,
            &moa_name,
            &profile,
            estimated_cost_usd,
            &attempts,
            request_id,
            started,
            "quorum",
        )
        .await;
    }
    let candidates: Vec<(String, String)> = attempts
        .iter()
        .filter(|a| a.ok())
        .map(|a| (a.client_model.clone(), a.text.clone()))
        .collect();
    let aggregator = match explicit_target(&snap.registry, &profile.aggregator) {
        Ok(t) => t,
        Err(err) => {
            return fail_core(
                &state,
                &client,
                &requested_model(&body, &target),
                Modality::Chat,
                "/v1/chat/completions",
                request_id,
                started,
                err,
            )
        }
    };
    let want_stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if want_stream {
        return stream_aggregator(
            state,
            client,
            snap,
            body,
            target,
            moa_name,
            profile,
            estimated_cost_usd,
            attempts,
            aggregator,
            candidates,
            request_id,
            started,
        )
        .await;
    }
    let mut agg_attempt = one_call(
        &snap,
        &aggregator,
        aggregator_body(
            &body,
            &profile.aggregator_prompt,
            &candidates,
            profile.aggregator_max_tokens,
            false,
        ),
        deadline,
    )
    .await;
    if agg_attempt.as_ref().ok().is_none_or(|a| !a.ok()) {
        for fallback in &profile.aggregator_fallbacks {
            if let Ok(fallback_target) = explicit_target(&snap.registry, fallback) {
                agg_attempt = one_call(
                    &snap,
                    &fallback_target,
                    aggregator_body(
                        &body,
                        &profile.aggregator_prompt,
                        &candidates,
                        profile.aggregator_max_tokens,
                        false,
                    ),
                    deadline,
                )
                .await;
                if agg_attempt.as_ref().is_ok_and(|a| a.ok()) {
                    break;
                }
            }
        }
    }
    match agg_attempt {
        Ok(attempt) if attempt.ok() => {
            record_children(
                &state,
                &client,
                &body,
                &target,
                &request_id,
                std::slice::from_ref(&attempt),
                "aggregator",
            );
            let value = completion_from_text(
                &request_id,
                &attempt.client_model,
                &attempt.text,
                &attempt.usage,
            );
            finish_parent(
                &state,
                &client,
                &body,
                &mut target,
                &moa_name,
                &profile,
                estimated_cost_usd,
                &attempts,
                Some(&attempt),
                false,
                "",
                &request_id,
                started,
                200,
                None,
            );
            let mut value = value;
            retarget_answer(&mut target, &attempt);
            target.moa = Some(report(
                &moa_name,
                &profile,
                estimated_cost_usd,
                &attempts,
                false,
                "",
                &target,
            ));
            attach_route(&mut value, &target);
            json_response(StatusCode::OK, value, &request_id, Some(&target))
        }
        _ => {
            let Some((model, text)) = best_candidate(&attempts) else {
                return degrade_worker(
                    state,
                    client,
                    snap,
                    body,
                    target,
                    &moa_name,
                    &profile,
                    estimated_cost_usd,
                    &attempts,
                    request_id,
                    started,
                    "aggregator",
                )
                .await;
            };
            let value = completion_from_text(&request_id, &model, &text, &TokenUsage::default());
            finish_parent(
                &state,
                &client,
                &body,
                &mut target,
                &moa_name,
                &profile,
                estimated_cost_usd,
                &attempts,
                None,
                true,
                "aggregator",
                &request_id,
                started,
                200,
                None,
            );
            if let Ok(()) = retarget(&snap.registry, &mut target, &model) {}
            target.moa = Some(report(
                &moa_name,
                &profile,
                estimated_cost_usd,
                &attempts,
                true,
                "aggregator",
                &target,
            ));
            let mut value = value;
            attach_route(&mut value, &target);
            json_response(StatusCode::OK, value, &request_id, Some(&target))
        }
    }
}

fn providers_allowed(registry: &Registry, profile: &MoaProfile) -> Result<(), String> {
    if profile.allow_providers.is_empty() {
        return Ok(());
    }
    for proposer in &profile.proposers {
        let Some(model) = registry.find(&proposer.model) else {
            continue;
        };
        if !profile
            .allow_providers
            .iter()
            .any(|name| name == &model.provider)
        {
            return Err(format!(
                "proposer '{}' is outside moa allow_providers",
                proposer.model
            ));
        }
    }
    Ok(())
}

fn resolve_ids(registry: &Registry, ids: &[String]) -> Result<Vec<Target>, Error> {
    ids.iter().map(|id| explicit_target(registry, id)).collect()
}

fn explicit_target(registry: &Registry, id: &str) -> Result<Target, Error> {
    let _ = lookup(registry, id)?;
    airouter_registry::resolve(registry, &RouteRequest::explicit(id, Modality::Chat))
}

fn retry_targets(
    registry: &Registry,
    profile: &MoaProfile,
    originals: &[Target],
    attempts: &[Attempt],
) -> Vec<Target> {
    let mut out = Vec::new();
    for (proposer, original) in profile.proposers.iter().zip(originals.iter()) {
        let failed = attempts
            .iter()
            .any(|a| a.client_model == original.client_model && !a.ok());
        if !failed {
            continue;
        }
        let fallback = proposer
            .fallbacks
            .first()
            .map(String::as_str)
            .unwrap_or_else(|| profile.worker());
        if let Ok(target) = explicit_target(registry, fallback) {
            out.push(target);
        }
    }
    out
}

async fn fan_out(
    snap: &Snap,
    targets: &[Target],
    body: &Value,
    prompt: &str,
    max_tokens: u64,
    allow_tools: bool,
    concurrency: usize,
    deadline: Duration,
) -> Vec<Attempt> {
    let bodies: Vec<(Target, Value)> = targets
        .iter()
        .cloned()
        .map(|target| (target, proposer_body(body, prompt, max_tokens, allow_tools)))
        .collect();
    let concurrency = concurrency.max(1);
    let futs = bodies
        .into_iter()
        .map(|(target, body)| async move { one_call(snap, &target, body, deadline).await });
    let collected = tokio::time::timeout(deadline, async {
        let mut stream = stream::iter(futs).buffer_unordered(concurrency);
        let mut out = Vec::new();
        while let Some(result) = stream.next().await {
            out.push(result);
        }
        out
    })
    .await;
    match collected {
        Ok(results) => results
            .into_iter()
            .map(|result| match result {
                Ok(attempt) => attempt,
                Err((target, err)) => failed_attempt(&target, &err),
            })
            .collect(),
        Err(_) => targets
            .iter()
            .map(|target| Attempt {
                client_model: target.client_model.clone(),
                provider: target.provider.clone(),
                upstream_model: target.upstream_model.clone(),
                pricing: target.pricing.clone(),
                status: 504,
                latency_ms: deadline.as_millis() as u64,
                cost: 0.0,
                text: String::new(),
                usage: TokenUsage::default(),
                error: Some("timeout".into()),
            })
            .collect(),
    }
}

async fn one_call(
    snap: &Snap,
    target: &Target,
    mut body: Value,
    deadline: Duration,
) -> Result<Attempt, (Target, String)> {
    body["stream"] = json!(false);
    let Some(provider) = snap.providers.get(&target.provider).cloned() else {
        return Err((
            target.clone(),
            format!("provider {} missing", target.provider),
        ));
    };
    let ctx = context(snap, target);
    let started = Instant::now();
    let timeout = deadline.min(ctx.timeout);
    let result = tokio::time::timeout(timeout, provider.chat(body, &ctx)).await;
    let latency_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(Ok(ChatResult::Complete(value))) => {
            let usage = TokenUsage::from_openai_chat(&value);
            let text = message_text(&value);
            let cost = compute_cost(&target.pricing, &usage, Modality::Chat);
            Ok(Attempt {
                client_model: target.client_model.clone(),
                provider: target.provider.clone(),
                upstream_model: target.upstream_model.clone(),
                pricing: target.pricing.clone(),
                status: 200,
                latency_ms,
                cost,
                text,
                usage,
                error: None,
            })
        }
        Ok(Ok(ChatResult::Stream(_))) => Err((target.clone(), "proposer returned a stream".into())),
        Ok(Err(err)) => Err((target.clone(), err_label(&err))),
        Err(_) => Err((target.clone(), "timeout".into())),
    }
}

fn failed_attempt(target: &Target, err: &str) -> Attempt {
    let status = if err == "timeout" {
        504
    } else if let Some(rest) = err.strip_prefix("upstream ") {
        rest.parse().unwrap_or(502)
    } else {
        502
    };
    Attempt {
        client_model: target.client_model.clone(),
        provider: target.provider.clone(),
        upstream_model: target.upstream_model.clone(),
        pricing: target.pricing.clone(),
        status,
        latency_ms: 0,
        cost: 0.0,
        text: String::new(),
        usage: TokenUsage::default(),
        error: Some(err.to_string()),
    }
}

fn err_label(err: &ProviderError) -> String {
    match err {
        ProviderError::Upstream { status, .. } => format!("upstream {status}"),
        ProviderError::Transport(_) => "upstream 502".into(),
        other => redact(&other.to_string()),
    }
}

fn context(snap: &Snap, target: &Target) -> CallContext {
    CallContext {
        client_model: target.client_model.clone(),
        upstream_model: target.upstream_model.clone(),
        timeout: snap
            .timeouts
            .get(&target.provider)
            .copied()
            .unwrap_or_else(|| Duration::from_secs(180)),
        max_retries: snap.max_retries,
        backoff_ms: snap.backoff_ms,
    }
}

fn proposer_body(original: &Value, prompt: &str, max_tokens: u64, allow_tools: bool) -> Value {
    let mut body = original.clone();
    body["stream"] = json!(false);
    if let Some(map) = body.as_object_mut() {
        map.remove("stream_options");
        if !allow_tools {
            map.remove("tools");
            map.remove("tool_choice");
        }
    }
    body["max_tokens"] = json!(max_tokens);
    prepend_system(&mut body, prompt);
    body
}

fn aggregator_body(
    original: &Value,
    prompt: &str,
    candidates: &[(String, String)],
    cap: u64,
    stream: bool,
) -> Value {
    let mut body = original.clone();
    body["stream"] = json!(stream);
    if let Some(map) = body.as_object_mut() {
        map.remove("tools");
        map.remove("tool_choice");
        if !stream {
            map.remove("stream_options");
        }
    }
    cap_max_tokens(&mut body, cap);
    prepend_system(&mut body, prompt);
    let block = candidate_block(candidates);
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        messages.push(json!({"role": "user", "content": block}));
    }
    body
}

fn candidate_block(candidates: &[(String, String)]) -> String {
    let mut out = String::from("Candidate answers:\n");
    for (index, (model, text)) in candidates.iter().enumerate() {
        out.push_str(&format!("### Candidate {} ({model})\n{text}\n", index + 1));
    }
    out
}

fn cap_max_tokens(body: &mut Value, cap: u64) {
    let current = body.get("max_tokens").and_then(|v| v.as_u64());
    body["max_tokens"] = json!(current.map(|value| value.min(cap)).unwrap_or(cap));
    if let Some(current) = body.get("max_completion_tokens").and_then(|v| v.as_u64()) {
        body["max_completion_tokens"] = json!(current.min(cap));
    }
}

fn prepend_system(body: &mut Value, prompt: &str) {
    let message = json!({"role": "system", "content": prompt});
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        messages.insert(0, message);
    } else {
        body["messages"] = json!([message]);
    }
}

fn message_text(value: &Value) -> String {
    let content = &value["choices"][0]["message"]["content"];
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn best_candidate(attempts: &[Attempt]) -> Option<(String, String)> {
    attempts
        .iter()
        .filter(|a| a.ok())
        .max_by_key(|a| a.text.len())
        .map(|a| (a.client_model.clone(), a.text.clone()))
}

fn completion_from_text(id: &str, model: &str, text: &str, usage: &TokenUsage) -> Value {
    json!({
        "id": format!("chatcmpl-{id}"),
        "object": "chat.completion",
        "created": now_unix(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": text},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": usage.input_tokens,
            "completion_tokens": usage.output_tokens,
        }
    })
}

fn retarget_answer(target: &mut Target, attempt: &Attempt) {
    target.provider = attempt.provider.clone();
    target.upstream_model = attempt.upstream_model.clone();
    target.client_model = attempt.client_model.clone();
    target.pricing = attempt.pricing.clone();
    target.fallbacks.clear();
}

async fn degrade_worker(
    state: AppState,
    client: ClientId,
    snap: Snap,
    body: Value,
    mut target: Target,
    moa_name: &str,
    profile: &MoaProfile,
    estimated_cost_usd: f64,
    attempts: &[Attempt],
    request_id: String,
    started: Instant,
    why: &str,
) -> Response {
    let worker = match explicit_target(&snap.registry, profile.worker()) {
        Ok(worker) => worker,
        Err(err) => {
            return fail_core(
                &state,
                &client,
                &requested_model(&body, &target),
                Modality::Chat,
                "/v1/chat/completions",
                request_id,
                started,
                err,
            )
        }
    };
    let mut worker_body = body.clone();
    worker_body["stream"] = json!(false);
    match one_call(
        &snap,
        &worker,
        worker_body,
        Duration::from_millis(profile.overall_timeout_ms),
    )
    .await
    {
        Ok(attempt) if attempt.ok() => {
            record_children(
                &state,
                &client,
                &body,
                &target,
                &request_id,
                std::slice::from_ref(&attempt),
                "worker",
            );
            let mut value = completion_from_text(
                &request_id,
                &attempt.client_model,
                &attempt.text,
                &attempt.usage,
            );
            finish_parent(
                &state,
                &client,
                &body,
                &mut target,
                moa_name,
                profile,
                estimated_cost_usd,
                attempts,
                Some(&attempt),
                true,
                why,
                &request_id,
                started,
                200,
                None,
            );
            retarget_answer(&mut target, &attempt);
            target.moa = Some(report(
                moa_name,
                profile,
                estimated_cost_usd,
                attempts,
                true,
                why,
                &target,
            ));
            attach_route(&mut value, &target);
            json_response(StatusCode::OK, value, &request_id, Some(&target))
        }
        Ok(attempt) => provider_error(
            &state,
            &client,
            &body,
            &mut target,
            moa_name,
            profile,
            estimated_cost_usd,
            attempts,
            &request_id,
            started,
            attempt.status,
            attempt.error.unwrap_or_else(|| "worker failed".into()),
        ),
        Err((_, err)) => provider_error(
            &state,
            &client,
            &body,
            &mut target,
            moa_name,
            profile,
            estimated_cost_usd,
            attempts,
            &request_id,
            started,
            502,
            err,
        ),
    }
}

async fn stream_aggregator(
    state: AppState,
    client: ClientId,
    snap: Snap,
    body: Value,
    mut target: Target,
    moa_name: String,
    profile: MoaProfile,
    estimated_cost_usd: f64,
    attempts: Vec<Attempt>,
    aggregator: Target,
    candidates: Vec<(String, String)>,
    request_id: String,
    started: Instant,
) -> Response {
    let agg_body = aggregator_body(
        &body,
        &profile.aggregator_prompt,
        &candidates,
        profile.aggregator_max_tokens,
        true,
    );
    let Some(provider) = snap.providers.get(&aggregator.provider).cloned() else {
        return degrade_to_candidate(
            &state,
            &client,
            &body,
            &mut target,
            &moa_name,
            &profile,
            estimated_cost_usd,
            &attempts,
            &request_id,
            started,
        );
    };
    let ctx = context(&snap, &aggregator);
    match provider.chat(agg_body, &ctx).await {
        Ok(ChatResult::Stream(upstream)) => {
            let prior_cost: f64 = attempts.iter().map(|a| a.cost).sum();
            let child_meta = child_meta(
                &client,
                &body,
                &target,
                &request_id,
                &aggregator.client_model,
                &aggregator.provider,
                &aggregator.upstream_model,
                &aggregator.pricing,
                "aggregator",
            );
            let parent_base = parent_shell(
                &client,
                &body,
                &target,
                &moa_name,
                &profile,
                estimated_cost_usd,
                &attempts,
                false,
                "",
                &request_id,
            );
            let mapped = upstream.map(|item| item.map_err(|e| e.to_string()));
            let observed = crate::observe::Observe::new(
                mapped,
                Box::new(move |finish| {
                    state.record(
                        &child_meta,
                        Finish {
                            usage: finish.usage.clone(),
                            ttft_ms: finish.ttft_ms,
                            status: finish.status,
                            error: finish.error.clone(),
                            latency_ms: finish.latency_ms,
                        },
                    );
                    let agg_cost = compute_cost(&child_meta.pricing, &finish.usage, Modality::Chat);
                    let mut parent = parent_base.clone();
                    parent.cost_override = Some(prior_cost + agg_cost);
                    state.record(
                        &parent,
                        Finish {
                            usage: finish.usage,
                            ttft_ms: finish.ttft_ms,
                            status: finish.status,
                            error: finish.error,
                            latency_ms: started.elapsed().as_millis() as u64,
                        },
                    );
                }),
            );
            target.moa = Some(report(
                &moa_name,
                &profile,
                estimated_cost_usd,
                &attempts,
                false,
                "",
                &target,
            ));
            retarget_answer(
                &mut target,
                &Attempt {
                    client_model: aggregator.client_model.clone(),
                    provider: aggregator.provider.clone(),
                    upstream_model: aggregator.upstream_model.clone(),
                    pricing: aggregator.pricing.clone(),
                    status: 200,
                    latency_ms: 0,
                    cost: 0.0,
                    text: String::new(),
                    usage: TokenUsage::default(),
                    error: None,
                },
            );
            sse_response(observed, &request_id, &target)
        }
        Ok(ChatResult::Complete(value)) => {
            let usage = TokenUsage::from_openai_chat(&value);
            let pricing = aggregator.pricing.clone();
            let attempt = Attempt {
                client_model: aggregator.client_model.clone(),
                provider: aggregator.provider,
                upstream_model: aggregator.upstream_model,
                pricing: pricing.clone(),
                status: 200,
                latency_ms: started.elapsed().as_millis() as u64,
                cost: compute_cost(&pricing, &usage, Modality::Chat),
                text: message_text(&value),
                usage,
                error: None,
            };
            record_children(
                &state,
                &client,
                &body,
                &target,
                &request_id,
                std::slice::from_ref(&attempt),
                "aggregator",
            );
            finish_parent(
                &state,
                &client,
                &body,
                &mut target,
                &moa_name,
                &profile,
                estimated_cost_usd,
                &attempts,
                Some(&attempt),
                false,
                "",
                &request_id,
                started,
                200,
                None,
            );
            let content = if attempt.text.is_empty() {
                message_text(&value)
            } else {
                attempt.text.clone()
            };
            let chunks = synthesize_sse(&request_id, &content, &attempt.usage);
            let mapped = stream::iter(chunks.into_iter().map(Ok::<_, String>));
            let observed = crate::observe::Observe::new(mapped, Box::new(|_| {}));
            retarget_answer(&mut target, &attempt);
            target.moa = Some(report(
                &moa_name,
                &profile,
                estimated_cost_usd,
                &attempts,
                false,
                "",
                &target,
            ));
            sse_response(observed, &request_id, &target)
        }
        Err(_) => degrade_to_candidate(
            &state,
            &client,
            &body,
            &mut target,
            &moa_name,
            &profile,
            estimated_cost_usd,
            &attempts,
            &request_id,
            started,
        ),
    }
}

fn degrade_to_candidate(
    state: &AppState,
    client: &ClientId,
    body: &Value,
    target: &mut Target,
    moa_name: &str,
    profile: &MoaProfile,
    estimated_cost_usd: f64,
    attempts: &[Attempt],
    request_id: &str,
    started: Instant,
) -> Response {
    let Some((model, text)) = best_candidate(attempts) else {
        return provider_error(
            state,
            client,
            body,
            target,
            moa_name,
            profile,
            estimated_cost_usd,
            attempts,
            request_id,
            started,
            502,
            "aggregator failed".into(),
        );
    };
    finish_parent(
        state,
        client,
        body,
        target,
        moa_name,
        profile,
        estimated_cost_usd,
        attempts,
        None,
        true,
        "aggregator",
        request_id,
        started,
        200,
        None,
    );
    target.client_model = model.clone();
    target.moa = Some(report(
        moa_name,
        profile,
        estimated_cost_usd,
        attempts,
        true,
        "aggregator",
        target,
    ));
    let chunks = synthesize_sse(request_id, &text, &TokenUsage::default());
    let mapped = stream::iter(chunks.into_iter().map(Ok::<_, String>));
    let observed = crate::observe::Observe::new(mapped, Box::new(|_| {}));
    sse_response(observed, request_id, target)
}

fn synthesize_sse(id: &str, content: &str, usage: &TokenUsage) -> Vec<Bytes> {
    let chunk = |delta: Value, finish: Value, with_usage: bool| {
        let mut value = json!({
            "id": format!("chatcmpl-{id}"),
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish,
            }]
        });
        if with_usage {
            value["usage"] = json!({
                "prompt_tokens": usage.input_tokens,
                "completion_tokens": usage.output_tokens,
            });
        }
        Bytes::from(format!("data: {value}\n\n"))
    };
    vec![
        chunk(json!({"role": "assistant"}), Value::Null, false),
        chunk(json!({"content": content}), Value::Null, false),
        chunk(json!({}), json!("stop"), true),
        Bytes::from("data: [DONE]\n\n"),
    ]
}

fn record_children(
    state: &AppState,
    client: &ClientId,
    body: &Value,
    target: &Target,
    parent_id: &str,
    attempts: &[Attempt],
    phase: &str,
) {
    for attempt in attempts {
        let mut meta = child_meta(
            client,
            body,
            target,
            parent_id,
            &attempt.client_model,
            &attempt.provider,
            &attempt.upstream_model,
            &attempt.pricing,
            phase,
        );
        meta.cost_override = Some(attempt.cost);
        meta.id_override = Some(new_id());
        state.record(
            &meta,
            Finish {
                usage: attempt.usage.clone(),
                ttft_ms: None,
                status: attempt.status,
                error: attempt.error.clone(),
                latency_ms: attempt.latency_ms,
            },
        );
    }
}

fn child_meta(
    client: &ClientId,
    body: &Value,
    target: &Target,
    parent_id: &str,
    model: &str,
    provider: &str,
    upstream: &str,
    pricing: &Pricing,
    phase: &str,
) -> UsageMeta {
    let mut meta = crate::usage_meta(parent_id, client, body, target, "/v1/chat/completions");
    meta.parent_id = Some(parent_id.to_string());
    meta.phase = phase.to_string();
    meta.requested_model = model.to_string();
    meta.provider = provider.to_string();
    meta.upstream_model = upstream.to_string();
    meta.pricing = pricing.clone();
    meta.id_override = Some(new_id());
    meta
}

fn finish_parent(
    state: &AppState,
    client: &ClientId,
    body: &Value,
    target: &mut Target,
    moa_name: &str,
    profile: &MoaProfile,
    estimated_cost_usd: f64,
    attempts: &[Attempt],
    extra: Option<&Attempt>,
    degraded: bool,
    degrade_reason: &str,
    request_id: &str,
    started: Instant,
    status: u16,
    error: Option<String>,
) {
    let mut meta = parent_shell(
        client,
        body,
        target,
        moa_name,
        profile,
        estimated_cost_usd,
        attempts,
        degraded,
        degrade_reason,
        request_id,
    );
    let mut usage = TokenUsage::default();
    let mut cost = 0.0;
    for attempt in attempts {
        cost += attempt.cost;
        add_usage(&mut usage, &attempt.usage);
    }
    if let Some(extra) = extra {
        cost += extra.cost;
        add_usage(&mut usage, &extra.usage);
    }
    meta.cost_override = Some(cost);
    target.moa = Some(report(
        moa_name,
        profile,
        estimated_cost_usd,
        attempts,
        degraded,
        degrade_reason,
        target,
    ));
    state.record(
        &meta,
        Finish {
            usage,
            ttft_ms: None,
            status,
            error,
            latency_ms: started.elapsed().as_millis() as u64,
        },
    );
}

fn parent_shell(
    client: &ClientId,
    body: &Value,
    target: &Target,
    moa_name: &str,
    profile: &MoaProfile,
    estimated_cost_usd: f64,
    attempts: &[Attempt],
    degraded: bool,
    degrade_reason: &str,
    request_id: &str,
) -> UsageMeta {
    let mut meta = crate::usage_meta(request_id, client, body, target, "/v1/chat/completions");
    meta.provider = "moa".into();
    meta.upstream_model = moa_name.to_string();
    meta.phase = "moa".into();
    meta.moa_degraded = degraded;
    meta.moa_proposers = attempts.len() as u32;
    if let Some(route) = meta.route.as_mut() {
        route.moa_mode = "full".into();
        if !degrade_reason.is_empty() {
            route.reason = format!("{}; degraded: {degrade_reason}", route.reason);
        }
        route.estimated_cost_usd = estimated_cost_usd;
    }
    let _ = profile;
    meta
}

fn add_usage(into: &mut TokenUsage, usage: &TokenUsage) {
    if usage.known {
        into.known = true;
        into.input_tokens += usage.input_tokens;
        into.output_tokens += usage.output_tokens;
        into.cached_tokens += usage.cached_tokens;
    }
}

fn report(
    moa_name: &str,
    profile: &MoaProfile,
    estimated_cost_usd: f64,
    attempts: &[Attempt],
    degraded: bool,
    degrade_reason: &str,
    target: &Target,
) -> MoaReport {
    MoaReport {
        mode: "full".into(),
        profile: moa_name.to_string(),
        reason: target
            .route
            .as_ref()
            .map(|r| r.reason.clone())
            .unwrap_or_default(),
        estimated_cost_usd,
        proposers: attempts.iter().map(Attempt::report).collect(),
        aggregator: profile.aggregator.clone(),
        quorum: profile.quorum,
        degraded,
        degrade_reason: degrade_reason.to_string(),
    }
}

fn requested_model(body: &Value, target: &Target) -> String {
    body.get("model")
        .and_then(|m| m.as_str())
        .unwrap_or(&target.client_model)
        .to_string()
}

fn provider_error(
    state: &AppState,
    client: &ClientId,
    body: &Value,
    target: &mut Target,
    moa_name: &str,
    profile: &MoaProfile,
    estimated_cost_usd: f64,
    attempts: &[Attempt],
    request_id: &str,
    started: Instant,
    status: u16,
    message: String,
) -> Response {
    finish_parent(
        state,
        client,
        body,
        target,
        moa_name,
        profile,
        estimated_cost_usd,
        attempts,
        None,
        true,
        "aggregator",
        request_id,
        started,
        status,
        Some(redact(&message)),
    );
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    crate::api_error(
        code,
        "upstream_error",
        &redact(&message),
        Some("upstream"),
        None,
    )
}
