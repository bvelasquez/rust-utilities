# Auto-routing improvements

Status: proposed
Scope: `crates/core`, `crates/registry`, `crates/gateway`, `crates/store`, `crates/cli`
Priority: item 4 (task detection) first

This plan covers five improvements to `auto` routing. Item 4 (better task
detection) is the priority and is sequenced first, after a small measurement
harness that everything else depends on.

## Background

Today `auto` resolves through `airouter_registry::rules::resolve_auto`:

1. `crates/gateway/src/lib.rs::resolve_request` builds a `RequestSummary` from
   the request body (`summarize`).
2. `crates/gateway/src/signals.rs::extract` produces `Signals` from local
   keyword rules, an optional local embedding matcher, or the hosted Jev
   classifier.
3. `crates/registry/src/rules.rs::pick` filters candidates (`admit`), scores
   them as `quality - lambda * estimated_cost - mu * latency_seconds`, applies
   the safe-model gate when `confidence < confidence_floor`, and returns the
   winner plus a `fallbacks` chain.

Known weaknesses this plan addresses:

1. The feedback loop is open: `latency_ms` and thumbs `feedback` are recorded
   but never feed back into routing.
2. Confidence is not comparable across sources and the safe-model gate is
   effectively dead for the default local signals.
3. `quality` / `task_quality` are hand-set and uncalibrated.
4. Task detection is brittle keyword matching.
5. Output length is estimated as `tokens / 4`, which is wrong in predictable
   cases.

## Constraints and principles

- `RulesRouter` stays **synchronous**. Classification runs in the gateway
  before `resolve()`; a hosted classifier never runs inside the registry.
