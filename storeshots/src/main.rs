mod ads;
mod background;
mod brand;
mod cache;
mod capabilities;
mod composite;
mod config;
mod copy;
mod devices;
mod discover;
mod export;
mod fonts;
mod gemini;
mod interactive;
mod keys;
mod openrouter;
mod output;
mod pipeline;
mod print;
mod prompts;
mod sizes;
mod text_client;
mod validate;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use config::StoreshotsConfig;
use discover::{init_app, resolve_app_root};
use export::{export_filename, resize_to, write_png_rgb};
use output::Envelope;
use prompts::overrides_from_cli;
use serde::Serialize;
use sizes::{ExportSize, IPHONE_SIZES};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "storeshots",
    about = "Generate marketing assets: brand boards, App Store screenshots, print",
    version,
    after_help = "Agents: run `storeshots capabilities --json` and `storeshots config schema --json`.\nHumans: run `storeshots interactive` for menu mode.\nUse --json on subcommands for structured output."
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init {
        #[arg(long, short)]
        app: Option<PathBuf>,
        #[arg(long)]
        name: Option<String>,
    },
    Brand {
        #[command(subcommand)]
        command: BrandCommands,
    },
    Copy {
        #[command(subcommand)]
        command: CopyCommands,
    },
    Mobile {
        #[command(subcommand)]
        command: MobileCommands,
    },
    Print {
        #[command(subcommand)]
        command: PrintCommands,
    },
    Ads {
        #[command(subcommand)]
        command: AdsCommands,
    },
    /// Alias for `mobile render`
    Render {
        #[arg(long, short)]
        app: Option<PathBuf>,
        #[arg(long)]
        no_ai: bool,
        #[arg(long)]
        all_sizes: bool,
        #[arg(long, value_delimiter = ',')]
        only: Vec<usize>,
        #[arg(long, default_value = "en")]
        locale: String,
        #[arg(long)]
        yes: bool,
    },
    /// Alias for `mobile validate`
    Validate {
        #[arg(long, short)]
        app: Option<PathBuf>,
    },
    Run {
        #[arg(long, short)]
        app: Option<PathBuf>,
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        #[arg(long)]
        no_ai: bool,
        #[arg(long)]
        all_sizes: bool,
        #[arg(long)]
        yes: bool,
        #[arg(long, action = clap::ArgAction::Append)]
        prompt_append: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append)]
        prompt_file: Vec<PathBuf>,
    },
    Capabilities,
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },
    /// Menu-driven session for humans (stays open until you quit)
    #[command(visible_alias = "i")]
    Interactive {
        #[arg(long, short)]
        app: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum BrandCommands {
    /// Generate docs/BRAND.md from project source via LLM
    Extract {
        #[arg(long, short)]
        app: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
        #[arg(long, action = clap::ArgAction::Append)]
        prompt_append: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append)]
        prompt_file: Vec<PathBuf>,
    },
    /// Check BRAND.md completeness
    Validate {
        #[arg(long, short)]
        app: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum CopyCommands {
    Suggest {
        #[arg(long, short)]
        app: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        features: Option<String>,
        #[arg(long, action = clap::ArgAction::Append)]
        prompt_append: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append)]
        prompt_file: Vec<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum MobileCommands {
    Render {
        #[arg(long, short)]
        app: Option<PathBuf>,
        #[arg(long)]
        no_ai: bool,
        #[arg(long)]
        all_sizes: bool,
        #[arg(long, value_delimiter = ',')]
        only: Vec<usize>,
        #[arg(long, default_value = "en")]
        locale: String,
        #[arg(long)]
        yes: bool,
    },
    Validate {
        #[arg(long, short)]
        app: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum PrintCommands {
    /// List supported print layout formats (`--format` values for `print render`)
    Formats,
    /// LLM → print copy in storeshots.toml [print.copy]
    Suggest {
        #[arg(long, short)]
        app: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
        #[arg(long, action = clap::ArgAction::Append)]
        prompt_append: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append)]
        prompt_file: Vec<PathBuf>,
    },
    /// Render print collateral to storeshots/out/print/ (PNG + PDF)
    Render {
        #[arg(long, short)]
        app: Option<PathBuf>,
        #[arg(
            long,
            help = "Print layout format (run `storeshots print formats` for the full list)",
            long_help = "Print layout format.\n\nSupported values:\n  trifold              Tri-fold brochure (11×8.5 in, outside + inside)\n  single-landscape     One-page flyer, landscape\n  single-portrait      One-page flyer, portrait\n  business-card        Business card front/back\n\nRun `storeshots print formats` for sizes and output files.",
            value_parser = clap::builder::PossibleValuesParser::new(print::format_id_slice())
        )]
        format: String,
        #[arg(
            long,
            help = "Business-card only: front, back, or both (default: both)"
        )]
        variant: Option<String>,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum AdsCommands {
    /// List supported ad sizes and format groups
    Formats,
    /// LLM → ad layouts in storeshots.toml [ads.items]
    Suggest {
        #[arg(long, short)]
        app: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
        #[arg(long, action = clap::ArgAction::Append)]
        prompt_append: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append)]
        prompt_file: Vec<PathBuf>,
    },
    /// Render marketing ads to storeshots/out/ads/
    Render {
        #[arg(long, short)]
        app: Option<PathBuf>,
        #[arg(long)]
        no_ai: bool,
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        #[arg(long, value_delimiter = ',', help = "Format group or format id filter")]
        formats: Vec<String>,
        #[arg(long, default_value = "en")]
        locale: String,
        #[arg(long)]
        yes: bool,
    },
    /// Check ads output dimensions and completeness
    Validate {
        #[arg(long, short)]
        app: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCommands {
    Schema,
    Keys,
}

#[derive(Subcommand, Debug)]
enum EnvCommands {
    Schema,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => {
            if cli.json {
                bail!("subcommand required when using --json");
            }
            if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                interactive::run(None).await
            } else {
                use clap::CommandFactory;
                Cli::command().print_help()?;
                println!();
                println!("Tip: run `storeshots interactive` for menu mode.");
                Ok(())
            }
        }
        Some(Commands::Init { app, name }) => cmd_init(app, name, cli.json),
        Some(Commands::Brand { command }) => match command {
            BrandCommands::Extract {
                app,
                dry_run,
                yes,
                prompt_append,
                prompt_file,
            } => {
                cmd_brand_extract(app, dry_run, yes, prompt_append, prompt_file, cli.json).await
            }
            BrandCommands::Validate { app } => cmd_brand_validate(app, cli.json),
        },
        Some(Commands::Copy { command }) => match command {
            CopyCommands::Suggest {
                app,
                dry_run,
                yes,
                features,
                prompt_append,
                prompt_file,
            } => {
                cmd_copy_suggest(
                    app,
                    dry_run,
                    yes,
                    features,
                    prompt_append,
                    prompt_file,
                    cli.json,
                )
                .await
            }
        },
        Some(Commands::Mobile { command }) => match command {
            MobileCommands::Render {
                app,
                no_ai,
                all_sizes,
                only,
                locale,
                yes,
            } => cmd_render(app, no_ai, all_sizes, only, locale, yes, cli.json).await,
            MobileCommands::Validate { app } => cmd_validate(app, cli.json),
        },
        Some(Commands::Print { command }) => match command {
            PrintCommands::Formats => cmd_print_formats(cli.json),
            PrintCommands::Suggest {
                app,
                dry_run,
                yes,
                prompt_append,
                prompt_file,
            } => {
                cmd_print_suggest(app, dry_run, yes, prompt_append, prompt_file, cli.json).await
            }
            PrintCommands::Render {
                app,
                format,
                variant,
                yes,
            } => cmd_print_render(app, format, variant, yes, cli.json),
        },
        Some(Commands::Ads { command }) => match command {
            AdsCommands::Formats => cmd_ads_formats(cli.json),
            AdsCommands::Suggest {
                app,
                dry_run,
                yes,
                prompt_append,
                prompt_file,
            } => cmd_ads_suggest(app, dry_run, yes, prompt_append, prompt_file, cli.json).await,
            AdsCommands::Render {
                app,
                no_ai,
                only,
                formats,
                locale,
                yes,
            } => cmd_ads_render(app, no_ai, only, formats, locale, yes, cli.json).await,
            AdsCommands::Validate { app } => cmd_ads_validate(app, cli.json),
        },
        Some(Commands::Render {
            app,
            no_ai,
            all_sizes,
            only,
            locale,
            yes,
        }) => cmd_render(app, no_ai, all_sizes, only, locale, yes, cli.json).await,
        Some(Commands::Validate { app }) => cmd_validate(app, cli.json),
        Some(Commands::Run {
            app,
            only,
            no_ai,
            all_sizes,
            yes,
            prompt_append,
            prompt_file,
        }) => {
            cmd_run(
                app,
                only,
                no_ai,
                all_sizes,
                yes,
                prompt_append,
                prompt_file,
                cli.json,
            )
            .await
        }
        Some(Commands::Capabilities) => cmd_capabilities(cli.json),
        Some(Commands::Config { command }) => match command {
            ConfigCommands::Schema => cmd_config_schema(cli.json),
            ConfigCommands::Keys => cmd_config_keys(cli.json),
        },
        Some(Commands::Env { command }) => match command {
            EnvCommands::Schema => cmd_env_schema(cli.json),
        },
        Some(Commands::Interactive { app }) => interactive::run(app).await,
    }
}

