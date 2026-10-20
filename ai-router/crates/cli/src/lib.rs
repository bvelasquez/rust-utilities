//! Agent-friendly CLI. Every command accepts `--json` and does not require a TTY,
//! except `tui`, `serve --tui`, and `config edit`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use airouter_config::{
    default_config_path, file_mode, init_at, is_loopback_bind, json_schema, load_file, resolve_for,
    save_file, static_doctor, validate_for, ProviderConfig, ProviderType, Report, ResolvedProvider,
};
use airouter_core::{now_unix, Modality, ModelConfig, Pricing};
use airouter_gateway::{parse_since, ServeOpts};
use airouter_providers::{http_client, test_provider};
use airouter_registry::{
    cheaperinference_models_from_list, deepinfra_list_url, deepinfra_models_from_list,
    find_catalog, search_catalog,
};
use airouter_store::{GroupBy, Store};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(
    name = "airouter",
    version,
    about = "Personal OpenAI-compatible router for your own provider keys"
)]
struct Cli {
    /// Config file. Defaults to $AIROUTER_CONFIG or ~/.config/airouter/config.json.
    #[arg(long, global = true, env = "AIROUTER_CONFIG")]
    config: Option<PathBuf>,
    /// Machine-readable output.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a config, a local API token, and provider templates.
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Serve the OpenAI-compatible API.
    Serve {
        /// Also open the dashboard in this terminal.
        #[arg(long)]
        tui: bool,
    },
    /// Attach the dashboard to an already running server.
    Tui {
        /// Admin base URL. Defaults to the config's admin_listen.
        #[arg(long)]
        admin: Option<String>,
    },
    Provider {
        #[command(subcommand)]
        cmd: ProviderCmd,
    },
    Models {
        #[command(subcommand)]
        cmd: ModelsCmd,
    },
    Alias {
        #[command(subcommand)]
        cmd: AliasCmd,
    },
    /// Spend and token totals from the local usage database.
    Usage {
        #[arg(long, default_value = "30d")]
        since: String,
        /// model, provider, day, client, or route
        #[arg(long, default_value = "model")]
        by: String,
    },
    /// Re-score logged auto routes against the current profiles.
    Route {
        #[command(subcommand)]
        cmd: RouteCmd,
    },
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Print the JSON Schema for the config file.
    Schema,
    /// Check config permissions, secrets, and upstream reachability.
    Doctor {
        #[arg(long)]
        offline: bool,
    },
}

#[derive(Subcommand)]
enum RouteCmd {
    /// Compare recorded auto-route cost with an all-safe-model baseline.
    Replay {
        #[arg(long, default_value = "30d")]
        since: String,
    },
}

#[derive(Subcommand)]
enum ProviderCmd {
    List,
    Add {
        name: String,
        #[arg(long, value_name = "TYPE")]
        r#type: String,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        base_url: Option<String>,
    },
    Remove {
        name: String,
    },
    /// Make a minimal authenticated call.
    Test {
        name: String,
        #[arg(long)]
        model: Option<String>,
    },
}

#[derive(Subcommand)]
enum ModelsCmd {
    List,
    Search {
        query: String,
    },
    Add {
        id: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        upstream: Option<String>,
        #[arg(long)]
        modality: Option<String>,
        #[arg(long)]
        context: Option<u64>,
        #[arg(long)]
        input_per_mtok: Option<f64>,
        #[arg(long)]
        output_per_mtok: Option<f64>,
        #[arg(long)]
        per_image: Option<f64>,
        #[arg(long)]
        per_1k_chars: Option<f64>,
        #[arg(long)]
        per_minute: Option<f64>,
        #[arg(long)]
        embedding_per_mtok: Option<f64>,
    },
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    Price {
        id: String,
    },
}

#[derive(Subcommand)]
enum AliasCmd {
    List,
    Set { name: String, model: String },
    Remove { name: String },
}

#[derive(Subcommand)]
enum ConfigCmd {
    Path,
    Validate,
    Schema,
    Edit,
}

struct Printed {
    text: String,
    json: Value,
    code: u8,
}

pub fn run() -> u8 {
    run_from(std::env::args_os())
}

pub fn run_from<I, T>(args: I) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();
            return err.exit_code() as u8;
        }
    };
    let json_mode = cli.json;
    let path = cli.config.clone().unwrap_or_else(default_config_path);
    let result = dispatch(cli, &path);
    match result {
        Ok(printed) => {
            if !printed.text.is_empty() || json_mode {
                if json_mode {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&printed.json).unwrap_or_else(|_| "{}".into())
                    );
                } else {
                    println!("{}", printed.text);
                }
            }
            printed.code
        }
        Err(err) => {
            if json_mode {
                println!("{}", json!({"ok": false, "error": err}));
            } else {
                eprintln!("error: {err}");
            }
            1
        }
    }
}

