# EchoType-Linux 系统架构设计

## 1. 核心设计原则

- **松耦合守护进程 (Loosely-coupled Daemon)**: 不侵入修改 Fcitx5 核心 C++ 代码，通过标准 DBus API 与其协作。即使 EchoType-Linux 发生重载或退出，Fcitx5 本身依然能够正常打字，保证极高的系统可用性。
- **Wayland / X11 协议中立**: 通过直接调用 `org.fcitx.Fcitx5.InputMethod.CommitString` 将文字打入当前输入法活跃上下文，完全无需关心合成器（GNOME Wayland、KDE Plasma、Sway、XFCE）对按键模拟的权限限制。
- **端云双引擎透明回退**: 语音识别采用透明回退策略：云端已配置时优先请求云端 API，请求失败（断网/超时/鉴权错误）自动回退至本地 ONNX 模型，对用户无感。

## 2. 模块细化

### 2.1 Fcitx5 DBus 交互模块 (`src/fcitx/`)
- 维护对 Session DBus 的异步长连接。
- 监听 `org.fcitx.Fcitx5.InputContext` 获得当前聚焦窗口的输入上下文路径。
- 实现 `commit_string(&str)`，将转换后的文字直接注入目标应用程序光标位置。

### 2.2 语音识别与音频采集 (`src/asr/`)
- `recorder.rs`：使用 `cpal` 监听默认系统麦克风，以 16kHz 单声道 16bit PCM 采样音频流；内置能量法静音裁剪 (VAD-lite，20ms 帧 RMS 判定) 与 60 秒录音时长上限保护。
- `local_sherpa.rs`：封装 Sherpa-ONNX 离线推理接口 (调用 `sherpa-onnx-offline` 预编译引擎)。
- `cloud_api.rs`：封装 OpenAI Whisper 兼容的 REST 云端识别请求 (multipart 上传 16kHz WAV，非流式)，支持 Whisper/SiliconFlow/DashScope 等端点。

### 2.3 剪贴板与常用语系统 (`src/clipboard/`)
- `monitor.rs`：通过 `arboard` 轮询监听系统剪贴板变化 (启用 `wayland-data-control`，原生支持 Wayland)，去重并启发式过滤敏感凭据内容。
- `storage.rs`：SQLite 存储剪贴板历史与用户自定义常用短语（如常用地址、纳税号、代码片段等），历史按 `max_history_items` 自动裁剪。
- `sync_p2p.rs`：基于 `mdns-sd` 在局域网自动广播服务 `_echotype._tcp.local.`，使用预共享密钥派生 AES-256-GCM 密钥收发剪贴板同步包；带时间戳重放保护 (±30s 窗口)、远端内容 Echo 抑制与并发连接上限。
