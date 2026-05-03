import os
from PyQt5.QtWidgets import QWidget, QVBoxLayout, QLabel, QListWidget, QListWidgetItem, QLineEdit
from PyQt5.QtCore import Qt, QTimer, QSize
from PyQt5.QtGui import QImage, QIcon, QPixmap
from tailsync.ui.styles import get_theme_qss
from tailsync.constants import IMAGE_EXTS

class HistoryWindow(QWidget):
    def __init__(self, manager):
        super().__init__()
        self.manager = manager
        self.setWindowTitle("TailSync 历史记录")
        self.setFixedSize(360, 560)
        self.setStyleSheet(get_theme_qss())
        
        layout = QVBoxLayout(self)
        
        header = QLabel("📋 历史记录")
        header.setStyleSheet("font-size: 16px; font-weight: bold; color: #00ADFF; margin: 5px;")
        layout.addWidget(header)
        
        self.search_bar = QLineEdit()
        self.search_bar.setPlaceholderText("🔍 输入关键词检索...")
        self.search_bar.textChanged.connect(self.refresh)
        layout.addWidget(self.search_bar)
        
        self.list_widget = QListWidget()
        self.list_widget.setIconSize(QSize(65, 65))
        self.list_widget.itemDoubleClicked.connect(self.restore_item)
        layout.addWidget(self.list_widget)
        
    def refresh(self):
        """重绘列表"""
        self.list_widget.clear()
        keyword = self.search_bar.text().strip()
        items = self.manager.db.get_all(keyword if keyword else None)
        
        for item in items:
            # 这里的 text 逻辑可以根据你原始代码微调
            display_text = f"[{item['time']}] {item['type'].upper()}\n{item['desc']}"
            li = QListWidgetItem(display_text)
            
            # 如果是图片，显示缩略图
            data_path = item['data']
            if item['type'] == 'image' and os.path.exists(data_path):
                img = QImage(data_path)
                if not img.isNull():
                    pixmap = QPixmap.fromImage(img).scaled(65, 65, Qt.KeepAspectRatio, Qt.SmoothTransformation)
                    li.setIcon(QIcon(pixmap))
            
            li.setData(Qt.UserRole, item)
            self.list_widget.addItem(li)

    def restore_item(self, list_item):
        """双击回溯历史"""
        entry = list_item.data(Qt.UserRole)
        # 调用 manager 的恢复接口
        self.manager.restore_to_clipboard(entry)
        
        # UI 反馈
        original_text = list_item.text()
        list_item.setText("✅ 已回溯至剪贴板")
        QTimer.singleShot(1000, lambda: list_item.setText(original_text))