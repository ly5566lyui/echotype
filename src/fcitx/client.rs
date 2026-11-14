use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::{debug, info, warn};
use zbus::Connection;

const ECHOTYPE_LUA_EXTENSION: &str = r#"local fcitx = require('fcitx')

function onEchoTypeTrigger()
    local path = '/tmp/echotype_commit.txt'
    local f = io.open(path, 'r')
    if f then
        local text = f:read('*a')
        f:close()
        os.remove(path)
        if text and #text > 0 then
            fcitx.commitString(text)
        end
    end
    return false
end

fcitx.watchEvent(fcitx.EventType.InputMethodActivated, 'onEchoTypeTrigger')
fcitx.watchEvent(fcitx.EventType.InputMethodDeactivated, 'onEchoTypeTrigger')
fcitx.watchEvent(fcitx.EventType.SwitchInputMethod, 'onEchoTypeTrigger')
fcitx.watchEvent(fcitx.EventType.FocusIn, 'onEchoTypeTrigger')
"#;

/// 全进程共享的 Fcitx5 客户端 (避免每次上屏都重建 DBus 连接并检查 Lua 扩展)
static SHARED_CLIENT: Mutex<Option<FcitxClient>> = Mutex::new(None);

/// 与 Fcitx5 交互的 DBus 客户端
#[derive(Clone)]
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

    /// 确保 `~/.local/share/fcitx5/lua/imeapi/extensions/echotype.lua` 已安装并被 Fcitx5 加载
    async fn ensure_lua_extension(&self) -> Result<()> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let ext_dir = PathBuf::from(home).join(".local/share/fcitx5/lua/imeapi/extensions");
        let ext_file = ext_dir.join("echotype.lua");

        let needs_reload = if let Ok(existing) = std::fs::read_to_string(&ext_file) {
            existing != ECHOTYPE_LUA_EXTENSION
        } else {
            true
        };

        if needs_reload {
            std::fs::create_dir_all(&ext_dir)?;
            std::fs::write(&ext_file, ECHOTYPE_LUA_EXTENSION)?;
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

        // 1. 写入待提交文本至临时信箱 (权限收紧为 0600，避免同机其他用户读取)
        let commit_path = "/tmp/echotype_commit.txt";
        std::fs::write(commit_path, text.as_bytes())
            .context("Failed to write /tmp/echotype_commit.txt")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                commit_path,
                std::fs::Permissions::from_mode(0o600),
            );
        }

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

/// 预热共享 Fcitx5 连接 (daemon/GUI 启动时调用；不可用仅告警，不阻断)
pub async fn warm_up() -> Result<()> {
    if SHARED_CLIENT.lock().unwrap().is_some() {
        return Ok(());
    }
    let client = FcitxClient::new().await?;
    *SHARED_CLIENT.lock().unwrap() = Some(client);
    Ok(())
}

/// 复用共享 Fcitx5 连接提交文本；连接失效 (如 Fcitx5 重启) 时自动重连。
/// 全应用上屏统一走此入口，避免每次提交都重新建立 DBus 连接。
pub async fn commit_string_cached(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let cached = SHARED_CLIENT.lock().unwrap().clone();
    if let Some(client) = cached {
        match client.commit_string(text).await {
            Ok(()) => return Ok(()),
            Err(e) => warn!("Cached Fcitx5 connection failed ({e:#}), reconnecting..."),
        }
    }

    let client = FcitxClient::new().await?;
    let result = client.commit_string(text).await;
    if result.is_ok() {
        *SHARED_CLIENT.lock().unwrap() = Some(client);
    }
    result
}

/// 将候选词数量 best-effort 写入 Fcitx5 中文拼音引擎配置
/// (`~/.config/fcitx5/conf/pinyin.conf` 的 `PageSize`)。
/// 若该配置不存在则创建；需重启 Fcitx5 后生效。失败不影响主流程。
#[allow(dead_code)] // 设置中心已移除，保留供 config.toml 用户与后续 UI 使用
pub fn apply_candidate_count(count: i32) -> Result<()> {
    apply_conf_key(
        &PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
            .join(".config/fcitx5/conf")
            .join("pinyin.conf"),
        "PageSize",
        &count.to_string(),
    )
}

/// 将候选词字号写入 Fcitx5 经典界面配置 (`~/.config/fcitx5/conf/classicui.conf`
/// 的 `Font="Noto Sans CJK SC {size}"`)。需重启 Fcitx5 后生效。失败不影响主流程。
#[allow(dead_code)] // 设置中心已移除，保留供 config.toml 用户与后续 UI 使用
pub fn apply_candidate_font_size(size: i32) -> Result<()> {
    apply_conf_key(
        &PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
            .join(".config/fcitx5/conf")
            .join("classicui.conf"),
        "Font",
        &format!("\"Noto Sans CJK SC {}\"", size),
    )
}

/// 通用 fcitx5 conf 键写入: 替换已有键值，不存在则追加 (保留其余行)
fn apply_conf_key(conf_file: &Path, key: &str, value: &str) -> Result<()> {
    if let Some(parent) = conf_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = std::fs::read_to_string(conf_file).unwrap_or_default();
    let mut out = String::new();
    let mut replaced = false;
    for line in content.lines() {
        let is_target = match line.split_once('=') {
            Some((k, _)) => k.trim() == key,
            None => false,
        };
        if is_target {
            out.push_str(&format!("{}={}\n", key, value));
            replaced = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !replaced {
        out.push_str(&format!("{}={}\n", key, value));
    }
    std::fs::write(conf_file, out)?;
    info!("Fcitx5 conf updated: {} = {} in {:?}", key, value, conf_file);
    Ok(())
}
