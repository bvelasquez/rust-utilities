use crate::config::ModelUseConfig;
use crate::providers::types::Provider;
use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    Day,
    Week,
    Month,
}

impl Period {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "day" | "d" => Some(Period::Day),
            "week" | "w" => Some(Period::Week),
            "month" | "m" => Some(Period::Month),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Period::Day => "day",
            Period::Week => "week",
            Period::Month => "month",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TimeSeriesPoint {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub cost_usd: f64,
    pub by_provider: Vec<(Provider, f64)>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelSpend {
    pub provider: Provider,
    pub model: String,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SummaryData {
    pub period: Period,
    pub total_usd: f64,
    pub mtd_usd: f64,
    pub by_provider: Vec<(Provider, f64)>,
    pub series: Vec<TimeSeriesPoint>,
    pub top_models: Vec<ModelSpend>,
    pub budgets: Vec<crate::budget::BudgetStatus>,
}

pub fn period_start(period: Period, now: DateTime<Utc>) -> DateTime<Utc> {
    match period {
        Period::Day => now - Duration::days(30),
        Period::Week => now - Duration::weeks(12),
        Period::Month => now - Duration::days(365),
    }
}

pub fn month_start(now: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .unwrap_or(now)
}

pub fn bucket_key(day: DateTime<Utc>, period: Period) -> DateTime<Utc> {
    match period {
        Period::Day => day.date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc(),
        Period::Week => {
            let weekday = day.weekday().num_days_from_monday();
            let d = day.date_naive() - Duration::days(weekday as i64);
            d.and_hms_opt(0, 0, 0).unwrap().and_utc()
        }
        Period::Month => Utc.with_ymd_and_hms(day.year(), day.month(), 1, 0, 0, 0)
            .single()
            .unwrap_or(day),
    }
}

pub fn build_summary(
    daily_rows: &[(Provider, DateTime<Utc>, Option<String>, f64)],
    config: &ModelUseConfig,
    period: Period,
) -> SummaryData {
    let now = Utc::now();
    let start = period_start(period, now);
    let mtd_start = month_start(now);

    let mut series_map: std::collections::BTreeMap<DateTime<Utc>, std::collections::HashMap<Provider, f64>> =
        std::collections::BTreeMap::new();
    let mut total = 0.0f64;
    let mut mtd = 0.0f64;
    let mut by_provider: std::collections::HashMap<Provider, f64> = std::collections::HashMap::new();
    let mut mtd_by_provider: std::collections::HashMap<Provider, f64> = std::collections::HashMap::new();
    let mut model_totals: std::collections::HashMap<(Provider, String), f64> =
        std::collections::HashMap::new();

    for (provider, day, model, cost) in daily_rows {
        if *day >= mtd_start {
            mtd += cost;
            *mtd_by_provider.entry(*provider).or_insert(0.0) += cost;
        }
        if *day < start {
            continue;
        }
        total += cost;
        *by_provider.entry(*provider).or_insert(0.0) += cost;
        let key = bucket_key(*day, period);
        *series_map.entry(key).or_default().entry(*provider).or_insert(0.0) += cost;
        if let Some(m) = model {
            *model_totals.entry((*provider, m.clone())).or_insert(0.0) += cost;
        }
    }

    let mut series = Vec::new();
    for (bucket_start, pmap) in &series_map {
        let cost: f64 = pmap.values().sum();
        let mut by_p: Vec<(Provider, f64)> = pmap.iter().map(|(p, c)| (*p, *c)).collect();
        by_p.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let bucket_end = match period {
            Period::Day => *bucket_start + Duration::days(1),
            Period::Week => *bucket_start + Duration::weeks(1),
            Period::Month => {
                let y = bucket_start.year();
                let m = bucket_start.month();
                if m == 12 {
                    Utc.with_ymd_and_hms(y + 1, 1, 1, 0, 0, 0)
                        .single()
                        .unwrap_or(*bucket_start + Duration::days(31))
                } else {
                    Utc.with_ymd_and_hms(y, m + 1, 1, 0, 0, 0)
                        .single()
                        .unwrap_or(*bucket_start + Duration::days(31))
                }
            }
        };
        series.push(TimeSeriesPoint {
            start: *bucket_start,
            end: bucket_end,
            cost_usd: cost,
            by_provider: by_p,
        });
    }

    let mut top_models: Vec<ModelSpend> = model_totals
        .into_iter()
        .map(|((provider, model), cost_usd)| ModelSpend {
            provider,
            model,
            cost_usd,
        })
        .collect();
    top_models.sort_by(|a, b| b.cost_usd.partial_cmp(&a.cost_usd).unwrap_or(std::cmp::Ordering::Equal));
    top_models.truncate(15);

    let mut by_provider_vec: Vec<(Provider, f64)> = by_provider.into_iter().collect();
    by_provider_vec.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mtd_vec: Vec<(Provider, f64)> = mtd_by_provider.into_iter().collect();
    let budgets = crate::budget::all_budget_statuses(config, &mtd_vec, mtd);

    SummaryData {
        period,
        total_usd: total,
        mtd_usd: mtd,
        by_provider: by_provider_vec,
        series,
        top_models,
        budgets,
    }
}
