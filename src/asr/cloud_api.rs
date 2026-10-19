use super::recorder::save_wav_16k_mono;
use super::AsrEngine;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::process::Command;
use tracing::info;

/// 云端在线 ASR API 识别引擎 (兼容 OpenAI Whisper / SiliconFlow / DashScope 接口)
pub struct CloudAsrEngine {
    provider: String,
    api_key: String,
    endpoint: String,
}

impl CloudAsrEngine {
    pub fn new(provider: String, api_key: String, endpoint: String) -> Self {
        Self {
            provider,
            api_key,
            endpoint,
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.api_key.is_empty() && self.api_key != "YOUR_API_KEY_HERE" && !self.endpoint.is_empty()
    }
}

#[async_trait]
impl AsrEngine for CloudAsrEngine {
    async fn recognize(&self, pcm_data: &[i16]) -> Result<String> {
        if !self.is_configured() {
            anyhow::bail!("Cloud ASR API key or endpoint is not configured.");
        }

        info!(
            "Uploading {} samples to Cloud ASR [{}] at {}...",
            pcm_data.len(),
            self.provider,
            self.endpoint
        );

        let wav_path = PathBuf::from("/tmp/wetype_cloud_upload.wav");
        save_wav_16k_mono(&wav_path, pcm_data)?;

        let model_name = if self.provider == "siliconflow" {
            "FunAudioLLM/SenseVoiceSmall"
        } else {
            "whisper-1"
        };

        let output = Command::new("curl")
            .arg("-sS")
            .arg("--max-time")
            .arg("10")
            .arg("-X")
            .arg("POST")
            .arg(&self.endpoint)
            .arg("-H")
            .arg(format!("Authorization: Bearer {}", self.api_key))
            .arg("-F")
            .arg(format!("file=@{};type=audio/wav", wav_path.display()))
            .arg("-F")
            .arg(format!("model={}", model_name))
            .output()
            .await
            .context("Failed to execute curl for Cloud ASR request")?;

        if !output.status.success() {
            let err_str = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Cloud ASR request failed: {}", err_str);
        }

        let body = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&body)
            .with_context(|| format!("Failed to parse Cloud ASR JSON response: {}", body))?;

        if let Some(text) = json.get("text").and_then(|v| v.as_str()) {
            return Ok(text.trim().to_string());
        }

        anyhow::bail!("Unexpected Cloud ASR response format: {}", json)
    }

    fn name(&self) -> &'static str {
        "Cloud API Engine"
    }
}
