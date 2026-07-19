use std::collections::HashMap;

use serde::Serialize;

/// Live snapshot for TUI progress rendering during apply.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ApplySnapshot {
    pub phase: String,
    pub plan_id: i64,
    pub account_id: Option<String>,
    pub current: usize,
    pub total: usize,
    pub ok_count: usize,
    pub fail_count: usize,
    pub current_action: String,
    pub current_uid: u32,
    pub log: Vec<String>,
    pub action_totals: Vec<(String, usize)>,
}

impl ApplySnapshot {
    pub fn ratio(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.current as f64 / self.total as f64).clamp(0.0, 1.0)
        }
    }

    pub fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > 8 {
            self.log.remove(0);
        }
    }
}

pub struct ApplyProgress {
    snap: ApplySnapshot,
    action_totals: HashMap<String, usize>,
}

impl ApplyProgress {
    pub fn new(plan_id: i64, total: usize) -> Self {
        Self {
            snap: ApplySnapshot {
                phase: "Preparing".into(),
                plan_id,
                total,
                ..Default::default()
            },
            action_totals: HashMap::new(),
        }
    }

    pub fn snapshot(&self) -> ApplySnapshot {
        let mut snap = self.snap.clone();
        let mut totals: Vec<(String, usize)> = self
            .action_totals
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        totals.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        snap.action_totals = totals;
        snap
    }

    pub fn current_step(&self) -> usize {
        self.snap.current
    }

    pub fn set_phase(&mut self, phase: &str) {
        self.snap.phase = phase.into();
    }

    pub fn set_account(&mut self, account_id: &str) {
        self.snap.account_id = Some(account_id.into());
        self.snap.phase = "Applying via IMAP".into();
    }

    pub fn on_message(
        &mut self,
        index: usize,
        action: &str,
        uid: u32,
        ok: bool,
        detail: &str,
    ) {
        self.snap.current = index;
        self.snap.current_action = action.into();
        self.snap.current_uid = uid;
        if ok {
            self.snap.ok_count += 1;
            *self.action_totals.entry(action.to_string()).or_insert(0) += 1;
            self.push_log(format!("✓ {action} uid {uid}"));
        } else {
            self.snap.fail_count += 1;
            self.push_log(format!("✗ {action} uid {uid}: {detail}"));
        }
    }

    pub fn finish(&mut self) {
        self.snap.phase = "Complete".into();
        self.snap.current = self.snap.total;
    }

    fn push_log(&mut self, line: String) {
        self.snap.push_log(line);
    }
}

pub type ApplyProgressCallback<'a> = dyn FnMut(&ApplySnapshot) + 'a;