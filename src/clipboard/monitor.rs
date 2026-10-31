use anyhow::Result;
use arboard::Clipboard;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tracing::{debug, info};

/// 敏感内容启发式特征 (小写匹配，命中即不入库不同步)
const SENSITIVE_MARKERS: &[&str] = &[
    "-----begin ",      // PEM 私钥/证书
    "private key",      // 私钥字样
    "gsk_",             // API 密钥 (SiliconFlow 风格)
    "ghp_", "gho_", "github_pat_", // GitHub 令牌
    "akia",             // AWS AccessKeyId
    "xoxb-", "xoxp-",   // Slack 令牌
    "bearer ",          // HTTP 凭据
    "authorization:",
    "password:", "passwd:", "密码:", "密码：", // 半角/全角冒号都覆盖
    "api_key=", "apikey:",
];

/// 启发式判断剪贴板内容是否疑似敏感凭据
fn is_probably_sensitive(text: &str) -> bool {
    // 仅扫描前 4096 个字符，避免超长内容拖慢轮询
    let scan: String = text.chars().take(4096).collect();
    let lower = scan.to_lowercase();
    SENSITIVE_MARKERS.iter().any(|m| lower.contains(m))
}

pub struct ClipboardMonitor {
    running: Arc<AtomicBool>,
    filter_sensitive: bool,
}

impl ClipboardMonitor {
    #[allow(dead_code)] // 保留无过滤构造器作为默认 API
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            filter_sensitive: false,
        }
    }

    /// 启用敏感内容启发式过滤 (密码/密钥/令牌等不入库不同步)
    pub fn with_sensitive_filter(mut self, enabled: bool) -> Self {
        self.filter_sensitive = enabled;
        self
    }

    /// 启动剪贴板后台监听线程
    pub fn start<F>(&self, on_change: F) -> Result<()>
    where
        F: Fn(String) + Send + 'static,
    {
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();
        let filter_sensitive = self.filter_sensitive;

        std::thread::spawn(move || {
            let mut clipboard = match Clipboard::new() {
                Ok(cb) => cb,
                Err(e) => {
                    tracing::error!("Failed to initialize clipboard listener: {:?}", e);
                    return;
                }
            };

            let mut last_content = String::new();

            while running.load(Ordering::SeqCst) {
                if let Ok(text) = clipboard.get_text() {
                    if !text.is_empty() && text != last_content {
                        if filter_sensitive && is_probably_sensitive(&text) {
                            debug!(
                                "Skipped sensitive clipboard content ({} chars).",
                                text.chars().count()
                            );
                            last_content = text;
                        } else {
                            last_content = text.clone();
                            debug!("New clipboard text detected: {} chars", text.len());
                            on_change(text);
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });

        info!(
            "Clipboard monitor started (sensitive filter: {}).",
            filter_sensitive
        );
        Ok(())
    }

    #[allow(dead_code)] // 预留: 目前进程退出即结束监听
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_credentials() {
        assert!(is_probably_sensitive(
            "ghp_0123456789abcdefghijklmnopqrstuvwxyz"
        ));
        assert!(is_probably_sensitive("Password: hunter2"));
        assert!(is_probably_sensitive("-----BEGIN RSA PRIVATE KEY-----"));
        assert!(is_probably_sensitive("Authorization: Bearer abc.def.ghi"));
        assert!(is_probably_sensitive("数据库密码：p@ssw0rd"));
    }

    #[test]
    fn normal_content_not_flagged() {
        assert!(!is_probably_sensitive("今天中午吃什么？"));
        assert!(!is_probably_sensitive("cargo build --release"));
        assert!(!is_probably_sensitive("https://github.com/rust-lang/rust"));
    }
}
