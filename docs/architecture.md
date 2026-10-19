# WeType-Linux 系统架构设计

## 1. 核心设计原则

- **松耦合守护进程 (Loosely-coupled Daemon)**: 不侵入修改 Fcitx5 核心 C++ 代码，通过标准 DBus API 与其协作。即使 WeType-Linux 发生重载或退出，Fcitx5 本身依然能够正常打字，保证极高的系统可用性。
- **Wayland / X11 协议中立**: 通过直接调用 `org.fcitx.Fcitx5.InputMethod.CommitString` 将文字打入当前输入法活跃上下文，完全无需关心合成器（GNOME Wayland、KDE Plasma、Sway、XFCE）对按键模拟的权限限制。
- **端云双引擎透明回退**: 语音识别采用透明回退策略：优先在后台探测云端连接性，网络通畅时提供高准确率，断网时秒级切换至本地 ONNX 模型。

## 2. 模块细化

### 2.1 Fcitx5 DBus 交互模块 (`src/fcitx/`)
- 维护对 Session DBus 的异步长连接。
- 监听 `org.fcitx.Fcitx5.InputContext` 获得当前聚焦窗口的输入上下文路径。
- 实现 `commit_string(&str)`，将转换后的文字直接注入目标应用程序光标位置。

### 2.2 语音识别与音频采集 (`src/asr/`)
- `recorder.rs`：使用 `cpal` 监听默认系统麦克风，以 16kHz 单声道 16bit PCM 采样音频流，并结合简易能量 VAD 进行静音断句。
- `local_sherpa.rs`：封装 Sherpa-ONNX 离线推理接口。
- `cloud_api.rs`：封装 WebSocket/REST 云端流式语音识别请求。

### 2.3 剪贴板与常用语系统 (`src/clipboard/`)
- `monitor.rs`：通过 `arboard` 轮询/监听系统剪贴板变化，去重并过滤密码类敏感内容。
- `storage.rs`：SQLite 存储剪贴板历史与用户自定义常用短语（如常用地址、纳税号、代码片段等）。
- `sync_p2p.rs`：基于 `mdns-sd` 在局域网自动广播服务 `_wetype_sync._tcp`，使用预共享密钥派生 AES-256-GCM 密钥，广播与接收多端剪贴板同步数据包。
