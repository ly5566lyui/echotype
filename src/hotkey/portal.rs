//! Wayland 全局热键: XDG Desktop Portal `org.freedesktop.portal.GlobalShortcuts`
//!
//! 流程: CreateSession → BindShortcuts (弹出系统权限确认框) → ListenShortcut
//! → 监听会话上的 Activated 信号分发动作。
//! 需要 xdg-desktop-portal 与支持 GlobalShortcuts 的后端
//! (xdg-desktop-portal-gnome ≥ 46 / -kde / -wlroots / -hyprland 等)。

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;
use tracing::{info, warn};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use super::{HotkeyEvent, ShortcutSpec};

const PORTAL_DEST: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";
const REQUEST_IFACE: &str = "org.freedesktop.portal.Request";

/// 在后台任务中完成 Portal 注册并进入信号监听循环。
/// 注册结果通过日志反馈，失败不影响应用其余功能。
pub fn register(
    specs: Vec<ShortcutSpec>,
    dispatch: Arc<dyn Fn(HotkeyEvent) + Send + Sync>,
    rt: Handle,
) -> tokio::task::JoinHandle<()> {
    rt.spawn(async move {
        if let Err(e) = run_portal(specs, dispatch).await {
            warn!("Portal GlobalShortcuts unavailable: {:#}", e);
        }
    })
}

