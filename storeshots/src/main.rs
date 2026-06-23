mod background;
mod cache;
mod composite;
mod config;
mod copy;
mod devices;
mod discover;
mod export;
mod fonts;
mod gemini;
mod output;
mod sizes;
mod validate;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use config::StoreshotsConfig;
use discover::{init_app, resolve_app_root};
use export::{export_filename, resize_to, write_png_rgb};
use output::Envelope;
use serde::Serialize;
use sizes::{ExportSize, IPHONE_SIZES};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "storeshots",
    about = "Generate App Store marketing screenshots from raw captures",
    version,
    after_help = "Agents: run `storeshots --help` and use --json on subcommands for structured output."
)]
struct Cli {
    /// Emit JSON envelope on supported commands
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Create storeshots.toml and folder scaffold in an app repo
    Init {
        /// App project root (default: current directory)
        #[arg(long, short)]
        app: Option<PathBuf>,

        /// Override detected app name
        #[arg(long)]
        name: Option<String>,
    },

    /// Suggest headline copy via Gemini and update storeshots.toml
    Copy {
        #[command(subcommand)]
        command: CopyCommands,
    },

    /// Render screenshots to storeshots/out/
    Render {
        #[arg(long, short)]
        app: Option<PathBuf>,

        /// Skip AI backgrounds (use gradient fallback)
        #[arg(long)]
        no_ai: bool,

        /// Export all required iPhone sizes (default: largest only)
        #[arg(long)]
        all_sizes: bool,

        /// Comma-separated 1-based slide indices to render (default: all)
        #[arg(long, value_delimiter = ',')]
        only: Vec<usize>,

        /// Locale tag for filenames (default: en)
        #[arg(long, default_value = "en")]
        locale: String,

        /// Non-interactive: proceed without prompts
        #[arg(long)]
        yes: bool,
    },

    /// Validate rendered PNG dimensions
    Validate {
        #[arg(long, short)]
        app: Option<PathBuf>,
    },

    /// Show environment variables for automation
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },
}

