use crate::asr::recorder::AudioRecorder;
use crate::asr::{AsrEngine, HybridAsrEngine};
use crate::clipboard::ClipboardStorage;
use crate::config::AppConfig;
use crate::fcitx::FcitxClient;
use anyhow::Result;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::runtime::Handle;
use tracing::{error, info, warn};

slint::include_modules!();

pub fn run_pill_ui(config: AppConfig, rt: Handle) -> Result<()> {
    info!("Initializing WeChat-Keyboard floating pill UI (Pure Rust Slint)...");
    let pill = FloatingPill::new()?;

    // 初始化 ASR 引擎
    let engine = Arc::new(
        HybridAsrEngine::from_config(&config.asr)
            .unwrap_or_else(|e| panic!("Failed to initialize ASR engine: {:?}", e)),
    );

    // 录音机与状态
    let recorder: Arc<Mutex<Option<AudioRecorder>>> = Arc::new(Mutex::new(None));
    let start_time: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));

    // 加载快捷常用语
    let default_phrases = vec![
        "收到，马上处理！".to_string(),
        "好的，没问题。".to_string(),
        "辛苦了，非常感谢！".to_string(),
        "请稍等片刻，马上给您回复。".to_string(),
        "微信键盘 Linux 极简模式体验中。".to_string(),
    ];

    let data_dir = PathBuf::from("data");
    let _ = std::fs::create_dir_all(&data_dir);
    let storage = Arc::new(
        ClipboardStorage::new(data_dir.join("storage.db")).ok()
    );

    let initial_phrases: Vec<SharedString> = if let Some(ref st) = *storage {
        if let Ok(history) = st.get_recent_history(5) {
            if !history.is_empty() {
                history.into_iter().map(SharedString::from).collect()
            } else {
                default_phrases.into_iter().map(SharedString::from).collect()
            }
        } else {
            default_phrases.into_iter().map(SharedString::from).collect()
        }
    } else {
        default_phrases.into_iter().map(SharedString::from).collect()
    };

    pill.set_quick_phrases(ModelRc::new(VecModel::from(initial_phrases)));

    // 1. 录音开关回调
    {
        let pill_weak = pill.as_weak();
        let recorder = Arc::clone(&recorder);
        let start_time = Arc::clone(&start_time);
        let engine = Arc::clone(&engine);
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

                    info!("Running speech recognition ({} samples)...", audio_data.len());
                    match engine_clone.recognize(&audio_data).await {
                        Ok(text) => {
                            info!("ASR recognition result: \"{}\"", text);
                            if !text.is_empty() {
                                // 提交文本到 Fcitx5 激活窗口
                                match FcitxClient::new().await {
                                    Ok(fcitx) => {
                                        if let Err(e) = fcitx.commit_string(&text).await {
                                            error!("Failed to commit string to Fcitx5: {:?}", e);
                                        } else {
                                            info!("Successfully committed recognized text to Fcitx5!");
                                        }
                                    }
                                    Err(e) => {
                                        error!("Could not connect to Fcitx5 DBus: {:?}", e);
                                    }
                                }

                                let text_display = text.clone();
                                let _ = pill_weak_async.upgrade_in_event_loop(move |ui| {
                                    ui.set_is_processing(false);
                                    ui.set_title_text(format!("已上屏: {}", text_display).into());
                                    ui.set_subtitle_text("识别成功 ✓".into());
                                });

                                // 3秒后复原默认文案
                                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                                let _ = pill_weak_async.upgrade_in_event_loop(|ui| {
                                    ui.set_title_text("微信键盘 Linux".into());
                                    ui.set_subtitle_text("点击「语音」说话 · 自动上屏".into());
                                });
                            } else {
                                let _ = pill_weak_async.upgrade_in_event_loop(|ui| {
                                    ui.set_is_processing(false);
                                    ui.set_title_text("未识别到文字".into());
                                    ui.set_subtitle_text("说话声音太小或未包含语音".into());
                                });
                            }
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
        let storage = Arc::clone(&storage);

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
                if let Some(ref st) = *storage {
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
        let rt_handle = rt.clone();

        pill.on_select_phrase(move |phrase: SharedString| {
            info!("Selected quick phrase: \"{}\"", phrase);
            let phrase_str = phrase.to_string();
            let pill_weak_async = pill_weak.clone();

            rt_handle.spawn(async move {
                match FcitxClient::new().await {
                    Ok(fcitx) => {
                        if let Err(e) = fcitx.commit_string(&phrase_str).await {
                            error!("Failed to commit quick phrase: {:?}", e);
                        } else {
                            info!("Successfully committed quick phrase to active window!");
                        }
                    }
                    Err(e) => {
                        error!("Could not connect to Fcitx5: {:?}", e);
                    }
                }

                let _ = pill_weak_async.upgrade_in_event_loop(move |ui| {
                    ui.set_title_text("常用语已上屏 ✓".into());
                    ui.set_subtitle_text(phrase_str.into());
                });

                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let _ = pill_weak_async.upgrade_in_event_loop(|ui| {
                    ui.set_title_text("微信键盘 Linux".into());
                    ui.set_subtitle_text("按 Super+V 快捷语音 · 纯 Rust 原生".into());
                });
            });
        });
    }

    // 5. 点击选中剪贴板历史回调
    {
        let pill_weak = pill.as_weak();
        let rt_handle = rt.clone();

        pill.on_select_clipboard(move |clip: SharedString| {
            info!("Selected clipboard item: \"{}\"", clip);
            let clip_str = clip.to_string();
            let pill_weak_async = pill_weak.clone();

            rt_handle.spawn(async move {
                match FcitxClient::new().await {
                    Ok(fcitx) => {
                        if let Err(e) = fcitx.commit_string(&clip_str).await {
                            error!("Failed to commit clipboard item: {:?}", e);
                        } else {
                            info!("Successfully committed clipboard item to active window!");
                        }
                    }
                    Err(e) => {
                        error!("Could not connect to Fcitx5: {:?}", e);
                    }
                }

                let _ = pill_weak_async.upgrade_in_event_loop(move |ui| {
                    ui.set_title_text("剪贴板内容已粘贴 ✓".into());
                    ui.set_subtitle_text(clip_str.into());
                });

                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let _ = pill_weak_async.upgrade_in_event_loop(|ui| {
                    ui.set_title_text("微信键盘 Linux".into());
                    ui.set_subtitle_text("按 Super+V 快捷语音 · 纯 Rust 原生".into());
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
                        if let Ok(fcitx) = FcitxClient::new().await {
                            let _ = fcitx.commit_string("😄").await;
                        }
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

    // 7. 窗口拖拽移动回调
    {
        let pill_weak = pill.as_weak();
        pill.on_drag_move(move |dx, dy| {
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
        pill.on_open_settings(move || {
            info!("Opening WeChat-Keyboard Settings Window...");
            let mut guard = settings_ref.lock().unwrap();
            if guard.is_none() {
                if let Ok(win) = SettingsWindow::new() {
                    let sw_weak = win.as_weak();
                    win.on_close_settings(move || {
                        if let Some(sw) = sw_weak.upgrade() {
                            let _ = sw.window().hide();
                        }
                    });
                    *guard = Some(win);
                }
            }
            if let Some(ref win) = *guard {
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

    // 根据配置设置提示文字中的快捷键
    pill.set_subtitle_text(format!("按 {} 快捷语音 · 纯 Rust 原生", config.general.voice_shortcut).into());

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
        let mut guard = settings_instance.lock().unwrap();
        if guard.is_none() {
            if let Ok(win) = SettingsWindow::new() {
                let sw_weak = win.as_weak();
                win.on_close_settings(move || {
                    if let Some(sw) = sw_weak.upgrade() {
                        let _ = sw.window().hide();
                    }
                });
                *guard = Some(win);
            }
        }
        if let Some(ref win) = *guard {
            let _ = win.show();
        }
    }

    // 支持后台静默启动
    if config.general.start_hidden || std::env::var("WETYPE_HIDDEN").is_ok() {
        info!("Starting WeType-Linux in background daemon mode (window hidden)...");
        let _ = pill.window().hide();
    }

    info!("Displaying WeType-Linux floating pill...");
    pill.run()?;
    Ok(())
}
