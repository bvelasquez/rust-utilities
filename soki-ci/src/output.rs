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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
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
            data: Some(data),
            warnings: vec![],
            errors: vec![],
            next_actions: vec![],
            timestamp: Utc::now().to_rfc3339(),
        }
    }

    pub fn err(command: &str, errors: Vec<String>, next_actions: Vec<String>) -> Self {
        Self {
            success: false,
            command: command.into(),
            inputs: None,
            data: None,
            warnings: vec![],
            errors,
            next_actions,
            timestamp: Utc::now().to_rfc3339(),
        }
    }

    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }

    pub fn with_inputs(mut self, inputs: Value) -> Self {
        self.inputs = Some(inputs);
        self
    }

    pub fn print_json(&self) -> Result<()> {
        println!("{}", serde_json::to_string_pretty(self)?);
        Ok(())
    }
}
