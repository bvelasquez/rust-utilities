mod api;
mod auth;
mod capabilities;
mod config;
mod gaql;
mod interactive;
mod ops;
mod output;
mod safety;
mod util;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use config::{default_credentials_path, find_project_config, ProjectConfig};
use output::{print_raw, Envelope};
use std::path::PathBuf;
use std::time::Duration;
use util::normalize_customer_id;

#[derive(Parser, Debug)]
#[command(
    name = "gads",
    about = "Agent-first Google Ads CLI — read, query, mutate campaigns",
    version,
    after_help = "Agents: run `gads capabilities --json` and `gads env schema --json`.\nHumans: run `gads interactive` for menu mode.\nUse --json on subcommands for structured envelope output."
)]
struct Cli {
    #[arg(long, global = true, help = "Structured JSON envelope output")]
    json: bool,

    #[arg(long, global = true, help = "Compact JSON (no envelope, raw API shape)")]
    compact: bool,

    #[arg(long, global = true, help = "Path to credentials JSON")]
    credentials: Option<PathBuf>,

    #[arg(long, global = true, help = "Default customer ID (overrides gads.toml)")]
    customer: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// OAuth setup and credential status
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
    /// List accessible customer accounts
    Customers,
    /// Get customer account details
    Customer {
        customer_id: Option<String>,
    },
    /// List manager account hierarchy
    #[command(name = "account-hierarchy")]
    AccountHierarchy {
        customer_id: Option<String>,
    },
    /// List campaigns
    Campaigns {
        customer_id: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// Get a campaign
    Campaign {
        #[command(subcommand)]
        command: CampaignCommands,
    },
    /// List campaign budgets
    #[command(name = "campaign-budgets")]
    CampaignBudgets {
        customer_id: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// List ad groups
    #[command(name = "ad-groups")]
    AdGroups {
        customer_id: Option<String>,
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// Get an ad group
    #[command(name = "ad-group")]
    AdGroup {
        #[command(subcommand)]
        command: AdGroupCommands,
    },
    /// List ads
    Ads {
        customer_id: Option<String>,
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long, name = "ad-group")]
        ad_group: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// Get an ad
    Ad {
        #[command(subcommand)]
        command: AdCommands,
    },
    /// Campaign performance stats
    #[command(name = "campaign-stats")]
    CampaignStats {
        customer_id: Option<String>,
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long)]
        segments: Option<String>,
        #[arg(long, default_value = "1000")]
        limit: usize,
    },
    /// Ad group performance stats
    #[command(name = "ad-group-stats")]
    AdGroupStats {
        customer_id: Option<String>,
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long, name = "ad-group")]
        ad_group: Option<String>,
        #[arg(long, default_value = "1000")]
        limit: usize,
    },
    /// Ad performance stats
    #[command(name = "ad-stats")]
    AdStats {
        customer_id: Option<String>,
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long, name = "ad-group")]
        ad_group: Option<String>,
        #[arg(long, default_value = "1000")]
        limit: usize,
    },
    /// Keyword performance stats
    #[command(name = "keyword-stats")]
    KeywordStats {
        customer_id: Option<String>,
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long, name = "ad-group")]
        ad_group: Option<String>,
        #[arg(long, default_value = "1000")]
        limit: usize,
    },
    /// List keywords
    Keywords {
        customer_id: Option<String>,
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long, name = "ad-group")]
        ad_group: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// List audience segments
    Audiences {
        customer_id: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// List remarketing lists
    #[command(name = "user-lists")]
    UserLists {
        customer_id: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// List negative keyword lists
    #[command(name = "negative-keywords")]
    NegativeKeywords {
        customer_id: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// List assets
    Assets {
        customer_id: Option<String>,
        #[arg(long, name = "type")]
        asset_type: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// List campaign extensions
    Extensions {
        customer_id: Option<String>,
        #[arg(long)]
        campaign: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// List conversion actions
    #[command(name = "conversion-actions")]
    ConversionActions {
        customer_id: Option<String>,
        #[arg(long, default_value = "100")]
        limit: usize,
    },
    /// Run raw GAQL
    Query {
        customer_id: Option<String>,
        gaql: String,
    },
    /// Billing setup
    Billing {
        customer_id: Option<String>,
    },
    /// Recent change history
    #[command(name = "change-status")]
    ChangeStatus {
        customer_id: Option<String>,
        #[arg(long, default_value = "50")]
        limit: usize,
    },
    /// Account performance rollup (shortcut)
    Summary {
        customer_id: Option<String>,
        #[arg(long)]
        start: Option<String>,
        #[arg(long)]
        end: Option<String>,
        #[arg(long, default_value = "30")]
        days: i64,
    },
    /// Conversion tags filtered by domain (shortcut)
    #[command(name = "conversion-tags")]
    ConversionTags {
        customer_id: Option<String>,
        #[arg(long)]
        domain: Option<String>,
    },
    /// Budget operations
    Budget {
        #[command(subcommand)]
        command: BudgetCommands,
    },
    /// Keyword operations
    Keyword {
        #[command(subcommand)]
        command: KeywordCommands,
    },
    /// Mutate a single resource type (campaigns, adGroups, etc.)
    Mutate {
        customer_id: Option<String>,
        resource: String,
        #[arg(long, help = "JSON file with { \"operations\": [...] }")]
        file: PathBuf,
        #[arg(long)]
        partial_failure: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    /// GoogleAdsService.mutate — multi-resource atomic batch
    #[command(name = "mutate-batch")]
    MutateBatch {
        customer_id: Option<String>,
        #[arg(long, help = "JSON file with { \"mutateOperations\": [...] }")]
        file: PathBuf,
        #[arg(long)]
        partial_failure: bool,
        #[arg(long)]
        validate_only: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    /// Machine-readable command catalog
    Capabilities,
    /// Environment variable schema
    Env {
        #[command(subcommand)]
        command: EnvCommands,
    },
    /// Menu-driven session for humans
    #[command(visible_alias = "i")]
    Interactive {
        #[arg(long)]
        customer: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum AuthCommands {
    /// OAuth browser login — saves credentials.json
    Login {
        #[arg(long)]
        developer_token: Option<String>,
        #[arg(long)]
        client_id: Option<String>,
        #[arg(long)]
        client_secret: Option<String>,
        #[arg(long, default_value = "0")]
        port: u16,
        #[arg(long)]
        no_browser: bool,
    },
    /// Show credential status (no secrets)
    Status,
}

#[derive(Subcommand, Debug)]
enum EnvCommands {
    Schema,
}

#[derive(Subcommand, Debug)]
enum CampaignCommands {
    /// Get a campaign
    Get {
        customer_id: Option<String>,
        campaign_id: String,
    },
    /// Set campaign status (ENABLED, PAUSED, REMOVED)
    #[command(name = "set-status")]
    SetStatus {
        customer_id: Option<String>,
        campaign_id: String,
        status: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    /// Create a Search campaign (requires existing budget resource name)
    #[command(name = "create-search")]
    CreateSearch {
        customer_id: Option<String>,
        #[arg(long)]
        name: String,
        #[arg(long, help = "Full budget resource name, e.g. customers/123/campaignBudgets/456")]
        budget: String,
        #[arg(long, default_value = "PAUSED")]
        status: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum BudgetCommands {
    /// Create a daily campaign budget
    Create {
        customer_id: Option<String>,
        #[arg(long)]
        name: String,
        #[arg(long, help = "Daily budget in account currency micros (e.g. 5000000 = $5)")]
        amount_micros: i64,
        #[arg(long, default_value = "STANDARD")]
        delivery_method: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum AdGroupCommands {
    /// Get an ad group
    Get {
        customer_id: Option<String>,
        ad_group_id: String,
    },
    /// Create a search ad group
    Create {
        customer_id: Option<String>,
        #[arg(long, help = "Full campaign resource name")]
        campaign: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "PAUSED")]
        status: String,
        #[arg(long)]
        cpc_bid_micros: Option<i64>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    /// Set ad group status (ENABLED, PAUSED, REMOVED)
    #[command(name = "set-status")]
    SetStatus {
        customer_id: Option<String>,
        ad_group_id: String,
        status: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum AdCommands {
    /// Get an ad
    Get {
        customer_id: Option<String>,
        ad_group_id: String,
        ad_id: String,
    },
    /// Create a responsive search ad
    #[command(name = "create-rsa")]
    CreateRsa {
        customer_id: Option<String>,
        #[arg(long, help = "Full ad group resource name")]
        ad_group: String,
        #[arg(long)]
        url: String,
        #[arg(long, value_delimiter = '|')]
        headlines: Vec<String>,
        #[arg(long, value_delimiter = '|')]
        descriptions: Vec<String>,
        #[arg(long, default_value = "PAUSED")]
        status: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    /// Set ad status (ENABLED, PAUSED, REMOVED)
    #[command(name = "set-status")]
    SetStatus {
        customer_id: Option<String>,
        ad_group_id: String,
        ad_id: String,
        status: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum KeywordCommands {
    /// Add a keyword to an ad group
    Add {
        customer_id: Option<String>,
        #[arg(long, help = "Full ad group resource name")]
        ad_group: String,
        #[arg(long)]
        text: String,
        #[arg(long, default_value = "PHRASE")]
        match_type: String,
        #[arg(long, default_value = "ENABLED")]
        status: String,
        #[arg(long)]
        cpc_bid_micros: Option<i64>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
}

struct RunContext {
    json: bool,
    compact: bool,
    credentials: Option<PathBuf>,
    project: Option<ProjectConfig>,
    default_customer: Option<String>,
}

impl RunContext {
    fn resolve_customer(&self, explicit: Option<&str>) -> Result<String> {
        if let Some(id) = explicit {
            let id = if let Some(proj) = &self.project {
                proj.resolve_customer(id)
            } else {
                id
            };
            return Ok(normalize_customer_id(id));
        }
        if let Some(id) = &self.default_customer {
            return Ok(normalize_customer_id(id));
        }
        if let Some(proj) = &self.project {
            if let Some(id) = &proj.default_customer_id {
                return Ok(normalize_customer_id(id));
            }
        }
        bail!("customer ID required — pass as argument or set --customer / gads.toml")
    }

    async fn client(&self) -> Result<api::AdsClient> {
        let creds = auth::load_credentials(self.credentials.as_deref()).await?;
        Ok(api::AdsClient::new(creds))
    }

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
}

async fn run_resource_mutate(
    ctx: &RunContext,
    command: &str,
    customer_id: Option<String>,
    resource: &str,
    body: serde_json::Value,
    dry_run: bool,
    yes: bool,
    action: &str,
) -> Result<()> {
    let cid = ctx.resolve_customer(customer_id.as_deref())?;
    safety::require_mutation_approval(yes, dry_run, action)?;
    if dry_run {
        return ctx.emit(
            command,
            serde_json::json!({ "dryRun": true, "body": body }),
        );
    }
    let client = ctx.client().await?;
    let ops_list = body["operations"].clone();
    ctx.emit(
        command,
        ops::mutate_resource(&client, &cid, resource, ops_list, false).await?,
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let project = find_project_config(&std::env::current_dir()?)
        .and_then(|p| ProjectConfig::load(&p).ok());

    let ctx = RunContext {
        json: cli.json,
        compact: cli.compact,
        credentials: cli.credentials,
        project,
        default_customer: cli.customer,
    };

    match cli.command {
        Commands::Auth { command } => run_auth(&ctx, command).await,
        Commands::Customers => {
            let client = ctx.client().await?;
            let data = client.list_accessible_customers().await?;
            ctx.emit("customers", data)
        }
        Commands::Customer { customer_id } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit("customer", rc.customer().await?)
        }
        Commands::AccountHierarchy { customer_id } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit("account-hierarchy", rc.account_hierarchy().await?)
        }
        Commands::Campaigns {
            customer_id,
            status,
            limit,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "campaigns",
                rc.campaigns(status.as_deref(), limit).await?,
            )
        }
        Commands::Campaign { command } => match command {
            CampaignCommands::Get {
                customer_id,
                campaign_id,
            } => {
                let cid = ctx.resolve_customer(customer_id.as_deref())?;
                let client = ctx.client().await?;
                let rc = ops::ReadContext {
                    client: &client,
                    customer_id: &cid,
                };
                ctx.emit("campaign get", rc.campaign(&campaign_id).await?)
            }
            CampaignCommands::SetStatus {
                customer_id,
                campaign_id,
                status,
                dry_run,
                yes,
            } => {
                let cid = ctx.resolve_customer(customer_id.as_deref())?;
                let body = ops::campaign_status_update(&cid, &campaign_id, &status);
                run_resource_mutate(
                    &ctx,
                    "campaign set-status",
                    Some(cid),
                    "campaigns",
                    body,
                    dry_run,
                    yes,
                    &format!("set campaign {campaign_id} status={status}"),
                )
                .await
            }
            CampaignCommands::CreateSearch {
                customer_id,
                name,
                budget,
                status,
                dry_run,
                yes,
            } => {
                let cid = ctx.resolve_customer(customer_id.as_deref())?;
                let body = ops::search_campaign_create(&cid, &name, &budget, &status);
                run_resource_mutate(
                    &ctx,
                    "campaign create-search",
                    Some(cid),
                    "campaigns",
                    body,
                    dry_run,
                    yes,
                    &format!("create search campaign {name}"),
                )
                .await
            }
        }
        Commands::CampaignBudgets { customer_id, limit } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit("campaign-budgets", rc.campaign_budgets(limit).await?)
        }
        Commands::AdGroups {
            customer_id,
            campaign,
            status,
            limit,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "ad-groups",
                rc.ad_groups(campaign.as_deref(), status.as_deref(), limit)
                    .await?,
            )
        }
        Commands::AdGroup { command } => match command {
            AdGroupCommands::Get {
                customer_id,
                ad_group_id,
            } => {
                let cid = ctx.resolve_customer(customer_id.as_deref())?;
                let client = ctx.client().await?;
                let rc = ops::ReadContext {
                    client: &client,
                    customer_id: &cid,
                };
                ctx.emit("ad-group get", rc.ad_group(&ad_group_id).await?)
            }
            AdGroupCommands::Create {
                customer_id,
                campaign,
                name,
                status,
                cpc_bid_micros,
                dry_run,
                yes,
            } => {
                let cid = ctx.resolve_customer(customer_id.as_deref())?;
                let body = ops::ad_group_create(&cid, &campaign, &name, &status, cpc_bid_micros);
                run_resource_mutate(
                    &ctx,
                    "ad-group create",
                    Some(cid),
                    "adGroups",
                    body,
                    dry_run,
                    yes,
                    &format!("create ad group {name}"),
                )
                .await
            }
            AdGroupCommands::SetStatus {
                customer_id,
                ad_group_id,
                status,
                dry_run,
                yes,
            } => {
                let cid = ctx.resolve_customer(customer_id.as_deref())?;
                let body = ops::ad_group_status_update(&cid, &ad_group_id, &status);
                run_resource_mutate(
                    &ctx,
                    "ad-group set-status",
                    Some(cid),
                    "adGroups",
                    body,
                    dry_run,
                    yes,
                    &format!("set ad group {ad_group_id} status={status}"),
                )
                .await
            }
        }
        Commands::Ads {
            customer_id,
            campaign,
            ad_group,
            status,
            limit,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "ads",
                rc.ads(
                    campaign.as_deref(),
                    ad_group.as_deref(),
                    status.as_deref(),
                    limit,
                )
                .await?,
            )
        }
        Commands::Ad { command } => match command {
            AdCommands::Get {
                customer_id,
                ad_group_id,
                ad_id,
            } => {
                let cid = ctx.resolve_customer(customer_id.as_deref())?;
                let client = ctx.client().await?;
                let rc = ops::ReadContext {
                    client: &client,
                    customer_id: &cid,
                };
                ctx.emit("ad get", rc.ad(&ad_group_id, &ad_id).await?)
            }
            AdCommands::CreateRsa {
                customer_id,
                ad_group,
                url,
                headlines,
                descriptions,
                status,
                dry_run,
                yes,
            } => {
                if headlines.len() < 3 {
                    bail!("RSA requires at least 3 headlines (--headlines a|b|c)");
                }
                if descriptions.len() < 2 {
                    bail!("RSA requires at least 2 descriptions (--descriptions a|b)");
                }
                let cid = ctx.resolve_customer(customer_id.as_deref())?;
                let body = ops::responsive_search_ad_create(
                    &cid,
                    &ad_group,
                    &url,
                    &headlines,
                    &descriptions,
                    &status,
                );
                run_resource_mutate(
                    &ctx,
                    "ad create-rsa",
                    Some(cid),
                    "adGroupAds",
                    body,
                    dry_run,
                    yes,
                    "create responsive search ad",
                )
                .await
            }
            AdCommands::SetStatus {
                customer_id,
                ad_group_id,
                ad_id,
                status,
                dry_run,
                yes,
            } => {
                let cid = ctx.resolve_customer(customer_id.as_deref())?;
                let body = ops::ad_group_ad_status_update(&cid, &ad_group_id, &ad_id, &status);
                run_resource_mutate(
                    &ctx,
                    "ad set-status",
                    Some(cid),
                    "adGroupAds",
                    body,
                    dry_run,
                    yes,
                    &format!("set ad {ad_id} status={status}"),
                )
                .await
            }
        }
        Commands::CampaignStats {
            customer_id,
            start,
            end,
            campaign,
            segments,
            limit,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "campaign-stats",
                rc.campaign_stats(
                    &start,
                    &end,
                    campaign.as_deref(),
                    segments.as_deref(),
                    limit,
                )
                .await?,
            )
        }
        Commands::AdGroupStats {
            customer_id,
            start,
            end,
            campaign,
            ad_group,
            limit,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "ad-group-stats",
                rc.ad_group_stats(
                    &start,
                    &end,
                    campaign.as_deref(),
                    ad_group.as_deref(),
                    limit,
                )
                .await?,
            )
        }
        Commands::AdStats {
            customer_id,
            start,
            end,
            campaign,
            ad_group,
            limit,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "ad-stats",
                rc.ad_stats(
                    &start,
                    &end,
                    campaign.as_deref(),
                    ad_group.as_deref(),
                    limit,
                )
                .await?,
            )
        }
        Commands::KeywordStats {
            customer_id,
            start,
            end,
            campaign,
            ad_group,
            limit,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "keyword-stats",
                rc.keyword_stats(
                    &start,
                    &end,
                    campaign.as_deref(),
                    ad_group.as_deref(),
                    limit,
                )
                .await?,
            )
        }
        Commands::Keywords {
            customer_id,
            campaign,
            ad_group,
            status,
            limit,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "keywords",
                rc.keywords(
                    campaign.as_deref(),
                    ad_group.as_deref(),
                    status.as_deref(),
                    limit,
                )
                .await?,
            )
        }
        Commands::Audiences { customer_id, limit } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit("audiences", rc.audiences(limit).await?)
        }
        Commands::UserLists { customer_id, limit } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit("user-lists", rc.user_lists(limit).await?)
        }
        Commands::NegativeKeywords { customer_id, limit } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit("negative-keywords", rc.negative_keywords(limit).await?)
        }
        Commands::Assets {
            customer_id,
            asset_type,
            limit,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "assets",
                rc.assets(asset_type.as_deref(), limit).await?,
            )
        }
        Commands::Extensions {
            customer_id,
            campaign,
            limit,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "extensions",
                rc.extensions(campaign.as_deref(), limit).await?,
            )
        }
        Commands::ConversionActions { customer_id, limit } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit("conversion-actions", rc.conversion_actions(limit).await?)
        }
        Commands::Query { customer_id, gaql } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit("query", rc.raw_query(&gaql).await?)
        }
        Commands::Billing { customer_id } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit("billing", rc.billing().await?)
        }
        Commands::ChangeStatus { customer_id, limit } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit("change-status", rc.change_status(limit).await?)
        }
        Commands::Summary {
            customer_id,
            start,
            end,
            days,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let (start, end) = resolve_date_range(start, end, days)?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "summary",
                rc.performance_summary(&start, &end).await?,
            )
        }
        Commands::ConversionTags { customer_id, domain } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let client = ctx.client().await?;
            let rc = ops::ReadContext {
                client: &client,
                customer_id: &cid,
            };
            ctx.emit(
                "conversion-tags",
                rc.conversion_tags(domain.as_deref()).await?,
            )
        }
        Commands::Mutate {
            customer_id,
            resource,
            file,
            partial_failure,
            dry_run,
            yes,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let body = ops::load_mutate_body(&file)?;
            let ops_list = body
                .get("operations")
                .cloned()
                .context("mutate JSON must contain \"operations\" array")?;
            safety::require_mutation_approval(yes, dry_run, &format!("mutate {resource}"))?;
            if dry_run {
                return ctx.emit(
                    "mutate",
                    serde_json::json!({ "dryRun": true, "resource": resource, "operations": ops_list }),
                );
            }
            let client = ctx.client().await?;
            ctx.emit(
                "mutate",
                ops::mutate_resource(&client, &cid, &resource, ops_list, partial_failure).await?,
            )
        }
        Commands::MutateBatch {
            customer_id,
            file,
            partial_failure,
            validate_only,
            dry_run,
            yes,
        } => {
            let cid = ctx.resolve_customer(customer_id.as_deref())?;
            let body = ops::load_mutate_body(&file)?;
            let mutate_ops = body
                .get("mutateOperations")
                .cloned()
                .context("mutate-batch JSON must contain \"mutateOperations\" array")?;
            safety::require_mutation_approval(yes, dry_run, "mutate-batch")?;
            if dry_run {
                return ctx.emit(
                    "mutate-batch",
                    serde_json::json!({ "dryRun": true, "mutateOperations": mutate_ops }),
                );
            }
            let client = ctx.client().await?;
            ctx.emit(
                "mutate-batch",
                ops::mutate_google_ads(
                    &client,
                    &cid,
                    mutate_ops,
                    partial_failure,
                    validate_only,
                )
                .await?,
            )
        }
        Commands::Budget { command } => match command {
            BudgetCommands::Create {
                customer_id,
                name,
                amount_micros,
                delivery_method,
                dry_run,
                yes,
            } => {
                let cid = ctx.resolve_customer(customer_id.as_deref())?;
                let body = ops::campaign_budget_create(&cid, &name, amount_micros, &delivery_method);
                run_resource_mutate(
                    &ctx,
                    "budget create",
                    Some(cid),
                    "campaignBudgets",
                    body,
                    dry_run,
                    yes,
                    &format!("create budget {name}"),
                )
                .await
            }
        }
        Commands::Keyword { command } => match command {
            KeywordCommands::Add {
                customer_id,
                ad_group,
                text,
                match_type,
                status,
                cpc_bid_micros,
                dry_run,
                yes,
            } => {
                let cid = ctx.resolve_customer(customer_id.as_deref())?;
                let body = ops::keyword_create(
                    &cid,
                    &ad_group,
                    &text,
                    &match_type,
                    &status,
                    cpc_bid_micros,
                );
                run_resource_mutate(
                    &ctx,
                    "keyword add",
                    Some(cid),
                    "adGroupCriteria",
                    body,
                    dry_run,
                    yes,
                    &format!("add keyword {text}"),
                )
                .await
            }
        }
        Commands::Capabilities => {
            if ctx.json || ctx.compact {
                ctx.emit("capabilities", capabilities::capabilities_json())
            } else {
                ctx.emit("capabilities", capabilities::capabilities_json())
            }
        }
        Commands::Env { command } => match command {
            EnvCommands::Schema => ctx.emit("env schema", capabilities::env_schema_json()),
        },
        Commands::Interactive { customer } => {
            interactive::run(customer, ctx.credentials).await
        }
    }
}

