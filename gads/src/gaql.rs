//! GAQL query builders — v24 field names (start_date_time / end_date_time).

pub fn customer() -> &'static str {
    "SELECT customer.id, customer.descriptive_name, customer.currency_code, customer.time_zone, customer.auto_tagging_enabled, customer.manager, customer.test_account, customer.status FROM customer"
}

pub fn account_hierarchy() -> &'static str {
    "SELECT customer_client.client_customer, customer_client.level, customer_client.manager, customer_client.descriptive_name, customer_client.currency_code, customer_client.time_zone, customer_client.id, customer_client.status FROM customer_client ORDER BY customer_client.level"
}

pub fn campaigns(status: Option<&str>, limit: usize) -> String {
    let mut query = "SELECT campaign.id, campaign.name, campaign.status, campaign.advertising_channel_type, campaign.bidding_strategy_type, campaign.campaign_budget, campaign.start_date_time, campaign.end_date_time, campaign.serving_status FROM campaign".to_string();
    if let Some(s) = status {
        query.push_str(&format!(" WHERE campaign.status = '{s}'"));
    }
    query.push_str(&format!(" ORDER BY campaign.id LIMIT {limit}"));
    query
}

pub fn campaign(campaign_id: &str) -> String {
    format!(
        "SELECT campaign.id, campaign.name, campaign.status, campaign.advertising_channel_type, campaign.advertising_channel_sub_type, campaign.bidding_strategy_type, campaign.campaign_budget, campaign.start_date_time, campaign.end_date_time, campaign.serving_status, campaign.network_settings.target_google_search, campaign.network_settings.target_search_network, campaign.network_settings.target_content_network, campaign.url_custom_parameters FROM campaign WHERE campaign.id = {campaign_id}"
    )
}

pub fn campaign_budgets(limit: usize) -> String {
    format!(
        "SELECT campaign_budget.id, campaign_budget.name, campaign_budget.amount_micros, campaign_budget.total_amount_micros, campaign_budget.status, campaign_budget.delivery_method, campaign_budget.period, campaign_budget.type FROM campaign_budget ORDER BY campaign_budget.id LIMIT {limit}"
    )
}

pub fn ad_groups(campaign: Option<&str>, status: Option<&str>, limit: usize) -> String {
    let mut query = "SELECT ad_group.id, ad_group.name, ad_group.status, ad_group.type, ad_group.campaign, ad_group.cpc_bid_micros, ad_group.cpm_bid_micros FROM ad_group".to_string();
    let mut conditions = Vec::new();
    if let Some(c) = campaign {
        conditions.push(format!("campaign.id = {c}"));
    }
    if let Some(s) = status {
        conditions.push(format!("ad_group.status = '{s}'"));
    }
    if !conditions.is_empty() {
        query.push_str(&format!(" WHERE {}", conditions.join(" AND ")));
    }
    query.push_str(&format!(" ORDER BY ad_group.id LIMIT {limit}"));
    query
}

pub fn ad_group(ad_group_id: &str) -> String {
    format!(
        "SELECT ad_group.id, ad_group.name, ad_group.status, ad_group.type, ad_group.campaign, ad_group.cpc_bid_micros, ad_group.cpm_bid_micros, ad_group.target_cpa_micros, ad_group.effective_target_cpa_micros, ad_group.effective_target_roas FROM ad_group WHERE ad_group.id = {ad_group_id}"
    )
}

pub fn ads(campaign: Option<&str>, ad_group: Option<&str>, status: Option<&str>, limit: usize) -> String {
    let mut query = "SELECT ad_group_ad.ad.id, ad_group_ad.ad.name, ad_group_ad.ad.type, ad_group_ad.ad.final_urls, ad_group_ad.status, ad_group_ad.ad_group, ad_group_ad.policy_summary.approval_status FROM ad_group_ad".to_string();
    let mut conditions = Vec::new();
    if let Some(c) = campaign {
        conditions.push(format!("campaign.id = {c}"));
    }
    if let Some(ag) = ad_group {
        conditions.push(format!("ad_group.id = {ag}"));
    }
    if let Some(s) = status {
        conditions.push(format!("ad_group_ad.status = '{s}'"));
    }
    if !conditions.is_empty() {
        query.push_str(&format!(" WHERE {}", conditions.join(" AND ")));
    }
    query.push_str(&format!(" ORDER BY ad_group_ad.ad.id LIMIT {limit}"));
    query
}

