use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "wetype-linux",
    version,
    about = "A modern input method enhancement companion for Linux (Fcitx5)"
)]
pub struct Cli {
    /// 配置文件路径
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// 开启详细调试日志
    #[arg(short, long)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 启动现代桌面图形界面 (默认双击桌面图标启动)
    Gui,
    /// 启动后台常驻守护服务
    Daemon,
    /// 触发一次语音输入并自动上屏
    VoiceInput {
        /// 指定录音时长（秒）；若不指定则采用回车交互控制开始/停止
        #[arg(short, long)]
        duration: Option<u64>,
        /// 直接使用指定的 WAV 音频文件进行识别与上屏测试
        #[arg(short, long)]
        wav: Option<PathBuf>,
    },
    /// 列出或管理常用语与剪贴板历史
    Phrases {
        #[arg(short, long)]
        add: Option<String>,
        #[arg(short, long)]
        list: bool,
    },
    /// 测试与 Fcitx5 的 DBus 连通性并注入文字上屏
    TestFcitx {
        /// 自定义测试上屏文字
        #[arg(short, long, default_value = "【WeType-Linux DBus测试成功】")]
        text: String,
    },
}
