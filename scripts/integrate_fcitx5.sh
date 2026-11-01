#!/usr/bin/env bash
set -euo pipefail

echo "=========================================================="
echo "    WeType-Linux 深度集成 Fcitx5 与 Nord 风格视觉配置"
echo "=========================================================="

# 自定位仓库根目录 (脚本可在任意 CWD 下执行)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(dirname "${SCRIPT_DIR}")"
cd "${REPO_DIR}"

HOME_DIR="${HOME}"
FCITX5_DATA="${HOME_DIR}/.local/share/fcitx5"
FCITX5_CONFIG="${HOME_DIR}/.config/fcitx5"
BIN_DIR="${HOME_DIR}/.local/bin"

# 1. 安装候选框主题与图标 (防御性遍历: 仅部署仓库中实际存在的主题目录)
echo "--> 1. 部署 Fcitx5 候选框主题与应用图标..."
mkdir -p "${FCITX5_DATA}/themes"
for theme_dir in themes/wetype-*; do
    if [ -d "${theme_dir}" ]; then
        cp -r "${theme_dir}" "${FCITX5_DATA}/themes/"
    fi
done
echo "   ✓ 主题已安装至 ${FCITX5_DATA}/themes/"

# 2. 启用 Midnight 暗色主题 (wetype-midnight)
echo "--> 2. 激活候选框 Midnight 暗色主题..."
mkdir -p "${FCITX5_CONFIG}/conf"
cat > "${FCITX5_CONFIG}/conf/classicui.conf" << 'EOF'
# 垂直或水平候选列表
Vertical Candidate List=False
# 使用 WeType Nord 专属主题
Theme=wetype-midnight
# 候选框字体
Font="Noto Sans CJK SC 11"
# 菜单字体
MenuFont="Noto Sans CJK SC 10"
# 首选 DPI 缩放
PerScreenDPI=True
EOF
echo "   ✓ classicui.conf 配置已生效 (Theme=wetype-midnight)"

# 3. 配置 Rime 拼音引擎显示名
echo "--> 3. 配置 Rime 输入法显示名..."
RIME_DIR="${FCITX5_DATA}/rime"
mkdir -p "${RIME_DIR}"

cat > "${RIME_DIR}/default.custom.yaml" << 'EOF'
patch:
  "schema_list/@0":
    schema: luna_pinyin_simp
EOF

cat > "${RIME_DIR}/luna_pinyin_simp.custom.yaml" << 'EOF'
patch:
  "schema/name": "WeType 键盘"
EOF

# 触发 Rime 重新部署
if command -v rime_deployer &>/dev/null; then
    echo "   * 正在部署 Rime 拼音架构..."
    rime_deployer --build "${RIME_DIR}" || true
fi
echo "   ✓ 拼音核心已定制为「WeType 键盘」"

# 4. 配置 GNOME 系统全局快捷键 (Super+V 快速语音)
if command -v gsettings &>/dev/null; then
    echo "--> 4. 注册 GNOME 全局快捷键..."
    KB_VOICE="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/wetype-voice/"

    gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "['$KB_VOICE']" || true

    gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB_VOICE name "WeType 语音输入" || true
    gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB_VOICE command "${BIN_DIR}/wetype-linux voice-input" || true
    gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB_VOICE binding "<Super>v" || true

    echo "   ✓ 全局快捷键已注册："
    echo "     - Super + V : 唤起 4 秒快速语音转文字自动上屏"
fi

# 5. 安装应用图标 (Nord 风格, 基于 Lucide 图形重绘)
echo "--> 5. 安装应用图标..."
ICON_DIR="${HOME_DIR}/.local/share/icons/hicolor/scalable/apps"
mkdir -p "${ICON_DIR}"
if [ -f "icons/hicolor/scalable/apps/wetype-linux.svg" ]; then
    install -m 644 "icons/hicolor/scalable/apps/wetype-linux.svg" "${ICON_DIR}/wetype-linux.svg"
    gtk-update-icon-cache -q -t -f "${HOME_DIR}/.local/share/icons/hicolor" 2>/dev/null || true
    echo "   ✓ 应用图标已安装至 ${ICON_DIR}/wetype-linux.svg"
fi

# 6. 优雅重启 Fcitx5 使配置立即生效
echo "--> 6. 重启 Fcitx5 输入法服务..."
if command -v fcitx5-remote &>/dev/null; then
    fcitx5-remote -r || true
fi

echo "=========================================================="
echo "          Fcitx5 集成已全部就绪！"
echo "  1. 拼音候选框已应用 WeType Midnight 主题 (Nord 风)"
echo "  2. 随时按 Super + V 即可极速离线语音打字"
echo "  3. 剪贴板同步与常用语库由后台服务自动维护"
echo "=========================================================="