fn require_yes(yes: bool, json: bool, dry_run: bool) -> Result<()> {
    if !yes && !dry_run && !json {
        bail!("use --yes to proceed, or --dry-run to preview");
    }
    Ok(())
}

pub(crate) fn cmd_init(app: Option<PathBuf>, name: Option<String>, json: bool) -> Result<()> {
    let root = resolve_app_root(app)?;
    let cfg = init_app(&root, name)?;

    if json {
        Envelope::ok(
            "init",
            serde_json::json!({
                "app_root": root,
                "config": root.join("storeshots.toml"),
                "raw_dir": root.join("storeshots/raw"),
                "prompts_dir": root.join("storeshots/prompts"),
                "brand": root.join(&cfg.paths.brand),
            }),
        )
        .with_next_actions(vec![
            "Add raw PNG captures to storeshots/raw/".into(),
            "Run: storeshots brand extract --yes".into(),
            "Run: storeshots copy suggest --yes".into(),
            "Run: storeshots mobile render --all-sizes --yes".into(),
        ])
        .print_json()?;
    } else {
        println!(
            "{} Initialized storeshots in {}",
            "✓".green(),
            root.display()
        );
        println!("  Config: {}", root.join("storeshots.toml").display());
        println!("  Prompts: {}", root.join("storeshots/prompts").display());
        println!("  Brand: {}", cfg.brand_path(&root).display());
        println!("  App name: {}", cfg.app.name);
    }
    Ok(())
}