async fn run_portal(
    specs: Vec<ShortcutSpec>,
    dispatch: Arc<dyn Fn(HotkeyEvent) + Send + Sync>,
) -> Result<()> {
    let conn = zbus::Connection::session()
        .await
        .context("failed to connect to DBus session bus")?;

    let portal = zbus::Proxy::new(&conn, PORTAL_DEST, PORTAL_PATH, PORTAL_IFACE).await?;

    let unique = conn
        .unique_name()
        .context("DBus connection has no unique name")?
        .to_string();
    let sender = unique.trim_start_matches(':').replace('.', "_");
    let token_base = format!(
        "echotype{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
    );
    let request_path = |suffix: &str| {
        format!("/org/freedesktop/portal/desktop/request/{sender}/{token_base}{suffix}")
    };

    // ---- 1. CreateSession (先挂 Response 监听再调用，避免竞态) ----
    // 注意: zbus Proxy<'a> 会借用 path 字符串，须绑定到局部变量避免临时值生命周期问题
    let req1_path = request_path("_session");
    let req1_proxy =
        zbus::Proxy::new(&conn, PORTAL_DEST, req1_path.as_str(), REQUEST_IFACE).await?;
    let mut req1_stream = req1_proxy.receive_signal("Response").await?;

    let mut create_opts: HashMap<&str, Value> = HashMap::new();
    create_opts.insert("handle_token", Value::from(format!("{}_session", token_base)));
    create_opts.insert("session_handle_token", Value::from(token_base.clone()));
    let session_ret: OwnedObjectPath = portal
        .call("CreateSession", &create_opts)
        .await
        .context(
            "portal CreateSession failed (is xdg-desktop-portal with a GlobalShortcuts backend running?)",
        )?;

    let (code, results) = match req1_stream.next().await {
        Some(msg) => msg
            .body()
            .deserialize::<(u32, HashMap<String, OwnedValue>)>()
            .context("failed to parse CreateSession Response")?,
        None => bail!("CreateSession request dropped before response"),
    };
    anyhow::ensure!(code == 0, "CreateSession rejected by portal (code {})", code);

    // 规范中 Response results 的 session_handle 是字符串 (s)，
    // 部分实现也可能返回 ObjectPath (o) —— 两种类型都接受；
    // 注意 CreateSession 的直接返回值是 request 路径，不能当会话句柄用
    let session_path = results
        .get("session_handle")
        .and_then(|v| match &**v {
            Value::Str(s) => Some(s.to_string()),
            Value::ObjectPath(p) => Some(p.to_string()),
            _ => None,
        })
        .filter(|p| p.contains("/session/"))
        .unwrap_or_else(|| session_ret.to_string());
    info!("Portal shortcuts session created: {}", session_path);

    // ---- 2. BindShortcuts (触发桌面权限确认弹窗) ----
    let req2_path = request_path("_bind");
    let req2_proxy =
        zbus::Proxy::new(&conn, PORTAL_DEST, req2_path.as_str(), REQUEST_IFACE).await?;
    let mut req2_stream = req2_proxy.receive_signal("Response").await?;

    let mut binds: Vec<(String, HashMap<&str, Value>)> = Vec::new();
    for spec in &specs {
        let mut props: HashMap<&str, Value> = HashMap::new();
        props.insert("description", Value::from(spec.description.clone()));
        if let Some(trigger) = &spec.portal_trigger {
            props.insert("preferred_trigger", Value::from(trigger.clone()));
        }
        // XDG GlobalShortcuts 规范: shortcuts 为 a(sa{sv}) —— 每项仅 (id, 属性表)
        binds.push((spec.id.clone(), props));
    }

    let session_obj = ObjectPath::try_from(session_path.as_str())
        .context("invalid session object path")?;
    let _bind_ret: OwnedObjectPath = portal
        .call(
            "BindShortcuts",
            &(&session_obj, binds, "", HashMap::<&str, Value>::new()),
        )
        .await
        .context("portal BindShortcuts failed")?;

    // 等待用户在桌面权限弹窗中确认
    if let Some(msg) = req2_stream.next().await {
        let (code, _): (u32, HashMap<String, OwnedValue>) = msg
            .body()
            .deserialize()
            .context("failed to parse BindShortcuts Response")?;
        anyhow::ensure!(
            code == 0,
            "BindShortcuts rejected by user or portal (code {})",
            code
        );
    }
    info!("Portal shortcuts bound ({} shortcuts).", specs.len());

    // ---- 3. ListenShortcut ----
    let _listen_ret: OwnedObjectPath = portal
        .call(
            "ListenShortcut",
            &(&session_obj, HashMap::<&str, Value>::new()),
        )
        .await
        .context("portal ListenShortcut failed")?;

    // ---- 4. Activated / Deactivated 信号循环 (按下与松开，支持按住说话) ----
    let session_proxy =
        zbus::Proxy::new(&conn, PORTAL_DEST, session_path.as_str(), PORTAL_IFACE).await?;
    let mut activated = session_proxy.receive_signal("Activated").await?;
    let mut deactivated = session_proxy.receive_signal("Deactivated").await?;

    let triggers: Vec<String> = specs.iter().map(|s| s.shortcut.clone()).collect();
    info!("Portal global shortcuts active: {:?}", triggers);

    loop {
        tokio::select! {
            msg = activated.next() => match msg {
                Some(m) => handle_shortcut_signal(&m, true, &specs, &dispatch),
                None => break,
            },
            msg = deactivated.next() => match msg {
                Some(m) => handle_shortcut_signal(&m, false, &specs, &dispatch),
                None => break,
            },
        }
    }

    bail!("portal shortcut signal stream ended unexpectedly");
}

/// 解析 Activated/Deactivated 信号并分发 (两者消息体签名一致: (o s t a{sv}))
fn handle_shortcut_signal(
    msg: &zbus::message::Message,
    is_press: bool,
    specs: &[ShortcutSpec],
    dispatch: &Arc<dyn Fn(HotkeyEvent) + Send + Sync>,
) {
    match msg
        .body()
        .deserialize::<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>)>()
    {
        Ok((_, shortcut_id, _, _)) => {
            if let Some(spec) = specs.iter().find(|s| s.id == shortcut_id) {
                dispatch(HotkeyEvent {
                    action: spec.action,
                    is_press,
                });
            }
        }
        Err(e) => warn!("Failed to parse shortcut signal: {}", e),
    }
}