fn dispatch(cli: Cli, path: &Path) -> Result<Printed, String> {
    match cli.command {
        Command::Serve { tui } => {
            serve(path, tui)?;
            Ok(Printed {
                text: String::new(),
                json: json!({"ok": true}),
                code: 0,
            })
        }
        Command::Tui { admin } => {
            let url = admin.unwrap_or_else(|| admin_url(path));
            require_tty("tui")?;
            airouter_tui::run(&url)?;
            Ok(Printed {
                text: String::new(),
                json: json!({"ok": true}),
                code: 0,
            })
        }
        Command::Init { force } => Ok(cmd_init(path, force)),
        Command::Schema => Ok(cmd_schema()),
        Command::Doctor { offline } => cmd_doctor(path, offline),
        Command::Config { cmd } => match cmd {
            ConfigCmd::Path => Ok(Printed {
                text: path.display().to_string(),
                json: json!({"path": path.display().to_string()}),
                code: 0,
            }),
            ConfigCmd::Validate => Ok(report_print(&load_and_validate(path)?)),
            ConfigCmd::Schema => Ok(cmd_schema()),
            ConfigCmd::Edit => {
                require_tty("config edit")?;
                edit_file(path)?;
                Ok(Printed {
                    text: format!("edited {}", path.display()),
                    json: json!({"path": path}),
                    code: 0,
                })
            }
        },
        Command::Provider { cmd } => provider_cmd(path, cmd),
        Command::Models { cmd } => models_cmd(path, cmd),
        Command::Alias { cmd } => alias_cmd(path, cmd),
        Command::Usage { since, by } => usage_cmd(path, &since, &by),
        Command::Route { cmd } => match cmd {
            RouteCmd::Replay { since } => route_replay(path, &since),
        },
    }
}

fn serve(path: &Path, tui: bool) -> Result<(), String> {
    if !path.exists() {
        return Err(format!(
            "config not found at {} (run `airouter init`)",
            path.display()
        ));
    }
    if tui {
        require_tty("serve --tui")?;
        let url = admin_url(path);
        init_tracing(Some(&log_path(path)));
        let flag = Arc::new(AtomicBool::new(false));
        let flag2 = flag.clone();
        let path_buf = path.to_path_buf();
        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
            rt.block_on(airouter_gateway::serve(ServeOpts {
                config_path: path_buf,
                shutdown: Some(flag2),
            }))
            .map_err(|e| e.to_string())
        });
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .map_err(|e| e.to_string())?;
        for _ in 0..50 {
            if client
                .get(format!("{url}/health"))
                .send()
                .ok()
                .is_some_and(|r| r.status().is_success())
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let tui_result = airouter_tui::run(&url);
        flag.store(true, Ordering::Relaxed);
        let server_result = handle
            .join()
            .unwrap_or_else(|_| Err("server thread panicked".into()));
        tui_result?;
        server_result
    } else {
        init_tracing(None);
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        rt.block_on(airouter_gateway::serve(ServeOpts {
            config_path: path.to_path_buf(),
            shutdown: None,
        }))
        .map_err(|e| e.to_string())
    }
}

fn init_tracing(log_file: Option<&Path>) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if let Some(path) = log_file {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .with_writer(move || file.try_clone().expect("log file"))
                .try_init();
            return;
        }
    }
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn log_path(config: &Path) -> PathBuf {
    let file = load_file(config).unwrap_or_default();
    airouter_config::default_state_dir(&file.server).join("airouter.log")
}

fn admin_url(config: &Path) -> String {
    let listen = load_file(config)
        .map(|c| c.server.admin_listen)
        .unwrap_or_else(|_| "127.0.0.1:8788".into());
    if listen.starts_with("http://") || listen.starts_with("https://") {
        listen
    } else {
        format!("http://{listen}")
    }
}

fn require_tty(cmd: &str) -> Result<(), String> {
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal() {
        Ok(())
    } else {
        Err(format!("{cmd} needs a terminal"))
    }
}

