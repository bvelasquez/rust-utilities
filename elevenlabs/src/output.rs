use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct Envelope<T> {
    pub success: bool,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Value>,
    pub data: T,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub next_actions: Vec<String>,
    pub timestamp: String,
}

impl<T: Serialize> Envelope<T> {
    pub fn ok(command: &str, data: T) -> Self {
        Self {
            success: true,
            command: command.into(),
            inputs: None,
            data,
            warnings: vec![],
            errors: vec![],
            next_actions: vec![],
            timestamp: Utc::now().to_rfc3339(),
        }
    }

    pub fn with_next_actions(mut self, actions: Vec<String>) -> Self {
        self.next_actions = actions;
        self
    }

    pub fn print_json(&self) -> Result<()> {
        println!("{}", serde_json::to_string_pretty(self)?);
        Ok(())
    }
}

pub fn print_raw(data: &Value, compact: bool) -> Result<()> {
    if compact {
        println!("{}", serde_json::to_string(data)?);
    } else {
        println!("{}", serde_json::to_string_pretty(data)?);
    }
    Ok(())
}
