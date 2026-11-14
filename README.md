# WeType-Linux 🐧

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Linux%20(Wayland%20%7C%20X11)-brightgreen.svg)]()
[![CI](https://github.com/ly5566lyui/-wetype-/actions/workflows/ci.yml/badge.svg)](https://github.com/ly5566lyui/-wetype-/actions/workflows/ci.yml)

**WeType-Linux** 是一个专为 Linux 桌面环境打造的**现代输入法增强守护程序**。基于 **Fcitx5 + Rime (雾凇拼音)** 基座，提供比肩主流商业输入法的流畅功能体验，包括**端云混合语音输入、跨设备局域网剪贴板同步、智能常用语快速上屏**等。

---

## 🌟 核心特性

- 🎙️ **端云混合语音输入**：
  - **网络连通时**：调用高精度云端 ASR API，实时流式断句与标点识别。
  - **离线断网时**：自动秒级降级至本地轻量级 **Sherpa-ONNX** 引擎，完全本地计算，保护隐私。
- ⚡ **无缝 Fcitx5 DBus 注入**：
  - 完美适配 **Wayland** 与 **X11**，不依赖不稳定的全局虚拟按键拦截，直接通过 DBus `org.fcitx.Fcitx5.InputMethod` 协议将文字精准注入到光标焦点。
- 📋 **跨设备剪贴板 P2P 同步**：
  - 局域网基于 mDNS 自动发现设备，采用 **AES-GCM-256** 强加密，安全同步手机与电脑剪贴板。
- 💬 **常用语与快捷短语管理**：
  - 本地 SQLite 高效检索，随时一键调出常用模版、邮箱、地址、长文本并直接上屏。
- 🦀 **Rust 单二进制构建**：
  - 超低内存驻留（常驻仅 10~20MB），启动速度极快，系统级安全保障。

---

## 📊 功能完成度

| 功能 | 状态 | 说明 |
|---|---|---|
| Fcitx5 文字上屏 (Wayland / X11) | ✅ | DBus + Lua 桥接扩展自动部署，无需按键模拟 |
| 语音输入 (端云混合) | ✅ | 云端 API 优先，断网自动降级本地 Sherpa-ONNX |
| 静音裁剪 / 录音时长保护 | ✅ | 能量法 VAD-lite + 60 秒上限看门狗 |
| 全局热键 | ✅ | 语音上屏 + 剪贴板重贴；X11: XGrabKey，Wayland: XDG Portal |
| 常用语 / 剪贴板历史 | ✅ | SQLite 存储 + 敏感内容过滤 + 上限裁剪 |
| 纯后台运行 | ✅ | 无悬浮窗，配置经 `config.toml` 管理 |
| 局域网 P2P 剪贴板同步 | ✅ | mDNS 自动发现 + AES-256-GCM 端到端加密 |
| Fcitx5 候选框主题 (Nord 风) | ✅ | Midnight / Daylight 双主题 + 原创应用图标 |
| 常用语弹窗选择器 / 边写边译 / AI 助理 | 🚧 | 规划中 |

---

## 🏗️ 架构拓扑

```text
┌─────────────────────────────────────────────────────────────┐
│                       用户交互层 (UI)                        │
│   · 常用语浮窗 / 剪贴板历史 / 语音录制麦克风浮条 (Rust UI)     │
└───────────────▲─────────────────────────────▲───────────────┘
                │                             │
┌───────────────▼─────────────────────────────▼───────────────┐
│               核心守护服务：Rust Daemon (单二进制)            │
│  ┌─────────────────┐ ┌─────────────────┐ ┌────────────────┐ │
│  │ 语音输入模块     │ │ 跨设备剪贴板模块 │ │ 快捷键与常用语 │ │
│  │ · 有网: 云端API  │ │ · 局域网 mDNS   │ │ · SQLite 本地  │ │
│  │ · 无网: 本地Sherpa│ │ · AES-GCM 加密  │ │   词条模板     │ │
│  └────────▲────────┘ └────────▲────────┘ └────────▲───────┘ │
└───────────┼───────────────────┼───────────────────┼─────────┘
            │                   │                   │
            └───────────────────┼───────────────────┘
                                │ DBus 通信
                    ┌───────────▼───────────┐
                    │     Fcitx5 框架核心   │
                    │   (处理打字上屏与快捷键) │
                    └───────────▲───────────┘
                                │
                    ┌───────────▼───────────┐
                    │ Rime 引擎 + 雾凇拼音  │
                    │   (负责强大的拼音词库) │
                    └───────────────────────┘
```

---

## 🚀 快速开始

### 1. 系统先决条件

确保系统中已安装 Fcitx5 及 Rime 拼音引擎：

```bash
# Arch Linux
sudo pacman -S fcitx5 fcitx5-rime rime-ice fcitx5-configtool

# Ubuntu / Debian
sudo apt install fcitx5 fcitx5-rime librime-data
```

> 💡 **其它依赖说明**
> - **构建依赖**：`cpal` 录音需要 ALSA 开发头文件 —— Debian/Ubuntu: `sudo apt install libasound2-dev`；Arch: `sudo pacman -S alsa-lib`。SQLite 已随构建内置 (`rusqlite/bundled`)，无需系统库。
> - **Wayland 全局热键**：需要 `xdg-desktop-portal` 及支持 GlobalShortcuts 的后端 (GNOME 46+ / KDE / wlroots 系)，首次触发会弹出系统授权弹窗；X11 下无此要求。
> - **P2P 剪贴板同步**：所有设备须配置相同的 `pair_secret`（口令为空时该功能自动禁用，不做明文同步）。

详细的 Fcitx5 与雾凇拼音配置指南见：[docs/rime_setup_guide.md](docs/rime_setup_guide.md)。

### 2. 构建与运行

```bash
# 克隆仓库
git clone https://github.com/ly5566lyui/-wetype-.git
cd -wetype-

# 编译发布版本
cargo build --release

# 复制默认配置
cp config.example.toml ~/.config/wetype/config.toml

# 启动守护进程
./target/release/wetype-linux daemon
```

---

## 📁 项目结构

```text
wetype-linux/
├── src/
│   ├── main.rs          # 入口: daemon (默认) / voice-input / phrases / test-fcitx 子命令
│   ├── config.rs        # 配置加载与持久化 (TOML, 缺省字段向后兼容)
│   ├── cli.rs           # clap 命令行定义
│   ├── asr/             # 语音识别: recorder(录音+VAD) / cloud_api / local_sherpa / 混合调度
│   ├── clipboard/       # monitor(监听+敏感过滤) / storage(SQLite) / sync_p2p(mDNS+AES-GCM)
│   ├── fcitx/           # Fcitx5 DBus 客户端 + Lua 桥接扩展自动部署
│   ├── hotkey/          # 全局热键: X11 XGrabKey / Wayland Portal GlobalShortcuts
├── docs/                # 架构文档与 Rime 配置指南 (docs/dev/ 为开发纪要)
├── icons/               # 应用图标 (SVG, Nord 风)
├── scripts/             # install / download_models / integrate_fcitx5 部署脚本
├── systemd/             # 用户级 systemd 服务
├── themes/              # Fcitx5 候选框主题 (Midnight / Daylight, Nord 风)
└── .github/             # CI 工作流与 Issue 模板
```

## 🛠️ 开发与测试

```bash
cargo check              # 静态检查
cargo test               # 单元测试 (加解密 / 快捷键解析 / 静音裁剪 / 配置兼容等 19 例)
cargo run -- daemon      # 本地运行守护服务
cargo run -- -v daemon   # 调试模式运行守护进程
```

提交 PR 前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)；持续集成定义在 [.github/workflows/ci.yml](.github/workflows/ci.yml)。

---

## 📖 文档

- [系统详细架构与模块交互设计](docs/architecture.md)
- [Fcitx5 + Rime + 雾凇拼音安装与调优指南](docs/rime_setup_guide.md)

---

## ⚖️ 免责与商标声明 (Disclaimer & Trademarks)

1. **项目性质**：本项目是一个独立的开源非商业项目，旨在为 Linux (Wayland / X11) 桌面环境提供现代化的输入法增强体验。
2. **商标归属**：“微信”、“微信输入法”、“WeChat”等名称及相关标识均为腾讯公司（Tencent Inc.）的合法注册商标。本项目与腾讯公司没有任何关联、授权、合作或背书关系。
3. **独立实现**：本项目所有代码均为独立编写（Clean-room Implementation），未包含任何未经授权的专有逆向代码、受保护的二进制文件或专有私有资产。所有核心功能均由公开透明的开源方案（如 Fcitx5, Rime, Sherpa-ONNX 等）驱动。
4. **仅供学习研究**：本项目代码及文档仅供技术交流与学习参考。

---

## 🙏 致谢与上游开源项目 (Credits & Acknowledgments)

本项目基于并受益于以下优秀的开源项目：

- [Fcitx5](https://github.com/fcitx/fcitx5) - 现代化 Linux 输入法框架
- [Rime (中州韵)](https://github.com/rime/librime) - 优秀的跨平台输入法算法引擎
- [rime-ice (雾凇拼音)](https://github.com/iDvel/rime-ice) - 长期维护的高质量简体中文词库
- [Sherpa-ONNX](https://github.com/k2-fsa/sherpa-onnx) - 高性能、跨平台的端侧语音识别推理引擎
- [zbus](https://github.com/dbus2/zbus) - 纯 Rust 高性能 DBus 库
- [Lucide](https://lucide.dev) - 应用图标基于其键盘与声波图形重绘 (ISC)
- [Nord](https://www.nordtheme.com) - 候选框主题配色灵感来源
