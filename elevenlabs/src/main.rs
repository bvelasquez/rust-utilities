mod api;
mod auth;
mod capabilities;
mod config;
mod ops;
mod output;
mod safety;

use anyhow::Result;
use clap::{Parser, Subcommand};
use output::{print_raw, Envelope};
use std::path::PathBuf;

use api::{DEFAULT_OUTPUT_FORMAT, DEFAULT_SFX_MODEL, DEFAULT_STT_MODEL, DEFAULT_TTS_MODEL};
use auth::{api_key_status, load_api_key, set_api_key_interactive};
use ops::read_text_arg;

#[derive(Parser, Debug)]
#[command(
    name = "elabs",
    about = "Agent-first ElevenLabs CLI — TTS, STT, sound effects, voice cloning and design",
    version,
    after_help = "Agents: run `elabs capabilities --json` and `elabs env schema --json`.\nUse --json on subcommands for structured envelope output."
)]
struct Cli {
    #[arg(long, global = true, help = "Structured JSON envelope output")]
    json: bool,

    #[arg(long, global = true, help = "Compact JSON (no envelope, raw API shape)")]
    compact: bool,

    #[arg(long, global = true, help = "Path to config (~/.config/elabs/config.toml)")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Manage ElevenLabs API key
    Apikey {
        #[command(subcommand)]
        command: ApikeyCommands,
    },
    /// List and create voices
    Voices {
        #[command(subcommand)]
        command: VoicesCommands,
    },
    /// Text-to-speech
    Tts {
        #[command(subcommand)]
        command: TtsCommands,
    },
    /// Speech-to-text
    Stt {
        #[command(subcommand)]
        command: SttCommands,
    },
    /// Sound effects — generate, list, and download
    Sfx {
        #[command(subcommand)]
        command: SfxCommands,
    },
    /// List available models
    Models,
    /// Machine-readable command catalog
    Capabilities,
    /// Environment variable schema
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },
}

#[derive(Subcommand, Debug)]
enum ApikeyCommands {
    /// Save API key to config file
    Set {
        #[arg(help = "API key (omit to prompt securely)")]
        key: Option<String>,
        #[arg(long, help = "Read key from ELEVENLABS_API_KEY / ELABS_API_KEY")]
        from_env: bool,
    },
    /// Show whether API key is configured (no secret)
    Status,
}