fn cmd_init(path: &Path, force: bool) -> Printed {
    match init_at(path, force) {
        Ok(result) => Printed {
            text: format!(
                "created {}\ntoken file {}\nlocal API token (shown once in this form; it is also in the token file):\n{}\nlisten {}",
                result.config_path.display(),
                result.token_path.display(),
                result.token,
                result.listen
            ),
            json: json!({
                "config": result.config_path,
                "token_path": result.token_path,
                "token": result.token,
                "listen": result.listen,
            }),
            code: 0,
        },
        Err(err) => Printed {
            text: err.to_string(),
            json: json!({"ok": false, "error": err.to_string()}),
            code: 1,
        },
    }
}

fn cmd_schema() -> Printed {
    let schema = json_schema();
    Printed {
        text: serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".into()),
        json: schema,
        code: 0,
    }
}

fn load_and_validate(path: &Path) -> Result<Report, String> {
    let file = load_file(path).map_err(|e| e.to_string())?;
    validate_for(path, &file).map_err(|e| e.to_string())
}

fn report_print(report: &Report) -> Printed {
    let mut text = String::new();
    if report.errors.is_empty() {
        text.push_str("errors: none\n");
    } else {
        text.push_str("errors:\n");
        for err in &report.errors {
            text.push_str(&format!("- {err}\n"));
        }
    }
    if report.warnings.is_empty() {
        text.push_str("warnings: none\n");
    } else {
        text.push_str("warnings:\n");
        for warning in &report.warnings {
            text.push_str(&format!("- {warning}\n"));
        }
    }
    Printed {
        text,
        json: json!({"ok": report.ok(), "errors": report.errors, "warnings": report.warnings}),
        code: if report.ok() { 0 } else { 1 },
    }
}

fn cmd_doctor(path: &Path, offline: bool) -> Result<Printed, String> {
    let mut report = static_doctor(path).map_err(|e| e.to_string())?;
    let file = load_file(path).map_err(|e| e.to_string())?;
    let resolved = resolve_for(path, &file).map_err(|e| e.to_string())?;
    let mode = file_mode(path)
        .map(|m| format!("{m:03o}"))
        .unwrap_or_else(|| "n/a".into());
    let mut checks = Vec::new();
    if !offline {
        let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
        checks = rt.block_on(probe_all(&resolved.providers));
        for check in &checks {
            if !check.reachable {
                report
                    .warnings
                    .push(format!("{} unreachable: {}", check.name, check.detail));
            }
        }
    }
    let mut printed = report_print(&report);
    let header = format!(
        "config: {}\nmode: {mode}\nlisten: {} ({})\nadmin: {} ({})\napi keys resolved: {}\nmodels enabled: {}\n",
        path.display(),
        file.server.listen,
        if is_loopback_bind(&file.server.listen) { "loopback" } else { "public" },
        file.server.admin_listen,
        if is_loopback_bind(&file.server.admin_listen) { "loopback" } else { "public" },
        resolved.keys.len(),
        file.models.iter().filter(|m| m.enabled).count(),
    );
    printed.text = format!("{header}{}", printed.text);
    if !checks.is_empty() {
        printed.text.push_str("reachability:\n");
        for check in &checks {
            printed.text.push_str(&format!(
                "- {} {} {}\n",
                check.name,
                if check.reachable { "ok" } else { "down" },
                check.detail
            ));
        }
    }
    if let Value::Object(map) = &mut printed.json {
        map.insert("config".into(), json!(path));
        map.insert("mode".into(), json!(mode));
        map.insert("checks".into(), json!(checks));
    }
    Ok(printed)
}

#[derive(serde::Serialize)]
struct Reachability {
    name: String,
    reachable: bool,
    detail: String,
}

async fn probe_all(providers: &BTreeMap<String, ResolvedProvider>) -> Vec<Reachability> {
    let client = http_client();
    let mut out = Vec::new();
    for (name, provider) in providers {
        if !provider.enabled {
            continue;
        }
        let url = provider.base_url.clone();
        let result = client
            .get(&url)
            .timeout(Duration::from_secs(3))
            .send()
            .await;
        let (reachable, detail) = match result {
            Ok(resp) => (true, format!("HTTP {}", resp.status())),
            Err(err) => (false, err.to_string()),
        };
        out.push(Reachability {
            name: name.clone(),
            reachable,
            detail,
        });
    }
    out
}

