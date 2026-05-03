from PyQt5.QtWidgets import QWidget, QVBoxLayout, QHBoxLayout, QLabel, QProgressBar, QPushButton, QApplication
from PyQt5.QtCore import Qt, QTimer, pyqtSignal
from tailsync.ui.styles import get_theme_qss

class ProgressWindow(QWidget):
    # 定义取消信号，供主程序拦截
    cancel_requested = pyqtSignal()

    def __init__(self):
        super().__init__()
        self.setWindowFlags(Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.Tool)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.setFixedSize(320, 95) # 稍微加宽以容纳按钮[cite: 25]
        
        l = QVBoxLayout(self)
        l.setContentsMargins(0, 0, 0, 0)
        
        self.container = QWidget()
        self.container.setStyleSheet(get_theme_qss() + """
            QWidget#container { 
                background: rgba(35,35,35,240); 
                border-radius: 10px; 
                border: 1px solid #00ADFF; 
            }
        """)
        self.container.setObjectName("container")
        
        inner = QVBoxLayout(self.container)
        self.title_label = QLabel("准备传输...")
        self.title_label.setStyleSheet("font-weight: bold; color: #00ADFF; background: transparent;")
        
        # 水平布局：放置进度条和停止按钮[cite: 25]
        bar_layout = QHBoxLayout()
        
        self.bar = QProgressBar()
        self.bar.setRange(0, 100)
        self.bar.setValue(0)
        
        self.stop_btn = QPushButton("🛑 停止")
        self.stop_btn.setFixedSize(65, 24)
        self.stop_btn.setStyleSheet("""
            QPushButton { 
                background-color: #FF4D4D; 
                color: white; 
                border-radius: 4px; 
                font-size: 11px; 
                font-weight: bold;
            }
            QPushButton:hover { background-color: #FF6666; }
        """)
        self.stop_btn.clicked.connect(self.cancel_requested.emit) # 点击时发射取消信号[cite: 25]
        
        bar_layout.addWidget(self.bar)
        bar_layout.addWidget(self.stop_btn)
        
        self.info_label = QLabel("正在初始化...")
        self.info_label.setStyleSheet("font-size: 10px; background: transparent;")
        
        inner.addWidget(self.title_label)
        inner.addLayout(bar_layout)
        inner.addWidget(self.info_label)
        l.addWidget(self.container)

    def update_progress(self, val, title, info, is_done=False):
        self.title_label.setText(title)
        self.info_label.setText(info)
        self.bar.setValue(val)
        
        if not self.isVisible():
            sc = QApplication.primaryScreen().availableGeometry()
            self.move(sc.x() + sc.width() - 340, sc.y() + sc.height() - 115)
            self.show()
            
        if is_done or val >= 100:
            QTimer.singleShot(1500, self.hide)
        
        QApplication.processEvents()