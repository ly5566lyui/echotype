use anyhow::Result;
use arboard::Clipboard;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tracing::{debug, info};

pub struct ClipboardMonitor {
    running: Arc<AtomicBool>,
}

impl ClipboardMonitor {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 启动剪贴板后台监听线程
    pub fn start<F>(&self, on_change: F) -> Result<()>
    where
        F: Fn(String) + Send + 'static,
    {
        self.running.store(true, Ordering::SeqCst);
        let running = self.running.clone();

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
                        last_content = text.clone();
                        debug!("New clipboard text detected: {} chars", text.len());
                        on_change(text);
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        });

        info!("Clipboard monitor started.");
        Ok(())
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
