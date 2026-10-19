use anyhow::Result;
use tracing::info;

/// 全局热键监听器抽象
pub struct HotkeyManager;

impl HotkeyManager {
    pub fn new() -> Self {
        Self
    }

    /// 监听语音输入和常用语快捷键
    pub fn listen<F>(&self, shortcut: &str, callback: F) -> Result<()>
    where
        F: Fn() + Send + 'static,
    {
        info!("Registering global shortcut: {}", shortcut);
        // 在 Wayland 下推荐通过 XDG Desktop Portal GlobalShortcuts 接口注册
        // 在 X11 下通过 XGrabKey 注册
        Ok(())
    }
}
