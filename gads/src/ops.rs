use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::api::AdsClient;
use crate::gaql;
use crate::util::normalize_customer_id;

pub struct ReadContext<'a> {
    pub client: &'a AdsClient,
    pub customer_id: &'a str,
}

impl<'a> ReadContext<'a> {
    pub fn id(&self) -> String {
        normalize_customer_id(self.customer_id)
    }

    async fn query(&self, gaql: &str) -> Result<Value> {
        self.client.search_stream(&self.id(), gaql).await
    }

    pub async fn customer(&self) -> Result<Value> {
        self.query(gaql::customer()).await
    }

    pub async fn account_hierarchy(&self) -> Result<Value> {
        self.query(gaql::account_hierarchy()).await
    }

    pub async fn campaigns(&self, status: Option<&str>, limit: usize) -> Result<Value> {
        self.query(&gaql::campaigns(status, limit)).await
    }

    pub async fn campaign(&self, campaign_id: &str) -> Result<Value> {
        self.query(&gaql::campaign(campaign_id)).await
    }

    pub async fn campaign_budgets(&self, limit: usize) -> Result<Value> {
        self.query(&gaql::campaign_budgets(limit)).await
    }

    pub async fn ad_groups(
        &self,
        campaign: Option<&str>,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Value> {
        self.query(&gaql::ad_groups(campaign, status, limit))
            .await
    }

    pub async fn ad_group(&self, ad_group_id: &str) -> Result<Value> {
        self.query(&gaql::ad_group(ad_group_id)).await
    }

    pub async fn ads(
        &self,
        campaign: Option<&str>,
        ad_group: Option<&str>,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Value> {
        self.query(&gaql::ads(campaign, ad_group, status, limit))
            .await
    }

    pub async fn ad(&self, ad_group_id: &str, ad_id: &str) -> Result<Value> {
        self.query(&gaql::ad(ad_group_id, ad_id)).await
    }

    pub async fn campaign_stats(
        &self,
        start: &str,
        end: &str,
        campaign: Option<&str>,
        segments: Option<&str>,
        limit: usize,
    ) -> Result<Value> {
        self.query(&gaql::campaign_stats(start, end, campaign, segments, limit))
            .await
    }

    pub async fn ad_group_stats(
        &self,
        start: &str,
        end: &str,
        campaign: Option<&str>,
        ad_group: Option<&str>,
        limit: usize,
    ) -> Result<Value> {
        self.query(&gaql::ad_group_stats(start, end, campaign, ad_group, limit))
            .await
    }

    pub async fn ad_stats(
        &self,
        start: &str,
        end: &str,
        campaign: Option<&str>,
        ad_group: Option<&str>,
        limit: usize,
    ) -> Result<Value> {
        self.query(&gaql::ad_stats(start, end, campaign, ad_group, limit))
            .await
    }

    pub async fn keyword_stats(
        &self,
        start: &str,
        end: &str,
        campaign: Option<&str>,
        ad_group: Option<&str>,
        limit: usize,
    ) -> Result<Value> {
        self.query(&gaql::keyword_stats(start, end, campaign, ad_group, limit))
            .await
    }

    pub async fn keywords(
        &self,
        campaign: Option<&str>,
        ad_group: Option<&str>,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Value> {
        self.query(&gaql::keywords(campaign, ad_group, status, limit))
            .await
    }

    pub async fn audiences(&self, limit: usize) -> Result<Value> {
        self.query(&gaql::audiences(limit)).await
    }

    pub async fn user_lists(&self, limit: usize) -> Result<Value> {
        self.query(&gaql::user_lists(limit)).await
    }

    pub async fn negative_keywords(&self, limit: usize) -> Result<Value> {
        self.query(&gaql::negative_keywords(limit)).await
    }

    pub async fn assets(&self, asset_type: Option<&str>, limit: usize) -> Result<Value> {
        self.query(&gaql::assets(asset_type, limit)).await
    }

    pub async fn extensions(&self, campaign: Option<&str>, limit: usize) -> Result<Value> {
        self.query(&gaql::extensions(campaign, limit)).await
    }

    pub async fn conversion_actions(&self, limit: usize) -> Result<Value> {
        self.query(&gaql::conversion_actions(limit)).await
    }

    pub async fn billing(&self) -> Result<Value> {
        self.query(gaql::billing()).await
    }

    pub async fn change_status(&self, limit: usize) -> Result<Value> {
        self.query(&gaql::change_status(limit)).await
    }

    pub async fn raw_query(&self, gaql: &str) -> Result<Value> {
        self.query(gaql).await
    }

    pub async fn performance_summary(&self, start: &str, end: &str) -> Result<Value> {
        self.query(&gaql::performance_summary(start, end)).await
    }

    pub async fn conversion_tags(&self, domain: Option<&str>) -> Result<Value> {
        let data = self.query(gaql::conversion_tags()).await?;
        if let Some(domain) = domain {
            Ok(filter_conversion_tags_by_domain(data, domain))
        } else {
            Ok(data)
        }
    }
}

fn filter_conversion_tags_by_domain(data: Value, domain: &str) -> Value {
    let domain_lower = domain.to_lowercase();
    let needle = domain_lower.trim_start_matches("www.");

    fn row_matches_domain(row: &Value, needle: &str) -> bool {
        let snippets = row
            .pointer("/conversionAction/tagSnippets")
            .or_else(|| row.pointer("/conversion_action/tag_snippets"));
        if let Some(arr) = snippets.and_then(|v| v.as_array()) {
            for snip in arr {
                let hay = serde_json::to_string(snip).unwrap_or_default().to_lowercase();
                if hay.contains(needle) {
                    return true;
                }
            }
        }
        let name = row
            .pointer("/conversionAction/name")
            .or_else(|| row.pointer("/conversion_action/name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        name.contains(needle)
    }

    if let Some(arr) = data.as_array() {
        let mut filtered = Vec::new();
        for chunk in arr {
            if let Some(results) = chunk.pointer("/results").and_then(|v| v.as_array()) {
                let kept: Vec<_> = results
                    .iter()
                    .filter(|r| row_matches_domain(r, needle))
                    .cloned()
                    .collect();
                if !kept.is_empty() {
                    let mut copy = chunk.clone();
                    if let Some(obj) = copy.as_object_mut() {
                        obj.insert("results".into(), Value::Array(kept));
                    }
                    filtered.push(copy);
                }
            }
        }
        return Value::Array(filtered);
    }
    data
}

pub async fn mutate_resource(
    client: &AdsClient,
    customer_id: &str,
    resource: &str,
    operations: Value,
    partial_failure: bool,
) -> Result<Value> {
    let cid = normalize_customer_id(customer_id);
    let body = if partial_failure {
        json!({ "operations": operations, "partialFailure": true })
    } else {
        json!({ "operations": operations })
    };
    client.mutate_resource(&cid, resource, &body).await
}

pub async fn mutate_google_ads(
    client: &AdsClient,
    customer_id: &str,
    mutate_operations: Value,
    partial_failure: bool,
    validate_only: bool,
) -> Result<Value> {
    let cid = normalize_customer_id(customer_id);
    let mut body = json!({ "mutateOperations": mutate_operations });
    if partial_failure {
        body["partialFailure"] = json!(true);
    }
    if validate_only {
        body["validateOnly"] = json!(true);
    }
    client.mutate_google_ads(&cid, &body).await
}

pub fn campaign_status_update(
    customer_id: &str,
    campaign_id: &str,
    status: &str,
) -> Value {
    let cid = normalize_customer_id(customer_id);
    json!({
        "operations": [{
            "updateMask": "status",
            "update": {
                "resourceName": format!("customers/{cid}/campaigns/{campaign_id}"),
                "status": status
            }
        }]
    })
}

pub fn campaign_budget_create(
    customer_id: &str,
    name: &str,
    amount_micros: i64,
    delivery_method: &str,
) -> Value {
    let cid = normalize_customer_id(customer_id);
    json!({
        "operations": [{
            "create": {
                "resourceName": format!("customers/{cid}/campaignBudgets/-1"),
                "name": name,
                "amountMicros": amount_micros.to_string(),
                "deliveryMethod": delivery_method,
                "explicitlyShared": false
            }
        }]
    })
}

pub fn search_campaign_create(
    customer_id: &str,
    name: &str,
    budget_resource: &str,
    status: &str,
) -> Value {
    let cid = normalize_customer_id(customer_id);
    json!({
        "operations": [{
            "create": {
                "resourceName": format!("customers/{cid}/campaigns/-2"),
                "name": name,
                "status": status,
                "advertisingChannelType": "SEARCH",
                "campaignBudget": budget_resource,
                "networkSettings": {
                    "targetGoogleSearch": true,
                    "targetSearchNetwork": true,
                    "targetContentNetwork": false,
                    "targetPartnerSearchNetwork": false
                },
                "targetSpend": {}
            }
        }]
    })
}

pub fn ad_group_create(
    customer_id: &str,
    campaign_resource: &str,
    name: &str,
    status: &str,
    cpc_bid_micros: Option<i64>,
) -> Value {
    let cid = normalize_customer_id(customer_id);
    let mut create = json!({
        "resourceName": format!("customers/{cid}/adGroups/-3"),
        "campaign": campaign_resource,
        "name": name,
        "status": status,
        "type": "SEARCH_STANDARD"
    });
    if let Some(bid) = cpc_bid_micros {
        create["cpcBidMicros"] = json!(bid.to_string());
    }
    json!({ "operations": [{ "create": create }] })
}

pub fn responsive_search_ad_create(
    _customer_id: &str,
    ad_group_resource: &str,
    final_url: &str,
    headlines: &[String],
    descriptions: &[String],
    status: &str,
) -> Value {
    let headlines_json: Vec<Value> = headlines
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut h = json!({ "text": t });
            if i == 0 {
                h["pinnedField"] = json!("HEADLINE_1");
            }
            h
        })
        .collect();
    let descriptions_json: Vec<Value> = descriptions
        .iter()
        .map(|t| json!({ "text": t }))
        .collect();

    json!({
        "operations": [{
            "create": {
                "adGroup": ad_group_resource,
                "status": status,
                "ad": {
                    "responsiveSearchAd": {
                        "headlines": headlines_json,
                        "descriptions": descriptions_json
                    },
                    "finalUrls": [final_url]
                }
            }
        }]
    })
}

pub fn keyword_create(
    customer_id: &str,
    ad_group_resource: &str,
    text: &str,
    match_type: &str,
    status: &str,
    bid_micros: Option<i64>,
) -> Value {
    let cid = normalize_customer_id(customer_id);
    let mut criterion = json!({
        "resourceName": format!("customers/{cid}/adGroupCriteria/-4"),
        "adGroup": ad_group_resource,
        "status": status,
        "keyword": {
            "text": text,
            "matchType": match_type
        }
    });
    if let Some(bid) = bid_micros {
        criterion["cpcBidMicros"] = json!(bid.to_string());
    }
    json!({
        "operations": [{ "create": criterion }]
    })
}

pub fn ad_group_status_update(
    customer_id: &str,
    ad_group_id: &str,
    status: &str,
) -> Value {
    let cid = normalize_customer_id(customer_id);
    json!({
        "operations": [{
            "updateMask": "status",
            "update": {
                "resourceName": format!("customers/{cid}/adGroups/{ad_group_id}"),
                "status": status
            }
        }]
    })
}

pub fn ad_group_ad_status_update(
    customer_id: &str,
    ad_group_id: &str,
    ad_id: &str,
    status: &str,
) -> Value {
    let cid = normalize_customer_id(customer_id);
    json!({
        "operations": [{
            "updateMask": "status",
            "update": {
                "resourceName": format!("customers/{cid}/adGroupAds/{ad_group_id}~{ad_id}"),
                "status": status
            }
        }]
    })
}

pub fn load_mutate_body(path: &std::path::Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).context("read mutate JSON file")?;
    serde_json::from_str(&text).context("parse mutate JSON")
}
