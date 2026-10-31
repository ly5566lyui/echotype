use crate::asr::recorder::{AudioRecorder, MAX_RECORD_SECS};
use crate::asr::{AsrEngine, HybridAsrEngine};
use crate::clipboard::sync_p2p::P2pSyncManager;
use crate::clipboard::{ClipboardMonitor, ClipboardStorage};
use crate::config::AppConfig;
use crate::{fcitx, hotkey};
use anyhow::Result;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::runtime::Handle;
use tracing::{debug, error, info, warn};

slint::include_modules!();

/// 胶囊默认副标题 (随设置中的快捷键变化)
fn default_subtitle(shortcut: &str) -> SharedString {
    format!("按 {} 快捷语音 · 纯 Rust 原生", shortcut).into()
}

pub fn run_pill_ui(config: AppConfig, config_path: PathBuf, rt: Handle) -> Result<()> {
    info!("Initializing WeChat-Keyboard floating pill UI (Pure Rust Slint)...");
    let pill = FloatingPill::new()?;

    // 共享配置: 设置窗口保存后即时更新，拖拽开关 / 自动隐藏等运行时行为从这里读取
    let shared: Arc<Mutex<AppConfig>> = Arc::new(Mutex::new(config));
    let voice_shortcut = shared.lock().unwrap().general.voice_shortcut.clone();

    // 初始化 ASR 引擎
    let engine = {
        let cfg = shared.lock().unwrap();
        Arc::new(HybridAsrEngine::from_config(&cfg.asr).unwrap_or_else(|e| {
            panic!("Failed to initialize ASR engine: {:#}", e)
        }))
    };

    // 录音机与状态
    let recorder: Arc<Mutex<Option<AudioRecorder>>> = Arc::new(Mutex::new(None));
    let start_time: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));

    // 本地存储 (XDG 数据目录): 剪贴板历史与常用语
    let data_dir = AppConfig::default_data_dir();
    let _ = std::fs::create_dir_all(&data_dir);
    let storage: Arc<Option<ClipboardStorage>> =
        Arc::new(ClipboardStorage::new(data_dir.join("storage.db")).ok());

    // 常用语初始列表: 设置库 > 剪贴板历史 > 内置默认
    let default_phrases = vec![
        "收到，马上处理！".to_string(),
        "好的，没问题。".to_string(),
        "辛苦了，非常感谢！".to_string(),
        "请稍等片刻，马上给您回复。".to_string(),
        "微信键盘 Linux 极简模式体验中。".to_string(),
    ];
    let initial_phrases: Vec<SharedString> = {
        let stored: Option<Vec<String>> = match storage.as_ref() {
            Some(st) => st
                .get_quick_phrases(10)
                .ok()
                .filter(|v| !v.is_empty())
                .or_else(|| st.get_recent_history(5).ok().filter(|v| !v.is_empty())),
            None => None,
        };
        match stored {
            Some(list) => list.into_iter().map(SharedString::from).collect(),
            None => default_phrases.into_iter().map(SharedString::from).collect(),
        }
    };
    pill.set_quick_phrases(ModelRc::new(VecModel::from(initial_phrases)));

    // 1. 录音开关回调
    {
        let pill_weak = pill.as_weak();
        let recorder = Arc::clone(&recorder);
        let start_time = Arc::clone(&start_time);
        let engine = Arc::clone(&engine);
        let shared_flow = Arc::clone(&shared);
        let voice_sc = voice_shortcut.clone();
        let rt_handle = rt.clone();

        pill.on_toggle_recording(move || {
            let pill_ui = match pill_weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };

            let is_recording = pill_ui.get_is_recording();
            if is_recording {
                // 停止录音 -> 转入识别
                info!("Stopping voice recording, processing ASR...");
                pill_ui.set_is_recording(false);
                pill_ui.set_is_processing(true);
                *start_time.lock().unwrap() = None;

                let audio_data = {
                    let rec_guard = recorder.lock().unwrap();
                    if let Some(ref rec) = *rec_guard {
                        rec.stop()
                    } else {
                        Vec::new()
                    }
                };

                let pill_weak_async = pill_weak.clone();
                let engine_clone = Arc::clone(&engine);
                let shared_async = Arc::clone(&shared_flow);
                let voice_sc_async = voice_sc.clone();

                rt_handle.spawn(async move {
                    if audio_data.is_empty() {
                        warn!("Captured audio buffer is empty!");
                        let _ = pill_weak_async.upgrade_in_event_loop(|ui| {
                            ui.set_is_processing(false);
                            ui.set_title_text("未检测到有效声音".into());
                            ui.set_subtitle_text("请重试".into());
                        });
                        return;
                    }

                    info!(
                        "Running speech recognition ({} samples)...",
                        audio_data.len()
                    );
                    match engine_clone.recognize(&audio_data).await {
                        Ok(text) => {
                            info!("ASR recognition result: \"{}\"", text);
                            if !text.is_empty() {
                                // 提交文本到 Fcitx5 激活窗口 (复用共享连接)
                                match fcitx::commit_string_cached(&text).await {
                                    Ok(()) => {
                                        info!("Successfully committed recognized text to Fcitx5!")
                                    }
                                    Err(e) => {
                                        error!("Failed to commit string to Fcitx5: {:#}", e)
                                    }
                                }

                                let text_display = text.clone();
                                let _ = pill_weak_async.upgrade_in_event_loop(move |ui| {
                                    ui.set_is_processing(false);
                                    ui.set_title_text(format!("已上屏: {}", text_display).into());
                                    ui.set_subtitle_text("识别成功 ✓".into());
                                });
                            } else {
                                let _ = pill_weak_async.upgrade_in_event_loop(|ui| {
                                    ui.set_is_processing(false);
                                    ui.set_title_text("未识别到文字".into());
                                    ui.set_subtitle_text("说话声音太小或未包含语音".into());
                                });
                            }

                            // 3 秒后: 按设置决定自动隐藏或复原默认文案
                            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                            let auto_hide = shared_async
                                .lock()
                                .map(|g| g.general.auto_hide_after_commit)
                                .unwrap_or(false);
                            let _ = pill_weak_async.upgrade_in_event_loop(move |ui| {
                                if auto_hide {
                                    let _ = ui.window().hide();
                                }
                                ui.set_title_text("微信键盘 Linux".into());
                                ui.set_subtitle_text(default_subtitle(&voice_sc_async));
                            });
                        }
                        Err(e) => {
                            error!("ASR recognition failed: {:?}", e);
                            let _ = pill_weak_async.upgrade_in_event_loop(|ui| {
                                ui.set_is_processing(false);
                                ui.set_title_text("识别异常".into());
                                ui.set_subtitle_text("请检查 ASR 引擎或网络".into());
                            });
                        }
                    }
                });
            } else {
                // 开始录音
                info!("Starting voice recording...");
                let rec = AudioRecorder::new();
                if let Err(e) = rec.start() {
                    error!("Failed to start audio recorder: {:?}", e);
                    pill_ui.set_title_text("录音设备启动失败".into());
                    pill_ui.set_subtitle_text(format!("{:?}", e).into());
                    return;
                }

                *recorder.lock().unwrap() = Some(rec);
                *start_time.lock().unwrap() = Some(Instant::now());
                pill_ui.set_is_recording(true);
                pill_ui.set_duration_text("00:00".into());

                // 录音时长保护: 超过上限自动触发一次停止 (复用正常停止/识别流程)
                {
                    let pill_wd = pill_weak.clone();
                    rt_handle.spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(MAX_RECORD_SECS)).await;
                        let _ = pill_wd.upgrade_in_event_loop(|ui| {
                            if ui.get_is_recording() {
                                ui.invoke_toggle_recording();
                            }
                        });
                    });
                }

                // 启动计时器更新录音时长
                let pill_weak_timer = pill_weak.clone();
                let start_time_timer = Arc::clone(&start_time);

                rt_handle.spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        let elapsed = {
                            let guard = start_time_timer.lock().unwrap();
                            match *guard {
                                Some(t) => t.elapsed().as_secs(),
                                None => break, // 录音已结束
                            }
                        };

                        let minutes = elapsed / 60;
                        let seconds = elapsed % 60;
                        let time_str = format!("{:02}:{:02}", minutes, seconds);

                        let _ = pill_weak_timer.upgrade_in_event_loop(move |ui| {
                            if ui.get_is_recording() {
                                ui.set_duration_text(time_str.into());
                            }
                        });
                    }
                });
            }
        });
    }

    // 2. 常用语/剪贴板抽屉展开/收起回调
    {
        let pill_weak = pill.as_weak();
        let storage_drawer = Arc::clone(&storage);

        pill.on_toggle_drawer(move || {
            let pill_ui = match pill_weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };

            let current = pill_ui.get_show_drawer();
            let new_state = !current;
            pill_ui.set_show_drawer(new_state);

            if new_state {
                // 打开时刷新剪贴板与常用语
                if let Some(st) = storage_drawer.as_ref() {
                    if let Ok(history) = st.get_recent_history(5) {
                        if !history.is_empty() {
                            let items: Vec<SharedString> =
                                history.into_iter().map(SharedString::from).collect();
                            pill_ui.set_clipboard_items(ModelRc::new(VecModel::from(items)));
                        }
                    }
                }
            }
        });
    }

    // 3. 标签切换回调
    {
        let pill_weak = pill.as_weak();
        pill.on_switch_tab(move |tab_idx: i32| {
            if let Some(ui) = pill_weak.upgrade() {
                ui.set_active_tab(tab_idx);
            }
        });
    }

    // 4. 点击选中常用语回调
    {
        let pill_weak = pill.as_weak();
        let shared_sel = Arc::clone(&shared);
        let voice_sc_sel = voice_shortcut.clone();
        let rt_handle = rt.clone();

        pill.on_select_phrase(move |phrase: SharedString| {
            info!("Selected quick phrase: \"{}\"", phrase);
            let phrase_str = phrase.to_string();
            let pill_weak_async = pill_weak.clone();
            let shared_async = Arc::clone(&shared_sel);
            let voice_sc_async = voice_sc_sel.clone();

            rt_handle.spawn(async move {
                match fcitx::commit_string_cached(&phrase_str).await {
                    Ok(()) => info!("Successfully committed quick phrase to active window!"),
                    Err(e) => error!("Failed to commit quick phrase: {:#}", e),
                }

                let _ = pill_weak_async.upgrade_in_event_loop(move |ui| {
                    ui.set_title_text("常用语已上屏 ✓".into());
                    ui.set_subtitle_text(phrase_str.into());
                });

                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let auto_hide = shared_async
                    .lock()
                    .map(|g| g.general.auto_hide_after_commit)
                    .unwrap_or(false);
                let _ = pill_weak_async.upgrade_in_event_loop(move |ui| {
                    if auto_hide {
                        let _ = ui.window().hide();
                    }
                    ui.set_title_text("微信键盘 Linux".into());
                    ui.set_subtitle_text(default_subtitle(&voice_sc_async));
                });
            });
        });
    }

    // 5. 点击选中剪贴板历史回调
    {
        let pill_weak = pill.as_weak();
        let shared_sel = Arc::clone(&shared);
        let voice_sc_sel = voice_shortcut.clone();
        let rt_handle = rt.clone();

        pill.on_select_clipboard(move |clip: SharedString| {
            info!("Selected clipboard item: \"{}\"", clip);
            let clip_str = clip.to_string();
            let pill_weak_async = pill_weak.clone();
            let shared_async = Arc::clone(&shared_sel);
            let voice_sc_async = voice_sc_sel.clone();

            rt_handle.spawn(async move {
                match fcitx::commit_string_cached(&clip_str).await {
                    Ok(()) => info!("Successfully committed clipboard item to active window!"),
                    Err(e) => error!("Failed to commit clipboard item: {:#}", e),
                }

                let _ = pill_weak_async.upgrade_in_event_loop(move |ui| {
                    ui.set_title_text("剪贴板内容已粘贴 ✓".into());
                    ui.set_subtitle_text(clip_str.into());
                });

                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let auto_hide = shared_async
                    .lock()
                    .map(|g| g.general.auto_hide_after_commit)
                    .unwrap_or(false);
                let _ = pill_weak_async.upgrade_in_event_loop(move |ui| {
                    if auto_hide {
                        let _ = ui.window().hide();
                    }
                    ui.set_title_text("微信键盘 Linux".into());
                    ui.set_subtitle_text(default_subtitle(&voice_sc_async));
                });
            });
        });
    }

    // 6. 快捷功能坞 8 格工具点击回调
    {
        let pill_weak = pill.as_weak();
        let rt_handle = rt.clone();

        pill.on_click_tool(move |tool_idx: i32| {
            let pill_ui = match pill_weak.upgrade() {
                Some(ui) => ui,
                None => return,
            };

            match tool_idx {
                0 => {
                    // 语音转文字
                    pill_ui.invoke_toggle_recording();
                }
                1 => {
                    // 快捷表情
                    let pill_weak_async = pill_weak.clone();
                    rt_handle.spawn(async move {
                        let _ = fcitx::commit_string_cached("😄").await;
                        let _ = pill_weak_async.upgrade_in_event_loop(|ui| {
                            ui.set_title_text("表情已输入 😄".into());
                            ui.set_subtitle_text("微信键盘快捷表情".into());
                        });
                    });
                }
                2 => {
                    // 键盘选择
                    pill_ui.set_title_text("当前输入法: Fcitx5".into());
                    pill_ui.set_subtitle_text("拼音 / 双拼 / 五笔 全键盘支持".into());
                }
                3 => {
                    // 边写边译
                    pill_ui.set_title_text("边写边译模式".into());
                    pill_ui.set_subtitle_text("输入中文即可自动翻译并上屏英文".into());
                }
                4 => {
                    // 切换到剪贴板 Tab
                    pill_ui.set_active_tab(1);
                }
                5 => {
                    // 切换到常用语 Tab
                    pill_ui.set_active_tab(0);
                }
                6 => {
                    // 问 AI
                    pill_ui.set_title_text("微信键盘 智能AI".into());
                    pill_ui.set_subtitle_text("端侧/云端大模型协同助理准备就绪".into());
                }
                7 => {
                    // 智能表达
                    pill_ui.set_title_text("智能表达 / 文本润色".into());
                    pill_ui.set_subtitle_text("支持错别字校正与生动表达建议".into());
                }
                _ => {}
            }
        });
    }

    // 7. 窗口拖拽移动回调 (受设置中的 allow_drag 开关控制)
    {
        let pill_weak = pill.as_weak();
        let shared_drag = Arc::clone(&shared);

        pill.on_drag_move(move |dx, dy| {
            let allow = shared_drag
                .lock()
                .map(|g| g.general.allow_drag)
                .unwrap_or(true);
            if !allow {
                return;
            }
            if let Some(p) = pill_weak.upgrade() {
                let win = p.window();
                let cur = win.position();
                win.set_position(slint::PhysicalPosition::new(
                    cur.x + dx as i32,
                    cur.y + dy as i32,
                ));
            }
        });
    }

    // 8. 右键菜单: 打开独立设置中心窗口
    let settings_instance: Arc<Mutex<Option<SettingsWindow>>> = Arc::new(Mutex::new(None));
    {
        let settings_ref = settings_instance.clone();
        let shared_cfg = Arc::clone(&shared);
        let cfg_path = config_path.clone();
        let storage_ref = Arc::clone(&storage);
        let pill_weak = pill.as_weak();

        pill.on_open_settings(move || {
            info!("Opening WeChat-Keyboard Settings Window...");
            if settings_ref.lock().unwrap().is_none() {
                if let Err(e) = open_settings_window(
                    &settings_ref,
                    &shared_cfg,
                    &cfg_path,
                    &storage_ref,
                    pill_weak.clone(),
                ) {
                    error!("Failed to open settings window: {:#}", e);
                    return;
                }
            }
            if let Some(win) = settings_ref.lock().unwrap().as_ref() {
                let _ = win.show();
            }
        });
    }

    // 9. 右键菜单: 简繁切换
    {
        let pill_weak = pill.as_weak();
        pill.on_toggle_traditional(move || {
            if let Some(p) = pill_weak.upgrade() {
                p.set_title_text("微信键盘 (繁体中文)".into());
                p.set_subtitle_text("已切换至繁体中文输入模式".into());
            }
        });
    }

    // 10. 右键菜单: 最小化到后台待命
    {
        let pill_weak = pill.as_weak();
        pill.on_minimize_to_tray(move || {
            if let Some(p) = pill_weak.upgrade() {
                info!("WeType floating pill minimized to background.");
                let _ = p.window().hide();
            }
        });
    }

    // 11. 右键菜单: 彻底退出程序
    {
        pill.on_quit_app(move || {
            info!("Quitting WeChat-Keyboard daemon completely...");
            let _ = slint::quit_event_loop();
        });
    }

    // 12. 药丸右上角 ✕ 按钮: 最小化到后台 (不杀进程)
    {
        let pill_weak = pill.as_weak();
        pill.on_close_window(move || {
            if let Some(p) = pill_weak.upgrade() {
                info!("Close button clicked -> Minimizing floating pill to background...");
                let _ = p.window().hide();
            }
        });
    }

    // 13. 全局热键: 语音 (开始/停止) 与常用语/剪贴板抽屉；隐藏状态下先唤起窗口
    let _hotkey_handle = {
        let (voice_sc, clip_sc) = {
            let cfg = shared.lock().unwrap();
            (
                cfg.general.voice_shortcut.clone(),
                cfg.general.clipboard_shortcut.clone(),
            )
        };
        let specs = vec![
            hotkey::ShortcutSpec::new(
                "wetype.voice",
                &voice_sc,
                "WeType 语音输入",
                hotkey::HotkeyAction::Voice,
            ),
            hotkey::ShortcutSpec::new(
                "wetype.clipboard",
                &clip_sc,
                "WeType 常用语 / 剪贴板历史",
                hotkey::HotkeyAction::Clipboard,
            ),
        ];
        let pill_weak = pill.as_weak();
        let dispatch: Arc<dyn Fn(hotkey::HotkeyAction) + Send + Sync> = Arc::new(move |action| {
            let pw = pill_weak.clone();
            let _ = pw.upgrade_in_event_loop(move |ui| match action {
                hotkey::HotkeyAction::Voice => {
                    let _ = ui.window().show();
                    ui.invoke_toggle_recording();
                }
                hotkey::HotkeyAction::Clipboard => {
                    let _ = ui.window().show();
                    ui.invoke_toggle_drawer();
                }
            });
        });
        info!(
            "Registering global hotkeys: voice={}, clipboard={}",
            voice_sc, clip_sc
        );
        hotkey::register(specs, dispatch, rt.clone())
    };

    // 14. 后台任务: 剪贴板监听 (历史入库, 敏感过滤 + 上限裁剪) + 局域网 P2P 同步广播
    {
        let rt_bg = rt.clone();
        let storage_bg = Arc::clone(&storage);
        let (p2p_cfg, ignore_sensitive, max_history) = {
            let cfg = shared.lock().unwrap();
            (
                cfg.p2p_sync.clone(),
                cfg.clipboard.ignore_sensitive,
                cfg.clipboard.max_history_items,
            )
        };

        rt_bg.spawn(async move {
            let p2p: Option<Arc<P2pSyncManager>> = if p2p_cfg.enabled {
                match P2pSyncManager::start(&p2p_cfg).await {
                    Ok(manager) => Some(manager),
                    Err(e) => {
                        warn!("P2P sync disabled: {:#}", e);
                        None
                    }
                }
            } else {
                None
            };

            if p2p.is_some() || storage_bg.is_some() {
                let monitor = ClipboardMonitor::new().with_sensitive_filter(ignore_sensitive);
                let st = Arc::clone(&storage_bg);
                if let Err(e) = monitor.start(move |text| {
                    if let Some(st) = st.as_ref() {
                        if let Err(e) = st.insert_history(&text) {
                            tracing::debug!("Failed to store clipboard history: {:#}", e);
                        }
                        let _ = st.trim_history(max_history);
                    }
                    if let Some(p2p) = p2p.as_ref() {
                        p2p.broadcast(&text);
                    }
                }) {
                    warn!("Clipboard monitor failed to start: {:#}", e);
                }
            }
        });
    }

    // 根据配置设置提示文字中的快捷键
    pill.set_subtitle_text(default_subtitle(&voice_shortcut));

    // 支持环境变量控制初始展开抽屉与指定 Tab（便于视觉测试与快捷启动）
    if std::env::var("WETYPE_EXPANDED").is_ok() {
        pill.set_show_drawer(true);
        if let Ok(tab_str) = std::env::var("WETYPE_TAB") {
            if let Ok(tab_idx) = tab_str.parse::<i32>() {
                pill.set_active_tab(tab_idx);
            }
        }
    }

    if std::env::var("WETYPE_CONTEXT_MENU").is_ok() {
        pill.set_show_context_menu(true);
    }

    if std::env::var("WETYPE_SETTINGS").is_ok() {
        info!("Opening Settings Window via WETYPE_SETTINGS flag...");
        if settings_instance.lock().unwrap().is_none() {
            if let Err(e) = open_settings_window(
                &settings_instance,
                &shared,
                &config_path,
                &storage,
                pill.as_weak(),
            ) {
                error!("Failed to open settings window: {:#}", e);
            }
        }
        if let Some(win) = settings_instance.lock().unwrap().as_ref() {
            let _ = win.show();
        }
    }

    // 支持后台静默启动
    let start_hidden = shared.lock().unwrap().general.start_hidden;
    if start_hidden || std::env::var("WETYPE_HIDDEN").is_ok() {
        info!("Starting WeType-Linux in background daemon mode (window hidden)...");
        let _ = pill.window().hide();
    }

    info!("Displaying WeType-Linux floating pill...");
    pill.run()?;
    Ok(())
}