pub(crate) async fn cmd_brand_extract(
    app: Option<PathBuf>,
    dry_run: bool,
    yes: bool,
    prompt_append: Vec<String>,
    prompt_file: Vec<PathBuf>,
    json: bool,
) -> Result<()> {
    require_yes(yes, json, dry_run)?;
    let root = resolve_app_root(app)?;
    let cfg = StoreshotsConfig::load_relaxed(&root)?;
    let overrides = overrides_from_cli(&prompt_append, &prompt_file);

    let markdown =
        brand::extract::extract_brand(&root, &cfg, &overrides, dry_run).await?;

    if json {
        Envelope::ok(
            "brand extract",
            serde_json::json!({
                "dry_run": dry_run,
                "brand_path": cfg.brand_path(&root),
                "chars": markdown.len(),
                "preview": if dry_run { markdown.chars().take(500).collect::<String>() } else { String::new() },
            }),
        )
        .with_next_actions(vec!["storeshots brand validate".into()])
        .print_json()?;
    } else if dry_run {
        println!("{}", markdown);
    } else {
        println!(
            "{} Wrote brand guide to {}",
            "✓".green(),
            cfg.brand_path(&root).display()
        );
    }
    Ok(())
}

pub(crate) fn cmd_brand_validate(app: Option<PathBuf>, json: bool) -> Result<()> {
    let root = resolve_app_root(app)?;
    let cfg = StoreshotsConfig::load_relaxed(&root)?;
    let issues = brand::validate::validate_brand_file(&root, &cfg)?;

    let has_errors = issues.iter().any(|i| i.severity == "error");

    if json {
        Envelope::ok(
            "brand validate",
            serde_json::json!({ "ok": !has_errors, "issues": issues }),
        )
        .print_json()?;
    } else if issues.is_empty() {
        println!("{} Brand guide looks good", "✓".green());
    } else {
        for issue in &issues {
            let mark = if issue.severity == "error" {
                "✗".red()
            } else {
                "!".yellow()
            };
            println!("{} {}", mark, issue.message);
        }
    }

    brand::validate::ensure_valid(&issues)?;
    Ok(())
}

