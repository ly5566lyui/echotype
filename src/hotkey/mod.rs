//! 全局热键模块
//!
//! - X11 / Windows: `global-hotkey` crate (Linux 下基于 XGrabKey)
//! - Wayland: XDG Desktop Portal `org.freedesktop.portal.GlobalShortcuts`
//!   (首次触发时由 xdg-desktop-portal 弹出系统权限确认对话框)
//!
//! 返回的 [`HotkeyHandle`] 必须由调用方持有，句柄 Drop 时 X11 热键自动注销。

mod portal;

use anyhow::{Context, Result};
// 注意: 0.6.x 中 HotKey/Code/Modifiers 位于 hotkey 子模块 (docs.rs 已核实，根导出会编译失败)
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;
use tracing::{info, warn};

/// 热键触发后的动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    /// 语音输入 (开始/停止录音切换，或按住说话)
    Voice,
    /// 常用语 / 剪贴板抽屉
    Clipboard,
}

/// 热键触发事件：动作 + 按键状态
///
/// `is_press`: true = 按下, false = 松开。
/// 关闭"按住说话"时上层通常只响应按下；开启后按下开始录音、松开结束。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyEvent {
    pub action: HotkeyAction,
    pub is_press: bool,
}

/// 一条待注册的快捷键描述
#[derive(Debug, Clone)]
pub struct ShortcutSpec {
    /// 稳定 ID (Portal 注册与信号分发用)
    pub id: String,
    /// 用户可读的快捷键串，如 "Ctrl+Alt+V"
    pub shortcut: String,
    /// Portal 权限弹窗中的动作描述
    pub description: String,
    pub action: HotkeyAction,
    /// GTK 加速器格式 (Portal preferred_trigger)，如 "<Control><Alt>v"
    pub portal_trigger: Option<String>,
}

impl ShortcutSpec {
    pub fn new(id: &str, shortcut: &str, description: &str, action: HotkeyAction) -> Self {
        Self {
            id: id.to_string(),
            shortcut: shortcut.to_string(),
            description: description.to_string(),
            action,
            portal_trigger: gtk_accelerator(shortcut),
        }
    }
}

/// 注册成功后必须持有返回的句柄；Drop 时 X11 热键自动注销
pub struct HotkeyHandle {
    _x11_manager: Option<GlobalHotKeyManager>,
    _portal_task: Option<tokio::task::JoinHandle<()>>,
}

