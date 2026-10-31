#!/usr/bin/env bash
set -euo pipefail

echo "=========================================================="
echo "    WeType-Linux 深度集成 Fcitx5 与系统级微信键盘配置"
echo "=========================================================="

HOME_DIR="${HOME}"
FCITX5_DATA="${HOME_DIR}/.local/share/fcitx5"
FCITX5_CONFIG="${HOME_DIR}/.config/fcitx5"
BIN_DIR="${HOME_DIR}/.local/bin"

# 1. 安装微信键盘主题 (防御性遍历: 仅部署仓库中实际存在的主题目录)
echo "--> 1. 部署 Fcitx5 微信键盘视觉主题..."
mkdir -p "${FCITX5_DATA}/themes"
for theme_dir in themes/wetype-*; do
    if [ -d "${theme_dir}" ]; then
        cp -r "${theme_dir}" "${FCITX5_DATA}/themes/"
    fi
done
echo "   ✓ 主题已安装至 ${FCITX5_DATA}/themes/"

# 2. 启用微信暗黑主题 (wetype-dark)
echo "--> 2. 激活候选框微信键盘主题..."
mkdir -p "${FCITX5_CONFIG}/conf"
cat > "${FCITX5_CONFIG}/conf/classicui.conf" << 'EOF'
# 垂直或水平候选列表
Vertical Candidate List=False
# 使用微信键盘专属主题
Theme=wetype-dark
# 候选框字体
Font="Noto Sans CJK SC 11"
# 菜单字体
MenuFont="Noto Sans CJK SC 10"
# 首选 DPI 缩放
PerScreenDPI=True
EOF
echo "   ✓ classicui.conf 配置已生效 (Theme=wetype-dark)"

# 3. 配置 Rime 拼音引擎为「微信键盘」
echo "--> 3. 配置系统输入法名称为「微信键盘」..."
RIME_DIR="${FCITX5_DATA}/rime"
mkdir -p "${RIME_DIR}"

cat > "${RIME_DIR}/default.custom.yaml" << 'EOF'
patch:
  "schema_list/@0":
    schema: luna_pinyin_simp
EOF

cat > "${RIME_DIR}/luna_pinyin_simp.custom.yaml" << 'EOF'
patch:
  "schema/name": "微信键盘"
EOF

# 触发 Rime 重新部署
if command -v rime_deployer &>/dev/null; then
    echo "   * 正在部署 Rime 拼音架构..."
    rime_deployer --build "${RIME_DIR}" || true
fi
echo "   ✓ 拼音核心已定制为「微信键盘」"

# 4. 配置 GNOME 系统全局快捷键 (Super+V 语音，Ctrl+Alt+W 悬浮药丸)
if command -v gsettings &>/dev/null; then
    echo "--> 4. 注册 GNOME 全局快捷键..."
    KB_VOICE="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/wetype-voice/"
    KB_PILL="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/wetype-pill/"

    gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "['$KB_VOICE', '$KB_PILL']" || true

    gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB_VOICE name "微信键盘语音输入" || true
    gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB_VOICE command "${BIN_DIR}/wetype-linux voice-input" || true
    gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB_VOICE binding "<Super>v" || true

    gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB_PILL name "微信键盘药丸悬浮窗" || true
    gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB_PILL command "${BIN_DIR}/wetype-linux gui" || true
    gsettings set org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$KB_PILL binding "<Control><Alt>w" || true

    echo "   ✓ 全局快捷键已注册："
    echo "     - Super + V      : 唤起 4 秒快速语音转文字自动上屏"
    echo "     - Ctrl + Alt + W : 打开/关闭 微信键盘药丸悬浮胶囊"
fi

# 5. 优雅重启 Fcitx5 使配置立即生效
echo "--> 5. 重启 Fcitx5 输入法服务..."
if command -v fcitx5-remote &>/dev/null; then
    fcitx5-remote -r || true
fi

echo "=========================================================="
echo "          Fcitx5 微信键盘系统集成已全部就绪！"
echo "  1. 拼音候选框已蜕变为微信键盘主题风格"
echo "  2. 随时按 Super + V 即可极速离线语音打字"
echo "  3. 随时按 Ctrl + Alt + W 呼出微信药丸悬浮窗"
echo "=========================================================="