#[derive(Subcommand, Debug)]
enum CopyCommands {
    /// Generate titles/subtitles from app context
    Suggest {
        #[arg(long, short)]
        app: Option<PathBuf>,

        #[arg(long)]
        dry_run: bool,

        #[arg(long)]
        yes: bool,

        /// Extra feature context for the model
        #[arg(long)]
        features: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum EnvCommands {
    Schema,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { app, name } => cmd_init(app, name, cli.json),
        Commands::Copy { command } => match command {
            CopyCommands::Suggest {
                app,
                dry_run,
                yes,
                features,
            } => cmd_copy_suggest(app, dry_run, yes, features, cli.json).await,
        },
        Commands::Render {
            app,
            no_ai,
            all_sizes,
            only,
            locale,
            yes,
        } => cmd_render(app, no_ai, all_sizes, only, locale, yes, cli.json).await,
        Commands::Validate { app } => cmd_validate(app, cli.json),
        Commands::Env { command } => match command {
            EnvCommands::Schema => cmd_env_schema(cli.json),
        },
    }
}

fn cmd_init(app: Option<PathBuf>, name: Option<String>, json: bool) -> Result<()> {
    let root = resolve_app_root(app)?;
    let cfg = init_app(&root, name)?;

    if json {
        Envelope::ok(
            "init",
            serde_json::json!({
                "app_root": root,
                "config": root.join("storeshots.toml"),
                "raw_dir": root.join("storeshots/raw"),
            }),
        )
        .with_next_actions(vec![
            "Add raw PNG captures to storeshots/raw/".into(),
            "Edit storeshots.toml slides and brand colors".into(),
            "Run: storeshots copy suggest --yes".into(),
            "Run: storeshots render --all-sizes --yes".into(),
        ])
        .print_json()?;
    } else {
        println!(
            "{} Initialized storeshots in {}",
            "✓".green(),
            root.display()
        );
        println!("  Config: {}", root.join("storeshots.toml").display());
        println!("  Drop raw screenshots in: {}", root.join("storeshots/raw").display());
        println!("  App name: {}", cfg.app.name);
    }
    Ok(())
}

async fn cmd_copy_suggest(
    app: Option<PathBuf>,
    dry_run: bool,
    yes: bool,
    features: Option<String>,
    json: bool,
) -> Result<()> {
    let root = resolve_app_root(app)?;
    let mut cfg = StoreshotsConfig::load(&root)?;

    if !yes && !dry_run && !json {
        bail!("use --yes to apply copy changes, or --dry-run to preview");
    }

    let client = gemini::GeminiClient::from_env()?;
    let hint = features.unwrap_or_else(|| copy::read_features_hint(&root));
    let suggested = copy::suggest_copy(&client, &cfg.ai.text_model, &cfg, &hint).await?;

    if dry_run {
        if json {
            Envelope::ok("copy suggest", suggested).print_json()?;
        } else {
            println!("{}", serde_json::to_string_pretty(&suggested)?);
        }
        return Ok(());
    }

    copy::apply_copy(&mut cfg, &suggested);
    cfg.save(&root)?;

    if json {
        Envelope::ok(
            "copy suggest",
            serde_json::json!({ "applied": true, "slides": suggested.slides }),
        )
        .print_json()?;
    } else {
        println!("{} Updated copy in storeshots.toml", "✓".green());
        for s in &suggested.slides {
            println!("  {} — {}", s.id.bold(), s.title.replace('\n', " / "));
        }
    }
    Ok(())
}

async fn cmd_render(
    app: Option<PathBuf>,
    no_ai: bool,
    all_sizes: bool,
    only: Vec<usize>,
    locale: String,
    yes: bool,
    json: bool,
) -> Result<()> {
    let root = resolve_app_root(app)?;
    let cfg = StoreshotsConfig::load(&root)?;

    if !cfg.stores.apple_iphone {
        bail!("stores.apple_iphone is false; iOS rendering is the only supported target in v0.1");
    }

    if !yes && !json {
        bail!("use --yes to render (may call Gemini for backgrounds)");
    }

    let use_ai = cfg.ai.backgrounds && !no_ai;
    let client = if use_ai {
        Some(gemini::GeminiClient::from_env()?)
    } else {
        None
    };

    let fonts = fonts::FontSet::load(&root, cfg.brand.font.as_deref())?;
    let (design_w, design_h) = composite::design_canvas_for_iphone();

    let sizes: Vec<&ExportSize> = if all_sizes {
        IPHONE_SIZES.iter().collect()
    } else {
        vec![&IPHONE_SIZES[0]]
    };

    let slide_indices: Vec<usize> = if only.is_empty() {
        (1..=cfg.slides.items.len()).collect()
    } else {
        only
    };

    let mut written = Vec::new();
    let warnings = Vec::new();

    for &idx in &slide_indices {
        if idx == 0 || idx > cfg.slides.items.len() {
            bail!("invalid slide index {idx}; valid range 1..={}", cfg.slides.items.len());
        }
        let slide = &cfg.slides.items[idx - 1];
        let rendered = composite::render_slide(composite::RenderContext {
            app_root: &root,
            cfg: &cfg,
            slide,
            canvas_w: design_w,
            canvas_h: design_h,
            use_ai,
            client: client.as_ref(),
            fonts: &fonts,
        })
        .await?;

        for size in &sizes {
            let out_img = if size.w == design_w && size.h == design_h {
                rendered.clone()
            } else {
                resize_to(&rendered, size)
            };
            let out_dir = StoreshotsConfig::out_dir(&root, "apple/iphone", size.label);
            let filename = export_filename(idx - 1, &slide.id, &locale, size);
            let out_path = out_dir.join(&filename);
            write_png_rgb(&out_path, &out_img)?;
            written.push(out_path);
        }
    }

    if json {
        Envelope::ok(
            "render",
            serde_json::json!({
                "files": written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "ai_backgrounds": use_ai,
                "sizes": sizes.iter().map(|s| s.label).collect::<Vec<_>>(),
            }),
        )
        .with_warnings(warnings)
        .print_json()?;
    } else {
        println!(
            "{} Rendered {} file(s) to {}",
            "✓".green(),
            written.len(),
            root.join("storeshots/out").display()
        );
        for p in &written {
            println!("  {}", p.display());
        }
    }

    let _ = warnings;
    Ok(())
}

fn cmd_validate(app: Option<PathBuf>, json: bool) -> Result<()> {
    let root = resolve_app_root(app)?;
    let out_dir = root.join("storeshots/out");
    let issues = validate::validate_outputs(&out_dir);

    if json {
        #[derive(Serialize)]
        struct Data {
            ok: bool,
            issues: Vec<validate::ValidationIssue>,
        }
        let ok = issues.is_empty();
        Envelope::ok(
            "validate",
            Data {
                ok,
                issues: issues.clone(),
            },
        )
        .print_json()?;
    } else if issues.is_empty() {
        println!("{} All outputs look valid", "✓".green());
    } else {
        for issue in &issues {
            println!("{} {} — {}", "✗".red(), issue.path, issue.message);
        }
        bail!("validation failed");
    }

    if !issues.is_empty() {
        bail!("validation failed");
    }
    Ok(())
}

fn cmd_env_schema(json: bool) -> Result<()> {
    let schema = serde_json::json!({
        "variables": [
            { "key": "GEMINI_API_KEY", "aliases": ["GOOGLE_API_KEY"], "required_for": ["render with AI backgrounds", "copy suggest"], "secret": true },
            { "key": "STORESHOTS_MODEL_IMAGE", "aliases": [], "required_for": [], "default": "gemini-2.5-flash-image" },
            { "key": "STORESHOTS_MODEL_TEXT", "aliases": [], "required_for": [], "default": "gemini-2.5-flash" },
        ],
        "precedence": "CLI flags > storeshots.toml > environment > defaults"
    });

    if json {
        Envelope::ok("env schema", schema).print_json()?;
    } else {
        println!("{}", serde_json::to_string_pretty(&schema)?);
    }
    Ok(())
}