async fn run_auth(ctx: &RunContext, command: AuthCommands) -> Result<()> {
    match command {
        AuthCommands::Login {
            developer_token,
            client_id,
            client_secret,
            port,
            no_browser,
        } => {
            let creds_path = ctx
                .credentials
                .clone()
                .unwrap_or_else(default_credentials_path);

            let mut existing: auth::Credentials = if creds_path.is_file() {
                serde_json::from_str(&std::fs::read_to_string(&creds_path)?)
                    .unwrap_or(auth::Credentials {
                        access_token: String::new(),
                        developer_token: String::new(),
                        login_customer_id: None,
                        client_id: None,
                        client_secret: None,
                        refresh_token: None,
                        token_expiry: None,
                    })
            } else {
                auth::Credentials {
                    access_token: String::new(),
                    developer_token: String::new(),
                    login_customer_id: None,
                    client_id: None,
                    client_secret: None,
                    refresh_token: None,
                    token_expiry: None,
                }
            };

            let dev = developer_token
                .or_else(|| {
                    std::env::var("GOOGLE_ADS_DEVELOPER_TOKEN")
                        .ok()
                        .or_else(|| std::env::var("GADS_DEVELOPER_TOKEN").ok())
                })
                .or_else(|| {
                    if existing.developer_token.is_empty() {
                        None
                    } else {
                        Some(existing.developer_token.clone())
                    }
                })
                .context("missing --developer-token")?;

            let cid = client_id
                .or_else(|| existing.client_id.clone())
                .context("missing --client-id")?;
            let secret = client_secret
                .or_else(|| existing.client_secret.clone())
                .context("missing --client-secret")?;

            let bind_port = if port == 0 { 0 } else { port };

            let actual_port = if bind_port == 0 {
                let probe = tiny_http::Server::http("127.0.0.1:0")
                    .map_err(|e| anyhow::anyhow!("bind oauth server: {e}"))?;
                let p = probe
                    .server_addr()
                    .to_ip()
                    .map(|a| a.port())
                    .unwrap_or(8787);
                drop(probe);
                p
            } else {
                bind_port
            };

            let redirect_uri = format!("http://127.0.0.1:{actual_port}");
            let url = auth::auth_url(&cid, &redirect_uri);

            if no_browser {
                eprintln!("Open this URL in your browser:\n{url}");
            } else {
                eprintln!("Opening browser for authorization…");
                eprintln!("If it doesn't open, visit:\n{url}");
                let _ = auth::open_browser(&url);
            }

            let (code, _) =
                auth::run_local_oauth_server(actual_port, Duration::from_secs(120)).await?;
            let (access, refresh, expiry) =
                auth::exchange_auth_code(&cid, &secret, &code, &redirect_uri).await?;

            existing.developer_token = dev;
            existing.client_id = Some(cid);
            existing.client_secret = Some(secret);
            existing.access_token = access;
            existing.refresh_token = Some(refresh);
            existing.token_expiry = Some(expiry);

            auth::save_credentials(&creds_path, &existing)?;
            eprintln!(
                "{} {}",
                "Credentials saved to".green().bold(),
                creds_path.display()
            );
            Ok(())
        }
        AuthCommands::Status => {
            let creds = auth::load_credentials(ctx.credentials.as_deref()).await?;
            ctx.emit("auth status", auth::auth_status(&creds))
        }
    }
}

fn resolve_date_range(
    start: Option<String>,
    end: Option<String>,
    days: i64,
) -> Result<(String, String)> {
    use chrono::{Duration, Utc};
    if let (Some(s), Some(e)) = (start, end) {
        return Ok((s, e));
    }
    let end_date = Utc::now().date_naive();
    let start_date = end_date - Duration::days(days);
    Ok((start_date.to_string(), end_date.to_string()))
}
