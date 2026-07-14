use crate::config::{BudgetsConfig, ModelUseConfig};
use crate::providers::types::Provider;
use chrono::{Datelike, Utc};

#[derive(Debug, Clone, serde::Serialize)]
pub struct BudgetStatus {
    pub label: String,
    pub spent_usd: f64,
    pub budget_usd: Option<f64>,
    pub ratio: Option<f64>,
    pub over_budget: bool,
}

pub fn provider_budget(config: &ModelUseConfig, provider: Provider) -> Option<f64> {
    match provider {
        Provider::Openrouter => config.budgets.openrouter.monthly_usd,
        Provider::Anthropic => config.budgets.anthropic.monthly_usd,
        Provider::Openai => config.budgets.openai.monthly_usd,
        Provider::Cursor => config.budgets.cursor.monthly_usd,
    }
}

pub fn prorated_budget(monthly: f64, period: crate::aggregate::Period) -> f64 {
    let now = Utc::now();
    match period {
        crate::aggregate::Period::Day => monthly / days_in_month(now.year(), now.month()) as f64,
        crate::aggregate::Period::Week => monthly * 7.0 / days_in_month(now.year(), now.month()) as f64,
        crate::aggregate::Period::Month => monthly,
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    if month == 12 {
        return 31;
    }
    chrono::NaiveDate::from_ymd_opt(year, month + 1, 1)
        .and_then(|d| {
            chrono::NaiveDate::from_ymd_opt(year, month, 1).map(|start| (d - start).num_days() as u32)
        })
        .unwrap_or(30)
}

pub fn budget_status(label: &str, spent: f64, budget: Option<f64>) -> BudgetStatus {
    let ratio = budget.map(|b| if b > 0.0 { spent / b } else { 0.0 });
    let over_budget = ratio.map(|r| r >= 1.0).unwrap_or(false);
    BudgetStatus {
        label: label.to_string(),
        spent_usd: spent,
        budget_usd: budget,
        ratio,
        over_budget,
    }
}

pub fn all_budget_statuses(
    config: &ModelUseConfig,
    mtd_by_provider: &[(Provider, f64)],
    mtd_total: f64,
) -> Vec<BudgetStatus> {
    let mut out = vec![budget_status(
        "global",
        mtd_total,
        config.budgets.global_monthly_usd,
    )];
    for (provider, spent) in mtd_by_provider {
        out.push(budget_status(
            &provider.to_string(),
            *spent,
            provider_budget(config, *provider),
        ));
    }
    out
}

pub fn set_global_budget(config: &mut ModelUseConfig, monthly: f64) {
    config.budgets.global_monthly_usd = Some(monthly);
}

pub fn set_provider_budget(config: &mut ModelUseConfig, provider: Provider, monthly: f64) {
    match provider {
        Provider::Openrouter => config.budgets.openrouter.monthly_usd = Some(monthly),
        Provider::Anthropic => config.budgets.anthropic.monthly_usd = Some(monthly),
        Provider::Openai => config.budgets.openai.monthly_usd = Some(monthly),
        Provider::Cursor => config.budgets.cursor.monthly_usd = Some(monthly),
    }
}

pub fn budgets_list_data(config: &ModelUseConfig) -> BudgetsConfig {
    config.budgets.clone()
}
