use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Kind of generation a model (and an endpoint) performs.
///
/// `Video` is accepted in config so a later async-job adapter can light it up
/// without a schema break. No public video route is served in v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    Chat,
    Embedding,
    Image,
    AudioTts,
    AudioStt,
    Video,
}

impl Modality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Embedding => "embedding",
            Self::Image => "image",
            Self::AudioTts => "audio_tts",
            Self::AudioStt => "audio_stt",
            Self::Video => "video",
        }
    }
}

impl std::fmt::Display for Modality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
