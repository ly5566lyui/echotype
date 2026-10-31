use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// 所有配置段均实现 Default 且标注 #[serde(default)]，
// 缺失的字段/段落自动取默认值 —— 旧版本配置文件在新字段引入后无需手工迁移。

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub asr: AsrConfig,
    pub clipboard: ClipboardConfig,
    pub p2p_sync: P2pSyncConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub log_level: String,
    pub voice_shortcut: String,
    pub clipboard_shortcut: String,
    pub start_hidden: bool,
    pub auto_hide_after_commit: bool,
    pub allow_drag: bool,
    /// 悬浮窗主题: 0 深色沉浸, 1 极简纯白
    pub ui_theme: i32,
    /// Fcitx5 候选词数量 (5/7/9), 保存时同步写入 fcitx5 配置
    pub candidate_count: i32,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            voice_shortcut: "Ctrl+Alt+V".to_string(),
            clipboard_shortcut: "Ctrl+Alt+H".to_string(),
            start_hidden: false,
            auto_hide_after_commit: false,
            allow_drag: true,
            ui_theme: 0,
            candidate_count: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AsrConfig {
    pub mode: String, // "hybrid", "cloud_only", "local_only"
    /// 识别语种偏好: 0 普通话, 1 英语, 2 粤语
    pub language: i32,
    /// 智能断句与标点符号
    pub auto_punctuation: bool,
    pub local: LocalAsrConfig,
    pub cloud: CloudAsrConfig,
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            mode: "hybrid".to_string(),
            language: 0,
            auto_punctuation: true,
            local: Default::default(),
            cloud: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalAsrConfig {
    pub enabled: bool,
    pub model_dir: String,
    pub tokens: String,
    pub num_threads: usize,
}

impl Default for LocalAsrConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            model_dir: "~/.local/share/wetype/models".to_string(),
            tokens: "~/.local/share/wetype/models/tokens.txt".to_string(),
            num_threads: 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CloudAsrConfig {
    pub enabled: bool,
    pub provider: String,
    pub api_key: String,
    pub endpoint: String,
}

impl Default for CloudAsrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "aliyun".to_string(),
            api_key: String::new(),
            endpoint: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardConfig {
    pub max_history_items: usize,
    pub ignore_sensitive: bool,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            max_history_items: 200,
            ignore_sensitive: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct P2pSyncConfig {
    pub enabled: bool,
    pub device_name: String,
    pub pair_secret: String,
    pub port: u16,
}

impl Default for P2pSyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            device_name: "Linux-Desktop".to_string(),
            pair_secret: String::new(),
            port: 42890,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: Default::default(),
            asr: Default::default(),
            clipboard: Default::default(),
            p2p_sync: Default::default(),
        }
    }
}

impl AppConfig {
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file at {:?}", path.as_ref()))?;
        let config: AppConfig = toml::from_str(&content)
            .with_context(|| "Failed to parse TOML configuration")?;
        Ok(config)
    }

    /// 将当前配置持久化到 TOML 文件 (自动创建父目录)
    pub fn save_to_path<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create config directory {:?}", parent)
                })?;
            }
        }
        let content = toml::to_string_pretty(self)
            .with_context(|| "Failed to serialize TOML configuration")?;
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write config file at {:?}", path))?;
        Ok(())
    }

    pub fn default_config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config/wetype/config.toml")
    }

    /// 剪贴板历史 / SQLite 数据库等运行时数据目录 (XDG: ~/.local/share/wetype)
    pub fn default_data_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("wetype")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_roundtrip_preserves_fields() {
        let cfg = AppConfig::default();
        let text = toml::to_string_pretty(&cfg).expect("serialize failed");
        let parsed: AppConfig = toml::from_str(&text).expect("parse failed");
        assert_eq!(parsed.general.voice_shortcut, cfg.general.voice_shortcut);
        assert_eq!(parsed.general.candidate_count, cfg.general.candidate_count);
        assert_eq!(parsed.asr.language, cfg.asr.language);
        assert_eq!(parsed.clipboard.max_history_items, cfg.clipboard.max_history_items);
        assert_eq!(parsed.p2p_sync.port, cfg.p2p_sync.port);
    }

    #[test]
    fn minimal_toml_fills_defaults() {
        // 旧版/手写的不完整配置文件应能正常加载并补默认值
        let cfg: AppConfig =
            toml::from_str("[general]\nlog_level = \"debug\"\n").expect("parse failed");
        assert_eq!(cfg.general.log_level, "debug");
        assert_eq!(cfg.general.allow_drag, true);
        assert_eq!(cfg.general.candidate_count, 5);
        assert_eq!(cfg.asr.mode, "hybrid");
        assert_eq!(cfg.asr.auto_punctuation, true);
        assert_eq!(cfg.clipboard.max_history_items, 200);
        assert_eq!(cfg.p2p_sync.port, 42890);
    }
}
