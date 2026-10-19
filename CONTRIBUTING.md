# 贡献指南 (Contributing Guide)

感谢关注并愿意为 WeType-Linux 做出贡献！

## 开发环境搭建

1. **安装基础依赖 (Ubuntu/Debian)**:
   ```bash
   sudo apt install -y build-essential libasound2-dev libdbus-1-dev
   ```
2. **安装 Rust 工具链**:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
3. **运行测试**:
   ```bash
   cargo check
   cargo test
   ```

## 代码规范与 PR 提交流程

- 提交代码前请务必运行：
  ```bash
  cargo fmt
  cargo clippy -- -D warnings
  ```
- 保持模块间高内聚低耦合，新的 ASR 引擎需实现 `src/asr/mod.rs` 中的 `AsrEngine` trait。
