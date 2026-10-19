# Fcitx5 + Rime (雾凇拼音) Linux 快速配置指南

为了让 Linux 上的拼音打字体验达到类似商业输入法的词库质量，建议使用 **Rime + 雾凇拼音 (rime-ice)** 作为拼音基座。

---

## 1. 安装软件包

### Arch Linux / Manjaro
```bash
sudo pacman -S fcitx5 fcitx5-configtool fcitx5-gtk fcitx5-qt fcitx5-rime rime-ice
```

### Ubuntu 22.04 / 24.04 / Debian
```bash
sudo apt update
sudo apt install fcitx5 fcitx5-rime fcitx5-config-qt librime-data
```

---

## 2. 安装雾凇拼音 (rime-ice)

如果你的发行版仓库中没有直接打包 `rime-ice`，可以通过 Git 直接拉取到 Rime 用户数据目录：

```bash
# 进入 Rime 配置目录
mkdir -p ~/.local/share/fcitx5/rime
cd ~/.local/share/fcitx5/rime

# 克隆雾凇拼音仓库
git clone --depth 1 https://github.com/iDvel/rime-ice.git temp_rime
cp -r temp_rime/* .
rm -rf temp_rime

# 重新部署 Rime
# 或者右键点击系统托盘 Fcitx5 图标 -> 重新部署 (Deploy)
```

---

## 3. 设置默认方案

在 `~/.local/share/fcitx5/rime/default.custom.yaml` 中配置：

```yaml
patch:
  schema_list:
    - schema: rime_ice       # 雾凇拼音（全拼）
    - schema: double_pinyin  # 若需要自然码双拼
```

重启 Fcitx5 服务或执行部署后，即可享受到具备现代互联网热词、Emoji 联想和纠错能力的打字体验。