pub(crate) async fn cmd_copy_suggest(
    app: Option<PathBuf>,
    dry_run: bool,
    yes: bool,
    features: Option<String>,
    prompt_append: Vec<String>,
    prompt_file: Vec<PathBuf>,
    json: bool,
) -> Result<()> {
    require_yes(yes, json, dry_run)?;
    let root = resolve_app_root(app)?;
    let mut cfg = StoreshotsConfig::load_relaxed(&root)?;
    let overrides = overrides_from_cli(&prompt_append, &prompt_file);

    let hint = features.unwrap_or_else(|| copy::read_features_hint(&root));
    let suggested = copy::suggest_copy(&root, &cfg, &hint, &overrides).await?;

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

pub(crate) async fn cmd_render(
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
        bail!("stores.apple_iphone is false; iOS rendering is the only supported mobile target");
    }

    if !yes && !json {
        bail!("use --yes to render (may call Gemini for backgrounds)");
    }

    let use_ai = cfg.ai.backgrounds && !no_ai;
    let client = if use_ai {
        match text_client::gemini_for_render(&root, &cfg) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("warning: {e}; rendering with gradient backgrounds instead");
                None
            }
        }
    } else {
        None
    };
    let use_ai = use_ai && client.is_some();

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

    for &idx in &slide_indices {
        if idx == 0 || idx > cfg.slides.items.len() {
            bail!(
                "invalid slide index {idx}; valid range 1..={}",
                cfg.slides.items.len()
            );
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
            "mobile render",
            serde_json::json!({
                "files": written.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "ai_backgrounds": use_ai,
                "sizes": sizes.iter().map(|s| s.label).collect::<Vec<_>>(),
            }),
        )
        .print_json()?;
    } else {
        println!(
            "{} Rendered {} file(s) to {}",
            "✓".green(),
            written.len(),
            root.join("storeshots/out/mobile").display()
        );
        for p in &written {
            println!("  {}", p.display());
        }
    }
    Ok(())
}

