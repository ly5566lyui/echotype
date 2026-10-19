use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub asr: AsrConfig,
    pub clipboard: ClipboardConfig,
    pub p2p_sync: P2pSyncConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub log_level: String,
    pub voice_shortcut: String,
    pub clipboard_shortcut: String,
    #[serde(default)]
    pub start_hidden: bool,
    #[serde(default)]
    pub auto_hide_after_commit: bool,
    #[serde(default = "default_true")]
    pub allow_drag: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrConfig {
    pub mode: String, // "hybrid", "cloud_only", "local_only"
    pub local: LocalAsrConfig,
    pub cloud: CloudAsrConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAsrConfig {
    pub enabled: bool,
    pub model_dir: String,
    pub tokens: String,
    pub num_threads: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudAsrConfig {
    pub enabled: bool,
    pub provider: String,
    pub api_key: String,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardConfig {
    pub max_history_items: usize,
    pub ignore_sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2pSyncConfig {
    pub enabled: bool,
    pub device_name: String,
    pub pair_secret: String,
    pub port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig {
                log_level: "info".to_string(),
                voice_shortcut: "Ctrl+Alt+V".to_string(),
                clipboard_shortcut: "Ctrl+Alt+H".to_string(),
                start_hidden: false,
                auto_hide_after_commit: false,
                allow_drag: true,
            },
            asr: AsrConfig {
                mode: "hybrid".to_string(),
                local: LocalAsrConfig {
                    enabled: true,
                    model_dir: "~/.local/share/wetype/models".to_string(),
                    tokens: "~/.local/share/wetype/models/tokens.txt".to_string(),
                    num_threads: 2,
                },
                cloud: CloudAsrConfig {
                    enabled: false,
                    provider: "aliyun".to_string(),
                    api_key: String::new(),
                    endpoint: String::new(),
                },
            },
            clipboard: ClipboardConfig {
                max_history_items: 200,
                ignore_sensitive: true,
            },
            p2p_sync: P2pSyncConfig {
                enabled: false,
                device_name: "Linux-Desktop".to_string(),
                pair_secret: String::new(),
                port: 42890,
            },
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

    pub fn default_config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config/wetype/config.toml")
    }
}
