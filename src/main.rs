//! WeType-Linux —— 面向 Linux 桌面的现代化输入法增强守护程序。
//!
//! 基于 Fcitx5 (DBus) + Rime (雾凇拼音) 底座，提供端云混合语音输入、
//! 局域网 P2P 剪贴板同步、常用语快捷上屏与全局热键等能力。
//! 模块交互总览见 `docs/architecture.md`，开发纪要见 `docs/dev/`。

mod asr;
mod cli;
mod clipboard;
mod config;
mod fcitx;
mod hotkey;
mod ui;

use anyhow::Result;
use asr::recorder::{AudioRecorder, MAX_RECORD_SECS};
use asr::{AsrEngine, HybridAsrEngine};
use clap::Parser;
use cli::{Cli, Commands};
use config::AppConfig;
use fcitx::FcitxClient;
use hotkey::{HotkeyAction, ShortcutSpec};
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    // 1. 初始化日志记录器
    let filter = if args.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    info!("Starting WeType-Linux v{}...", env!("CARGO_PKG_VERSION"));

    // 2. 加载配置文件
    let config_path = args.config.unwrap_or_else(AppConfig::default_config_path);
    let config = if config_path.exists() {
        info!("Loading configuration from {:?}", config_path);
        AppConfig::load_from_path(&config_path)?
    } else {
        info!(
            "Configuration not found at {:?}, using defaults.",
            config_path
        );
        AppConfig::default()
    };

    // 3. 路由子命令 (默认启动 GUI 桌面应用)
    match args.command.unwrap_or(Commands::Gui) {
        Commands::Gui => {
            info!("Launching WeType-Linux floating pill capsule UI (Pure Rust Slint)...");
            let rt = tokio::runtime::Handle::current();
            tokio::task::block_in_place(|| ui::run_pill_ui(config, config_path, rt))?;
        }
        Commands::TestFcitx { text } => {
            info!("Testing connection to Fcitx5 DBus service...");
            match FcitxClient::new().await {
                Ok(client) => {
                    info!("Successfully connected to Fcitx5!");
                    client.commit_string(&text).await?;
                    info!("Text committed: \"{}\"", text);
                }
                Err(e) => {
                    error!("Failed to connect to Fcitx5: {:?}", e);
                    return Err(e);
                }
            }
        }
        Commands::VoiceInput { duration, wav } => {
            let engine = HybridAsrEngine::from_config(&config.asr)?;

            let text = if let Some(wav_path) = wav {
                info!("Recognizing existing WAV file: {:?}", wav_path);
                engine.local_engine().recognize_wav_file(&wav_path).await?
            } else {
                let recorder = AudioRecorder::new();
                use std::io::IsTerminal;
                if let Some(secs) = duration {
                    info!("Recording from microphone for {} seconds...", secs);
                    recorder.start()?;
                    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                } else if !std::io::stdin().is_terminal() {
                    info!("Non-terminal hotkey invocation detected, recording for 4 seconds...");
                    recorder.start()?;
                    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                } else {
                    info!("Press Enter to START recording...");
                    let mut line = String::new();
                    let _ = std::io::stdin().read_line(&mut line);

                    recorder.start()?;
                    info!("Recording... Press Enter to STOP recording.");
                    let _ = std::io::stdin().read_line(&mut line);
                }

                let audio_data = recorder.stop();
                info!(
                    "Audio captured ({} samples at 16kHz, {:.2}s). Running ASR...",
                    audio_data.len(),
                    audio_data.len() as f64 / 16000.0
                );

                engine.recognize(&audio_data).await?
            };

            info!("ASR Recognized Text: \"{}\"", text);

            if !text.is_empty() {
                match fcitx::commit_string_cached(&text).await {
                    Ok(()) => info!("Successfully injected recognized text into active window!"),
                    Err(e) => error!("Could not inject text into Fcitx5: {:#}", e),
                }
            }
        }
        Commands::Phrases {
            add,
            list,
            history,
        } => {
            let data_dir = AppConfig::default_data_dir();
            std::fs::create_dir_all(&data_dir)?;
            let storage = clipboard::ClipboardStorage::new(data_dir.join("storage.db"))?;

            if list {
                println!("=== Quick Phrases ===");
                for (i, p) in storage.get_quick_phrases(50)?.iter().enumerate() {
                    println!("{}. {}", i + 1, p);
                }
            } else if history {
                println!("=== Recent Clipboard History ===");
                for (i, p) in storage.get_recent_history(20)?.iter().enumerate() {
                    println!("{}. {}", i + 1, p);
                }
            } else if let Some(content) = add {
                storage.add_quick_phrase(&format!("p_{}", chrono_now()), &content, None)?;
                info!("Added quick phrase successfully.");
            } else {
                println!("Nothing to do. Use --add <text>, --list or --history.");
            }
        }
        Commands::Daemon => {
            info!("Running in Daemon mode.");
            run_daemon(config).await?;
        }
    }

    Ok(())
}

