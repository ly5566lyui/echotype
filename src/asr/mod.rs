pub mod cloud_api;
pub mod local_sherpa;
pub mod postprocess;
pub mod recorder;

use crate::config::AsrConfig;
use anyhow::Result;
use async_trait::async_trait;
use cloud_api::CloudAsrEngine;
use local_sherpa::LocalSherpaEngine;
use postprocess::polish;
use recorder::trim_silence;
use std::path::PathBuf;
use tracing::{debug, info, warn};

#[async_trait]
pub trait AsrEngine: Send + Sync {
    /// 识别 PCM 音频数据 (16kHz, 16bit, 单声道) 并返回识别文本
    async fn recognize(&self, pcm_data: &[i16]) -> Result<String>;
    /// 获取当前引擎名称
    #[allow(dead_code)] // 预留: 供状态展示与引擎诊断使用
    fn name(&self) -> &'static str;
}

/// 端云混合语音识别调度器：有网且配置了 API Key 时优先走云端，失败或断网时自动降级至本地 Sherpa-ONNX
pub struct HybridAsrEngine {
    mode: String,
    auto_punctuation: bool,
    cloud: CloudAsrEngine,
    local: LocalSherpaEngine,
}

impl HybridAsrEngine {
    pub fn from_config(cfg: &AsrConfig) -> Result<Self> {
        let cloud = CloudAsrEngine::new(
            cfg.cloud.provider.clone(),
            cfg.cloud.api_key.clone(),
            cfg.cloud.endpoint.clone(),
        );
        let local = LocalSherpaEngine::new(PathBuf::from(&cfg.local.model_dir))?;
        Ok(Self {
            mode: cfg.mode.clone(),
            auto_punctuation: cfg.auto_punctuation,
            cloud,
            local,
        })
    }

    pub fn local_engine(&self) -> &LocalSherpaEngine {
        &self.local
    }
}

#[async_trait]
impl AsrEngine for HybridAsrEngine {
    async fn recognize(&self, pcm_data: &[i16]) -> Result<String> {
        // 静音裁剪 (VAD-lite)：去掉首尾静音，降低噪声误识别与上传体积
        let trimmed = trim_silence(pcm_data);
        if trimmed.is_empty() {
            warn!("No speech detected after silence trimming; skipping ASR.");
            return Ok(String::new());
        }
        if trimmed.len() != pcm_data.len() {
            debug!(
                "Silence trimmed: {} -> {} samples.",
                pcm_data.len(),
                trimmed.len()
            );
        }
        let pcm_data: &[i16] = &trimmed;

        let raw = if self.mode == "cloud_only" {
            self.cloud.recognize(pcm_data).await?
        } else if self.mode == "local_only" {
            self.local.recognize(pcm_data).await?
        } else {
            // Hybrid 模式：若云端已配置则优先尝试云端，否则或报错时回退到本地模型
            if self.cloud.is_configured() {
                match self.cloud.recognize(pcm_data).await {
                    Ok(text) => {
                        info!("Cloud ASR succeeded.");
                        text
                    }
                    Err(e) => {
                        warn!(
                            "Cloud ASR failed ({:?}), seamlessly falling back to Local Sherpa-ONNX...",
                            e
                        );
                        self.local.recognize(pcm_data).await?
                    }
                }
            } else {
                info!("Cloud ASR not configured, using Local Sherpa-ONNX engine.");
                self.local.recognize(pcm_data).await?
            }
        };

        // 语音后处理: 句末标点补全 + 全角冒号规范化 + 重复标点折叠
        let text = polish(&raw, self.auto_punctuation);
        if text != raw {
            debug!("Postprocessed ASR text: \"{}\" -> \"{}\"", raw, text);
        }
        Ok(text)
    }

    fn name(&self) -> &'static str {
        "Hybrid ASR Engine (Cloud + Local Sherpa-ONNX)"
    }
}
