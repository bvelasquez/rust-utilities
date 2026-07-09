use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::api::{ElevenLabsClient, DEFAULT_SFX_MODEL};
use crate::auth::AuthState;

pub fn is_sound_effect_history_item(item: &Value) -> bool {
    item.get("model_id")
        .and_then(|v| v.as_str())
        .map(|m| m.contains("text_to_sound"))
        .unwrap_or(false)
}

pub fn infer_audio_extension(output_format: &str) -> &'static str {
    if output_format.starts_with("mp3") {
        "mp3"
    } else if output_format.starts_with("wav") {
        "wav"
    } else if output_format.starts_with("pcm") {
        "pcm"
    } else if output_format.starts_with("opus") {
        "opus"
    } else {
        "bin"
    }
}

pub async fn run_tts(
    auth: &AuthState,
    config_path: &Option<PathBuf>,
    voice_id: &str,
    text: &str,
    model_id: &str,
    output_format: &str,
    output: Option<PathBuf>,
) -> Result<Value> {
    let client = ElevenLabsClient::new(auth, config_path.as_ref())?;
    let audio = client
        .text_to_speech(voice_id, text, model_id, output_format)
        .await?;

    let out_path = if let Some(p) = output {
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        tokio::fs::write(&p, &audio).await?;
        p
    } else {
        let ext = infer_audio_extension(output_format);
        let p = PathBuf::from(format!("elabs-tts.{ext}"));
        tokio::fs::write(&p, &audio).await?;
        p
    };

    Ok(json!({
        "voiceId": voice_id,
        "modelId": model_id,
        "outputFormat": output_format,
        "outputPath": out_path.display().to_string(),
        "bytes": audio.len(),
        "textLength": text.len(),
    }))
}

pub async fn run_stt(
    auth: &AuthState,
    config_path: &Option<PathBuf>,
    file: &Path,
    model_id: &str,
    language_code: Option<&str>,
    diarize: bool,
    output: Option<PathBuf>,
) -> Result<Value> {
    if !file.is_file() {
        bail!("file not found: {}", file.display());
    }
    let client = ElevenLabsClient::new(auth, config_path.as_ref())?;
    let transcript = client
        .speech_to_text(file, model_id, language_code, diarize)
        .await?;

    let output_path = output.clone();
    if let Some(p) = output {
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        tokio::fs::write(&p, serde_json::to_string_pretty(&transcript)?).await?;
    }

    let mut result = transcript;
    if let Some(p) = output_path {
        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "outputPath".to_string(),
                json!(p.display().to_string()),
            );
        }
    }
    Ok(result)
}

pub async fn run_voices_list(
    auth: &AuthState,
    config_path: &Option<PathBuf>,
    search: Option<&str>,
    page_size: u32,
    next_page_token: Option<&str>,
) -> Result<Value> {
    let client = ElevenLabsClient::new(auth, config_path.as_ref())?;
    client
        .list_voices(search, page_size, next_page_token)
        .await
}

pub async fn run_models_list(
    auth: &AuthState,
    config_path: &Option<PathBuf>,
) -> Result<Value> {
    let client = ElevenLabsClient::new(auth, config_path.as_ref())?;
    client.list_models().await
}

pub async fn run_sfx_create(
    auth: &AuthState,
    config_path: &Option<PathBuf>,
    text: &str,
    model_id: &str,
    output_format: &str,
    duration_seconds: Option<f64>,
    loop_audio: bool,
    prompt_influence: Option<f64>,
    output: Option<PathBuf>,
) -> Result<Value> {
    if let Some(d) = duration_seconds {
        if !(0.5..=30.0).contains(&d) {
            bail!("duration must be between 0.5 and 30 seconds");
        }
    }
    if let Some(p) = prompt_influence {
        if !(0.0..=1.0).contains(&p) {
            bail!("prompt-influence must be between 0.0 and 1.0");
        }
    }

    let client = ElevenLabsClient::new(auth, config_path.as_ref())?;
    let audio = client
        .create_sound_effect(
            text,
            model_id,
            output_format,
            duration_seconds,
            loop_audio,
            prompt_influence,
        )
        .await?;

    let out_path = if let Some(p) = output {
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        tokio::fs::write(&p, &audio).await?;
        p
    } else {
        let ext = infer_audio_extension(output_format);
        let p = PathBuf::from(format!("elabs-sfx.{ext}"));
        tokio::fs::write(&p, &audio).await?;
        p
    };

    Ok(json!({
        "modelId": model_id,
        "outputFormat": output_format,
        "outputPath": out_path.display().to_string(),
        "bytes": audio.len(),
        "text": text,
        "durationSeconds": duration_seconds,
        "loop": loop_audio,
        "promptInfluence": prompt_influence,
    }))
}

pub async fn run_sfx_list(
    auth: &AuthState,
    config_path: &Option<PathBuf>,
    search: Option<&str>,
    page_size: u32,
    start_after: Option<&str>,
    model_id: Option<&str>,
    include_all: bool,
) -> Result<Value> {
    let client = ElevenLabsClient::new(auth, config_path.as_ref())?;
    let model_filter = if include_all {
        None
    } else {
        Some(model_id.unwrap_or(DEFAULT_SFX_MODEL))
    };
    let mut result = client
        .list_history(page_size, start_after, search, model_filter)
        .await?;

    if !include_all {
        if let Some(history) = result.get_mut("history").and_then(|h| h.as_array_mut()) {
            history.retain(is_sound_effect_history_item);
        }
    }

    Ok(result)
}

