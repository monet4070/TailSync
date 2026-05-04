import os
import sys
import threading
from PyQt5.QtWidgets import QApplication, QSystemTrayIcon, QMenu, QAction, QSlider, QWidget, QHBoxLayout, QVBoxLayout, QLabel, QWidgetAction
from PyQt5.QtCore import Qt
from PyQt5.QtGui import QIcon, QFont

from tailsync.core import SyncManager, Settings
from tailsync.ui import NotificationWindow, ProgressWindow, HistoryWindow, get_theme_qss
from tailsync.network import start_network_server
from tailsync.utils import get_resource
from tailsync.utils.paths import fix_system_path

fix_system_path()

class TailSyncApp:
    def __init__(self):
        self.app = QApplication(sys.argv)
        self.app.setQuitOnLastWindowClosed(False)
        self.app.setStyleSheet(get_theme_qss())

        font_family = "Microsoft YaHei" if sys.platform == "win32" else "PingFang SC"
        self.app.setFont(QFont(font_family, 9))

        self.settings = Settings()
        self.mgr = SyncManager(settings=self.settings)

        self.prog_win = ProgressWindow()
        self.hw = HistoryWindow(self.mgr)

        self.mgr.progress_signal.connect(self.prog_win.update_progress)
        self.mgr.notification_signal.connect(lambda t, m, fp="": NotificationWindow(t, m, fp).show_smart())
        self.mgr.history_updated.connect(self.hw.refresh)
        self.mgr.peers_updated.connect(self.update_menu)

        self.prog_win.cancel_requested.connect(self.mgr.cancel_current_transfer)

        threading.Thread(target=start_network_server, args=(self.mgr,), daemon=True).start()

        icon_path = get_resource("assets/icon.png")
        self.tray_icon = QIcon(icon_path) if os.path.exists(icon_path) else QIcon()
        self.app.setWindowIcon(self.tray_icon)
        self.tray = QSystemTrayIcon(self.tray_icon, self.app)

        self.tray.activated.connect(self.on_tray_activated)

        self.update_menu()
        self.tray.show()

    def on_tray_activated(self, reason):
        if reason == QSystemTrayIcon.Trigger:
            self.mgr.refresh_peers()

    def update_menu(self):
        menu = QMenu()

        show_history = QAction("📜 历史记录", menu)
        show_history.triggered.connect(self.show_history)
        menu.addAction(show_history)

        refresh_act = QAction("🔄 手动刷新设备", menu)
        refresh_act.triggered.connect(lambda: (self.mgr.refresh_peers(), self.update_menu()))
        menu.addAction(refresh_act)

        menu.addSeparator()

        device_menu = menu.addMenu("🖥️ 设备控制")
        peers = self.mgr.peers

        if not peers:
            no_device = QAction("未发现可用设备", device_menu)
            no_device.setEnabled(False)
            device_menu.addAction(no_device)
        else:
            for peer in peers:
                p_ip = peer['ip']
                p_act = QAction(f"💻 {peer['name']}", device_menu, checkable=True)
                p_act.setChecked(peer.get('active', True))
                p_act.triggered.connect(lambda checked, ip=p_ip: self.mgr.toggle_peer_status(ip, checked))
                device_menu.addAction(p_act)

        settings_menu = menu.addMenu("⚙️ 功能设置")

        notif_toggle = QAction("🔔 显示通知", settings_menu, checkable=True)
        notif_toggle.setChecked(self.settings.get("show_notifications", True))
        notif_toggle.triggered.connect(lambda checked: self.settings.set("show_notifications", checked))
        settings_menu.addAction(notif_toggle)

        progress_toggle = QAction("📊 传输进度条", settings_menu, checkable=True)
        progress_toggle.setChecked(self.settings.get("show_progress", True))
        progress_toggle.triggered.connect(lambda checked: self.settings.set("show_progress", checked))
        settings_menu.addAction(progress_toggle)

        # 历史记录上限滑块（10-100，修改后下次同步/接收时生效）
        current_limit = self.settings.get("history_limit", 20)
        history_widget = QWidget()
        history_layout = QVBoxLayout(history_widget)
        history_layout.setContentsMargins(10, 4, 10, 4)
        history_title = QLabel("📝 历史记录上限")
        history_title.setStyleSheet("color: inherit; border: none; background: transparent; padding: 0;")
        slider_row = QWidget()
        slider_row_layout = QHBoxLayout(slider_row)
        slider_row_layout.setContentsMargins(0, 0, 0, 0)
        history_label = QLabel(str(current_limit))
        history_label.setFixedWidth(24)
        history_label.setStyleSheet("color: #00ADFF; font-weight: bold; border: none; background: transparent;")
        history_slider = QSlider(Qt.Horizontal)
        history_slider.setRange(10, 100)
        history_slider.setValue(current_limit)
        history_slider.valueChanged.connect(
            lambda v, lbl=history_label: (self.settings.set("history_limit", v), lbl.setText(str(v)))
        )
        slider_row_layout.addWidget(history_slider)
        slider_row_layout.addWidget(history_label)
        history_layout.addWidget(history_title)
        history_layout.addWidget(slider_row)
        slider_action = QWidgetAction(settings_menu)
        slider_action.setDefaultWidget(history_widget)
        settings_menu.addAction(slider_action)

        menu.addSeparator()

        quit_action = QAction("退出", menu)
        quit_action.triggered.connect(self.app.quit)
        menu.addAction(quit_action)

        self.tray.setContextMenu(menu)

    def show_history(self):
        self.hw.refresh()
        self.hw.show()
        self.hw.raise_()
        self.hw.activateWindow()

    def run(self):
        sys.exit(self.app.exec_())

def main():
    app_instance = TailSyncApp()
    app_instance.run()

if __name__ == "__main__":
    main()