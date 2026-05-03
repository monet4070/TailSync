import os
import sys
import threading
from PyQt5.QtWidgets import QApplication, QSystemTrayIcon, QMenu, QAction
from PyQt5.QtGui import QIcon, QFont

from tailsync.core import SyncManager, Settings
from tailsync.ui import NotificationWindow, ProgressWindow, HistoryWindow, get_theme_qss
from tailsync.network import start_network_server
from tailsync.utils import get_resource

class TailSyncApp:
    def __init__(self):
        # 初始化基础应用设置[cite: 28]
        self.app = QApplication(sys.argv)
        self.app.setQuitOnLastWindowClosed(False)
        self.app.setStyleSheet(get_theme_qss())
        
        # 针对不同系统的字体适配[cite: 28]
        font_family = "Microsoft YaHei" if sys.platform == "win32" else "PingFang SC"
        self.app.setFont(QFont(font_family, 9))
        
        # 初始化配置与核心逻辑[cite: 28]
        self.settings = Settings()
        self.mgr = SyncManager(settings=self.settings)
        
        # 初始化 UI 窗口[cite: 28]
        self.prog_win = ProgressWindow()
        self.hw = HistoryWindow(self.mgr)
        
        # 建立信号桥梁[cite: 28]
        self.mgr.progress_signal.connect(self.prog_win.update_progress)
        self.mgr.notification_signal.connect(lambda t, m: NotificationWindow(t, m).show_smart())
        self.mgr.history_updated.connect(self.hw.refresh)
        
        # 【核心对接】连接进度条的停止按钮信号到 manager 的处理函数[cite: 28]
        self.prog_win.cancel_requested.connect(self.mgr.cancel_current_transfer)
        
        # 启动后台服务器线程[cite: 28]
        threading.Thread(target=start_network_server, args=(self.mgr,), daemon=True).start()
        
        # 托盘图标设置[cite: 28]
        icon_path = get_resource("assets/icon.png")
        self.tray_icon = QIcon(icon_path) if os.path.exists(icon_path) else QIcon()
        self.tray = QSystemTrayIcon(self.tray_icon, self.app)
        
        # 监听托盘点击事件[cite: 28]
        self.tray.activated.connect(self.on_tray_activated)
        
        self.update_menu()
        self.tray.show()

    def on_tray_activated(self, reason):
        """当用户点击托盘图标时触发[cite: 28]"""
        if reason == QSystemTrayIcon.Trigger:
            self.mgr.refresh_peers()
            self.update_menu()

    def update_menu(self):
        """构建动态托盘菜单[cite: 28]"""
        menu = QMenu()
        
        show_history = QAction("📜 历史记录", menu)
        show_history.triggered.connect(self.show_history)
        menu.addAction(show_history)
        
        refresh_act = QAction("🔄 手动刷新设备", menu)
        refresh_act.triggered.connect(self.mgr.refresh_peers)
        menu.addAction(refresh_act)
        
        menu.addSeparator()

        # 设备控制子菜单[cite: 28]
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
                # 绑定勾选状态切换逻辑[cite: 28]
                p_act.triggered.connect(lambda checked, ip=p_ip: self.mgr.toggle_peer_status(ip, checked))
                device_menu.addAction(p_act)

        # 功能设置子菜单[cite: 28]
        settings_menu = menu.addMenu("⚙️ 功能设置")
        
        notif_toggle = QAction("🔔 显示通知", settings_menu, checkable=True)
        notif_toggle.setChecked(self.settings.get("show_notifications", True))
        notif_toggle.triggered.connect(lambda checked: self.settings.set("show_notifications", checked))
        settings_menu.addAction(notif_toggle)
        
        progress_toggle = QAction("📊 传输进度条", settings_menu, checkable=True)
        progress_toggle.setChecked(self.settings.get("show_progress", True))
        progress_toggle.triggered.connect(lambda checked: self.settings.set("show_progress", checked))
        settings_menu.addAction(progress_toggle)

        menu.addSeparator()
        
        quit_action = QAction("退出", menu)
        quit_action.triggered.connect(self.app.quit)
        menu.addAction(quit_action)
        
        self.tray.setContextMenu(menu)

    def show_history(self):
        """激活显示历史记录窗口[cite: 28]"""
        self.hw.refresh()
        self.hw.show()
        self.hw.raise_()
        self.hw.activateWindow()

    def run(self):
        """进入 PyQt5 事件循环[cite: 28]"""
        sys.exit(self.app.exec_())

def main():
    app_instance = TailSyncApp()
    app_instance.run()

if __name__ == "__main__":
    main()