fn chrono_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 守护程序主事件循环
async fn run_daemon(config: AppConfig) -> Result<()> {
    // 预热共享 Fcitx5 连接 (不可用仅告警，后续上屏时会自动重试重连)
    if let Err(e) = fcitx::warm_up().await {
        warn!("Fcitx5 is not running or DBus unavailable: {:#}", e);
    }

    // 剪贴板历史与常用语 SQLite 存储 (XDG 数据目录)
    let data_dir = AppConfig::default_data_dir();
    std::fs::create_dir_all(&data_dir)?;
    let storage: Arc<Option<clipboard::ClipboardStorage>> = Arc::new(
        clipboard::ClipboardStorage::new(data_dir.join("storage.db"))
            .map_err(|e| {
                warn!("Clipboard storage unavailable: {:#}", e);
                e
            })
            .ok(),
    );

    // P2P 剪贴板同步 (仅在启用且设置了配对口令时启动，空口令会被拒绝)
    let p2p = if config.p2p_sync.enabled {
        match clipboard::sync_p2p::P2pSyncManager::start(&config.p2p_sync).await {
            Ok(manager) => Some(manager),
            Err(e) => {
                warn!("P2P sync disabled: {:#}", e);
                None
            }
        }
    } else {
        None
    };

    // 剪贴板监听: 历史入库 (带上限裁剪与敏感过滤) + 广播到局域网对端
    if storage.is_some() {
        let monitor = clipboard::ClipboardMonitor::new()
            .with_sensitive_filter(config.clipboard.ignore_sensitive);
        let st = Arc::clone(&storage);
        let p2p_cb = p2p.clone();
        let max_history = config.clipboard.max_history_items;
        monitor.start(move |text| {
            if let Some(st) = st.as_ref() {
                if let Err(e) = st.insert_history(&text) {
                    tracing::debug!("Failed to store clipboard history: {:#}", e);
                }
                let _ = st.trim_history(max_history);
            }
            if let Some(p2p) = p2p_cb.as_ref() {
                p2p.broadcast(&text);
            }
        })?;
    }

    // 全局热键: 语音输入 (开始/停止切换，识别后自动上屏)。句柄须持有至进程退出
    let _hotkeys = register_daemon_hotkeys(&config);

    info!("WeType-Linux daemon is running. Press Ctrl+C to terminate.");
    tokio::signal::ctrl_c().await?;
    info!("Shutting down WeType-Linux daemon.");
    Ok(())
}

/// 注册守护进程侧全局热键: 语音热键触发开始/停止录音，停止后识别并自动上屏。
/// 返回的句柄必须由调用方持有至进程退出 (Drop 即注销热键)。
/// 热键或 ASR 引擎不可用时返回 None，仅告警，不阻断守护进程其余功能。
fn register_daemon_hotkeys(config: &AppConfig) -> Option<hotkey::HotkeyHandle> {
    let engine = match HybridAsrEngine::from_config(&config.asr) {
        Ok(e) => Arc::new(e),
        Err(e) => {
            warn!("ASR engine unavailable, voice hotkey disabled: {:#}", e);
            return None;
        }
    };
    let recorder: Arc<Mutex<Option<AudioRecorder>>> = Arc::new(Mutex::new(None));
    let rt = tokio::runtime::Handle::current();

    let specs = vec![ShortcutSpec::new(
        "wetype.voice",
        &config.general.voice_shortcut,
        "WeType 语音输入",
        HotkeyAction::Voice,
    )];

    let dispatch: Arc<dyn Fn(HotkeyAction) + Send + Sync> = {
        let engine = Arc::clone(&engine);
        let recorder = Arc::clone(&recorder);
        let rt = rt.clone();
        Arc::new(move |action| match action {
            HotkeyAction::Voice => {
                let mut guard = match recorder.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                match guard.take() {
                    None => {
                        // 空闲 -> 开始录音
                        let rec = AudioRecorder::new();
                        match rec.start() {
                            Ok(()) => {
                                info!("Voice hotkey: recording started...");
                                *guard = Some(rec);

                                // 录音时长保护: 超过上限自动停止并识别，避免忘记按停止键
                                let recorder_wd = Arc::clone(&recorder);
                                let engine_wd = Arc::clone(&engine);
                                let rt_wd = rt.clone();
                                rt_wd.spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_secs(
                                        MAX_RECORD_SECS,
                                    ))
                                    .await;
                                    let rec =
                                        match recorder_wd.lock().ok().and_then(|mut g| g.take()) {
                                            Some(r) => r,
                                            None => return, // 已手动停止
                                        };
                                    info!(
                                        "Max recording duration ({}s) reached, auto-stopping...",
                                        MAX_RECORD_SECS
                                    );
                                    let audio = rec.stop();
                                    recognize_and_commit(engine_wd, audio).await;
                                });
                            }
                            Err(e) => error!("Failed to start recording: {:#}", e),
                        }
                    }
                    Some(rec) => {
                        // 录音中 -> 停止并识别上屏
                        drop(guard);
                        info!("Voice hotkey: recording stopped, running ASR...");
                        let audio = rec.stop();
                        let engine = Arc::clone(&engine);
                        rt.spawn(async move {
                            recognize_and_commit(engine, audio).await;
                        });
                    }
                }
            }
            HotkeyAction::Clipboard => {}
        })
    };

    let handle = hotkey::register(specs, dispatch, rt);
    info!(
        "Global hotkeys registered (voice: {}).",
        config.general.voice_shortcut
    );
    Some(handle)
}

/// 识别 PCM 并将结果上屏到 Fcitx5 焦点窗口 (手动停止与超时自动停止共用)
async fn recognize_and_commit(engine: Arc<HybridAsrEngine>, audio: Vec<i16>) {
    if audio.is_empty() {
        warn!("Captured audio buffer is empty.");
        return;
    }
    let text = match engine.recognize(&audio).await {
        Ok(t) => t,
        Err(e) => {
            error!("ASR failed: {:#}", e);
            return;
        }
    };
    if text.is_empty() {
        warn!("ASR returned empty text.");
        return;
    }
    info!("ASR result: \"{}\"", text);
    match fcitx::commit_string_cached(&text).await {
        Ok(()) => info!("Recognized text committed to active window."),
        Err(e) => error!("Failed to commit text: {:#}", e),
    }
}
