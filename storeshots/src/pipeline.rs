use crate::config::{PipelineStep, StoreshotsConfig};
use anyhow::{bail, Result};
use std::collections::{HashSet};

pub fn resolve_steps(cfg: &StoreshotsConfig, only: &[String]) -> Result<Vec<PipelineStep>> {
    let all = cfg.default_pipeline();
    let enabled: Vec<_> = all.into_iter().filter(|s| s.enabled).collect();

    let selected: Vec<PipelineStep> = if only.is_empty() {
        enabled
    } else {
        let want: HashSet<_> = only.iter().map(|s| s.as_str()).collect();
        enabled
            .into_iter()
            .filter(|s| want.contains(s.id.as_str()) || want.contains(s.phase.as_str()))
            .collect()
    };

    if selected.is_empty() {
        bail!("no pipeline steps selected; use --only brand,copy,mobile or enable steps in storeshots.toml");
    }

    topological_sort(&selected)
}

fn topological_sort(steps: &[PipelineStep]) -> Result<Vec<PipelineStep>> {
    let ids: HashSet<_> = steps.iter().map(|s| s.id.as_str()).collect();
    let mut sorted = Vec::new();
    let mut done: HashSet<&str> = HashSet::new();
    let mut remaining: Vec<_> = steps.iter().collect();

    while !remaining.is_empty() {
        let before = remaining.len();
        remaining.retain(|step| {
            let ready = step
                .depends_on
                .iter()
                .all(|d| done.contains(d.as_str()) || !ids.contains(d.as_str()));
            if ready {
                sorted.push((*step).clone());
                done.insert(step.id.as_str());
                false
            } else {
                true
            }
        });
        if remaining.len() == before {
            bail!("pipeline dependency cycle or missing step");
        }
    }
    Ok(sorted)
}