pub async fn run_sfx_download(
    auth: &AuthState,
    config_path: &Option<PathBuf>,
    history_item_id: &str,
    output: Option<PathBuf>,
    output_format: Option<&str>,
) -> Result<Value> {
    let client = ElevenLabsClient::new(auth, config_path.as_ref())?;
    let audio = client.download_history_audio(history_item_id).await?;

    let ext = output_format
        .map(infer_audio_extension)
        .unwrap_or("mp3");
    let out_path = if let Some(p) = output {
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
        tokio::fs::write(&p, &audio).await?;
        p
    } else {
        let p = PathBuf::from(format!("elabs-sfx-{history_item_id}.{ext}"));
        tokio::fs::write(&p, &audio).await?;
        p
    };

    Ok(json!({
        "historyItemId": history_item_id,
        "outputPath": out_path.display().to_string(),
        "bytes": audio.len(),
    }))
}

pub async fn run_voices_clone(
    auth: &AuthState,
    config_path: &Option<PathBuf>,
    name: &str,
    files: &[PathBuf],
    description: Option<&str>,
    remove_background_noise: bool,
    dry_run: bool,
) -> Result<Value> {
    if files.is_empty() {
        bail!("at least one --file is required for voice cloning");
    }
    for f in files {
        if !f.is_file() {
            bail!("file not found: {}", f.display());
        }
    }
    if dry_run {
        return Ok(json!({
            "dryRun": true,
            "name": name,
            "files": files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "description": description,
            "removeBackgroundNoise": remove_background_noise,
        }));
    }
    let client = ElevenLabsClient::new(auth, config_path.as_ref())?;
    client
        .clone_voice(name, files, description, remove_background_noise)
        .await
}

pub async fn run_voices_design(
    auth: &AuthState,
    config_path: &Option<PathBuf>,
    voice_description: &str,
    text: Option<&str>,
    auto_generate_text: bool,
    model_id: Option<&str>,
    dry_run: bool,
    strip_audio: bool,
) -> Result<Value> {
    if dry_run {
        return Ok(json!({
            "dryRun": true,
            "voiceDescription": voice_description,
            "text": text,
            "autoGenerateText": auto_generate_text,
            "modelId": model_id,
        }));
    }
    let client = ElevenLabsClient::new(auth, config_path.as_ref())?;
    let mut result = client
        .design_voice(
            voice_description,
            text,
            auto_generate_text,
            model_id,
        )
        .await?;

    if strip_audio {
        strip_preview_audio(&mut result);
    }
    Ok(result)
}

pub async fn run_voices_save(
    auth: &AuthState,
    config_path: &Option<PathBuf>,
    generated_voice_id: &str,
    voice_name: &str,
    voice_description: Option<&str>,
    dry_run: bool,
) -> Result<Value> {
    if dry_run {
        return Ok(json!({
            "dryRun": true,
            "generatedVoiceId": generated_voice_id,
            "voiceName": voice_name,
            "voiceDescription": voice_description,
        }));
    }
    let client = ElevenLabsClient::new(auth, config_path.as_ref())?;
    client
        .save_voice_from_preview(generated_voice_id, voice_name, voice_description)
        .await
}

fn strip_preview_audio(value: &mut Value) {
    if let Some(previews) = value.get_mut("previews").and_then(|p| p.as_array_mut()) {
        for preview in previews {
            if let Some(obj) = preview.as_object_mut() {
                if obj.contains_key("audio_base_64") {
                    let len = obj
                        .get("audio_base_64")
                        .and_then(|v| v.as_str())
                        .map(|s| s.len())
                        .unwrap_or(0);
                    obj.insert("audio_base_64".into(), json!(null));
                    obj.insert("audioOmitted".into(), json!(true));
                    obj.insert("audioBase64Length".into(), json!(len));
                }
            }
        }
    }
}

pub fn read_text_arg(text: Option<String>, text_file: Option<PathBuf>) -> Result<String> {
    match (text, text_file) {
        (Some(t), None) => Ok(t),
        (None, Some(path)) => std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display())),
        (Some(_), Some(_)) => bail!("use either --text or --text-file, not both"),
        (None, None) => bail!("--text or --text-file is required"),
    }
}

#[cfg(test)]
mod tests {
    use super::{infer_audio_extension, is_sound_effect_history_item, strip_preview_audio};
    use serde_json::json;

    #[test]
    fn infer_audio_extension_mp3() {
        assert_eq!(infer_audio_extension("mp3_44100_128"), "mp3");
    }

    #[test]
    fn is_sound_effect_history_item_matches_model() {
        let sfx = json!({ "model_id": "eleven_text_to_sound_v2" });
        let tts = json!({ "model_id": "eleven_multilingual_v2" });
        assert!(is_sound_effect_history_item(&sfx));
        assert!(!is_sound_effect_history_item(&tts));
    }

    #[test]
    fn strip_preview_audio_omits_base64() {
        let mut v = json!({
            "previews": [{ "audio_base_64": "AAAA", "generated_voice_id": "x" }]
        });
        strip_preview_audio(&mut v);
        let preview = &v["previews"][0];
        assert!(preview["audio_base_64"].is_null());
        assert_eq!(preview["audioOmitted"], true);
        assert_eq!(preview["audioBase64Length"], 4);
    }
}