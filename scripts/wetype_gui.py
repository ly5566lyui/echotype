#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
WeType-Linux Desktop Application (GTK4 + Libadwaita)
Inspired by modern WeChat Keyboard UX.
"""

import os
import sys
import subprocess
import threading
import time
import sqlite3
from pathlib import Path

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Gtk, Adw, GLib, Gdk, Pango

APP_CSS = """
window {
    background-color: #f7f8fa;
}
.header-banner {
    background: linear-gradient(135deg, #07c160 0%, #059e4e 100%);
    color: white;
    border-radius: 16px;
    padding: 18px;
    margin-bottom: 6px;
}
.title-large {
    font-size: 20px;
    font-weight: 800;
}
.subtitle-light {
    font-size: 13px;
    opacity: 0.92;
}
.card-box {
    background-color: #ffffff;
    border-radius: 14px;
    padding: 16px;
    border: 1px solid #e6e8eb;
}
.mic-btn-idle {
    background-color: #07c160;
    color: white;
    font-weight: bold;
    font-size: 16px;
    border-radius: 28px;
    padding: 12px 28px;
}
.mic-btn-recording {
    background-color: #fa5151;
    color: white;
    font-weight: bold;
    font-size: 16px;
    border-radius: 28px;
    padding: 12px 28px;
}
.action-btn {
    border-radius: 10px;
    padding: 6px 14px;
    font-weight: 600;
}
.result-view {
    font-size: 15px;
    padding: 10px;
    border-radius: 10px;
    background-color: #f2f4f7;
}
.phrase-row {
    padding: 8px 10px;
    border-bottom: 1px solid #f0f0f0;
}
.status-badge {
    background-color: rgba(255, 255, 255, 0.22);
    border-radius: 20px;
    padding: 4px 12px;
    font-size: 12px;
    font-weight: 600;
}
"""


class WeTypeWindow(Adw.ApplicationWindow):
    def __init__(self, app):
        super().__init__(application=app)
        self.set_title("WeType-Linux (微信输入法 Linux 桌面端)")
        self.set_default_size(540, 720)

        self.home = Path.home()
        self.wetype_bin = self.home / ".local/bin/wetype-linux"
        if not self.wetype_bin.exists():
            self.wetype_bin = self.home / "wetype-linux/target/debug/wetype-linux"

        self.db_path = self.home / ".local/share/wetype/phrases.db"
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self._init_db()

        self.is_recording = False
        self.rec_process = None
        self.rec_start_time = 0.0
        self.rec_wav_path = Path("/tmp/wetype_gui_rec.wav")

        self._build_ui()
        self._check_fcitx_status()

    def _init_db(self):
        conn = sqlite3.connect(self.db_path)
        c = conn.cursor()
        c.execute(
            """CREATE TABLE IF NOT EXISTS phrases (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                text TEXT NOT NULL UNIQUE
            )"""
        )
        c.execute("SELECT COUNT(*) FROM phrases")
        if c.fetchone()[0] == 0:
            defaults = [
                "你好，我正在使用 WeType-Linux 语音输入法！",
                "收到，我马上处理，稍后回复您。",
                "请问方便发一下相关文档或截图吗？",
                "地址：贵州省贵阳市花溪区贵州民族大学",
            ]
            for item in defaults:
                c.execute("INSERT OR IGNORE INTO phrases (text) VALUES (?)", (item,))
        conn.commit()
        conn.close()

    def _get_phrases(self):
        conn = sqlite3.connect(self.db_path)
        c = conn.cursor()
        c.execute("SELECT id, text FROM phrases ORDER BY id DESC LIMIT 20")
        rows = c.fetchall()
        conn.close()
        return rows

    def _add_phrase(self, text):
        text = text.strip()
        if not text:
            return
        conn = sqlite3.connect(self.db_path)
        c = conn.cursor()
        c.execute("INSERT OR IGNORE INTO phrases (text) VALUES (?)", (text,))
        conn.commit()
        conn.close()

    def _delete_phrase(self, pid):
        conn = sqlite3.connect(self.db_path)
        c = conn.cursor()
        c.execute("DELETE FROM phrases WHERE id = ?", (pid,))
        conn.commit()
        conn.close()

    def _build_ui(self):
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.set_content(main_box)

        # HeaderBar
        header = Adw.HeaderBar()
        main_box.append(header)

        # Scrolled Content
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_vexpand(True)
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        main_box.append(scrolled)

        content_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=14)
        content_box.set_margin_top(14)
        content_box.set_margin_bottom(20)
        content_box.set_margin_start(16)
        content_box.set_margin_end(16)
        scrolled.set_child(content_box)

        # 1. Top Banner Card
        banner = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        banner.add_css_class("header-banner")

        title_col = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        title_col.set_hexpand(True)
        lbl_title = Gtk.Label(label="WeType · 智能输入助手", xalign=0)
        lbl_title.add_css_class("title-large")
        lbl_sub = Gtk.Label(
            label="Fcitx5 + Rime 雾凇拼音 | Sherpa-ONNX 离线/云端混合语音", xalign=0
        )
        lbl_sub.add_css_class("subtitle-light")
        title_col.append(lbl_title)
        title_col.append(lbl_sub)

        self.status_badge = Gtk.Label(label="● 检测 Fcitx5...")
        self.status_badge.add_css_class("status-badge")
        self.status_badge.set_valign(Gtk.Align.CENTER)

        banner.append(title_col)
        banner.append(self.status_badge)
        content_box.append(banner)

        # 2. Voice Input Studio Card
        voice_card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        voice_card.add_css_class("card-box")

        voice_header = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        lbl_voice_title = Gtk.Label(label="🎙️ 语音输入工作台", xalign=0)
        lbl_voice_title.set_markup("<b>🎙️ 语音输入工作台</b>")
        lbl_voice_title.set_hexpand(True)

        self.mode_combo = Gtk.DropDown.new_from_strings(
            ["端云混合 (自动降级)", "仅本地离线 (Paraformer)", "仅云端 API"]
        )
        self.mode_combo.set_selected(0)
        voice_header.append(lbl_voice_title)
        voice_header.append(self.mode_combo)
        voice_card.append(voice_header)

        # Mic Button & Timer Row
        mic_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        mic_row.set_halign(Gtk.Align.CENTER)
        mic_row.set_margin_top(6)
        mic_row.set_margin_bottom(4)

        self.mic_btn = Gtk.Button(label="🎙️ 点击开始说话")
        self.mic_btn.add_css_class("mic-btn-idle")
        self.mic_btn.connect("clicked", self.on_mic_clicked)
        mic_row.append(self.mic_btn)

        sample_btn = Gtk.Button(label="🎧 测试示例语音")
        sample_btn.add_css_class("action-btn")
        sample_btn.set_tooltip_text("使用内置中文语音样本测试 Sherpa-ONNX 神经网络识别")
        sample_btn.connect("clicked", self.on_sample_clicked)
        mic_row.append(sample_btn)

        voice_card.append(mic_row)

        self.lbl_voice_state = Gtk.Label(
            label="就绪：点击上方绿色按钮对着麦克风说话，或点击右侧测试示例语音"
        )
        self.lbl_voice_state.set_wrap(True)
        voice_card.append(self.lbl_voice_state)

        # Recognized text editor
        self.text_view = Gtk.TextView()
        self.text_view.set_wrap_mode(Gtk.WrapMode.WORD_CHAR)
        self.text_view.set_top_margin(8)
        self.text_view.set_bottom_margin(8)
        self.text_view.set_left_margin(10)
        self.text_view.set_right_margin(10)
        self.text_view.set_size_request(-1, 74)
        self.text_view.add_css_class("result-view")
        self.text_buffer = self.text_view.get_buffer()
        self.text_buffer.set_text("你好，欢迎体验 Linux 版微信输入法助手！")
        voice_card.append(self.text_view)

        # Action Buttons for Recognized Text
        act_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        btn_commit_now = Gtk.Button(label="⚡ 立即上屏")
        btn_commit_now.add_css_class("action-btn")
        btn_commit_now.connect("clicked", self.on_commit_now_clicked)

        btn_commit_delay = Gtk.Button(label="⏱️ 2秒后上屏 (切换窗口)")
        btn_commit_delay.add_css_class("action-btn")
        btn_commit_delay.connect("clicked", self.on_commit_delay_clicked)

        btn_copy = Gtk.Button(label="📋 复制文本")
        btn_copy.add_css_class("action-btn")
        btn_copy.connect("clicked", self.on_copy_clicked)

        act_row.append(btn_commit_now)
        act_row.append(btn_commit_delay)
        act_row.append(btn_copy)
        voice_card.append(act_row)

        content_box.append(voice_card)

        # 3. Quick Phrases & Clipboard Card
        phrase_card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        phrase_card.add_css_class("card-box")

        lbl_phrase_title = Gtk.Label(xalign=0)
        lbl_phrase_title.set_markup("<b>💬 快捷常用语与模板 (点击即可上屏)</b>")
        phrase_card.append(lbl_phrase_title)

        # Add phrase input row
        add_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
        self.phrase_entry = Gtk.Entry()
        self.phrase_entry.set_placeholder_text("输入新的常用语、邮箱、地址或代码片段...")
        self.phrase_entry.set_hexpand(True)
        self.phrase_entry.connect("activate", self.on_add_phrase_clicked)

        btn_add_phrase = Gtk.Button(label="➕ 添加")
        btn_add_phrase.add_css_class("action-btn")
        btn_add_phrase.connect("clicked", self.on_add_phrase_clicked)

        add_row.append(self.phrase_entry)
        add_row.append(btn_add_phrase)
        phrase_card.append(add_row)

        self.phrases_list_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        phrase_card.append(self.phrases_list_box)
        self._refresh_phrases_ui()

        content_box.append(phrase_card)

    def _refresh_phrases_ui(self):
        child = self.phrases_list_box.get_first_child()
        while child is not None:
            next_child = child.get_next_sibling()
            self.phrases_list_box.remove(child)
            child = next_child

        for pid, text in self._get_phrases():
            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            row.add_css_class("phrase-row")

            lbl = Gtk.Label(label=text, xalign=0)
            lbl.set_hexpand(True)
            lbl.set_ellipsize(Pango.EllipsizeMode.END)
            lbl.set_max_width_chars(38)
            lbl.set_tooltip_text(text)

            btn_send = Gtk.Button(label="2s上屏")
            btn_send.connect("clicked", lambda _b, t=text: self._delayed_commit(t, 2))

            btn_cp = Gtk.Button(label="复制")
            btn_cp.connect("clicked", lambda _b, t=text: self._copy_to_clipboard(t))

            btn_del = Gtk.Button(label="✕")
            btn_del.connect("clicked", lambda _b, i=pid: self._on_delete_phrase(i))

            row.append(lbl)
            row.append(btn_send)
            row.append(btn_cp)
            row.append(btn_del)
            self.phrases_list_box.append(row)

    def _on_delete_phrase(self, pid):
        self._delete_phrase(pid)
        self._refresh_phrases_ui()

    def on_add_phrase_clicked(self, _widget):
        text = self.phrase_entry.get_text().strip()
        if text:
            self._add_phrase(text)
            self.phrase_entry.set_text("")
            self._refresh_phrases_ui()

    def _get_current_text(self):
        start = self.text_buffer.get_start_iter()
        end = self.text_buffer.get_end_iter()
        return self.text_buffer.get_text(start, end, False).strip()

    def _set_current_text(self, text):
        self.text_buffer.set_text(text)

    def _check_fcitx_status(self):
        def worker():
            try:
                res = subprocess.run(
                    [
                        "gdbus",
                        "call",
                        "--session",
                        "--dest",
                        "org.fcitx.Fcitx5",
                        "--object-path",
                        "/controller",
                        "--method",
                        "org.fcitx.Fcitx.Controller1.CurrentInputMethod",
                    ],
                    capture_output=True,
                    text=True,
                    timeout=3,
                )
                if res.returncode == 0:
                    im = res.stdout.strip().strip("()',")
                    GLib.idle_add(
                        self.status_badge.set_label, f"🟢 Fcitx5 已就绪 ({im})"
                    )
                else:
                    GLib.idle_add(self.status_badge.set_label, "🟡 Fcitx5 未激活")
            except Exception:
                GLib.idle_add(self.status_badge.set_label, "🔴 Fcitx5 未运行")

        threading.Thread(target=worker, daemon=True).start()

    def on_mic_clicked(self, _btn):
        if not self.is_recording:
            self._start_recording()
        else:
            self._stop_and_recognize()

    def _start_recording(self):
        if self.rec_wav_path.exists():
            self.rec_wav_path.unlink()

        self.is_recording = True
        self.rec_start_time = time.time()
        self.mic_btn.set_label("⏹️ 正在聆听... 点击停止识别")
        self.mic_btn.remove_css_class("mic-btn-idle")
        self.mic_btn.add_css_class("mic-btn-recording")
        self.lbl_voice_state.set_label("🎙️ 请对着麦克风说话（16kHz 单声道录制中）...")

        # 启动 arecord 采集 16kHz 16-bit 单声道 WAV
        self.rec_process = subprocess.Popen(
            [
                "arecord",
                "-q",
                "-f",
                "S16_LE",
                "-r",
                "16000",
                "-c",
                "1",
                str(self.rec_wav_path),
            ]
        )
        GLib.timeout_add(200, self._update_rec_timer)

    def _update_rec_timer(self):
        if not self.is_recording:
            return False
        elapsed = time.time() - self.rec_start_time
        self.lbl_voice_state.set_label(f"🔴 正在录音中... 已录制 {elapsed:.1f} 秒")
        return True

    def _stop_and_recognize(self):
        self.is_recording = False
        if self.rec_process is not None:
            self.rec_process.terminate()
            try:
                self.rec_process.wait(timeout=1)
            except Exception:
                self.rec_process.kill()
            self.rec_process = None

        self.mic_btn.set_label("🎙️ 点击开始说话")
        self.mic_btn.remove_css_class("mic-btn-recording")
        self.mic_btn.add_css_class("mic-btn-idle")
        self.lbl_voice_state.set_label("⏳ 正在调用 Sherpa-ONNX 神经网络识别语音...")

        self._run_asr_on_wav(self.rec_wav_path)

    def on_sample_clicked(self, _btn):
        sample0 = self.home / ".local/share/wetype/models/test_wavs/0.wav"
        sample1 = self.home / ".local/share/wetype/models/test_wavs/1.wav"
        target = sample0 if int(time.time()) % 2 == 0 else sample1
        if not target.exists():
            self.lbl_voice_state.set_label(f"⚠️ 未找到样本文件: {target}")
            return
        self.lbl_voice_state.set_label(
            f"⏳ 正在使用真实语音样本 ({target.name}) 执行 Sherpa-ONNX 识别..."
        )
        self._run_asr_on_wav(target)

    def _run_asr_on_wav(self, wav_path: Path):
        def worker():
            t0 = time.time()
            try:
                res = subprocess.run(
                    [str(self.wetype_bin), "voice-input", "--wav", str(wav_path)],
                    capture_output=True,
                    text=True,
                    timeout=20,
                )
                dt = time.time() - t0
                recognized = ""
                for line in (res.stdout + "\n" + res.stderr).splitlines():
                    if 'ASR Recognized Text: "' in line:
                        recognized = line.split('ASR Recognized Text: "', 1)[1].rsplit(
                            '"', 1
                        )[0]
                        break

                if recognized:
                    GLib.idle_add(self._set_current_text, recognized)
                    GLib.idle_add(
                        self.lbl_voice_state.set_label,
                        f"✅ 识别完成（耗时 {dt:.2f}s），已同步至剪贴板与 Fcitx5！",
                    )
                else:
                    GLib.idle_add(
                        self.lbl_voice_state.set_label,
                        f"⚠️ 未识别到有效语音内容（耗时 {dt:.2f}s）",
                    )
            except Exception as e:
                GLib.idle_add(self.lbl_voice_state.set_label, f"❌ 识别出错: {e}")

        threading.Thread(target=worker, daemon=True).start()

    def on_commit_now_clicked(self, _btn):
        text = self._get_current_text()
        if text:
            self._commit_via_cli(text)

    def on_commit_delay_clicked(self, _btn):
        text = self._get_current_text()
        if text:
            self._delayed_commit(text, 2)

    def _delayed_commit(self, text: str, delay_sec: int):
        self.lbl_voice_state.set_label(
            f"⏱️ 请在 {delay_sec} 秒内将鼠标点击到目标输入窗口，即将自动打字上屏..."
        )

        def worker():
            time.sleep(delay_sec)
            self._commit_via_cli(text)

        threading.Thread(target=worker, daemon=True).start()

    def _commit_via_cli(self, text: str):
        try:
            subprocess.run(
                [str(self.wetype_bin), "test-fcitx", "--text", text],
                capture_output=True,
                text=True,
                timeout=5,
            )
            GLib.idle_add(
                self.lbl_voice_state.set_label,
                f"⚡ 已将「{text[:18]}...」上屏注入到活动窗口并复制！",
            )
        except Exception as e:
            GLib.idle_add(self.lbl_voice_state.set_label, f"❌ 上屏失败: {e}")

    def on_copy_clicked(self, _btn):
        text = self._get_current_text()
        self._copy_to_clipboard(text)

    def _copy_to_clipboard(self, text: str):
        display = Gdk.Display.get_default()
        if display:
            clipboard = display.get_clipboard()
            clipboard.set(text)
            self.lbl_voice_state.set_label(f"📋 已复制到剪贴板：「{text[:20]}」")


class WeTypeApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id="io.github.ly5566lyui.WeTypeLinux")

    def do_activate(self):
        provider = Gtk.CssProvider()
        provider.load_from_data(APP_CSS.encode("utf-8"))
        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(),
            provider,
            Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
        )
        win = self.props.active_window
        if not win:
            win = WeTypeWindow(self)
        win.present()


if __name__ == "__main__":
    app = WeTypeApp()
    sys.exit(app.run(sys.argv))
