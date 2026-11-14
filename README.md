# WeType-Linux 🐧

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021%20Edition-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Linux%20(Wayland%20%7C%20X11)-brightgreen.svg)]()
[![CI](https://github.com/ly5566lyui/-wetype-/actions/workflows/ci.yml/badge.svg)](https://github.com/ly5566lyui/-wetype-/actions/workflows/ci.yml)

**WeType-Linux** 是一个专为 Linux 桌面环境打造的**纯后台输入法增强守护程序**。基于 **Fcitx5 + Rime (雾凇拼音)** 基座，提供比肩主流商业输入法的语音体验：**按热键说话，说完自动上屏**，并附带**局域网剪贴板同步、剪贴板历史与常用语库**。

---

## 🌟 核心特性

- 🎙️ **语音输入，说完自动上屏**：
  - **离线优先**：本地 Sherpa-ONNX (Paraformer) 识别，录音不出机器；配置云端 API 后自动切换，断网无缝回退。
  - **静音自动结束**：说完停顿约 2 秒自动提交，60 秒上限保护，无需任何手动操作。
  - **文本后处理**：句末标点补全、全角符号规范化、无效输出过滤。
- ⚡ **无缝 Fcitx5 注入**：
  - 文字经 DBus `commitString` 精准注入光标焦点，不污染剪贴板、不丢字符。
  - 完美适配 **Wayland** 与 **X11**（GNOME/KDE/Hyprland/sway 等均实测可用）。
- ⌨️ **全局热键**：
  - X11：XGrabKey 全局钩子；GNOME Wayland：原生自定义键绑定（免授权弹窗）。
- 📋 **剪贴板历史与常用语库**：
  - SQLite 存储、敏感内容启发式过滤、历史上限管理；剪贴板热键一键重贴最近一条。
- 🌐 **局域网 P2P 剪贴板同步**：
  - mDNS 自动发现设备，AES-256-GCM 端到端加密，重放攻击防护。
- 🦀 **Rust 单二进制**：
  - 零图形界面依赖，常驻内存小，启动快，MIT 许可。

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
┌───────────────────────────────────────────────────────────┐
│                    键盘 / 桌面会话                          │
│   Super+V 语音 · Ctrl+Alt+H 重贴 (GNOME 原生键绑定)         │
└───────────────────────▲───────────────────────────────────┘
                        │ 命令 / 信号
┌───────────────────────▼───────────────────────────────────┐
│            WeType Daemon (Rust 单二进制, 无 UI)             │
│                                                           │
│   语音输入           剪贴板            全局热键             │
│   · cpal 录音      · 监听+敏感过滤    · X11 XGrabKey        │
│   · 能量 VAD       · SQLite 历史     · Wayland Portal      │
│   · 静音自动结束    · P2P 同步        · GNOME 原生键绑定     │
│   · 60s 上限       · 上限裁剪                               │
│        │                                                  │
│        ▼                                                  │
│   本地 Sherpa-ONNX ◄──► 云端 API (自动回退)                │
│        │                                                  │
│        ▼                                                  │
│   后处理: 标点补全 / 黑名单 / 符号规范化                    │
└───────────────────────┬───────────────────────────────────┘
                        │ DBus commitString (Lua 桥接)
              ┌─────────▼──────────┐
              │  Fcitx5 + Rime 词库 │
              └────────────────────┘
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

# 安装到用户目录 (含 GNOME 快捷键注册 / 主题 / 图标 / 自启动)
bash scripts/install.sh

# 可选: 下载离线语音模型 (不装则语音走云端 API 或提示缺失)
./scripts/download_models.sh
```

### 3. 日常使用

| 快捷键 | 功能 |
|---|---|
| `Super+V` | 开始语音输入，**说完停顿一下自动上屏**（GNOME 原生键绑定，由集成脚本注册） |
| `Ctrl+Alt+H` | 把最近一条剪贴板历史重新粘贴到光标处 |
| daemon 内热键 | X11 会话下按 `config.toml` 里的 `voice_shortcut` / `clipboard_shortcut` 触发（Wayland 会话由上面的 GNOME 键绑定接管） |

**语音行为**：同一个语音键，**按住 ≥0.3 秒松手** = 说的话直接上屏；**轻点一下** = 进入连续模式，说完再按一次结束。识别失败或静音会有通知提示。

所有配置项（热键、静音阈值、敏感过滤、P2P 口令、热词等）见 [config.example.toml](config.example.toml) 的逐项注释。

---

## 📁 项目结构

```text
wetype-linux/
├── src/
│   ├── main.rs          # 入口: daemon (默认) / voice-input / phrases / test-fcitx 子命令
│   ├── config.rs        # 配置加载与持久化 (TOML, 缺省字段向后兼容)
│   ├── cli.rs           # clap 命令行定义
│   ├── asr/             # 语音识别: recorder(录音+VAD) / cloud_api / local_sherpa / postprocess(后处理) / 混合调度
│   ├── clipboard/       # monitor(监听+敏感过滤) / storage(SQLite) / sync_p2p(mDNS+AES-GCM)
│   ├── fcitx/           # Fcitx5 DBus 客户端 + Lua 桥接扩展自动部署
│   └── hotkey/          # 全局热键: X11 XGrabKey / Wayland Portal GlobalShortcuts
│   └── postprocess      # 语音后处理: 标点补全 / 黑名单 / 符号规范化
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