/// 注册全局热键。永不失败：Wayland 走 Portal；X11 失败时自动回退 Portal；
/// 全部失败时仅记录日志 (UI 内按钮仍可用)。
///
/// - `WAYLAND_DISPLAY` 非空时走 Portal GlobalShortcuts；
/// - 否则走 X11 XGrabKey；X11 注册失败时自动回退尝试 Portal。
pub fn register(
    specs: Vec<ShortcutSpec>,
    dispatch: Arc<dyn Fn(HotkeyEvent) + Send + Sync>,
    rt: Handle,
) -> HotkeyHandle {
    let is_wayland = std::env::var("WAYLAND_DISPLAY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);

    if is_wayland {
        info!("Wayland session detected, registering global shortcuts via XDG Portal...");
        let task = portal::register(specs, dispatch, rt);
        return HotkeyHandle {
            _x11_manager: None,
            _portal_task: Some(task),
        };
    }

    match x11_register(specs.clone(), Arc::clone(&dispatch)) {
        Ok(manager) => HotkeyHandle {
            _x11_manager: Some(manager),
            _portal_task: None,
        },
        Err(e) => {
            warn!("X11 hotkey registration failed ({e:#}); falling back to XDG Portal...");
            let task = portal::register(specs, dispatch, rt);
            HotkeyHandle {
                _x11_manager: None,
                _portal_task: Some(task),
            }
        }
    }
}

/// X11 路径：通过 global-hotkey (XGrabKey) 注册，事件回调分发。
/// 以下 API 均已对照 docs.rs 0.6.4 核实:
/// - `register(&self, HotKey) -> Result<()>` (不返回 id，id 从 `HotKey::id()` 获取)
/// - `GlobalHotKeyEvent { id: u32, state: HotKeyState }` 且 `set_event_handler(Some(fn))` 可用
fn x11_register(
    specs: Vec<ShortcutSpec>,
    dispatch: Arc<dyn Fn(HotkeyEvent) + Send + Sync>,
) -> Result<GlobalHotKeyManager> {
    let manager =
        GlobalHotKeyManager::new().context("X11 global hotkey manager unavailable")?;

    let mut lookup: HashMap<u32, HotkeyAction> = HashMap::new();
    for spec in &specs {
        let Some((mods, code)) = parse_shortcut(&spec.shortcut) else {
            warn!("Cannot parse shortcut '{}', skipped.", spec.shortcut);
            continue;
        };
        let hotkey = HotKey::new(Some(mods), code);
        manager
            .register(hotkey)
            .with_context(|| format!("failed to grab hotkey '{}'", spec.shortcut))?;
        lookup.insert(hotkey.id(), spec.action);
        info!("Global hotkey registered: {} -> {:?}", spec.shortcut, spec.action);
    }

    // 事件分发: 用官方回调接口 set_event_handler，避免依赖 receiver 通道的 recv/try_recv
    // 版本差异；回调在事件源线程触发，dispatch 内部自行回到 UI 线程 / tokio 运行时
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if let Some(action) = lookup.get(&event.id) {
            dispatch(HotkeyEvent {
                action: *action,
                is_press: event.state == HotKeyState::Pressed,
            });
        }
    }));

    Ok(manager)
}

/// 解析 "Ctrl+Alt+V" / "Alt+Space" / "Super+V" 形式的快捷键串
fn parse_shortcut(s: &str) -> Option<(Modifiers, Code)> {
    let mut mods = Modifiers::empty();
    let mut key: Option<Code> = None;

    for token in s.split(|c| c == '+' || c == ' ') {
        if token.is_empty() {
            continue;
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "alt" | "option" => mods |= Modifiers::ALT,
            "shift" => mods |= Modifiers::SHIFT,
            "super" | "meta" | "win" | "cmd" => mods |= Modifiers::SUPER,
            other => key = parse_key(other),
        }
    }

    Some((mods, key?))
}

fn parse_key(k: &str) -> Option<Code> {
    // 单字母 / 单数字
    let mut chars = k.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphabetic() {
            return Some(letter_code(c));
        }
        if c.is_ascii_digit() {
            return Some(digit_code(c));
        }
    }
    match k.to_ascii_lowercase().as_str() {
        "space" => Some(Code::Space),
        "enter" | "return" => Some(Code::Enter),
        "tab" => Some(Code::Tab),
        "esc" | "escape" => Some(Code::Escape),
        "backquote" | "`" => Some(Code::Backquote),
        "up" => Some(Code::ArrowUp),
        "down" => Some(Code::ArrowDown),
        "left" => Some(Code::ArrowLeft),
        "right" => Some(Code::ArrowRight),
        "f1" => Some(Code::F1),
        "f2" => Some(Code::F2),
        "f3" => Some(Code::F3),
        "f4" => Some(Code::F4),
        "f5" => Some(Code::F5),
        "f6" => Some(Code::F6),
        "f7" => Some(Code::F7),
        "f8" => Some(Code::F8),
        "f9" => Some(Code::F9),
        "f10" => Some(Code::F10),
        "f11" => Some(Code::F11),
        "f12" => Some(Code::F12),
        _ => None,
    }
}