pub(crate) fn cmd_validate(app: Option<PathBuf>, json: bool) -> Result<()> {
    let root = resolve_app_root(app)?;
    let out_dir = root.join("storeshots/out/mobile");
    let legacy = root.join("storeshots/out");
    let issues = validate::validate_outputs(if out_dir.exists() {
        &out_dir
    } else {
        &legacy
    });

    if json {
        #[derive(Serialize)]
        struct Data {
            ok: bool,
            issues: Vec<validate::ValidationIssue>,
        }
        let ok = issues.is_empty();
        Envelope::ok("mobile validate", Data { ok, issues: issues.clone() }).print_json()?;
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

pub(crate) async fn cmd_print_suggest(
    app: Option<PathBuf>,
    dry_run: bool,
    yes: bool,
    prompt_append: Vec<String>,
    prompt_file: Vec<PathBuf>,
    json: bool,
) -> Result<()> {
    require_yes(yes, json, dry_run)?;
    let root = resolve_app_root(app)?;
    let mut cfg = StoreshotsConfig::load_relaxed(&root)?;
    let overrides = overrides_from_cli(&prompt_append, &prompt_file);
    let suggested = print::suggest_print_copy(&root, &cfg, &overrides).await?;

    if dry_run {
        if json {
            Envelope::ok("print suggest", &suggested).print_json()?;
        } else {
            println!("{}", serde_json::to_string_pretty(&suggested)?);
        }
        return Ok(());
    }

    print::apply_print_copy(&mut cfg, &suggested);
    cfg.save(&root)?;

    if json {
        Envelope::ok("print suggest", suggested)
            .with_next_actions(vec!["Run: storeshots print render --format business-card --yes".into()])
            .print_json()?;
    } else {
        println!("{} Updated print copy in storeshots.toml", "✓".green());
        println!("  card_tagline: {}", suggested.card_tagline);
        println!("  headline: {}", suggested.headline);
        println!("  bullets: {}", suggested.bullets.len());
    }
    Ok(())
}

pub(crate) fn cmd_print_formats(json: bool) -> Result<()> {
    if json {
        Envelope::ok(
            "print formats",
            serde_json::json!({
                "formats": print::PRINT_FORMATS,
                "usage": "storeshots print render --format <id> --yes",
            }),
        )
        .print_json()?;
    } else {
        println!("Supported print formats:\n");
        for spec in print::PRINT_FORMATS {
            println!(
                "  {}  {}",
                spec.id.cyan().bold(),
                spec.title
            );
            println!("      Size:   {}", spec.size);
            println!("      Output: {}", spec.output);
            if let Some(v) = spec.variant {
                println!("      Variant (--variant): {}", v);
            }
            println!();
        }
        println!(
            "Render: {} {}",
            "storeshots print render --format".dimmed(),
            "<id> --yes".cyan()
        );
    }
    Ok(())
}

pub(crate) fn cmd_print_render(
    app: Option<PathBuf>,
    format: String,
    variant: Option<String>,
    yes: bool,
    json: bool,
) -> Result<()> {
    if !yes && !json {
        bail!("use --yes to render print assets");
    }
    let root = resolve_app_root(app)?;
    let cfg = StoreshotsConfig::load_relaxed(&root)?;
    let variant_ref = variant.as_deref();
    let output = print::render_format(&root, &cfg, &format, variant_ref)?;

    if json {
        Envelope::ok(
            "print render",
            serde_json::json!({
                "format": output.format,
                "files": output.files,
            }),
        )
        .print_json()?;
    } else {
        println!(
            "{} Rendered print format '{}' ({} files)",
            "✓".green(),
            format,
            output.files.len()
        );
        for path in &output.files {
            println!("  {}", path.display());
        }
    }
    Ok(())
}

pub(crate) async fn cmd_run(
    app: Option<PathBuf>,
    only: Vec<String>,
    no_ai: bool,
    all_sizes: bool,
    yes: bool,
    prompt_append: Vec<String>,
    prompt_file: Vec<PathBuf>,
    json: bool,
) -> Result<()> {
    if !yes && !json {
        bail!("use --yes to run pipeline");
    }
    let root = resolve_app_root(app)?;
    let cfg = StoreshotsConfig::load_relaxed(&root)?;
    let steps = pipeline::resolve_steps(&cfg, &only)?;
    let overrides = overrides_from_cli(&prompt_append, &prompt_file);

    let mut completed = Vec::new();

    for step in &steps {
        match step.phase.as_str() {
            "brand" => {
                brand::extract::extract_brand(&root, &cfg, &overrides, false).await?;
                completed.push(step.id.clone());
            }
            "copy" => {
                let mut cfg_mut = cfg.clone();
                let hint = copy::read_features_hint(&root);
                let suggested = copy::suggest_copy(&root, &cfg_mut, &hint, &overrides).await?;
                copy::apply_copy(&mut cfg_mut, &suggested);
                cfg_mut.save(&root)?;
                completed.push(step.id.clone());
            }
            "mobile" => {
                cmd_render(
                    Some(root.clone()),
                    no_ai,
                    all_sizes,
                    vec![],
                    "en".into(),
                    true,
                    json,
                )
                .await?;
                completed.push(step.id.clone());
            }
            "print" => {
                cmd_print_render(Some(root.clone()), "trifold".into(), None, true, json)?;
                completed.push(step.id.clone());
            }
            "ads" => {
                cmd_ads_suggest(
                    Some(root.clone()),
                    false,
                    true,
                    prompt_append.clone(),
                    prompt_file.clone(),
                    json,
                )
                .await?;
                cmd_ads_render(
                    Some(root.clone()),
                    no_ai,
                    vec![],
                    vec![],
                    "en".into(),
                    true,
                    json,
                )
                .await?;
                completed.push(step.id.clone());
            }
            other => bail!("unknown pipeline phase: {other}"),
        }
    }

    if json {
        Envelope::ok(
            "run",
            serde_json::json!({ "completed_steps": completed }),
        )
        .print_json()?;
    } else {
        println!(
            "{} Pipeline completed: {}",
            "✓".green(),
            completed.join(", ")
        );
    }
    Ok(())
}

pub(crate) async fn cmd_ads_suggest(
    app: Option<PathBuf>,
    dry_run: bool,
    yes: bool,
    prompt_append: Vec<String>,
    prompt_file: Vec<PathBuf>,
    json: bool,
) -> Result<()> {
    require_yes(yes, json, dry_run)?;
    let root = resolve_app_root(app)?;
    let mut cfg = StoreshotsConfig::load_relaxed(&root)?;
    let overrides = overrides_from_cli(&prompt_append, &prompt_file);
    let suggested = ads::suggest_ads(&root, &cfg, &overrides).await?;

    if dry_run {
        if json {
            Envelope::ok("ads suggest", &suggested).print_json()?;
        } else {
            println!("{}", serde_json::to_string_pretty(&suggested)?);
        }
        return Ok(());
    }

    ads::apply_ads(&mut cfg, &suggested);
    cfg.save(&root)?;

    if json {
        Envelope::ok(
            "ads suggest",
            serde_json::json!({ "applied": true, "ads": suggested.ads }),
        )
        .with_next_actions(vec!["Run: storeshots ads render --yes".into()])
        .print_json()?;
    } else {
        println!("{} Updated ad layouts in storeshots.toml", "✓".green());
        for a in &suggested.ads {
            println!(
                "  {} — {} (groups: {})",
                a.id.bold(),
                a.headline.replace('\n', " / "),
                a.format_groups.join(", ")
            );
        }
    }
    Ok(())
}

pub(crate) async fn cmd_ads_render(
    app: Option<PathBuf>,
    no_ai: bool,
    only: Vec<String>,
    formats: Vec<String>,
    locale: String,
    yes: bool,
    json: bool,
) -> Result<()> {
    if !yes && !json {
        bail!("use --yes to render ads (may call Gemini for backgrounds)");
    }
    let root = resolve_app_root(app)?;
    let cfg = StoreshotsConfig::load_relaxed(&root)?;

    let output = ads::render_ads(&root, &cfg, no_ai, &only, &formats, &locale).await?;

    if json {
        Envelope::ok(
            "ads render",
            serde_json::json!({
                "files": output.files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "ads_rendered": output.ads_rendered,
                "ai_backgrounds": cfg.ai.backgrounds && !no_ai,
            }),
        )
        .print_json()?;
    } else {
        println!(
            "{} Rendered {} ad file(s) for {} concept(s)",
            "✓".green(),
            output.files.len(),
            output.ads_rendered
        );
        for p in &output.files {
            println!("  {}", p.display());
        }
    }
    Ok(())
}

pub(crate) fn cmd_ads_validate(app: Option<PathBuf>, json: bool) -> Result<()> {
    let root = resolve_app_root(app)?;
    let cfg = StoreshotsConfig::load_relaxed(&root)?;
    let issues = ads::validate_ads_output(&root, &cfg);

    if json {
        Envelope::ok(
            "ads validate",
            serde_json::json!({ "ok": issues.is_empty(), "issues": issues }),
        )
        .print_json()?;
    } else if issues.is_empty() {
        println!("{} All ad outputs look valid", "✓".green());
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

pub(crate) fn cmd_ads_formats(json: bool) -> Result<()> {
    let data = ads::formats_list_json();
    if json {
        Envelope::ok("ads formats", data).print_json()?;
    } else {
        println!("Supported ad format groups:\n");
        if let Some(groups) = data.get("groups").and_then(|g| g.as_array()) {
            for g in groups {
                println!(
                    "  {}  {}",
                    g["id"].as_str().unwrap_or("").cyan().bold(),
                    g["description"].as_str().unwrap_or("")
                );
            }
            println!();
        }
        println!("Supported ad sizes:\n");
        if let Some(formats) = data.get("formats").and_then(|f| f.as_array()) {
            for f in formats {
                println!(
                    "  {}  {}×{}  [{} / {}]",
                    f["id"].as_str().unwrap_or("").cyan(),
                    f["width"].as_u64().unwrap_or(0),
                    f["height"].as_u64().unwrap_or(0),
                    f["platform"].as_str().unwrap_or(""),
                    f["group"].as_str().unwrap_or(""),
                );
            }
        }
        println!(
            "\nSuggest: {} {}",
            "storeshots ads suggest --yes".dimmed(),
            "| Render: storeshots ads render --yes".cyan()
        );
    }
    Ok(())
}

pub(crate) fn cmd_capabilities(json: bool) -> Result<()> {
    let data = capabilities::capabilities_json();
    if json {
        Envelope::ok("capabilities", data).print_json()?;
    } else {
        println!("{}", serde_json::to_string_pretty(&data)?);
        println!("\nTip: storeshots capabilities --json");
    }
    Ok(())
}

fn cmd_config_schema(json: bool) -> Result<()> {
    let schema = capabilities::config_schema_json();
    if json {
        Envelope::ok("config schema", schema).print_json()?;
    } else {
        println!("{}", serde_json::to_string_pretty(&schema)?);
    }
    Ok(())
}

pub(crate) fn cmd_config_keys(json: bool) -> Result<()> {
    let schema = keys::keys_schema_json();
    if json {
        Envelope::ok("config keys", schema).print_json()?;
    } else {
        println!("{}", serde_json::to_string_pretty(&schema)?);
    }
    Ok(())
}

fn cmd_env_schema(json: bool) -> Result<()> {
    let schema = serde_json::json!({
        "variables": [
            {
                "key": "STORESHOTS_OPENROUTER_API_KEY",
                "aliases": [],
                "required_for": ["global storeshots CLI OpenRouter fallback"],
                "secret": true,
                "note": "Prefer per-project storeshots/secrets.toml or [ai.keys].openrouter_env"
            },
            {
                "key": "STORESHOTS_GEMINI_API_KEY",
                "aliases": [],
                "required_for": ["global storeshots CLI Gemini fallback"],
                "secret": true,
                "note": "Prefer per-project storeshots/secrets.toml or [ai.keys].gemini_env"
            },
            {
                "key": "OPENROUTER_API_KEY",
                "aliases": [],
                "required_for": ["legacy global fallback only"],
                "secret": true
            },
            {
                "key": "GEMINI_API_KEY",
                "aliases": ["GOOGLE_API_KEY"],
                "required_for": ["legacy global fallback only"],
                "secret": true
            },
            {
                "key": "STORESHOTS_MODEL_IMAGE",
                "aliases": [],
                "required_for": [],
                "default": "gemini-2.5-flash-image"
            },
            {
                "key": "STORESHOTS_MODEL_TEXT",
                "aliases": [],
                "required_for": [],
                "default": "google/gemini-2.5-flash (openrouter) or gemini-2.5-flash (gemini)"
            },
        ],
        "projectSecretsFile": {
            "default": "storeshots/secrets.toml",
            "gitignore": true,
            "fields": ["openrouter", "gemini"]
        },
        "projectEnvNames": {
            "description": "Set openrouter_env / gemini_env in storeshots.toml to point at per-project env var NAMES",
            "example": { "openrouter_env": "SOKI_CREATIVE_OPENROUTER_API_KEY", "gemini_env": "SOKI_CREATIVE_GEMINI_API_KEY" }
        },
        "resolutionOrder": keys::keys_schema_json()["resolution_order"],
        "precedence": "secrets.toml literals > [ai.keys].*_env > STORESHOTS_* > legacy global env",
        "promptPrecedence": "CLI --prompt-append > per-item TOML > [ai.prompts.{phase}] > storeshots/prompts/{phase}.append.md"
    });

    if json {
        Envelope::ok("env schema", schema).print_json()?;
    } else {
        println!("{}", serde_json::to_string_pretty(&schema)?);
    }
    Ok(())
}
