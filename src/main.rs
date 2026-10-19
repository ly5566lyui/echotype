mod asr;
mod cli;
mod clipboard;
mod config;
mod fcitx;
mod hotkey;
mod ui;

use anyhow::Result;
use asr::{AsrEngine, HybridAsrEngine};
use clap::Parser;
use cli::{Cli, Commands};
use config::AppConfig;
use fcitx::FcitxClient;
use std::path::PathBuf;
use tracing::{error, info};
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
        info!("Configuration not found at {:?}, using defaults.", config_path);
        AppConfig::default()
    };

    // 3. 路由子命令 (默认启动 GUI 桌面应用)
    match args.command.unwrap_or(Commands::Gui) {
        Commands::Gui => {
            info!("Launching WeType-Linux floating pill capsule UI (Pure Rust Slint)...");
            let rt = tokio::runtime::Handle::current();
            tokio::task::block_in_place(|| {
                ui::run_pill_ui(config, rt)
            })?;
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
                let recorder = asr::recorder::AudioRecorder::new();
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
                match FcitxClient::new().await {
                    Ok(fcitx) => {
                        fcitx.commit_string(&text).await?;
                        info!("Successfully injected recognized text into active window!");
                    }
                    Err(e) => {
                        error!("Could not inject text into Fcitx5: {:?}", e);
                    }
                }
            }
        }
        Commands::Phrases { add, list } => {
            let data_dir = PathBuf::from("data");
            std::fs::create_dir_all(&data_dir)?;
            let storage = clipboard::ClipboardStorage::new(data_dir.join("storage.db"))?;

            if list {
                let phrases = storage.get_recent_history(20)?;
                println!("=== Recent Clipboard History ===");
                for (i, p) in phrases.iter().enumerate() {
                    println!("{}. {}", i + 1, p);
                }
            } else if let Some(content) = add {
                storage.add_quick_phrase(&format!("p_{}", chrono_now()), &content, None)?;
                info!("Added quick phrase successfully.");
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
    let _fcitx = match FcitxClient::new().await {
        Ok(client) => {
            info!("Fcitx5 DBus client ready.");
            Some(client)
        }
        Err(e) => {
            tracing::warn!("Fcitx5 is not running or DBus unavailable: {:?}", e);
            None
        }
    };

    let data_dir = PathBuf::from("data");
    let _ = std::fs::create_dir_all(&data_dir);
    if let Ok(storage) = clipboard::ClipboardStorage::new(data_dir.join("storage.db")) {
        let monitor = clipboard::ClipboardMonitor::new();
        let _ = monitor.start(move |text| {
            let _ = storage.insert_history(&text);
        });
    }

    if config.p2p_sync.enabled {
        let p2p = clipboard::sync_p2p::P2pSyncManager::new(
            config.p2p_sync.device_name.clone(),
            config.p2p_sync.port,
        );
        tokio::spawn(async move {
            let _ = p2p.start_discovery().await;
        });
    }

    info!("WeType-Linux daemon is running. Press Ctrl+C to terminate.");
    tokio::signal::ctrl_c().await?;
    info!("Shutting down WeType-Linux daemon.");
    Ok(())
}
