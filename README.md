# WeType-Linux 🐧

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/ly5566lyui/-wetype-/actions/workflows/ci.yml/badge.svg)](https://github.com/ly5566lyui/-wetype-/actions/workflows/ci.yml)
![Status](https://img.shields.io/badge/状态-仅供学习研究-orange)

> [!IMPORTANT]
> **这是一个仅供学习研究的个人实验项目，尚未成熟，请勿当生产力工具使用。**
> 功能以"跑通 + 验证"为目标，识别精度、健壮性、跨设备兼容性都有很大改进空间。
> 本项目为独立实现，与任何商业输入法产品均无关联；不使用、不包含、不分发任何第三方专有代码、模型或资源，详见文末「免责声明」。

**WeType-Linux** 是一个跑在 Linux 后台的**热键语音输入工具**：按热键说话，说完停顿一下，文字自动上屏到当前焦点窗口。附带局域网剪贴板同步与常用语库。纯 Rust、无图形界面、识别离线完成。

---

## ✅ 已实现（真机验证过）

| 功能 | 说明 |
|---|---|
| 热键语音上屏 | `Super+V` 开启语音窗口，说话后**静音自动结束**并上屏；也支持"说完再按一次结束" |
| 语音后处理 | 句末标点补全、全角符号规范化（12：30→12:30）、重复标点折叠 |
| 垃圾输出过滤 | `(笑声)`、`<|nospeech|>` 等无效识别结果自动丢弃 |
| 全局热键 | X11: XGrabKey；GNOME Wayland: 原生键绑定（集成脚本自动注册） |
| 剪贴板监听与历史 | SQLite 存储、敏感内容启发式过滤、历史上限 |
| 局域网 P2P 剪贴板同步 | mDNS 自动发现 + AES-256-GCM 加密 + 重放防护 |
| Fcitx5 候选框主题 | Midnight（暗）/ Daylight（亮）双主题，Nord 配色 |
| 桌面通知反馈 | 录音开始 / 已上屏 / 识别失败 均有通知提示 |
| 质量保障 | 31 个单元测试 + GitHub CI（构建 + 全量测试） |

以上功能已在 Debian 13 (GNOME Wayland) 上真机验证：构建、测试、daemon 运行、热键触发、真声识别上屏。

## ⚠️ 已知限制（读到这里就明白值不值得用）

- **识别精度一般**：本地引擎是 Paraformer small 量化模型，转写质量与商业输入法有差距；标点为规则补全，不是模型标点。
- **无图形界面**：纯后台 + 桌面通知反馈，所有配置编辑 `config.toml`。
- **常用语库暂无触发入口**：能存能管（`phrases --add/--list`），"弹窗选择器"在路线图中。
- **语音模型无热词提升**：paraformer 引擎仅支持 greedy_search（热词需 transducer 模型），已实测确认。
- **P2P 同步仅在同机双实例验证过**，跨物理机未测。
- **VMware 虚拟机内无法录音**：VMware 虚拟声卡不桥接宿主机麦克风输入（"主机已关闭"），在 VM 里测语音请用 USB 麦克风直通或物理机部署。

## 🚀 快速开始

### 1. 系统先决条件

```bash
# Arch Linux
sudo pacman -S fcitx5 fcitx5-rime rime-ice fcitx5-configtool alsa-lib

# Ubuntu / Debian
sudo apt install fcitx5 fcitx5-rime librime-data libasound2-dev
```

> 💡 **其它依赖说明**
> - **构建**：`libasound2-dev`（录音用）；SQLite 已随构建内置，无需系统库。
> - **离线语音模型**（可选，约 100MB）：`./scripts/download_models.sh`；不装则语音依赖云端 API 或提示缺失。
> - **GNOME Wayland 快捷键**：由 `scripts/integrate_fcitx5.sh` 自动注册，无需手动配置。

详细的 Fcitx5 与雾凇拼音配置指南见：[docs/rime_setup_guide.md](docs/rime_setup_guide.md)。

### 2. 构建与安装

```bash
git clone https://github.com/ly5566lyui/-wetype-.git
cd -wetype-

cargo build --release
bash scripts/install.sh    # 安装二进制 + systemd 服务 + 桌面入口 + GNOME 快捷键 + 主题/图标

# 可选: 离线语音模型
./scripts/download_models.sh

# 可选: 深度集成 (Rime 显示名 / GNOME 快捷键 / 候选框主题)
bash scripts/integrate_fcitx5.sh
```

### 3. 日常使用

| 快捷键 | 功能 |
|---|---|
| `Super+V` | 开始语音输入，**说完停顿一下自动上屏**（GNOME 原生键绑定，由集成脚本注册） |
| `Ctrl+Alt+H` | 把最近一条剪贴板历史重新粘贴到光标处 |
| daemon 内热键 | X11 会话下按 `config.toml` 里的 `voice_shortcut` / `clipboard_shortcut` 触发（Wayland 会话由上面的 GNOME 键绑定接管） |

**语音行为**：同一个语音键，**按住 ≥0.3 秒松手** = 说的话直接上屏；**轻点一下** = 进入连续模式，说完再按一次结束。识别失败或静音会有通知提示。

所有配置项（热键、静音阈值、敏感过滤、P2P 口令等）见 [config.example.toml](config.example.toml) 的逐项注释。

---

## 🏗️ 工作原理

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

## 📁 项目结构

```text
wetype-linux/
├── src/
│   ├── main.rs          # 入口: daemon (默认) / voice-input / repaste / phrases / test-fcitx
│   ├── config.rs        # 配置加载与持久化 (TOML, 缺省字段向后兼容)
│   ├── cli.rs           # clap 命令行定义
│   ├── asr/             # 录音+VAD / 云端 API / 本地 Sherpa / 后处理 / 混合调度
│   ├── clipboard/       # 监听+敏感过滤 / SQLite / P2P 同步 (mDNS+AES-GCM)
│   ├── fcitx/           # Fcitx5 DBus 客户端 + Lua 桥接扩展自动部署
│   └── hotkey/          # 全局热键: X11 XGrabKey / Wayland Portal GlobalShortcuts
├── docs/                # 架构文档与 Rime 配置指南 (docs/dev/ 为开发纪要)
├── icons/               # 应用图标 (SVG, Nord 风)
├── scripts/             # install / download_models / integrate_fcitx5 部署脚本
├── systemd/             # 用户级 systemd 服务
├── themes/              # Fcitx5 候选框主题 (Midnight / Daylight, Nord 风)
└── .github/             # CI / Release 工作流与 Issue 模板
```

## 🛠️ 开发与测试

```bash
cargo check              # 静态检查
cargo test               # 单元测试 (加解密 / 快捷键解析 / 静音裁剪 / 垃圾过滤 / 配置兼容等 31 例)
cargo run -- daemon      # 本地运行守护服务
cargo run -- -v daemon   # 调试模式运行守护进程
```

提交 PR 前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)；持续集成定义在 [.github/workflows/ci.yml](.github/workflows/ci.yml)。

---

## 🗺️ 路线图（想学什么往里加什么）

- Silero VAD 精准断句（替代能量阈值）
- `wetype-linux doctor` 自诊断命令
- 常用语弹窗选择器
- 纠错学习 / 闲置微调（参考 voiceio）
- transducer 引擎选型（解锁热词提升）
- IBus 前端双支持

---

## 📖 文档

- [系统架构与模块交互设计](docs/architecture.md)
- [Fcitx5 + Rime + 雾凇拼音安装与调优指南](docs/rime_setup_guide.md)
- [配置项逐项说明](config.example.toml)
- [更新日志](CHANGELOG.md)
- 开发纪要与同类项目对比分析：`docs/dev/`（不随仓库发布）

---

## ⚖️ 免责声明 (Disclaimer & Trademarks)

1. **项目性质与定位**：本项目是一个**仅供学习研究的个人开源实验项目**，非商业软件，未经过充分测试，功能不保证完整或可靠，请勿在生产环境或重要场合依赖。
2. **独立声明**：本项目为独立学习项目（Clean-room Implementation），与任何商业输入法产品或厂商均无关联、授权或背书关系；不使用、不包含、不分发任何第三方专有代码、识别模型或资源。核心功能由公开开源方案（Fcitx5、Rime、Sherpa-ONNX 等）驱动。
3. **商标中立**：本项目名称及内容不指向、不代言任何商业产品；如本项目名称与其他产品名称存在相似，纯属巧合且无任何混淆意图，相关权利方如需反馈可通过 Issue 联系。
4. **仅供学习研究**：本项目代码及文档仅供技术交流与学习参考，使用者需自行承担一切风险。

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
- 交互设计思路参考：[WowType](https://github.com/Gloridust/WowType)、[voiceio](https://github.com/Hugo0/voiceio)、[nerd-dictation](https://github.com/ideasman42/nerd-dictation)

## 📜 许可

[MIT](LICENSE) © 2026 WeType-Linux Contributors