pub fn ad(ad_group_id: &str, ad_id: &str) -> String {
    format!(
        "SELECT ad_group_ad.ad.id, ad_group_ad.ad.name, ad_group_ad.ad.type, ad_group_ad.ad.final_urls, ad_group_ad.ad.display_url, ad_group_ad.ad.responsive_search_ad.headlines, ad_group_ad.ad.responsive_search_ad.descriptions, ad_group_ad.status, ad_group_ad.ad_group, ad_group_ad.policy_summary.approval_status FROM ad_group_ad WHERE ad_group.id = {ad_group_id} AND ad_group_ad.ad.id = {ad_id}"
    )
}

pub fn campaign_stats(start: &str, end: &str, campaign: Option<&str>, segments: Option<&str>, limit: usize) -> String {
    let segment_fields = segments
        .map(|s| {
            let parts: Vec<String> = s
                .split(',')
                .map(|p| format!("segments.{}", p.trim()))
                .collect();
            format!(", {}", parts.join(", "))
        })
        .unwrap_or_default();
    let mut query = format!(
        "SELECT campaign.id, campaign.name, segments.date, metrics.impressions, metrics.clicks, metrics.cost_micros, metrics.conversions, metrics.conversions_value, metrics.ctr, metrics.average_cpc, metrics.average_cpm, metrics.interactions, metrics.all_conversions{segment_fields} FROM campaign WHERE segments.date BETWEEN '{start}' AND '{end}'"
    );
    if let Some(c) = campaign {
        query.push_str(&format!(" AND campaign.id = {c}"));
    }
    query.push_str(&format!(" ORDER BY segments.date LIMIT {limit}"));
    query
}

pub fn ad_group_stats(start: &str, end: &str, campaign: Option<&str>, ad_group: Option<&str>, limit: usize) -> String {
    let mut query = format!(
        "SELECT ad_group.id, ad_group.name, campaign.id, segments.date, metrics.impressions, metrics.clicks, metrics.cost_micros, metrics.conversions, metrics.conversions_value, metrics.ctr, metrics.average_cpc FROM ad_group WHERE segments.date BETWEEN '{start}' AND '{end}'"
    );
    if let Some(c) = campaign {
        query.push_str(&format!(" AND campaign.id = {c}"));
    }
    if let Some(ag) = ad_group {
        query.push_str(&format!(" AND ad_group.id = {ag}"));
    }
    query.push_str(&format!(" ORDER BY segments.date LIMIT {limit}"));
    query
}

pub fn ad_stats(start: &str, end: &str, campaign: Option<&str>, ad_group: Option<&str>, limit: usize) -> String {
    let mut query = format!(
        "SELECT ad_group_ad.ad.id, ad_group_ad.ad.name, ad_group_ad.ad.type, ad_group.id, campaign.id, segments.date, metrics.impressions, metrics.clicks, metrics.cost_micros, metrics.conversions, metrics.ctr, metrics.average_cpc FROM ad_group_ad WHERE segments.date BETWEEN '{start}' AND '{end}'"
    );
    if let Some(c) = campaign {
        query.push_str(&format!(" AND campaign.id = {c}"));
    }
    if let Some(ag) = ad_group {
        query.push_str(&format!(" AND ad_group.id = {ag}"));
    }
    query.push_str(&format!(" ORDER BY segments.date LIMIT {limit}"));
    query
}

pub fn keyword_stats(start: &str, end: &str, campaign: Option<&str>, ad_group: Option<&str>, limit: usize) -> String {
    let mut query = format!(
        "SELECT ad_group_criterion.keyword.text, ad_group_criterion.keyword.match_type, ad_group_criterion.status, ad_group.id, campaign.id, segments.date, metrics.impressions, metrics.clicks, metrics.cost_micros, metrics.conversions, metrics.ctr, metrics.average_cpc FROM keyword_view WHERE segments.date BETWEEN '{start}' AND '{end}'"
    );
    if let Some(c) = campaign {
        query.push_str(&format!(" AND campaign.id = {c}"));
    }
    if let Some(ag) = ad_group {
        query.push_str(&format!(" AND ad_group.id = {ag}"));
    }
    query.push_str(&format!(" ORDER BY metrics.impressions DESC LIMIT {limit}"));
    query
}

