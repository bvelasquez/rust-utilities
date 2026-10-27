# Using airouter from an agent

`airouter` is a CLI and a local HTTP server. Prefer the CLI for setup and `curl` or an OpenAI SDK for chat. Pass `--json` whenever you need to parse output. Commands exit `0` on success and `1` on failure. `--help` on any subcommand prints flags.

## Paths

```bash
airouter config path
```

Default file: `$AIROUTER_CONFIG` or `~/.config/airouter/config.json`. The local client token is the sibling `token` file. Both are mode `0600`.

Usage database: `$XDG_STATE_HOME/airouter/usage.db` or `~/.local/state/airouter/usage.db`, unless `server.data_dir` is set.

## Add a provider and a model

```bash
airouter provider add groq --type openai_compat \
  --base-url https://api.groq.com/openai/v1 \
  --api-key env:GROQ_API_KEY

airouter models search llama
airouter models add groq/llama-3.3-70b-versatile
airouter alias set fast groq/llama-3.3-70b-versatile
airouter config validate
airouter doctor
```

`models add <id>` copies pricing and modality from the built-in catalog when the id is known. `deepinfra/<org>/<model>` is priced from DeepInfra's public model list (standard tier, cached for a day). For anything else not in the catalog:

```bash
airouter models add ollama/llama3.2 \
  --provider ollama \
  --upstream llama3.2 \
  --modality chat \
  --context 8192 \
  --input-per-mtok 0 \
  --output-per-mtok 0
```

Provider types: `openai`, `openai_compat`, `anthropic`, `gemini`. `openai_compat` requires `--base-url`, except `groq`, `deepseek`, `xai`, `deepinfra`, and `cheaperinference`, which fill in their public base URL. DeepInfra is `https://api.deepinfra.com/v1/openai` and `env:DEEPINFRA_API_KEY`. Cheaper Inference is `https://api.cheaperinference.com/v1` and `env:CHEAPERINFERENCE_API_KEY`.

`models add cheaperinference/<id>` reads Cheaper Inference's authenticated `GET /v1/models` list, so the provider and key must resolve first. Prices there are the key's customer-facing discounted rates, cached for a day under `$XDG_STATE_HOME/airouter/cheaperinference-models.json`. `models search` also merges the list when the provider is configured. Decision models (`type: text`, endpoint `/v1/decisions`) are skipped because they have no chat route.

Modalities: `chat`, `embedding`, `image`, `audio_tts`, `audio_stt`. `video` is accepted in config and is not routable yet.

Do not create an alias named `auto` or `auto/...`. Those names are routing profiles, configured under `routing` and called as the model id `auto` or `auto/<profile>`.

## Call the API

Base URL `http://127.0.0.1:8787/v1` on the host, or `http://<tailscale-ip>:8787/v1` from another machine on the tailnet. Authorization is `Bearer` plus the contents of the token file.

```bash
TOKEN="$(tr -d '[:space:]' < ~/.config/airouter/token)"
curl -s http://127.0.0.1:8787/v1/chat/completions \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"model":"fast","messages":[{"role":"user","content":"Say hi"}]}'
```

Streaming is `stream: true` and Server-Sent Events, ending with `data: [DONE]`. Tool calls use the OpenAI `tools` / `tool_calls` shape for every provider, including Anthropic and Gemini.

Other routes: `POST /v1/embeddings`, `POST /v1/images/generations`, `POST /v1/audio/speech`, `POST /v1/audio/transcriptions`, `GET /v1/usage?since=7d&by=model`.

`since` accepts `30m`, `12h`, `7d`, an RFC3339 timestamp, or a unix timestamp (`0` means the whole log). `by` is `model`, `provider`, `day`, `client`, or `route`.

`POST /v1/feedback` with `{"request_id","rating":"up"|"down"}` records a thumbs rating on a stored request. The id is the `x-request-id` response header.

## Auto routing

`model` may be `auto` or `auto/<profile>` when `routing.profiles` defines that profile. `auto` uses `routing.default_profile`. Do not create an alias named `auto`, `auto/...`, `moa`, or `moa/...`.

Profiles without `mode` keep the legacy score: keep models that match modality, context, and required tools or vision, then rank

`quality - lambda * estimated_cost - mu * latency_seconds`.

Set `mode` to opt into economic selection. The router still applies those hard filters, plus `max_cost_usd` and `max_latency_ms` when they are set, then:

| `mode` | Choice |
|---|---|
| `budget` | Cheapest model at or above the task quality floor |
| `balanced` | Best `quality - lambda * cost - mu * latency` among models at or above the floor |
| `quality` | Highest task quality under the cost cap |
| `speed` | Lowest latency prior among models at or above the floor and under the cost cap |

The floor is `required_quality[task] + difficulty_step * difficulty`. Missing tasks use a built-in base and step, so an easy chat is not held to the same bar as hard reasoning. If nothing clears the floor, the router relaxes it once by `relax_step`, then uses `safe_model` when that model still passes the hard filters. Low classification confidence still prefers `safe_model`.

