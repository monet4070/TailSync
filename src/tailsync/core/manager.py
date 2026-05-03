import time
import hashlib
import threading
import socket
import struct
import json
import os
import datetime
from PyQt5.QtCore import QObject, pyqtSignal, QTimer, Qt, QUrl, QMimeData
from PyQt5.QtWidgets import QApplication
from PyQt5.QtGui import QImage

from tailsync.constants import GLOBAL_SETTINGS, BASE_DIR, PROGRESS_THRESHOLD, TCP_PORT
from tailsync.core.database import HistoryDB
from tailsync.utils.platform_utils import set_clipboard_file
from tailsync.network.tailscale import get_tailscale_peers 

class SyncManager(QObject):
    remote_update_signal = pyqtSignal(int, object) 
    history_updated = pyqtSignal()                 
    progress_signal = pyqtSignal(int, str, str, bool) 
    notification_signal = pyqtSignal(str, str)     

    def __init__(self, settings=None):
        super().__init__()
        self.settings = settings 
        self.clipboard = QApplication.clipboard()
        self.db = HistoryDB()
        self.peers = [] 
        
        self.transfer_cancelled = False
        
        # 状态追踪：用于精准排重与阴影过滤
        self.last_text_hash = ""
        self.last_img_f = None
        self.last_file_f = None
        self.blocked_filename = "" # 锁定当前同步的文件名
        self.file_recv_timestamp = 0 # 记录最近文件操作时间
        self.is_self_updating = False

        self.remote_update_signal.connect(self.update_local_clipboard)
        self.refresh_peers()

        self.timer = QTimer()
        self.timer.timeout.connect(self.process_and_broadcast)
        self.timer.start(800)

    # ================= 1. 历史回溯功能 (修复闪退的关键) =================

    def restore_to_clipboard(self, entry):
        """
        将历史记录项恢复至系统剪贴板[cite: 2]
        """
        self.is_self_updating = True 
        try:
            e_type = entry["type"]
            data = entry["data"]
            
            if e_type == "text":
                self.clipboard.setText(data)
            elif e_type == "image":
                if os.path.exists(data):
                    self.clipboard.setImage(QImage(data))
                else:
                    self.notification_signal.emit("TailSync", "❌ 缓存图片已丢失")
            elif e_type == "file":
                if os.path.exists(data):
                    # 使用验证有效的 QMimeData 注入逻辑
                    set_clipboard_file(data)
                else:
                    self.notification_signal.emit("TailSync", "❌ 原始文件已不存在")
            
            self.notification_signal.emit("TailSync", "✅ 已回溯至剪贴板")
        except Exception as e:
            print(f"回溯执行失败: {e}")
        finally:
            # 锁定 1.5 秒，防止回溯操作触发自同步[cite: 2]
            QTimer.singleShot(1500, lambda: setattr(self, 'is_self_updating', False))

    # ================= 2. 远程同步处理 (接收端) =================

    def update_local_clipboard(self, dt, data):
        """核心：处理远程同步信号，强化影子包拦截"""
        self.is_self_updating = True 
        msg = ""
        try:
            now = time.time()
            if dt == 0: # 收到纯文本
                txt = data.decode('utf-8').strip()
                
                # 拦截：URI 阴影文本 (file://) 和 3秒内的文件名回传
                is_shadow = txt.startswith("file://") or (txt == self.blocked_filename and (now - self.file_recv_timestamp < 3.0))
                if is_shadow:
                    print(f"接收端过滤影子文本: {txt}")
                    return
                
                new_hash = hashlib.md5(data).hexdigest()
                if new_hash == self.last_text_hash:
                    return
                self.last_text_hash = new_hash
                self.clipboard.setText(txt)
                msg = f"收到文本: {txt[:15]}..."
                self.add_to_history("text", msg, txt)

            elif dt == 1: # 收到图片
                # 拦截：接收文件后的 1.5 秒内，拦截可能的影子缩略图
                if (now - self.file_recv_timestamp < 1.5) and self.blocked_filename:
                    print("接收端过滤影子缩略图")
                    return

                img = QImage()
                img.loadFromData(data)
                img_f = (img.width(), img.height(), img.sizeInBytes())
                if img_f == self.last_img_f: return
                
                self.last_img_f = img_f
                save_path = os.path.join(BASE_DIR, f"recv_img_{int(time.time())}.png")
                img.save(save_path, "PNG")
                self.clipboard.setImage(img)
                msg = "收到图片"
                self.add_to_history("image", msg, save_path)

            elif dt == 2: # 收到文件
                file_path = os.path.normpath(data)
                fname = os.path.basename(file_path)
                
                # 激活拦截盾牌
                self.blocked_filename = fname
                self.file_recv_timestamp = now
                self.last_file_f = (file_path, os.stat(file_path).st_size)
                
                if set_clipboard_file(file_path):
                    msg = f"收到文件: {fname}"
                    self.add_to_history("file", fname, file_path)
            
            self.notification_signal.emit("TailSync", msg)
        except Exception as e:
            print(f"同步处理异常: {e}")
        finally:
            QTimer.singleShot(2500, lambda: setattr(self, 'is_self_updating', False))

    # ================= 3. 本地监听逻辑 (发送端) =================

    def process_and_broadcast(self):
        """发送端轮询，执行严格熔断与协议过滤"""
        if getattr(self, 'is_self_updating', False): return
        mime_data = self.clipboard.mimeData()
        target_ips = [ip for ip, enabled in (self.settings.get("enabled_remotes", {}) if self.settings else GLOBAL_SETTINGS["enabled_remotes"]).items() if enabled]
        if not mime_data or not target_ips: return

        # 1. 检测本地文件 (Urls)
        local_files_found = False
        if mime_data.hasUrls():
            for url in mime_data.urls():
                if url.isLocalFile(): 
                    fp = os.path.normpath(url.toLocalFile())
                    if os.path.isfile(fp) and not fp.startswith(BASE_DIR):
                        local_files_found = True
                        f_info = (fp, os.stat(fp).st_size)
                        if f_info != self.last_file_f:
                            self.last_file_f = f_info
                            self.blocked_filename = os.path.basename(fp)
                            self.file_recv_timestamp = time.time()
                            for ip in target_ips:
                                threading.Thread(target=self.send_task, args=(ip, b'\x02', fp), daemon=True).start()
                            self.add_to_history("file", os.path.basename(fp), fp)
            
            if local_files_found: return # 绝对熔断

        # 2. 检测图片
        if mime_data.hasImage():
            img = self.clipboard.image()
            if not img.isNull():
                img_info = (img.width(), img.height(), img.sizeInBytes())
                if img_info != self.last_img_f:
                    self.last_img_f = img_info
                    temp_path = os.path.join(BASE_DIR, f"sent_img_{int(time.time())}.png")
                    img.save(temp_path, "PNG")
                    for ip in target_ips:
                        threading.Thread(target=self.send_task, args=(ip, b'\x01', temp_path), daemon=True).start()
                    self.add_to_history("image", "发送图片", temp_path)
                    return

        # 3. 检测纯文本 (排除 URI 影子文本)
        if mime_data.hasText():
            t = self.clipboard.text().strip()
            # 过滤：file:// 协议、黑名单文件名、本地路径
            is_shadow = t.startswith("file://") or (t == self.blocked_filename and (time.time() - self.file_recv_timestamp < 3.0))
            if not t or is_shadow or os.path.exists(t):
                return
            
            new_hash = hashlib.md5(t.encode('utf-8')).hexdigest()
            if new_hash != self.last_text_hash:
                self.last_text_hash = new_hash
                for ip in target_ips:
                    threading.Thread(target=self.send_task, args=(ip, b'\x00', t), daemon=True).start()
                self.add_to_history("text", t[:20], t)

    # ================= 4. 辅助方法 =================

    def send_task(self, ip, mode, content):
        self.transfer_cancelled = False
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
                s.settimeout(10); s.connect((ip, TCP_PORT))
                if mode == b'\x00':
                    d = content.encode('utf-8'); s.sendall(mode + struct.pack('!I', len(d)) + d)
                elif mode == b'\x01':
                    with open(content, "rb") as f: d = f.read(); s.sendall(mode + struct.pack('!I', len(d)) + d)
                elif mode == b'\x02':
                    fsize, fname = os.path.getsize(content), os.path.basename(content)
                    header = json.dumps({"name": fname, "size": fsize}).encode()
                    s.sendall(mode + struct.pack('!I', len(header)) + header)
                    sent = 0
                    with open(content, "rb") as f:
                        while chunk := f.read(128 * 1024):
                            if self.transfer_cancelled: return
                            s.sendall(chunk); sent += len(chunk)
                            if fsize > PROGRESS_THRESHOLD:
                                self.progress_signal.emit(int(sent/fsize*100), "📤 发送中...", f"{ip}", False)
                    if fsize > PROGRESS_THRESHOLD: self.progress_signal.emit(100, "✅ 完成", "", True)
        except: pass

    def refresh_peers(self):
        try:
            _, peers_list = get_tailscale_peers()
            enabled_map = self.settings.get("enabled_remotes", {}) if self.settings else GLOBAL_SETTINGS["enabled_remotes"]
            for peer in peers_list:
                ip = peer['ip']
                if ip not in enabled_map: enabled_map[ip] = True 
                peer['active'] = enabled_map.get(ip, True)
            self.peers = peers_list
            if self.settings: self.settings.set("enabled_remotes", enabled_map)
        except: self.peers = []

    def toggle_peer_status(self, ip, enabled):
        enabled_map = self.settings.get("enabled_remotes", {}) if self.settings else GLOBAL_SETTINGS["enabled_remotes"]
        enabled_map[ip] = enabled
        if self.settings: self.settings.set("enabled_remotes", enabled_map)

    def add_to_history(self, t_type, desc, data):
        t_str = datetime.datetime.now().strftime("%H:%M:%S")
        self.db.add(t_str, t_type, desc, data)
        self.db.trim(GLOBAL_SETTINGS["history_limit"])
        self.history_updated.emit()

    def cancel_current_transfer(self):
        self.transfer_cancelled = True
        self.notification_signal.emit("TailSync", "🚫 传输已停止")