fn provider_cmd(path: &Path, cmd: ProviderCmd) -> Result<Printed, String> {
    match cmd {
        ProviderCmd::List => {
            let file = load_file(path).map_err(|e| e.to_string())?;
            let resolved = resolve_for(path, &file).map_err(|e| e.to_string())?;
            let rows: Vec<Value> = resolved
                .providers
                .values()
                .map(|p| {
                    json!({
                        "name": p.name,
                        "type": p.provider_type.as_str(),
                        "base_url": p.base_url,
                        "enabled": p.enabled,
                        "key": p.key_status,
                    })
                })
                .collect();
            let mut text = String::new();
            for row in &rows {
                text.push_str(&format!(
                    "{}  {}  enabled={}  key={}  {}\n",
                    row["name"].as_str().unwrap_or(""),
                    row["type"].as_str().unwrap_or(""),
                    row["enabled"],
                    row["key"].as_str().unwrap_or(""),
                    row["base_url"].as_str().unwrap_or("")
                ));
            }
            if text.is_empty() {
                text = "no providers\n".into();
            }
            Ok(Printed {
                text,
                json: json!({"providers": rows}),
                code: 0,
            })
        }
        ProviderCmd::Add {
            name,
            r#type,
            api_key,
            base_url,
        } => {
            let kind = parse_provider_type(&r#type)?;
            let base_url = base_url
                .or_else(|| airouter_config::known_compat_base_url(&name).map(str::to_string));
            if kind == ProviderType::OpenaiCompat && base_url.as_deref().unwrap_or("").is_empty() {
                return Err("openai_compat providers need --base-url".into());
            }
            let mut file = load_file(path).map_err(|e| e.to_string())?;
            file.providers.insert(
                name.clone(),
                ProviderConfig {
                    provider_type: kind,
                    api_key,
                    base_url,
                    headers: BTreeMap::new(),
                    timeout_secs: None,
                    enabled: true,
                },
            );
            save_file(path, &file).map_err(|e| e.to_string())?;
            Ok(Printed {
                text: format!("added provider {name}"),
                json: json!({"name": name}),
                code: 0,
            })
        }
        ProviderCmd::Remove { name } => {
            let mut file = load_file(path).map_err(|e| e.to_string())?;
            if file.providers.remove(&name).is_none() {
                return Err(format!("provider not found: {name}"));
            }
            let dangling: Vec<_> = file
                .models
                .iter()
                .filter(|m| m.provider == name)
                .map(|m| m.id.clone())
                .collect();
            save_file(path, &file).map_err(|e| e.to_string())?;
            let extra = if dangling.is_empty() {
                String::new()
            } else {
                format!("\nmodels still reference it: {}", dangling.join(", "))
            };
            Ok(Printed {
                text: format!("removed provider {name}{extra}"),
                json: json!({"name": name, "dangling_models": dangling}),
                code: 0,
            })
        }
        ProviderCmd::Test { name, model } => {
            let file = load_file(path).map_err(|e| e.to_string())?;
            let resolved = resolve_for(path, &file).map_err(|e| e.to_string())?;
            let provider = resolved
                .providers
                .get(&name)
                .ok_or_else(|| format!("provider not found: {name}"))?;
            let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
            let http = http_client();
            match rt.block_on(test_provider(provider, &http, model.as_deref())) {
                Ok(msg) => Ok(Printed {
                    text: msg.clone(),
                    json: json!({"ok": true, "message": msg}),
                    code: 0,
                }),
                Err(err) => Ok(Printed {
                    text: err.to_string(),
                    json: json!({"ok": false, "error": err.to_string()}),
                    code: 1,
                }),
            }
        }
    }
}

fn models_cmd(path: &Path, cmd: ModelsCmd) -> Result<Printed, String> {
    match cmd {
        ModelsCmd::List => {
            let file = load_file(path).map_err(|e| e.to_string())?;
            let rows: Vec<Value> = file.models.iter().map(model_row).collect();
            let mut text = String::new();
            for row in &rows {
                text.push_str(&format!(
                    "{}  {}  enabled={}  {}\n",
                    row["id"].as_str().unwrap_or(""),
                    row["modality"].as_str().unwrap_or(""),
                    row["enabled"],
                    row["provider"].as_str().unwrap_or("")
                ));
            }
            if text.is_empty() {
                text = "no models configured; try `airouter models search` and `airouter models add <id>`\n".into();
            }
            Ok(Printed {
                text,
                json: json!({"models": rows}),
                code: 0,
            })
        }
        ModelsCmd::Search { query } => {
            let mut hits = search_catalog(&query);
            let mut note = String::new();
            let q = query.to_ascii_lowercase();
            let by_query = |m: &ModelConfig| {
                m.id.to_ascii_lowercase().contains(&q)
                    || m.upstream.to_ascii_lowercase().contains(&q)
            };
            match deepinfra_catalog() {
                Ok(models) => hits.extend(models.into_iter().filter(&by_query)),
                Err(err) => note = format!("deepinfra prices unavailable: {err}\n"),
            }
            if cheaperinference_configured(path) {
                match cheaperinference_catalog(path) {
                    Ok(models) => hits.extend(models.into_iter().filter(&by_query)),
                    Err(err) => {
                        note.push_str(&format!("cheaperinference prices unavailable: {err}\n"))
                    }
                }
            }
            let rows: Vec<Value> = hits.iter().map(model_row).collect();
            let mut text = note;
            for model in &hits {
                text.push_str(&format!(
                    "{}  {}  {}\n",
                    model.id,
                    model.modality,
                    price_brief(model)
                ));
            }
            if hits.is_empty() {
                text.push_str("no catalog matches\n");
            }
            Ok(Printed {
                text,
                json: json!({"models": rows}),
                code: 0,
            })
        }
        ModelsCmd::Add {
            id,
            provider,
            upstream,
            modality,
            context,
            input_per_mtok,
            output_per_mtok,
            per_image,
            per_1k_chars,
            per_minute,
            embedding_per_mtok,
        } => {
            let mut file = load_file(path).map_err(|e| e.to_string())?;
            if let Some(existing) = file.models.iter_mut().find(|m| m.id == id) {
                existing.enabled = true;
                save_file(path, &file).map_err(|e| e.to_string())?;
                return Ok(Printed {
                    text: format!("enabled existing model {id}"),
                    json: json!({"id": id, "enabled": true}),
                    code: 0,
                });
            }
            let mut model = if let Some(mut catalog) = find_catalog(&id) {
                catalog.enabled = true;
                catalog
            } else if let Some(mut catalog) = find_deepinfra(&id) {
                catalog.enabled = true;
                catalog
            } else if let Some(mut catalog) = find_cheaperinference(path, &id) {
                catalog.enabled = true;
                catalog
            } else {
                let provider = provider
                    .clone()
                    .ok_or("model is not in the catalog; pass --provider and --upstream")?;
                let upstream = upstream
                    .clone()
                    .ok_or("model is not in the catalog; pass --provider and --upstream")?;
                ModelConfig {
                    id: id.clone(),
                    provider,
                    upstream,
                    modality: parse_modality(modality.as_deref().unwrap_or("chat"))?,
                    enabled: true,
                    context,
                    capabilities: Vec::new(),
                    pricing: Pricing::default(),
                    quality: None,
                    task_quality: std::collections::BTreeMap::new(),
                    latency_ms_prior: None,
                }
            };
            if let Some(provider) = provider {
                model.provider = provider;
            }
            if let Some(upstream) = upstream {
                model.upstream = upstream;
            }
            if let Some(modality) = modality {
                model.modality = parse_modality(&modality)?;
            }
            if let Some(context) = context {
                model.context = Some(context);
            }
            if let Some(v) = input_per_mtok {
                model.pricing.input_per_mtok = Some(v);
            }
            if let Some(v) = output_per_mtok {
                model.pricing.output_per_mtok = Some(v);
            }
            if let Some(v) = per_image {
                model.pricing.per_image = Some(v);
            }
            if let Some(v) = per_1k_chars {
                model.pricing.per_1k_chars = Some(v);
            }
            if let Some(v) = per_minute {
                model.pricing.per_minute = Some(v);
            }
            if let Some(v) = embedding_per_mtok {
                model.pricing.embedding_per_mtok = Some(v);
            }
            let id = model.id.clone();
            file.models.push(model);
            save_file(path, &file).map_err(|e| e.to_string())?;
            Ok(Printed {
                text: format!("added {id}"),
                json: json!({"id": id, "enabled": true}),
                code: 0,
            })
        }
        ModelsCmd::Enable { id } => set_enabled(path, &id, true),
        ModelsCmd::Disable { id } => set_enabled(path, &id, false),
        ModelsCmd::Price { id } => {
            let file = load_file(path).unwrap_or_default();
            let model = file
                .models
                .iter()
                .find(|m| m.id == id)
                .cloned()
                .or_else(|| find_catalog(&id))
                .or_else(|| find_deepinfra(&id))
                .or_else(|| find_cheaperinference(path, &id))
                .ok_or_else(|| format!("unknown model {id}"))?;
            let text = format!(
                "{} modality {}\ninput/mtok {:?}  output/mtok {:?}  cache read {:?}  cache write {:?}\nembedding {:?}  per image {:?}  per 1k chars {:?}  per minute {:?}",
                model.id,
                model.modality,
                model.pricing.input_per_mtok,
                model.pricing.output_per_mtok,
                model.pricing.cache_read_per_mtok,
                model.pricing.cache_write_per_mtok,
                model.pricing.embedding_per_mtok,
                model.pricing.per_image,
                model.pricing.per_1k_chars,
                model.pricing.per_minute
            );
            Ok(Printed {
                text,
                json: json!({"id": model.id, "pricing": model.pricing, "modality": model.modality.as_str()}),
                code: 0,
            })
        }
    }
}

fn price_brief(model: &ModelConfig) -> String {
    let pricing = &model.pricing;
    if let (Some(input), Some(output)) = (pricing.input_per_mtok, pricing.output_per_mtok) {
        let cache = pricing
            .cache_read_per_mtok
            .map(|v| format!(", cache read ${v}"))
            .unwrap_or_default();
        format!("${input}/${output} per mtok{cache}")
    } else if let Some(price) = pricing.embedding_per_mtok {
        format!("${price} per mtok embedded")
    } else if let Some(price) = pricing.per_image {
        format!("${price} per image")
    } else if let Some(price) = pricing.per_1k_chars {
        format!("${price} per 1k chars")
    } else if let Some(price) = pricing.per_minute {
        format!("${price} per minute")
    } else {
        "price unknown".into()
    }
}

fn find_deepinfra(id: &str) -> Option<ModelConfig> {
    let upstream = id.strip_prefix("deepinfra/")?;
    deepinfra_catalog()
        .ok()?
        .into_iter()
        .find(|model| model.upstream == upstream)
}

fn find_cheaperinference(path: &Path, id: &str) -> Option<ModelConfig> {
    let upstream = id.strip_prefix("cheaperinference/")?;
    cheaperinference_catalog(path)
        .ok()?
        .into_iter()
        .find(|model| model.upstream == upstream)
}

fn cheaperinference_configured(path: &Path) -> bool {
    load_file(path)
        .map(|file| file.providers.contains_key("cheaperinference"))
        .unwrap_or(false)
}

/// Fetch Cheaper Inference's authenticated catalog. Prices already reflect the
/// key's standard scope, so they are stored as returned.
fn cheaperinference_catalog(path: &Path) -> Result<Vec<ModelConfig>, String> {
    let cache = cheaperinference_cache_path();
    if let Some(body) = read_fresh_cache(&cache) {
        let models = cheaperinference_models_from_list(&body);
        if !models.is_empty() {
            return Ok(models);
        }
    }
    let file = load_file(path).map_err(|e| e.to_string())?;
    let resolved = resolve_for(path, &file).map_err(|e| e.to_string())?;
    let provider = resolved.providers.get("cheaperinference").ok_or_else(|| {
        "no 'cheaperinference' provider configured; run `airouter provider add cheaperinference \
         --type openai_compat --api-key env:CHEAPERINFERENCE_API_KEY`"
            .to_string()
    })?;
    let key = provider
        .api_key
        .as_deref()
        .ok_or_else(|| format!("cheaperinference has no API key ({})", provider.key_status))?;
    let url = format!("{}/models", provider.base_url.trim_end_matches('/'));
    let text = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("airouter/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?
        .get(&url)
        .bearer_auth(key)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())?;
    let body: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache, &text);
    Ok(cheaperinference_models_from_list(&body))
}