`x-airouter-mode: budget | balanced | quality | speed` overrides the profile mode for that call.

`quality` and `task_quality` are 0–1 priors. Without them, balanced and quality cannot tell models apart. `latency_ms_prior` is what `speed` compares. `airouter route calibrate` prints p50 latency from the usage log; `--apply` writes it onto matching models. It does not invent `task_quality`. Task fixtures live in `fixtures/tasks/`.

Output cost is estimated from the task (chat, code, reasoning, writing, extraction), not `tokens / 4`.

Successful chat completions include an `airouter` object (`profile`, `model`, `reason`, `confidence`, `task`, `rejected`, `mode`, `required_quality`, `selected_quality`, `estimated_cost_usd`) and an `x-airouter-route` header. The next eligible models are tried after a retryable upstream failure (429, 5xx, timeout) on a non-streaming single-model call. A started SSE stream is not failed over.

## On-demand mixture of agents

A profile with `"strategy": "moa_on_demand"` and a `moa_profile` may fan out after economic selection. MoA runs when the selected model is below `required_quality + quality_margin` and the estimated proposer-plus-aggregator cost is within `moa_triggers.max_cost_usd`. Until a model has a real quality prior, task, difficulty, token, and client triggers can also fire. `auto/adaptive` is the usual name for that profile; there is no `moa/<profile>` model id.

`x-airouter-moa: force` fans out unless the request has tools and `allow_tools` is false. `off` keeps a single model (or the worker on a follow-up). `auto` is the default. The response header `x-airouter-moa` is `single`, `worker`, or `full`.

A follow-up is any request that already contains an assistant message with text. Trivial follow-ups call `worker_model` (the aggregator when unset). A follow-up that itself misses the quality bar fans out again.

Proposers are non-streaming and see the full conversation plus `proposer_prompt`. The aggregator writes the user-visible answer. If too few proposers succeed, the call degrades to the worker. If the aggregator fails, the best proposer text is returned and `airouter.moa.degraded` is true. Tool requests do not fan out: the gateway does not merge `tool_calls`.

Streaming clients wait until the proposers finish, then receive the aggregator as SSE. The first token is delayed by that fan-out. When an upstream ignores `stream` and returns one JSON object, airouter synthesizes `role`, content, finish reason, and `[DONE]`.

MoA sends the prompt to every proposer provider. Usage is one parent row (`provider` `moa`) plus a child row per proposer and aggregator. Summaries count parent rows only, so costs are not doubled. `GET /v1/usage?by=model` attributes those child costs to the concrete models. Prompt text and candidate text are not stored.

`airouter moa eval --file fixtures/moa/eval.jsonl --profile adaptive` replays the policy offline and reports which calls would fan out, with fixture quality when the JSONL includes it.

A profile without `mode` still uses the legacy score:

```json
{
  "routing": {
    "default_profile": "cheap",
    "profiles": {
      "cheap": {
        "safe_model": "anthropic/claude-sonnet-4-5",
        "confidence_floor": 0.5,
        "lambda": 10,
        "mu": 0.02,
        "tiers": [
          {"task": "chat", "models": ["groq/llama-3.3-70b-versatile"]},
          {"task": "code", "models": ["anthropic/claude-sonnet-4-5"]}
        ]
      }
    }
  }
}
```

Task labels the local rules emit are `chat`, `code`, `extraction`, `writing`, and `reasoning`. A tier `task` of `*` matches all of them. Set `quality`, `task_quality`, and `latency_ms_prior` on a model to change the score. `quality` is 0–1.

Signals are local features by default (length, code fences, tools, images). Optional classifiers:

- `routing.signals.provider`: `none` (default), `embedding`, or `jev`.
- `embedding` scores the last user turn against `routing.signals.embedding.examples` with local token overlap. No network call.
- `jev` calls TypeSafe (`jev-latest` by default). The key is `routing.signals.jev.api_key`, usually `env:TYPESAFE_API_KEY`. A profile sends prompt text only when its `hosted_signal_clients` lists that client's label. Otherwise the call stays local. A timeout or error (`routing.signals.timeout_ms`, default 400) fails open to the local features.

`airouter route replay --since 30d` re-scores stored auto requests and prints recorded cost against the cost of always using each profile's safe model. `airouter doctor` warns when a profile has no safe model, or when Jev is configured with nobody opted in.

Errors use the OpenAI JSON shape: `{"error":{"message","type","code"}}`.

## What not to do

- Do not print provider keys or the local token in logs, issues, or chat transcripts.
- Do not bind `server.admin_listen` off loopback. The admin API has no authentication.
- `auto` and `auto/<profile>` pick a configured model only when that profile exists under `routing.profiles`. An alias cannot use those names.
- Catalog prices are defaults. If a bill looks wrong, set `pricing` on that model and check `airouter usage`.
