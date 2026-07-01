use crate::{
    cmd_brand_extract, cmd_brand_validate, cmd_capabilities, cmd_config_keys, cmd_copy_suggest,
    cmd_init, cmd_print_render, cmd_render, cmd_run, cmd_validate,
};
use crate::print::PRINT_FORMATS;
use anyhow::{Context, Result};
use colored::Colorize;
use crate::config::StoreshotsConfig;
use crate::discover::resolve_app_root;
use dialoguer::{Confirm, Input, MultiSelect, Select};
use std::path::PathBuf;

pub struct Session {
    pub app: Option<PathBuf>,
}

impl Session {
    fn root(&self) -> Result<PathBuf> {
        resolve_app_root(self.app.clone())
    }

    fn app_label(&self) -> String {
        match self.root() {
            Ok(root) => {
                let cfg_path = root.join("storeshots.toml");
                if cfg_path.exists() {
                    match StoreshotsConfig::load_relaxed(&root) {
                        Ok(cfg) => format!("{} ({})", cfg.app.name, root.display()),
                        Err(_) => root.display().to_string(),
                    }
                } else {
                    format!("{} (not initialized)", root.display())
                }
            }
            Err(e) => format!("(no project: {e})"),
        }
    }
}

pub async fn run(initial_app: Option<PathBuf>) -> Result<()> {
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        anyhow::bail!("interactive mode requires a terminal (try: storeshots interactive)");
    }

    let mut session = Session {
        app: initial_app,
    };

    print_banner();

    loop {
        println!();
        println!(
            "{} {}",
            "Project:".bold(),
            session.app_label().dimmed()
        );
        println!("{}", "─".repeat(48).dimmed());

        let items = [
            "Initialize project (storeshots init)",
            "Brand board",
            "Copy suggestions",
            "Mobile screenshots",
            "Print materials",
            "Run pipeline",
            "Validate & tools",
            "Change project folder",
            "Quit (q)",
        ];

        let choice = Select::new()
            .with_prompt("What would you like to do?")
            .items(&items)
            .default(0)
            .interact_opt()?;

        let Some(choice) = choice else {
            break;
        };

        if choice == items.len() - 1 {
            break;
        }

        let result = match choice {
            0 => menu_init(&mut session).await,
            1 => menu_brand(&session).await,
            2 => menu_copy(&session).await,
            3 => menu_mobile(&session).await,
            4 => menu_print(&session).await,
            5 => menu_pipeline(&session).await,
            6 => menu_tools(&session).await,
            7 => menu_change_project(&mut session),
            _ => Ok(()),
        };

        if let Err(e) = result {
            progress_err(&format!("{e:#}"));
            let cont = Confirm::new()
                .with_prompt("Return to main menu?")
                .default(true)
                .interact()?;
            if !cont {
                break;
            }
        }
    }

    println!();
    println!("{}", "Goodbye.".dimmed());
    Ok(())
}

fn print_banner() {
    println!();
    println!(
        "{} {}",
        "storeshots".bold().cyan(),
        "interactive".dimmed()
    );
    println!(
        "{}",
        "Menu-driven marketing assets — stay in this session until you quit (q)."
            .dimmed()
    );
}

fn progress_start(msg: &str) {
    println!();
    println!("{} {}", "→".cyan(), msg.bold());
}

fn progress_step(msg: &str) {
    println!("  {} {}", "·".dimmed(), msg);
}

fn progress_ok(msg: &str) {
    println!("{} {}", "✓".green(), msg);
}

fn progress_err(msg: &str) {
    println!("{} {}", "✗".red(), msg);
}

async fn run_labeled<F>(label: &str, fut: F) -> Result<()>
where
    F: std::future::Future<Output = Result<()>>,
{
    progress_start(label);
    match fut.await {
        Ok(()) => {
            progress_ok(label);
            Ok(())
        }
        Err(e) => {
            progress_err(&format!("{label}: {e:#}"));
            Err(e)
        }
    }
}

