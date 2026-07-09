# elabs

**elabs** is an agent-first [ElevenLabs](https://elevenlabs.io) CLI written in Rust. It wraps the ElevenLabs REST API for:

- **Text-to-speech** — generate audio from text
- **Speech-to-text** — transcribe audio files
- **Voice management** — list, clone (IVC), design, and save voices
- **Machine discovery** — `capabilities --json`, `env schema --json`

Works with Cursor, Claude Code, Codex, shell scripts, and CI.

## Requirements

- [Rust](https://rustup.rs/) 2021 edition
- ElevenLabs API key ([dashboard](https://elevenlabs.io/app/settings/api-keys))
- Optional: [sccache](https://github.com/mozilla/sccache) + [lld](https://lld.llvm.org/) for faster builds (see [../AGENT.md](../AGENT.md))

## Install

```bash
cd elevenlabs
./scripts/setup-sccache.sh   # optional, recommended
cargo build --release
cargo install --path .
```

Binary name: **`elabs`**

Verify:

```bash
elabs --version
elabs capabilities --json
elabs env schema --json
```

## Authentication

```bash
# Save API key to ~/.config/elabs/config.toml
elabs apikey set

# Or pass inline (avoid in shared shells)
elabs apikey set sk_your_key_here

# From environment
export ELEVENLABS_API_KEY=sk_your_key_here
elabs apikey set --from-env

# Check status (no secret printed)
elabs apikey status --json
```

Environment variables (override config file):

| Variable | Purpose |
|----------|---------|
| `ELEVENLABS_API_KEY` | API key (primary) |
| `ELABS_API_KEY` | API key alias |
| `ELABS_BASE_URL` | API base URL override |

## Commands

### Discovery

```bash
elabs capabilities --json
elabs env schema --json
elabs models list --json
elabs voices list --json
elabs voices list --search "george" --json
```

### Text-to-speech

```bash
elabs tts speak \
  --voice JBFqnCBsd6RMkjVDRZzb \
  --text "Hello from elabs." \
  --output hello.mp3 \
  --json

# Long text from file
elabs tts speak --voice <voice_id> --text-file script.txt -o out.mp3
```

Options:

- `--model` — default `eleven_multilingual_v2`
- `--format` — default `mp3_44100_128` (see ElevenLabs output format docs)

### Speech-to-text

```bash
elabs stt transcribe \
  --file recording.wav \
  --output transcript.json \
  --json
```

Options:

- `--model` — default `scribe_v2`
- `--language` — ISO 639-1 hint (optional)
- `--diarize` — speaker diarization

### Voice cloning (IVC)

```bash
elabs voices clone \
  --name "My Clone" \
  --file sample1.mp3 \
  --file sample2.mp3 \
  --description "Warm narrator" \
  --yes \
  --json
```

Mutations require `--yes`, `--dry-run`, or interactive confirmation.

### Voice design (text prompt)

```bash
# Step 1: generate previews
elabs voices design \
  --description "A calm British narrator, warm and articulate" \
  --auto-generate-text \
  --omit-audio \
  --yes \
  --json

# Step 2: save a preview as a voice
elabs voices save \
  --generated-voice-id <id_from_previews> \
  --name "British Narrator" \
  --yes \
  --json
```

## Output modes

| Flag | Behavior |
|------|----------|
| (default) | Pretty-printed JSON |
| `--json` | Structured envelope: `success`, `command`, `data`, `next_actions`, … |
| `--compact` | Single-line JSON, raw API shape |

## Agent workflow

1. `elabs capabilities --json` — discover commands
2. `elabs env schema --json` — env/config precedence
3. `elabs apikey status --json` — verify auth
4. `elabs voices list --json` — pick `voice_id`
5. `elabs tts speak --voice … --text … -o out.mp3 --json`
6. `elabs stt transcribe --file in.wav --json`

## Development

```bash
export RUSTC_WRAPPER=sccache
export CARGO_INCREMENTAL=0
cargo build
cargo test
```

See [../AGENT.md](../AGENT.md) for the fast compiler toolchain used across utilities Rust projects.