pub fn keywords(campaign: Option<&str>, ad_group: Option<&str>, status: Option<&str>, limit: usize) -> String {
    let mut query = "SELECT ad_group_criterion.criterion_id, ad_group_criterion.keyword.text, ad_group_criterion.keyword.match_type, ad_group_criterion.status, ad_group_criterion.quality_info.quality_score, ad_group_criterion.cpc_bid_micros, ad_group.id, campaign.id FROM ad_group_criterion WHERE ad_group_criterion.type = 'KEYWORD'".to_string();
    if let Some(c) = campaign {
        query.push_str(&format!(" AND campaign.id = {c}"));
    }
    if let Some(ag) = ad_group {
        query.push_str(&format!(" AND ad_group.id = {ag}"));
    }
    if let Some(s) = status {
        query.push_str(&format!(" AND ad_group_criterion.status = '{s}'"));
    }
    query.push_str(&format!(" ORDER BY ad_group_criterion.criterion_id LIMIT {limit}"));
    query
}

pub fn audiences(limit: usize) -> String {
    format!(
        "SELECT campaign_audience_view.resource_name, campaign.id, campaign.name, metrics.impressions, metrics.clicks FROM campaign_audience_view LIMIT {limit}"
    )
}

pub fn user_lists(limit: usize) -> String {
    format!(
        "SELECT user_list.id, user_list.name, user_list.description, user_list.membership_status, user_list.size_for_display, user_list.size_for_search, user_list.type FROM user_list ORDER BY user_list.id LIMIT {limit}"
    )
}

pub fn negative_keywords(limit: usize) -> String {
    format!(
        "SELECT shared_set.id, shared_set.name, shared_set.type, shared_set.status, shared_set.member_count FROM shared_set WHERE shared_set.type = 'NEGATIVE_KEYWORDS' LIMIT {limit}"
    )
}

pub fn assets(asset_type: Option<&str>, limit: usize) -> String {
    let mut query =
        "SELECT asset.id, asset.name, asset.type, asset.final_urls, asset.resource_name FROM asset"
            .to_string();
    if let Some(t) = asset_type {
        query.push_str(&format!(" WHERE asset.type = '{t}'"));
    }
    query.push_str(&format!(" ORDER BY asset.id LIMIT {limit}"));
    query
}

pub fn extensions(campaign: Option<&str>, limit: usize) -> String {
    let mut query = "SELECT campaign_asset.asset, campaign_asset.field_type, campaign_asset.status, campaign.id, campaign.name FROM campaign_asset".to_string();
    if let Some(c) = campaign {
        query.push_str(&format!(" WHERE campaign.id = {c}"));
    }
    query.push_str(&format!(" LIMIT {limit}"));
    query
}

pub fn conversion_actions(limit: usize) -> String {
    format!(
        "SELECT conversion_action.id, conversion_action.name, conversion_action.type, conversion_action.status, conversion_action.category, conversion_action.counting_type, conversion_action.click_through_lookback_window_days, conversion_action.view_through_lookback_window_days, conversion_action.tag_snippets FROM conversion_action ORDER BY conversion_action.id LIMIT {limit}"
    )
}

pub fn billing() -> &'static str {
    "SELECT billing_setup.id, billing_setup.status, billing_setup.payments_account, billing_setup.start_date_time, billing_setup.end_date_time FROM billing_setup"
}

pub fn change_status(limit: usize) -> String {
    format!(
        "SELECT change_status.resource_name, change_status.resource_type, change_status.resource_status, change_status.last_change_date_time FROM change_status ORDER BY change_status.last_change_date_time DESC LIMIT {limit}"
    )
}

pub fn performance_summary(start: &str, end: &str) -> String {
    format!(
        "SELECT metrics.impressions, metrics.clicks, metrics.cost_micros, metrics.conversions, metrics.conversions_value, metrics.ctr, metrics.average_cpc FROM customer WHERE segments.date BETWEEN '{start}' AND '{end}'"
    )
}

pub fn conversion_tags() -> &'static str {
    "SELECT conversion_action.id, conversion_action.name, conversion_action.status, conversion_action.type, conversion_action.tag_snippets FROM conversion_action WHERE conversion_action.status != 'REMOVED'"
}