fn letter_code(c: char) -> Code {
    match c.to_ascii_uppercase() {
        'A' => Code::KeyA,
        'B' => Code::KeyB,
        'C' => Code::KeyC,
        'D' => Code::KeyD,
        'E' => Code::KeyE,
        'F' => Code::KeyF,
        'G' => Code::KeyG,
        'H' => Code::KeyH,
        'I' => Code::KeyI,
        'J' => Code::KeyJ,
        'K' => Code::KeyK,
        'L' => Code::KeyL,
        'M' => Code::KeyM,
        'N' => Code::KeyN,
        'O' => Code::KeyO,
        'P' => Code::KeyP,
        'Q' => Code::KeyQ,
        'R' => Code::KeyR,
        'S' => Code::KeyS,
        'T' => Code::KeyT,
        'U' => Code::KeyU,
        'V' => Code::KeyV,
        'W' => Code::KeyW,
        'X' => Code::KeyX,
        'Y' => Code::KeyY,
        _ => Code::KeyZ,
    }
}

fn digit_code(c: char) -> Code {
    match c {
        '0' => Code::Digit0,
        '1' => Code::Digit1,
        '2' => Code::Digit2,
        '3' => Code::Digit3,
        '4' => Code::Digit4,
        '5' => Code::Digit5,
        '6' => Code::Digit6,
        '7' => Code::Digit7,
        '8' => Code::Digit8,
        _ => Code::Digit9,
    }
}

/// 将 "Ctrl+Alt+V" 转为 Portal 期望的 GTK 加速器串 "<Control><Alt>v"
fn gtk_accelerator(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut key: Option<String> = None;

    for token in s.split(|c| c == '+' || c == ' ').filter(|t| !t.is_empty()) {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => out.push_str("<Control>"),
            "alt" | "option" => out.push_str("<Alt>"),
            "shift" => out.push_str("<Shift>"),
            "super" | "meta" | "win" | "cmd" => out.push_str("<Super>"),
            other => {
                // 字母转小写；Enter 用规范名 Return；F 功能键保持首字母大写 (F1)
                // 注意: 仅用 chars() 迭代判断，避免对多字节 token 做字节切片导致 panic
                let name = if other.chars().count() == 1 {
                    other.to_ascii_lowercase()
                } else if other.eq_ignore_ascii_case("enter")
                    || other.eq_ignore_ascii_case("return")
                {
                    "Return".to_string()
                } else {
                    let rest = other
                        .strip_prefix('f')
                        .or_else(|| other.strip_prefix('F'));
                    match rest {
                        Some(rest)
                            if !rest.is_empty()
                                && rest.chars().all(|c| c.is_ascii_digit()) =>
                        {
                            format!("F{}", rest)
                        }
                        _ => other.to_ascii_lowercase(),
                    }
                };
                key = Some(name);
            }
        }
    }

    Some(format!("{}{}", out, key?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ctrl_alt_v() {
        let (mods, code) = parse_shortcut("Ctrl+Alt+V").unwrap();
        assert!(mods.contains(Modifiers::CONTROL));
        assert!(mods.contains(Modifiers::ALT));
        assert_eq!(code, Code::KeyV);
    }

    #[test]
    fn parses_alt_space() {
        let (mods, code) = parse_shortcut("Alt+Space").unwrap();
        assert!(mods.contains(Modifiers::ALT));
        assert_eq!(code, Code::Space);
    }

    #[test]
    fn parses_super_v() {
        let (mods, code) = parse_shortcut("Super+V").unwrap();
        assert!(mods.contains(Modifiers::SUPER));
        assert_eq!(code, Code::KeyV);
    }

    #[test]
    fn gtk_accelerator_format() {
        assert_eq!(
            gtk_accelerator("Ctrl+Alt+V").as_deref(),
            Some("<Control><Alt>v")
        );
        assert_eq!(gtk_accelerator("Alt+Space").as_deref(), Some("<Alt>space"));
        assert_eq!(
            gtk_accelerator("Super+F2").as_deref(),
            Some("<Super>F2")
        );
    }
}
