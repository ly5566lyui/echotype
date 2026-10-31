#!/usr/bin/env bash
set -euo pipefail

echo "=========================================================="
echo "          WeType-Linux 自动化安装与部署脚本"
echo "=========================================================="

# 1. 检查 Rust/Cargo 环境
if ! command -v cargo &>/dev/null; then
    echo "错误: 未找到 Cargo 环境。请先安装 Rust: https://rustup.rs/" >&2
    exit 1
fi

# 1.5 构建依赖与运行环境检查 (仅提示，不代装)
if command -v apt-get &>/dev/null; then
    dpkg -s libasound2-dev &>/dev/null || echo "提示: 建议安装 ALSA 构建依赖: sudo apt install -y libasound2-dev" >&2
    command -v fcitx5 &>/dev/null || echo "提示: 未检测到 Fcitx5，上屏功能依赖它: sudo apt install -y fcitx5 fcitx5-rime" >&2
elif command -v pacman &>/dev/null; then
    pacman -Q alsa-lib &>/dev/null || echo "提示: 建议安装 ALSA 构建依赖: sudo pacman -S alsa-lib" >&2
    command -v fcitx5 &>/dev/null || echo "提示: 未检测到 Fcitx5: sudo pacman -S fcitx5 fcitx5-rime rime-ice" >&2
fi

# 2. 编译 Release 二进制文件
echo "--> 正在编译 Release 版本..."
cargo build --release

# 3. 安装可执行文件至 ~/.local/bin
BIN_DIR="${HOME}/.local/bin"
mkdir -p "${BIN_DIR}"
install -m 755 target/release/wetype-linux "${BIN_DIR}/wetype-linux"
echo "✓ 二进制程序已安装至: ${BIN_DIR}/wetype-linux"

# 4. 初始化默认配置文件
CONFIG_DIR="${HOME}/.config/wetype"
mkdir -p "${CONFIG_DIR}"
if [ ! -f "${CONFIG_DIR}/config.toml" ]; then
    cp config.example.toml "${CONFIG_DIR}/config.toml"
    echo "✓ 默认配置文件已生成至: ${CONFIG_DIR}/config.toml"
else
    echo "ℹ 已存在配置文件，跳过覆盖: ${CONFIG_DIR}/config.toml"
fi

# 5. 安装 Systemd 用户服务
SYSTEMD_DIR="${HOME}/.config/systemd/user"
mkdir -p "${SYSTEMD_DIR}"
install -m 644 systemd/wetype.service "${SYSTEMD_DIR}/wetype.service"
systemctl --user daemon-reload || true
echo "✓ Systemd 用户服务已就绪: systemctl --user enable --now wetype"

# 6. 安装桌面应用快捷方式与自启动项
APP_DIR="${HOME}/.local/share/applications"
mkdir -p "${APP_DIR}"
install -m 644 wetype.desktop "${APP_DIR}/wetype.desktop"

DESKTOP_DIR="${HOME}/Desktop"
if [ -d "${DESKTOP_DIR}" ]; then
    install -m 755 wetype.desktop "${DESKTOP_DIR}/wetype.desktop"
    gio set -t string "${DESKTOP_DIR}/wetype.desktop" metadata::trusted true || true
fi

AUTOSTART_DIR="${HOME}/.config/autostart"
mkdir -p "${AUTOSTART_DIR}"
install -m 644 wetype.desktop "${AUTOSTART_DIR}/wetype.desktop"
echo "✓ 微信键盘助手桌面快捷方式与自启动已安装"

echo "=========================================================="
echo "安装完成！你可以通过以下方式启动 WeType-Linux："
echo "  1. 后台守护进程模式: systemctl --user start wetype"
echo "  2. 手动前台调试模式: wetype-linux daemon"
echo "=========================================================="