fn cheaperinference_cache_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(dir).join("airouter/cheaperinference-models.json");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/airouter/cheaperinference-models.json")
}

fn deepinfra_catalog() -> Result<Vec<ModelConfig>, String> {
    let cache = deepinfra_cache_path();
    if let Some(body) = read_fresh_cache(&cache) {
        let models = deepinfra_models_from_list(&body);
        if !models.is_empty() {
            return Ok(models);
        }
    }
    let text = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("airouter/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?
        .get(deepinfra_list_url())
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())?;
    let body: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache, &text);
    Ok(deepinfra_models_from_list(&body))
}

fn read_fresh_cache(path: &Path) -> Option<Value> {
    let meta = std::fs::metadata(path).ok()?;
    let age = meta.modified().ok()?.elapsed().ok()?;
    if age > Duration::from_secs(24 * 60 * 60) {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn deepinfra_cache_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(dir).join("airouter/deepinfra-models.json");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/airouter/deepinfra-models.json")
}

fn set_enabled(path: &Path, id: &str, enabled: bool) -> Result<Printed, String> {
    let mut file = load_file(path).map_err(|e| e.to_string())?;
    let Some(model) = file.models.iter_mut().find(|m| m.id == id) else {
        return Err(format!("model not configured: {id}"));
    };
    model.enabled = enabled;
    save_file(path, &file).map_err(|e| e.to_string())?;
    Ok(Printed {
        text: format!("{id} enabled={enabled}"),
        json: json!({"id": id, "enabled": enabled}),
        code: 0,
    })
}

fn model_row(m: &ModelConfig) -> Value {
    json!({
        "id": m.id,
        "provider": m.provider,
        "upstream": m.upstream,
        "modality": m.modality.as_str(),
        "enabled": m.enabled,
        "context": m.context,
        "capabilities": m.capabilities,
        "pricing": m.pricing,
    })
}

fn alias_cmd(path: &Path, cmd: AliasCmd) -> Result<Printed, String> {
    match cmd {
        AliasCmd::List => {
            let file = load_file(path).map_err(|e| e.to_string())?;
            let mut text = String::new();
            for (name, target) in &file.aliases {
                text.push_str(&format!("{name} -> {target}\n"));
            }
            if text.is_empty() {
                text = "no aliases\n".into();
            }
            Ok(Printed {
                text,
                json: json!(file.aliases),
                code: 0,
            })
        }
        AliasCmd::Set { name, model } => {
            if name == "auto" || name.starts_with("auto/") {
                return Err(
                    "the auto prefix is reserved for routing profiles; configure routing.profiles"
                        .into(),
                );
            }
            let mut file = load_file(path).map_err(|e| e.to_string())?;
            file.aliases.insert(name.clone(), model.clone());
            save_file(path, &file).map_err(|e| e.to_string())?;
            Ok(Printed {
                text: format!("{name} -> {model}"),
                json: json!({"name": name, "model": model}),
                code: 0,
            })
        }
        AliasCmd::Remove { name } => {
            let mut file = load_file(path).map_err(|e| e.to_string())?;
            if file.aliases.remove(&name).is_none() {
                return Err(format!("alias not found: {name}"));
            }
            save_file(path, &file).map_err(|e| e.to_string())?;
            Ok(Printed {
                text: format!("removed alias {name}"),
                json: json!({"name": name}),
                code: 0,
            })
        }
    }
}

fn usage_cmd(path: &Path, since: &str, by: &str) -> Result<Printed, String> {
    let file = load_file(path).map_err(|e| e.to_string())?;
    let db = airouter_config::default_state_dir(&file.server).join("usage.db");
    if !db.exists() {
        return Ok(Printed {
            text: "no usage recorded yet\n".into(),
            json: json!({"rows": [], "summary": {}}),
            code: 0,
        });
    }
    let group = GroupBy::parse(by).ok_or("by must be model, provider, day, client, or route")?;
    let now = now_unix();
    let since_ts = parse_since(since, now)?;
    let store = Store::open(&db).map_err(|e| e.to_string())?;
    let summary = store.summary_since(since_ts).map_err(|e| e.to_string())?;
    let rows = store
        .aggregate(since_ts, group)
        .map_err(|e| e.to_string())?;
    let mut text = format!(
        "since {since} ({since_ts})  requests {}  errors {}  in {}  out {}  cost ${:.4}\n",
        summary.requests,
        summary.errors,
        summary.input_tokens,
        summary.output_tokens,
        summary.cost_usd
    );
    for row in &rows {
        text.push_str(&format!(
            "{}  n={}  err={}  in={}  out={}  ${:.4}\n",
            row.key, row.requests, row.errors, row.input_tokens, row.output_tokens, row.cost_usd
        ));
    }
    Ok(Printed {
        text,
        json: json!({
            "since": since_ts,
            "summary": {
                "requests": summary.requests,
                "errors": summary.errors,
                "input_tokens": summary.input_tokens,
                "output_tokens": summary.output_tokens,
                "cost_usd": summary.cost_usd,
            },
            "rows": rows,
        }),
        code: 0,
    })
}

fn route_replay(path: &Path, since: &str) -> Result<Printed, String> {
    let file = load_file(path).map_err(|e| e.to_string())?;
    let db = airouter_config::default_state_dir(&file.server).join("usage.db");
    if !db.exists() {
        return Ok(Printed {
            text: "no usage recorded yet\n".into(),
            json: json!({"auto_requests": 0}),
            code: 0,
        });
    }
    let now = now_unix();
    let since_ts = parse_since(since, now)?;
    let store = Store::open(&db).map_err(|e| e.to_string())?;
    let rows = store.scan_since(since_ts).map_err(|e| e.to_string())?;
    let registry = airouter_registry::Registry::from_parts(
        file.models.clone(),
        file.aliases.clone(),
        file.routing.clone(),
    );
    let replay_rows: Vec<airouter_registry::ReplayRow> = rows
        .iter()
        .map(|row| airouter_registry::ReplayRow {
            requested_model: row.requested_model.clone(),
            upstream_model: row.upstream_model.clone(),
            status: row.status,
            input_tokens: row.input_tokens.unwrap_or(0),
            output_tokens: row.output_tokens.unwrap_or(0),
            cost_usd: row.cost_usd,
            route_signals: row.route_signals.clone(),
            route_profile: row.route_profile.clone(),
            feedback: row.feedback,
        })
        .collect();
    let report = airouter_registry::replay(&registry, &replay_rows);
    let savings = report.all_strong_cost - report.recorded_cost;
    let text = format!(
        "auto requests {}  compared {}  recorded ${:.4}  all-strong ${:.4}  savings ${:.4}\nstrong share {:.1}%  replay agreement {:.1}%  feedback +{} -{}\n",
        report.auto_requests,
        report.compared,
        report.recorded_cost,
        report.all_strong_cost,
        savings,
        report.strong_share * 100.0,
        report.agreement * 100.0,
        report.feedback_up,
        report.feedback_down
    );
    Ok(Printed {
        text,
        json: json!({
            "since": since_ts,
            "auto_requests": report.auto_requests,
            "compared": report.compared,
            "recorded_cost": report.recorded_cost,
            "all_strong_cost": report.all_strong_cost,
            "savings": savings,
            "strong_share": report.strong_share,
            "agreement": report.agreement,
            "feedback_up": report.feedback_up,
            "feedback_down": report.feedback_down,
        }),
        code: 0,
    })
}

fn edit_file(path: &Path) -> Result<(), String> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nano".into());
    let status = std::process::Command::new(&editor)
        .arg(path)
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{editor} exited with {status}"))
    }
}

