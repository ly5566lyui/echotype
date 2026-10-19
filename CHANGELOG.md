# 更新日志 (Changelog)

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) 格式。

## [0.1.0] - 2026-10-12

### 新增 (Added)
- 项目初始工程结构搭建与 Fcitx5 DBus 注入通信模块 (`src/fcitx/`)。
- 端云混合语音识别 Trait 抽象及 Sherpa-ONNX 离线适配接口 (`src/asr/`)。
- 本地 SQLite 剪贴板历史与快捷短语管理模块 (`src/clipboard/`)。
- 局域网 P2P 发现与 AES-GCM 端到端加密同步协议骨架。
- 自动化构建部署脚本 (`scripts/install.sh`) 与离线模型自动拉取脚本 (`scripts/download_models.sh`)。
- 仿微信键盘风格的 Fcitx5 极简高雅主题 (`themes/wetype-clean/`)。
- Systemd 用户服务 (`systemd/wetype.service`) 与桌面自启动配置。
- GitHub Actions 持续集成自动化工作流。