async fn menu_init(session: &mut Session) -> Result<()> {
    let root = session.root()?;
    let default_name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("My App")
        .to_string();

    let name: String = Input::new()
        .with_prompt("App / project display name")
        .default(default_name)
        .interact_text()?;

    let name_opt = if name.trim().is_empty() {
        None
    } else {
        Some(name)
    };

    run_labeled("Initialize project", async {
        cmd_init(Some(root.clone()), name_opt, false)?;
        session.app = Some(root);
        Ok(())
    })
    .await
}

async fn menu_brand(session: &Session) -> Result<()> {
    let items = [
        "Extract BRAND.md from source (LLM)",
        "Validate BRAND.md",
        "Back",
    ];
    let choice = Select::new()
        .with_prompt("Brand")
        .items(&items)
        .interact()?;

    let app = session.app.clone();
    match choice {
        0 => {
            let dry_run = Confirm::new()
                .with_prompt("Dry run (preview only, no write)?")
                .default(false)
                .interact()?;
            let yes = dry_run
                || Confirm::new()
                    .with_prompt("Proceed with brand extraction?")
                    .default(true)
                    .interact()?;

            run_labeled("Brand extract", async {
                cmd_brand_extract(app, dry_run, yes, vec![], vec![], false).await
            })
            .await
        }
        1 => {
            run_labeled("Brand validate", async {
                cmd_brand_validate(app, false)?;
                Ok(())
            })
            .await
        }
        _ => Ok(()),
    }
}

async fn menu_copy(session: &Session) -> Result<()> {
    let dry_run = Confirm::new()
        .with_prompt("Dry run (preview only)?")
        .default(false)
        .interact()?;

    let use_features = Confirm::new()
        .with_prompt("Provide a features hint?")
        .default(false)
        .interact()?;

    let features = if use_features {
        Some(
            Input::new()
                .with_prompt("Features hint")
                .interact_text()?,
        )
    } else {
        None
    };

    let yes = dry_run
        || Confirm::new()
            .with_prompt("Proceed with copy suggestion?")
            .default(true)
            .interact()?;

    let app = session.app.clone();
    run_labeled("Copy suggest", async {
        cmd_copy_suggest(app, dry_run, yes, features, vec![], vec![], false).await
    })
    .await
}

async fn menu_mobile(session: &Session) -> Result<()> {
    let items = ["Render screenshots", "Validate output", "Back"];
    let choice = Select::new()
        .with_prompt("Mobile")
        .items(&items)
        .interact()?;

    let app = session.app.clone();
    match choice {
        0 => {
            let no_ai = Confirm::new()
                .with_prompt("Skip AI backgrounds (use solid colors)?")
                .default(false)
                .interact()?;
            let all_sizes = Confirm::new()
                .with_prompt("Export all App Store sizes?")
                .default(true)
                .interact()?;
            let yes = Confirm::new()
                .with_prompt("Proceed with render?")
                .default(true)
                .interact()?;

            run_labeled("Mobile render", async {
                progress_step(if no_ai {
                    "AI backgrounds: off"
                } else {
                    "AI backgrounds: on"
                });
                progress_step(if all_sizes {
                    "Sizes: all App Store export sizes"
                } else {
                    "Sizes: primary only"
                });
                cmd_render(app, no_ai, all_sizes, vec![], "en".into(), yes, false).await
            })
            .await
        }
        1 => {
            run_labeled("Mobile validate", async {
                cmd_validate(app, false)?;
                Ok(())
            })
            .await
        }
        _ => Ok(()),
    }
}