fn parse_provider_type(value: &str) -> Result<ProviderType, String> {
    match value {
        "openai" => Ok(ProviderType::Openai),
        "openai_compat" | "openai-compat" | "compat" => Ok(ProviderType::OpenaiCompat),
        "anthropic" => Ok(ProviderType::Anthropic),
        "gemini" => Ok(ProviderType::Gemini),
        other => Err(format!(
            "unknown provider type '{other}' (openai, openai_compat, anthropic, gemini)"
        )),
    }
}

fn parse_modality(value: &str) -> Result<Modality, String> {
    match value {
        "chat" => Ok(Modality::Chat),
        "embedding" | "embeddings" => Ok(Modality::Embedding),
        "image" => Ok(Modality::Image),
        "audio_tts" | "tts" => Ok(Modality::AudioTts),
        "audio_stt" | "stt" | "transcription" => Ok(Modality::AudioStt),
        "video" => Ok(Modality::Video),
        other => Err(format!("unknown modality '{other}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use airouter_config::file_mode;

    #[test]
    fn init_add_model_alias_and_doctor() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.json");
        let cfg_s = cfg.to_str().unwrap();
        assert_eq!(run_from(["airouter", "--config", cfg_s, "init"]), 0);
        assert_eq!(file_mode(&cfg), Some(0o600));
        assert_eq!(
            run_from([
                "airouter",
                "--config",
                cfg_s,
                "models",
                "add",
                "openai/gpt-4o-mini"
            ]),
            0
        );
        assert_eq!(
            run_from([
                "airouter",
                "--config",
                cfg_s,
                "alias",
                "set",
                "fast",
                "openai/gpt-4o-mini"
            ]),
            0
        );
        let file = load_file(&cfg).unwrap();
        assert_eq!(file.models[0].upstream, "gpt-4o-mini");
        assert_eq!(
            file.aliases.get("fast").map(String::as_str),
            Some("openai/gpt-4o-mini")
        );
        assert_eq!(
            run_from(["airouter", "--config", cfg_s, "doctor", "--offline"]),
            0
        );
        assert_eq!(run_from(["airouter", "--config", cfg_s, "schema"]), 0);
        let missing = dir.path().join("missing.json");
        assert_eq!(
            run_from([
                "airouter",
                "--config",
                missing.to_str().unwrap(),
                "doctor",
                "--offline"
            ]),
            1
        );
    }
}