#[derive(Subcommand, Debug)]
enum VoicesCommands {
    /// List voices (GET /v2/voices)
    List {
        #[arg(long)]
        search: Option<String>,
        #[arg(long, default_value = "30")]
        page_size: u32,
        #[arg(long)]
        page_token: Option<String>,
    },
    /// Clone a voice from audio samples (POST /v1/voices/add)
    Clone {
        #[arg(long)]
        name: String,
        #[arg(long, required = true)]
        file: Vec<PathBuf>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        remove_background_noise: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    /// Design voice previews from a text prompt (POST /v1/text-to-voice/design)
    Design {
        #[arg(long)]
        description: String,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        auto_generate_text: bool,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, help = "Omit base64 audio from JSON output (smaller for agents)")]
        omit_audio: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    /// Save a design preview as a voice (POST /v1/text-to-voice)
    Save {
        #[arg(long)]
        generated_voice_id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum TtsCommands {
    /// Generate speech audio
    Speak {
        #[arg(long)]
        voice: String,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        text_file: Option<PathBuf>,
        #[arg(long, default_value = DEFAULT_TTS_MODEL)]
        model: String,
        #[arg(long, default_value = DEFAULT_OUTPUT_FORMAT)]
        format: String,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum SttCommands {
    /// Transcribe an audio file
    Transcribe {
        #[arg(long)]
        file: PathBuf,
        #[arg(long, default_value = DEFAULT_STT_MODEL)]
        model: String,
        #[arg(long)]
        language: Option<String>,
        #[arg(long)]
        diarize: bool,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum SfxCommands {
    /// Generate a sound effect from a text prompt (POST /v1/sound-generation)
    Create {
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        text_file: Option<PathBuf>,
        #[arg(long, default_value = DEFAULT_SFX_MODEL)]
        model: String,
        #[arg(long, default_value = DEFAULT_OUTPUT_FORMAT)]
        format: String,
        #[arg(long, help = "Target duration in seconds (0.5–30)")]
        duration: Option<f64>,
        #[arg(long, help = "Generate a seamlessly looping effect (v2 model only)")]
        r#loop: bool,
        #[arg(long, help = "How closely to follow the prompt (0.0–1.0, default 0.3)")]
        prompt_influence: Option<f64>,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
    /// List previously generated sound effects from history (GET /v1/history)
    List {
        #[arg(long, help = "Search term to filter by prompt text")]
        search: Option<String>,
        #[arg(long, default_value = "30")]
        page_size: u32,
        #[arg(long, help = "Pagination cursor (history_item_id)")]
        after: Option<String>,
        #[arg(long, help = "Include all history types, not just sound effects")]
        all: bool,
    },
    /// Download a sound effect from history (GET /v1/history/{id}/audio)
    Download {
        #[arg(long, help = "History item ID from `sfx list`")]
        id: String,
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum EnvCommands {
    Schema,
}

struct RunContext {
    json: bool,
    compact: bool,
    config: Option<PathBuf>,
}

impl RunContext {
    fn emit<T: serde::Serialize>(&self, command: &str, data: T) -> Result<()> {
        if self.compact {
            let v = serde_json::to_value(&data)?;
            print_raw(&v, true)
        } else if self.json {
            Envelope::ok(command, data).print_json()
        } else {
            let v = serde_json::to_value(&data)?;
            print_raw(&v, false)
        }
    }

    fn emit_with_next<T: serde::Serialize>(
        &self,
        command: &str,
        data: T,
        next: Vec<String>,
    ) -> Result<()> {
        if self.compact {
            let v = serde_json::to_value(&data)?;
            print_raw(&v, true)
        } else if self.json {
            Envelope::ok(command, data)
                .with_next_actions(next)
                .print_json()
        } else {
            let v = serde_json::to_value(&data)?;
            print_raw(&v, false)
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let ctx = RunContext {
        json: cli.json,
        compact: cli.compact,
        config: cli.config,
    };

    match cli.command {
        Commands::Apikey { command } => match command {
            ApikeyCommands::Set { key, from_env } => {
                let path = set_api_key_interactive(ctx.config.as_ref(), key, from_env)?;
                ctx.emit(
                    "apikey set",
                    serde_json::json!({
                        "saved": true,
                        "configPath": path.display().to_string(),
                    }),
                )
            }
            ApikeyCommands::Status => {
                ctx.emit("apikey status", api_key_status(ctx.config.as_ref()))
            }
        },
        Commands::Voices { command } => {
            let auth = load_api_key(ctx.config.as_ref())?;
            match command {
                VoicesCommands::List {
                    search,
                    page_size,
                    page_token,
                } => {
                    let data = ops::run_voices_list(
                        &auth,
                        &ctx.config,
                        search.as_deref(),
                        page_size,
                        page_token.as_deref(),
                    )
                    .await?;
                    ctx.emit("voices list", data)
                }
                VoicesCommands::Clone {
                    name,
                    file,
                    description,
                    remove_background_noise,
                    dry_run,
                    yes,
                } => {
                    safety::require_mutation_approval(
                        yes,
                        dry_run,
                        &format!("clone voice {name}"),
                    )?;
                    let data = ops::run_voices_clone(
                        &auth,
                        &ctx.config,
                        &name,
                        &file,
                        description.as_deref(),
                        remove_background_noise,
                        dry_run,
                    )
                    .await?;
                    ctx.emit_with_next(
                        "voices clone",
                        data,
                        vec!["elabs voices list --json".into()],
                    )
                }
                VoicesCommands::Design {
                    description,
                    text,
                    auto_generate_text,
                    model,
                    omit_audio,
                    dry_run,
                    yes,
                } => {
                    safety::require_mutation_approval(
                        yes,
                        dry_run,
                        "design voice previews",
                    )?;
                    let data = ops::run_voices_design(
                        &auth,
                        &ctx.config,
                        &description,
                        text.as_deref(),
                        auto_generate_text,
                        model.as_deref(),
                        dry_run,
                        omit_audio || ctx.json,
                    )
                    .await?;
                    ctx.emit_with_next(
                        "voices design",
                        data,
                        vec![
                            "Pick a generated_voice_id from previews".into(),
                            "elabs voices save --generated-voice-id <id> --name <name> --yes".into(),
                        ],
                    )
                }
                VoicesCommands::Save {
                    generated_voice_id,
                    name,
                    description,
                    dry_run,
                    yes,
                } => {
                    safety::require_mutation_approval(
                        yes,
                        dry_run,
                        &format!("save voice {name}"),
                    )?;
                    let data = ops::run_voices_save(
                        &auth,
                        &ctx.config,
                        &generated_voice_id,
                        &name,
                        description.as_deref(),
                        dry_run,
                    )
                    .await?;
                    ctx.emit_with_next(
                        "voices save",
                        data,
                        vec!["elabs voices list --json".into()],
                    )
                }
            }
        }
        Commands::Tts { command } => {
            let auth = load_api_key(ctx.config.as_ref())?;
            match command {
                TtsCommands::Speak {
                    voice,
                    text,
                    text_file,
                    model,
                    format,
                    output,
                } => {
                    let text_content = read_text_arg(text, text_file)?;
                    let data = ops::run_tts(
                        &auth,
                        &ctx.config,
                        &voice,
                        &text_content,
                        &model,
                        &format,
                        output,
                    )
                    .await?;
                    ctx.emit("tts speak", data)
                }
            }
        }
        Commands::Stt { command } => {
            let auth = load_api_key(ctx.config.as_ref())?;
            match command {
                SttCommands::Transcribe {
                    file,
                    model,
                    language,
                    diarize,
                    output,
                } => {
                    let data = ops::run_stt(
                        &auth,
                        &ctx.config,
                        &file,
                        &model,
                        language.as_deref(),
                        diarize,
                        output,
                    )
                    .await?;
                    ctx.emit("stt transcribe", data)
                }
            }
        }
        Commands::Sfx { command } => {
            let auth = load_api_key(ctx.config.as_ref())?;
            match command {
                SfxCommands::Create {
                    text,
                    text_file,
                    model,
                    format,
                    duration,
                    r#loop,
                    prompt_influence,
                    output,
                } => {
                    let text_content = read_text_arg(text, text_file)?;
                    let data = ops::run_sfx_create(
                        &auth,
                        &ctx.config,
                        &text_content,
                        &model,
                        &format,
                        duration,
                        r#loop,
                        prompt_influence,
                        output,
                    )
                    .await?;
                    ctx.emit_with_next(
                        "sfx create",
                        data,
                        vec![
                            "elabs sfx list --json".into(),
                            "afplay <outputPath>  # macOS playback".into(),
                        ],
                    )
                }
                SfxCommands::List {
                    search,
                    page_size,
                    after,
                    all,
                } => {
                    let data = ops::run_sfx_list(
                        &auth,
                        &ctx.config,
                        search.as_deref(),
                        page_size,
                        after.as_deref(),
                        None,
                        all,
                    )
                    .await?;
                    ctx.emit_with_next(
                        "sfx list",
                        data,
                        vec![
                            "Pick history_item_id from history[]".into(),
                            "elabs sfx download --id <history_item_id> -o out.mp3".into(),
                        ],
                    )
                }
                SfxCommands::Download { id, output } => {
                    let data = ops::run_sfx_download(&auth, &ctx.config, &id, output, None)
                        .await?;
                    ctx.emit("sfx download", data)
                }
            }
        }
        Commands::Models => {
            let auth = load_api_key(ctx.config.as_ref())?;
            let data = ops::run_models_list(&auth, &ctx.config).await?;
            ctx.emit("models list", data)
        }
        Commands::Capabilities => ctx.emit("capabilities", capabilities::capabilities_json()),
        Commands::Env { command } => match command {
            EnvCommands::Schema => ctx.emit("env schema", capabilities::env_schema_json()),
        },
    }
}