async fn menu_print(session: &Session) -> Result<()> {
    let format_labels: Vec<String> = PRINT_FORMATS
        .iter()
        .map(|f| format!("{} — {}", f.id, f.title))
        .collect();
    let format_ids: Vec<&str> = PRINT_FORMATS.iter().map(|f| f.id).collect();
    let format_idx = Select::new()
        .with_prompt("Print format")
        .items(&format_labels)
        .default(0)
        .interact()?;

    let yes = Confirm::new()
        .with_prompt("Proceed with print render?")
        .default(true)
        .interact()?;

    let app = session.app.clone();
    let format = format_ids[format_idx].to_string();
    run_labeled(&format!("Print render ({format})"), async {
        cmd_print_render(app, format, None, yes, false)?;
        Ok(())
    })
    .await
}

async fn menu_pipeline(session: &Session) -> Result<()> {
    let root = session.root()?;
    let cfg = StoreshotsConfig::load_relaxed(&root).context("load storeshots.toml")?;
    let steps = cfg.default_pipeline();

    if steps.is_empty() {
        progress_err("No pipeline steps configured in storeshots.toml");
        return Ok(());
    }

    let labels: Vec<String> = steps
        .iter()
        .map(|s| format!("{} ({})", s.id, s.phase))
        .collect();
    let defaults: Vec<bool> = steps.iter().map(|s| s.enabled).collect();

    let selected = MultiSelect::new()
        .with_prompt("Pipeline steps to run")
        .items(&labels)
        .defaults(&defaults)
        .interact()?;

    if selected.is_empty() {
        progress_step("No steps selected");
        return Ok(());
    }

    let only: Vec<String> = selected
        .into_iter()
        .map(|i| steps[i].id.clone())
        .collect();

    let no_ai = Confirm::new()
        .with_prompt("Skip AI backgrounds for mobile step?")
        .default(false)
        .interact()?;
    let all_sizes = Confirm::new()
        .with_prompt("Export all App Store sizes for mobile step?")
        .default(true)
        .interact()?;
    let yes = Confirm::new()
        .with_prompt("Run selected pipeline steps?")
        .default(true)
        .interact()?;

    progress_start("Pipeline run");
    for step_id in &only {
        progress_step(&format!("Queued: {step_id}"));
    }

    let app = session.app.clone();
    run_labeled("Pipeline run", async {
        cmd_run(
            app,
            only,
            no_ai,
            all_sizes,
            yes,
            vec![],
            vec![],
            false,
        )
        .await
    })
    .await
}

async fn menu_tools(session: &Session) -> Result<()> {
    let items = [
        "Validate mobile output",
        "Validate brand board",
        "Show capabilities",
        "Show API key resolution",
        "Back",
    ];
    let choice = Select::new()
        .with_prompt("Validate & tools")
        .items(&items)
        .interact()?;

    let app = session.app.clone();
    match choice {
        0 => {
            run_labeled("Mobile validate", async {
                cmd_validate(app, false)?;
                Ok(())
            })
            .await
        }
        1 => {
            run_labeled("Brand validate", async {
                cmd_brand_validate(app, false)?;
                Ok(())
            })
            .await
        }
        2 => {
            progress_start("Capabilities");
            cmd_capabilities(false)?;
            progress_ok("Capabilities printed");
            Ok(())
        }
        3 => {
            progress_start("Config keys");
            cmd_config_keys(false)?;
            progress_ok("Key resolution printed");
            Ok(())
        }
        _ => Ok(()),
    }
}

fn menu_change_project(session: &mut Session) -> Result<()> {
    let current = session
        .root()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| std::env::current_dir().unwrap().display().to_string());

    let path: String = Input::new()
        .with_prompt("Project directory (app repo root)")
        .default(current)
        .interact_text()?;

    let path = path.trim();
    if path.eq_ignore_ascii_case("q") {
        return Ok(());
    }

    let pb = PathBuf::from(path);
    if !pb.is_dir() {
        anyhow::bail!("not a directory: {}", pb.display());
    }

    session.app = Some(pb.canonicalize().context("resolve project path")?);
    progress_ok(&format!("Project set to {}", session.app_label()));
    Ok(())
}
