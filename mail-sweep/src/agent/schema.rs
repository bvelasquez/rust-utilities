use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageCategory {
    Priority,
    Personal,
    Work,
    Newsletter,
    Marketing,
    Notification,
    Receipt,
    Spam,
    Unknown,
}

impl MessageCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Priority => "priority",
            Self::Personal => "personal",
            Self::Work => "work",
            Self::Newsletter => "newsletter",
            Self::Marketing => "marketing",
            Self::Notification => "notification",
            Self::Receipt => "receipt",
            Self::Spam => "spam",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "priority" => Self::Priority,
            "personal" => Self::Personal,
            "work" => Self::Work,
            "newsletter" => Self::Newsletter,
            "marketing" => Self::Marketing,
            "notification" => Self::Notification,
            "receipt" => Self::Receipt,
            "spam" => Self::Spam,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailAction {
    Keep,
    MarkRead,
    Flag,
    Unflag,
    Archive,
    Move,
    Delete,
    Tag,
}

impl MailAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::MarkRead => "mark_read",
            Self::Flag => "flag",
            Self::Unflag => "unflag",
            Self::Archive => "archive",
            Self::Move => "move",
            Self::Delete => "delete",
            Self::Tag => "tag",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "keep" => Self::Keep,
            "mark_read" | "markread" => Self::MarkRead,
            "flag" => Self::Flag,
            "unflag" => Self::Unflag,
            "archive" => Self::Archive,
            "move" => Self::Move,
            "delete" => Self::Delete,
            "tag" => Self::Tag,
            _ => Self::Keep,
        }
    }

    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::Delete)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDecision {
    pub account_id: String,
    pub uid: u32,
    #[serde(default)]
    pub message_id: Option<i64>,
    pub category: MessageCategory,
    pub priority: u8,
    pub action: MailAction,
    #[serde(default)]
    pub target_folder: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub confidence: f32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationPlan {
    pub messages: Vec<MessageDecision>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessageInput {
    pub account_id: String,
    pub uid: u32,
    pub from_address: String,
    pub subject: String,
    pub date: String,
    pub list_unsubscribe: Option<String>,
    pub snippet: String,
    pub is_unread: bool,
}

/// One sender's pending mail summarized for the LLM (not per-message).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SenderGroupInput {
    pub sender: String,
    pub domain: String,
    pub message_count: usize,
    pub sample_subjects: Vec<String>,
    pub sample_snippet: String,
    pub has_list_unsubscribe: bool,
}

/// Reusable triage pattern the LLM proposes; applied to all matching cached mail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationPattern {
    pub match_pattern: String,
    pub category: String,
    pub priority: u8,
    pub action: String,
    #[serde(default)]
    pub target_folder: Option<String>,
    pub confidence: f32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternClassificationPlan {
    pub patterns: Vec<ClassificationPattern>,
    pub summary: String,
}

/// Detailed sender context for per-sender pattern suggestions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SenderDetailInput {
    pub sender: String,
    pub domain: String,
    pub message_count: usize,
    pub subjects: Vec<String>,
    pub sample_snippet: String,
    pub has_list_unsubscribe: bool,
    pub header_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternSuggestionPlan {
    pub patterns: Vec<ClassificationPattern>,
    pub summary: String,
}

/// One rule entry sent to the audit LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleAuditInput {
    pub index: usize,
    pub r#match: String,
    pub action: String,
    pub category: Option<String>,
    pub priority: Option<u8>,
    pub sample_subjects: Vec<String>,
    pub sample_senders: Vec<String>,
    pub match_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposedRule {
    pub r#match: String,
    pub action: String,
    pub category: Option<String>,
    pub priority: Option<u8>,
    pub target_folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleAuditSuggestion {
    pub kind: String,
    pub confidence: f32,
    pub reason: String,
    pub proposed_rules: Vec<ProposedRule>,
    pub retire_indices: Vec<usize>,
    pub example_subjects: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleAuditPlan {
    pub suggestions: Vec<RuleAuditSuggestion>,
    pub summary: String,
}
