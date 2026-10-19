use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing::{debug, info};
use zbus::Connection;

const WETYPE_LUA_EXTENSION: &str = r#"local fcitx = require('fcitx')

function onWeTypeTrigger()
    local path = '/tmp/wetype_commit.txt'
    local f = io.open(path, 'r')
    if f then
        local text = f:read('*a')
        f:close()
        os.remove(path)
        if text and #text > 0 then
            fcitx.commitString(text)
            local lf = io.open('/tmp/wetype_log.txt', 'a')
            if lf then
                lf:write('Committed: ' .. text .. ' | lastCommit=' .. tostring(fcitx.lastCommit()) .. '\n')
                lf:close()
            end
        end
    end
    return false
end

fcitx.watchEvent(fcitx.EventType.InputMethodActivated, 'onWeTypeTrigger')
fcitx.watchEvent(fcitx.EventType.InputMethodDeactivated, 'onWeTypeTrigger')
fcitx.watchEvent(fcitx.EventType.SwitchInputMethod, 'onWeTypeTrigger')
fcitx.watchEvent(fcitx.EventType.FocusIn, 'onWeTypeTrigger')
"#;

/// 与 Fcitx5 交互的 DBus 客户端
pub struct FcitxClient {
    connection: Connection,
}

impl FcitxClient {
    /// 建立与 Session Bus 的连接，确保 Lua 桥接扩展已就绪，并验证 Fcitx5 在线状态
    pub async fn new() -> Result<Self> {
        let connection = Connection::session()
            .await
            .context("Failed to connect to DBus session bus")?;

        let client = Self { connection };
        client.ping().await?;
        client.ensure_lua_extension().await?;
        Ok(client)
    }

    /// 探测 Fcitx5 是否正常运行
    pub async fn ping(&self) -> Result<String> {
        debug!("Pinging Fcitx5 via DBus...");
        let reply: String = self
            .connection
            .call_method(
                Some("org.fcitx.Fcitx5"),
                "/controller",
                Some("org.fcitx.Fcitx.Controller1"),
                "CurrentInputMethod",
                &(),
            )
            .await
            .context("Failed to call Fcitx5 Controller1.CurrentInputMethod (is Fcitx5 running?)")?
            .body()
            .deserialize()?;

        info!("Fcitx5 is alive. Current active IM: {}", reply);
        Ok(reply)
    }

    /// 确保 `~/.local/share/fcitx5/lua/imeapi/extensions/wetype.lua` 已安装并被 Fcitx5 加载
    async fn ensure_lua_extension(&self) -> Result<()> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let ext_dir = PathBuf::from(home).join(".local/share/fcitx5/lua/imeapi/extensions");
        let ext_file = ext_dir.join("wetype.lua");

        let needs_reload = if let Ok(existing) = std::fs::read_to_string(&ext_file) {
            existing != WETYPE_LUA_EXTENSION
        } else {
            true
        };

        if needs_reload {
            std::fs::create_dir_all(&ext_dir)?;
            std::fs::write(&ext_file, WETYPE_LUA_EXTENSION)?;
            info!("Installed Fcitx5 Lua bridge at {:?}", ext_file);

            let _: () = self
                .connection
                .call_method(
                    Some("org.fcitx.Fcitx5"),
                    "/controller",
                    Some("org.fcitx.Fcitx.Controller1"),
                    "ReloadAddonConfig",
                    &("imeapi",),
                )
                .await
                .context("Failed to reload Fcitx5 imeapi addon")?
                .body()
                .deserialize()?;
        }

        Ok(())
    }

    /// 向当前获得焦点的输入框提交文本 (Commit String)
    /// 原生支持 Wayland 与 X11
    pub async fn commit_string(&self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }

        info!("Committing string to Fcitx5: \"{}\"", text);

        // 1. 写入待提交文本至临时信箱
        let commit_path = "/tmp/wetype_commit.txt";
        std::fs::write(commit_path, text.as_bytes())
            .context("Failed to write /tmp/wetype_commit.txt")?;

        // 2. 通过快速双次 Toggle 触发 Fcitx5 内部事件回调并保持原输入法状态不变
        for _ in 0..2 {
            let _: () = self
                .connection
                .call_method(
                    Some("org.fcitx.Fcitx5"),
                    "/controller",
                    Some("org.fcitx.Fcitx.Controller1"),
                    "Toggle",
                    &(),
                )
                .await
                .context("Failed to trigger Fcitx5 Toggle over DBus")?
                .body()
                .deserialize()?;
        }

        // 3. 同步写入系统剪贴板作为兜底备份 (用户也可随时 Ctrl+V)
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(text.to_string());
        }

        debug!("String successfully committed to active Fcitx5 InputContext.");
        Ok(())
    }
}