/// 创建设置中心窗口并完成全部接线:
/// 初始属性装载 (来自共享配置) -> 交互回调 (改窗口内属性) -> 保存回调 (落盘 config.toml)
fn open_settings_window(
    settings_ref: &Arc<Mutex<Option<SettingsWindow>>>,
    shared: &Arc<Mutex<AppConfig>>,
    config_path: &PathBuf,
    storage: &Arc<Option<ClipboardStorage>>,
    pill_weak: slint::Weak<FloatingPill>,
) -> Result<()> {
    let win = SettingsWindow::new()?;
    let cfg = shared.lock().unwrap().clone();

    // 装载当前配置
    win.set_start_hidden(cfg.general.start_hidden);
    win.set_auto_hide(cfg.general.auto_hide_after_commit);
    win.set_allow_drag(cfg.general.allow_drag);
    win.set_voice_shortcut(cfg.general.voice_shortcut.clone().into());
    win.set_theme_mode(cfg.general.ui_theme);
    win.set_asr_lang(cfg.asr.language);
    win.set_auto_punctuation(cfg.asr.auto_punctuation);
    win.set_candidate_count(cfg.general.candidate_count);

    // 常用语列表: 库里有数据用库，否则保留 slint 内置默认
    let stored: Option<Vec<String>> = match storage.as_ref() {
        Some(st) => st.get_quick_phrases(20).ok(),
        None => None,
    };
    let phrases: Vec<SharedString> = match stored {
        Some(v) if !v.is_empty() => v.into_iter().map(SharedString::from).collect(),
        _ => win
            .get_phrase_list()
            .iter()
            .map(|s| s.to_string().into())
            .collect(),
    };
    win.set_phrase_list(ModelRc::new(VecModel::from(phrases)));

    // 关闭窗口
    {
        let weak = win.as_weak();
        win.on_close_settings(move || {
            if let Some(sw) = weak.upgrade() {
                let _ = sw.window().hide();
            }
        });
    }

    // 交互回调: 仅更新窗口内属性，点「保存」时统一落盘
    {
        let weak = win.as_weak();
        win.on_set_shortcut(move |s| {
            if let Some(sw) = weak.upgrade() {
                sw.set_voice_shortcut(s);
            }
        });
    }
    {
        let weak = win.as_weak();
        win.on_toggle_start_hidden(move || {
            if let Some(sw) = weak.upgrade() {
                sw.set_start_hidden(!sw.get_start_hidden());
            }
        });
    }
    {
        let weak = win.as_weak();
        win.on_toggle_auto_hide(move || {
            if let Some(sw) = weak.upgrade() {
                sw.set_auto_hide(!sw.get_auto_hide());
            }
        });
    }
    {
        let weak = win.as_weak();
        win.on_toggle_allow_drag(move || {
            if let Some(sw) = weak.upgrade() {
                sw.set_allow_drag(!sw.get_allow_drag());
            }
        });
    }
    {
        let weak = win.as_weak();
        win.on_set_theme(move |mode| {
            if let Some(sw) = weak.upgrade() {
                sw.set_theme_mode(mode);
            }
        });
    }
    {
        let weak = win.as_weak();
        win.on_set_lang(move |lang| {
            if let Some(sw) = weak.upgrade() {
                sw.set_asr_lang(lang);
            }
        });
    }

    // 保存: 读取窗口属性 -> 更新共享配置 -> 持久化 -> 应用可即时生效的部分
    {
        let weak = win.as_weak();
        let shared = Arc::clone(shared);
        let config_path = config_path.clone();
        let storage = Arc::clone(storage);
        let pill_weak = pill_weak.clone();

        win.on_save_settings(move || {
            let Some(sw) = weak.upgrade() else { return };
            let voice_shortcut = sw.get_voice_shortcut().to_string();
            let start_hidden = sw.get_start_hidden();
            let auto_hide = sw.get_auto_hide();
            let allow_drag = sw.get_allow_drag();
            let ui_theme = sw.get_theme_mode();
            let asr_lang = sw.get_asr_lang();
            let auto_punctuation = sw.get_auto_punctuation();
            let candidate_count = sw.get_candidate_count();
            let phrases: Vec<String> =
                sw.get_phrase_list().iter().map(|s| s.to_string()).collect();

            let mut saved_ok = false;
            {
                let mut cfg = shared.lock().unwrap();
                cfg.general.voice_shortcut = voice_shortcut.clone();
                cfg.general.start_hidden = start_hidden;
                cfg.general.auto_hide_after_commit = auto_hide;
                cfg.general.allow_drag = allow_drag;
                cfg.general.ui_theme = ui_theme;
                cfg.general.candidate_count = candidate_count;
                cfg.asr.language = asr_lang;
                cfg.asr.auto_punctuation = auto_punctuation;

                match cfg.save_to_path(&config_path) {
                    Ok(()) => saved_ok = true,
                    Err(e) => {
                        error!("Failed to save settings: {:#}", e);
                        sw.set_status_text(format!("保存失败: {e}").into());
                    }
                }
            }

            if saved_ok {
                info!("Settings saved to {:?}", config_path);
                if let Some(st) = storage.as_ref() {
                    if let Err(e) = st.replace_quick_phrases(&phrases) {
                        warn!("Failed to update quick phrases: {:#}", e);
                    }
                }
                if let Err(e) = fcitx::apply_candidate_count(candidate_count) {
                    debug!("Failed to write Fcitx5 candidate count: {:#}", e);
                }
                // 胶囊副标题中的快捷键即时刷新
                if let Some(pill) = pill_weak.upgrade() {
                    pill.set_subtitle_text(default_subtitle(&voice_shortcut));
                }
                sw.set_status_text("设置已保存 ✓ (快捷键需重启应用后生效)".into());
            }
        });
    }

    // 接管句柄并显示
    {
        let mut guard = settings_ref.lock().unwrap();
        *guard = Some(win);
    }
    if let Some(w) = settings_ref.lock().unwrap().as_ref() {
        let _ = w.show();
    }
    Ok(())
}
