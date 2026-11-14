use super::recorder::save_wav_16k_mono;
use super::AsrEngine;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{debug, info, warn};

/// 基于 Sherpa-ONNX 的离线本地语音识别引擎
pub struct LocalSherpaEngine {
    model_dir: PathBuf,
}

impl LocalSherpaEngine {
    pub fn new(model_dir: PathBuf) -> Result<Self> {
        let expanded = expand_tilde(&model_dir);
        info!("Initializing Local Sherpa-ONNX Engine at {:?}", expanded);
        Ok(Self {
            model_dir: expanded,
        })
    }

    /// 直接对指定 WAV 文件执行离线推理
    pub async fn recognize_wav_file(&self, wav_path: &Path) -> Result<String> {
        let bin_path = self.find_sherpa_binary();
        let effective_dir = if self.model_dir.join("tokens.txt").exists() {
            self.model_dir.clone()
        } else if let Some(parent) = self.model_dir.parent() {
            if parent.join("tokens.txt").exists() {
                parent.to_path_buf()
            } else {
                self.model_dir.clone()
            }
        } else {
            self.model_dir.clone()
        };

        let tokens_path = effective_dir.join("tokens.txt");
        let paraformer_int8 = effective_dir.join("model.int8.onnx");
        let paraformer_fp32 = effective_dir.join("model.onnx");

        let model_file = if paraformer_int8.exists() {
            paraformer_int8
        } else {
            paraformer_fp32
        };

        if let Some(bin) = bin_path {
            if tokens_path.exists() && model_file.exists() {
                info!(
                    "Invoking sherpa-onnx-offline ({:?}) with model {:?}",
                    bin, self.model_dir
                );

                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                let lib_dir = PathBuf::from(&home).join(".local/share/echotype/lib");

                let is_sense_voice = self
                    .model_dir
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("sense-voice");

                let mut cmd = Command::new(&bin);
                cmd.env("LD_LIBRARY_PATH", &lib_dir)
                    .arg(format!("--tokens={}", tokens_path.display()))
                    .arg("--num-threads=2");

                if is_sense_voice {
                    cmd.arg(format!("--sense-voice-model={}", model_file.display()))
                        .arg("--sense-voice-use-itn=1");
                } else {
                    cmd.arg(format!("--paraformer={}", model_file.display()));
                    // 注: paraformer 引擎仅支持 greedy_search，热词提升 (需要
                    // modified_beam_search) 不可用——热词黑名单由 postprocess 兜底
                }

                cmd.arg(wav_path);

                let output = cmd
                    .output()
                    .await
                    .context("Failed to execute sherpa-onnx-offline")?;

                let combined = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                debug!("sherpa-onnx output:\n{}", combined);

                for line in combined.lines().rev() {
                    let trimmed = line.trim();
                    if trimmed.starts_with('{') && trimmed.contains("\"text\"") {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                            if let Some(text) = val.get("text").and_then(|t| t.as_str()) {
                                return Ok(text.trim().to_string());
                            }
                        }
                    }
                }
            }
        }

        warn!(
            "Sherpa-ONNX binary or model not found in {:?}. Voice input unavailable (run scripts/download_models.sh to install offline ASR).",
            self.model_dir
        );
        anyhow::bail!("Sherpa-ONNX not installed; voice input unavailable (run scripts/download_models.sh)")
    }

    fn find_sherpa_binary(&self) -> Option<PathBuf> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let local_bin = PathBuf::from(&home).join(".local/share/echotype/bin/sherpa-onnx-offline");
        if local_bin.exists() {
            return Some(local_bin);
        }
        let user_bin = PathBuf::from(&home).join(".local/bin/sherpa-onnx-offline");
        if user_bin.exists() {
            return Some(user_bin);
        }
        None
    }
}

#[async_trait]
impl AsrEngine for LocalSherpaEngine {
    async fn recognize(&self, pcm_data: &[i16]) -> Result<String> {
        info!(
            "Running offline ASR inference on {} PCM samples ({:.2}s)...",
            pcm_data.len(),
            pcm_data.len() as f64 / 16000.0
        );

        if pcm_data.is_empty() {
            return Ok(String::new());
        }

        let wav_path = PathBuf::from("/tmp/echotype_recording.wav");
        save_wav_16k_mono(&wav_path, pcm_data)?;
        self.recognize_wav_file(&wav_path).await
    }

    fn name(&self) -> &'static str {
        "Sherpa-ONNX (Local)"
    }
}

pub fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    path.to_path_buf()
}
