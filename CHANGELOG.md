# 更新日志 (Changelog)

本项目遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.0.0/) 格式。

## [0.3.0] - 2026-11-01

### Changed (视觉重制 · 去商业化)
- **候选框主题整体换新**: 采用 Nord 配色体系新增 `WeType Midnight` (暗色) / `WeType Daylight` (亮色) 双主题，替代原绿色主题 (旧主题保留于 git 历史)
- **原创应用图标**: 键盘 + 声波双意象 SVG (Nord 配色)，图形参考 [Lucide](https://lucide.dev) (ISC) 重绘；集成脚本自动安装至 hicolor 图标目录
- 品牌文案全面中性化：集成脚本、桌面入口 (`WeType 输入法助手`)、README；免责声明中的商标说明保持不变
- 移除悬浮胶囊后同步清理集成脚本中的药丸快捷键绑定 (gui 子命令已不存在)

## [0.2.0] - 2026-10-27

> ✅ **构建验证**: 已在 Debian 13 (rustc 1.99.1, fcitx5 5.1.12) 虚拟机实测 —— `cargo build --release` 产出 18.6MB 单二进制，`cargo check` 零警告，**19/19 单元测试全部通过**；CLI (`phrases`/`daemon`/`test-fcitx`) 运行时验证通过，Fcitx5 DBus 探活与 Lua 桥自动部署正常。无头会话无法触达的部分 (GUI/热键/语音/双机 P2P) 需桌面会话验证。

### 新增 (Added)
- **设置持久化**: 设置中心窗口全回调接线，保存写入 `~/.config/wetype/config.toml` (`AppConfig::save_to_path`)；新增 `ui_theme`、`candidate_count`、`asr.language`、`asr.auto_punctuation` 配置项；候选词数量 best-effort 同步至 Fcitx5 `pinyin.conf`。
- **全局热键** (`src/hotkey/`): X11 走 `global-hotkey` (XGrabKey)，Wayland 走 XDG Desktop Portal `GlobalShortcuts` (zbus 纯 Rust 客户端，含权限弹窗流程)；X11 失败自动回退 Portal。GUI 与 daemon 均已接入：语音热键 = 开始/停止录音切换，剪贴板热键 = 抽屉唤起 (隐藏状态下自动先唤起窗口)。
- **局域网 P2P 剪贴板同步** (`src/clipboard/sync_p2p.rs` 完整实现): mDNS (`_wetype._tcp.local.`) 自动发现 + TCP 长度前缀帧 + AES-256-GCM 端到端加密 (SHA-256 派生自 `pair_secret`)；空口令拒绝启用；10 秒防回声；附加加解密单元测试。
- **常用语库管理**: SQLite 常用语读取 (`get_quick_phrases`) 与设置窗口整体覆盖 (`replace_quick_phrases`)；悬浮胶囊初始列表优先取常用语库。
- 剪贴板监听与 P2P 同步在 GUI 模式下后台并行运行 (不依赖 daemon 模式)。
- 「上屏后自动隐藏」与「允许拖拽」设置即时生效；数据目录迁移至 XDG 规范路径 `~/.local/share/wetype`。

### 优化 (Improved)
- **Fcitx5 连接复用**: 全应用统一走 `fcitx::commit_string_cached`，复用共享 DBus 连接，连接失效 (Fcitx5 重启) 自动重连；不再每次上屏重建连接并检查 Lua 扩展。
- **录音链路**: 新增能量法静音裁剪 (VAD-lite，去掉首尾静音降低误识别) 与 60 秒录音上限保护 (忘记按停止键自动停止并识别)；`stop()` 以完成标志等待替代固定 sleep，消除重采样竞态。
- **剪贴板**: `max_history_items` 上限裁剪生效；新增敏感内容启发式过滤 (私钥/令牌/密码等不入库不同步，`ignore_sensitive` 开关)。
- **隐私**: `/tmp/wetype_commit.txt` 权限收紧为 0600；移除无限增长的 Lua 调试日志文件。
- **配置兼容**: 所有配置段支持缺省字段/缺省段 (serde 默认值)，旧版不完整配置文件可正常加载。
- **单元测试**: 新增 config 往返/缺省解析、常用语库、历史裁剪、静音裁剪、敏感过滤等测试 (共 15 例)。
- `install.sh` 增加 ALSA/Fcitx5 依赖检查提示；README 补充构建依赖与 Wayland Portal/P2P 说明。

### 修复 (Fixed)
- **global-hotkey 0.6.4 API 错配**（经 docs.rs 在线核实）: `HotKey`/`Code`/`Modifiers` 需从 `hotkey` 子模块导入；`GlobalHotKeyEvent.id` 为 `u32`（配合 `HotKey::id()` 建查找表）；事件分发改用 `set_event_handler` 回调，弃用版本间签名不稳定的 receiver 通道。
- **Wayland 剪贴板**：`arboard` 启用 `wayland-data-control` feature（此前被 `default-features = false` 关闭，Wayland 下剪贴板读写只能依赖 XWayland）。
- **P2P 重放保护**：同步消息新增时间戳新鲜度校验（±30s 窗口），拒绝嗅探重放的历史密文帧；P2P 入站连接新增 16 路并发上限。
- **隐私**：云端识别上传的临时 WAV 文件权限收紧 0600 并在上传后立即删除（此前永久残留 `/tmp`）。
- `phrases --list` 此前误列剪贴板历史，现正确列出常用语；新增 `phrases --history` 列剪贴板历史。
- 守护进程数据目录由相对路径 `data/` 改为 XDG 数据目录。
- `scripts/integrate_fcitx5.sh`: Fcitx5 配置文件名修正为 `classicui.conf` (原 `classicui.config` 不会被 Fcitx5 读取)；主题部署改为防御性遍历 `themes/wetype-*`。

### 移除 (Removed)
- `scripts/wetype_gui.py`: 移除早期 GTK4 Python 原型，已被纯 Rust Slint UI 取代 (践行"零 Python 依赖"定位，代码保留于 git 历史)。

### 文档与工程化 (Docs & Tooling)
- 完成同类项目调研 ([fcitx5-voice-input](https://github.com/devcxl/fcitx5-voice-input)、vocalinux、espanso、KDE Connect、LocalSend 等)，结论与路线图归档至 `docs/dev/RESEARCH.md`；`docs/architecture.md` 三处失实描述 (流式识别/服务类型/回退策略) 修正为与代码一致。
- 新增 GitHub Actions CI (Linux 构建 + 全量单元测试) 与 CI 徽章。
- 新增 Issue 模板 (Bug 报告 / 功能建议) 与 `.editorconfig`。
- README 新增功能完成度表、项目结构图与开发测试指南。
- `docs/dev/` 归档开发纪要，`docs/` 仅保留正式文档。
- `wetype.desktop` 增加 `StartupWMClass`；CONTRIBUTING 移除多余的 `libdbus-1-dev` 依赖说明 (zbus 为纯 Rust 实现)。
- `src/main.rs` 增加 crate 级模块导览注释。

### 变更 (Changed)
- `rusqlite` 启用 `bundled` (无需系统 libsqlite3-dev)；`toml` 启用 `display`；新增依赖 `global-hotkey`、`mdns-sd`、`aes-gcm`、`sha2`、`dirs`、`futures-util`。

## [0.1.0] - 2026-10-12

### 新增 (Added)
- 项目初始工程结构搭建与 Fcitx5 DBus 注入通信模块 (`src/fcitx/`)。
- 端云混合语音识别 Trait 抽象及 Sherpa-ONNX 离线适配接口 (`src/asr/`)。
- 本地 SQLite 剪贴板历史与快捷短语管理模块 (`src/clipboard/`)。
- 局域网 P2P 发现与 AES-GCM 端到端加密同步协议骨架。
- 自动化构建部署脚本 (`scripts/install.sh`) 与离线模型自动拉取脚本 (`scripts/download_models.sh`)。
- 仿商业输入法风格的 Fcitx5 极简高雅主题 (`themes/wetype-clean/`)。
- Systemd 用户服务 (`systemd/wetype.service`) 与桌面自启动配置。
- GitHub Actions 持续集成自动化工作流。