- **Prompts are never stored** (`crates/store/src/lib.rs`: "Prompt and response
  bodies are not stored"). Task-detection accuracy cannot be measured
  retroactively from `usage.db`; evaluation needs a user-supplied labeled set.
- Replay reconstructs `Signals` from the `route_signals` JSON column, so new
  signal fields need serde defaults.
- `EmbeddingSettings.model` is reserved but unused; use it as the hook for a
  real embedding backend.
- No provider secrets in config as literals (`env:` / `file:` only). Do not log
  keys, the local token, or `Authorization` headers.
- Config remains the source of truth; the CLI suggests or writes values.

## Phase 0 - Measurement harness (prerequisite)

Nothing below is verifiable without this. Do it first.

### 0.1 Move the pure classifier into the registry

- Extract `lexical_signals`, `tokens`, and `cosine` from
  `crates/gateway/src/signals.rs` into a new `crates/registry/src/classify.rs`
  (pure, sync, no config dependency beyond examples).
- `gateway/signals.rs` keeps only async orchestration (timeout, Jev HTTP,
  cache) and calls `airouter_registry::classify`.
- Makes the classifier unit-testable and reusable by the CLI eval command.

### 0.2 Labeled fixture set

- Add `crates/registry/tests/fixtures/tasks.jsonl`, one JSON object per line:
  `{"text": "...", "task": "code", "difficulty": 2}`.
- Seed ~150-300 balanced examples across `chat` / `code` / `extraction` /
  `writing` / `reasoning`, including adversarial cases the current rules fail:
  - "I hit a bug in my billing workflow" (should be `chat`, not `code`)
  - "return this as json" (extraction)
  - "why does this happen" (reasoning)

### 0.3 `airouter route eval`

- New subcommand under `RouteCmd` in `crates/cli/src/lib.rs`:
  `airouter route eval [--file <jsonl>] [--json]`.
- Runs each text through the active classifier (local or embedding) and prints:
  - per-task precision / recall / F1
  - a confusion matrix
  - difficulty MAE
  - a confidence calibration table (Brier score + reliability buckets)
- Default file is the bundled fixtures so it runs out of the box.

**Acceptance:** baseline numbers printed for today's `local_signals`. That
baseline is what Phase 1 must beat.

## Phase 1 - Item 4: better task detection (PRIORITY)

Goal: replace keyword matching with a feature-based scorer, calibrated, with an
optional real embedding backend. Keep it local-first and deterministic.

### 1.1 Feature-based local classifier

Rewrite `local_signals` internals (same public signature) as weighted evidence
over features instead of `contains()` checks:

- **Code signals:** fenced blocks, language tags, code punctuation density
  (`;{}()=>`), identifier patterns (snake_case / camelCase / `fn ` / `def ` /
  `import `), stack traces, error words near code.
- **Distinguish "is code" from "talks about code":** separate `contains_code`
  (payload is code) from a `code_topic` feature (discusses code). Today `"bug"`
  anywhere sets `contains_code`; that is the main failure.
- **Extraction signals:** `response_format`, JSON / CSV / schema words, "pull
  out", "fields", short imperative over a long input.
- **Reasoning signals:** causal / question structure ("why", "compare",
  "prove", "trade-off"), math / units, multi-part questions, long context,
  `reasoning_effort`.
- **Writing signals:** deliverable nouns ("essay", "email", "post", "story"),
  tone / length instructions.
- **Chat default** only when nothing else fires.
- Score each task, take argmax, derive confidence from the **margin between the
  top two** (softmax or normalized margin) instead of hardcoded 0.6 / 0.7 /
  0.85.

### 1.2 Better difficulty

Small regression over `token_estimate` plus structural features (nesting depth,
number of sub-questions, code complexity) instead of fixed buckets. Output the
same `u8` 0-4 range so tiers are unchanged.

### 1.3 Upgrade the embedding path

- Ship **default examples** for the five tasks (or `embedding.defaults: true`)
  so users get embedding quality without authoring examples.
- Replace set-cosine with **BM25 / TF-IDF + character n-grams**, still
  dependency-light and local.
- Add `EmbeddingSettings.backend: "lexical" | "local"` (default `lexical`).
  The `"local"` backend uses a real embedding model via `fastembed` / `candle`
  behind a Cargo feature. **Dependency decision required** (see Open questions).

### 1.4 Calibrate confidence

Fit a monotone mapping (isotonic or a simple temperature) from raw margin to
P(task correct) using the Phase 0 fixtures, so confidence means the same thing
for local, embedding, and Jev. Store the mapping as constants or a small config
table. This feeds Phase 2.

**Files:** `crates/registry/src/classify.rs` (new),
`crates/registry/src/rules.rs`, `crates/gateway/src/signals.rs`,
`crates/core/src/routing.rs` (`EmbeddingSettings`, possibly `Signals`),
`crates/cli/src/lib.rs` (eval command).

**Acceptance:** eval accuracy >= 85% overall; code / extraction / reasoning
recall >= 0.75; "bug in my billing workflow" classifies as `chat`. Report
before / after from Phase 0.

## Phase 2 - Item 2: confidence semantics

Depends on Phase 1.4.

- Define confidence as **P(task correct)**, documented, on one scale across
  sources.
- Make `confidence_floor` actually gate. Today local confidence is always
  >= 0.6 and embedding is clamped to >= 0.5, so with the default floor of 0.5
  the safe-model gate is effectively dead for non-Jev traffic. Calibrated
  confidence makes it fire.
- Add per-source override: `confidence_floor` (global) plus optional
  `routing.signals.confidence_floor` / per-profile override, so a noisy lexical
  source can carry a higher floor than Jev.
- Clamp / calibrate `parse_jev_answers` into the same space.

**Acceptance:** on the fixtures, accuracy when confidence >= floor is
materially higher than below it; on real traffic the gate fires on a
non-trivial fraction (target ~5-20%) rather than ~0%.

## Phase 3 - Item 1: close the loop

Independent of Phase 1; can run in parallel after Phase 0.

### 3.1 Latency prior from data

- At server start and config reload, compute rolling p50 (and p90) `latency_ms`
  per upstream model from `usage.db`.
- Inject as `latency_ms_prior` only when the config value is unset (explicit
  config wins).
- Add `airouter route stats` to inspect; refresh on `POST /reload`.

### 3.2 Feedback to quality

- Compute a shrunk (Bayesian) thumbs-adjusted delta per `(model, task)` with a
  minimum sample count.
- Apply as a bounded adjustment to `quality` / `task_quality` at scoring time.
- Never let a handful of ratings swing routing.

### 3.3 Placement

- Keep SQLite work at build / reload time; the router stays synchronous reading
  a precomputed map.
- Guard against small-N and self-selection (only rated calls count) by
  requiring `n >= k` and shrinking toward the prior.

**Files:** `crates/gateway/src/lib.rs` (state build / reload),
`crates/registry/src/rules.rs` (`score`), `crates/store/src/lib.rs`
(aggregate queries), `crates/cli/src/lib.rs` (stats command).

**Acceptance:** `latency_ms_prior` reflects observed p50 within tolerance;
feedback deltas are bounded and only apply with sufficient samples; unit tests
with synthetic usage rows.

## Phase 4 - Item 5: output length

- Read `max_tokens` / `max_completion_tokens` in `summarize()` and add
  `max_output_tokens: Option<u64>` to `RequestSummary`.
- Add a heuristic estimator for when it is absent (instruction words like
  "write N words", "one paragraph", "summarize", output format) and an optional
  `typical_output_tokens` per-model hint.
- Use `min(max_tokens, heuristic, model_hint)` in `score()` instead of
  `(tokens / 4).max(32)`.
- Update `RouteInfo::signals_json` if new fields are surfaced.

**Acceptance:** cost estimates for "summarize this long doc" and "write a
2000-word essay" no longer invert; unit tests with the two canonical cases.

## Phase 5 - Item 3: evidence-based quality

Depends on Phase 0. Largest and most open-ended.

- Extend `airouter route eval` to optionally **call configured models** on a
  labeled set and score pass / fail (exact match, JSON validity, or a rubric),
  then emit suggested `quality` / `task_quality` values.
- Config stays the source of truth (per `AGENTS.md`); the CLI writes or
  suggests values, and `airouter config validate` warns when a model has no
  quality prior for a task it is routed to.
- Keep hand-set values authoritative unless the user opts into generated ones
  (e.g., a `quality_source` marker).

**Acceptance:** a user can regenerate quality priors from an eval run and see
the values reflected in `airouter route replay` agreement / strong-share.

## Sequencing

| Item | Phase | Order | Depends on |
|---|---|---|---|
| - | 0. Eval harness | 1st | - |
| **4. Task detection** | **1** | **2nd (priority)** | Phase 0 |
| 2. Confidence | 2 | 3rd | Phase 1.4 |
| 1. Feedback loop | 3 | parallel after 0 | - |
| 5. Output length | 4 | parallel after 0 | - |
| 3. Evidence-based quality | 5 | last | Phase 0 |

Rough total: ~8-10 focused days, with item 4 landing value in the first 2-3.

## Open questions

1. **Dependency:** real local embedding backend (`fastembed` / `candle`, adds a
   model download and a heavier dep tree) or purely lexical? Recommendation:
   start lexical (Phase 1.3) and gate the real model behind a feature flag.
2. **Where the labeled set lives:** bundled fixtures in-repo vs. user path
   only. Recommendation: bundle a starter set and allow `--file` overrides.
3. **Feedback auto-tuning:** should ratings adjust routing automatically, or
   only produce a report plus suggested config? Recommendation: report-first,
   opt-in later.
