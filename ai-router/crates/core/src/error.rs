use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("model disabled: {0}")]
    ModelDisabled(String),
    #[error("modality mismatch: model {model} is {have}, this endpoint needs {need}")]
    ModalityMismatch {
        model: String,
        have: String,
        need: String,
    },
    #[error("provider not configured: {0}")]
    ProviderMissing(String),
    #[error("provider {0} is disabled")]
    ProviderDisabled(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Invalid(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("auto routing is disabled; pick an explicit model or alias")]
    AutoRoutingDisabled,
    #[error("unknown routing profile '{0}'")]
    UnknownProfile(String),
    #[error("no model in profile '{0}' can serve this request")]
    NoRoute(String),
}
