from PyQt5.QtWidgets import QWidget, QVBoxLayout, QLabel, QApplication
from PyQt5.QtCore import Qt, QTimer, QPropertyAnimation, QPoint, QEasingCurve, QParallelAnimationGroup
from tailsync.ui.styles import is_dark_mode

class NotificationWindow(QWidget):
    def __init__(self, title, message):
        super().__init__()
        # 设置无边框、置顶、不在任务栏显示
        self.setWindowFlags(Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.Tool)
        self.setAttribute(Qt.WA_TranslucentBackground)
        self.setAttribute(Qt.WA_ShowWithoutActivating)
        self.setFixedSize(300, 95)
        self.setWindowOpacity(0.0)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        
        self.container = QWidget()
        bg_color = "rgba(45,45,45,230)" if is_dark_mode() else "rgba(255,255,255,245)"
        self.container.setStyleSheet(f"background: {bg_color}; border: 1px solid #00ADFF; border-radius: 12px;")
        
        inner = QVBoxLayout(self.container)
        t_label = QLabel(title)
        t_label.setStyleSheet("font-weight: bold; color: #00ADFF; font-size: 13px; border: none; background: transparent;")
        m_label = QLabel(message)
        m_label.setWordWrap(True)
        m_label.setStyleSheet("font-size: 11px; border: none; background: transparent;")
        
        inner.addWidget(t_label)
        inner.addWidget(m_label)
        layout.addWidget(self.container)

        # 动画组：位置上升 + 不透明度增加
        self.anim_group = QParallelAnimationGroup()
        self.pos_anim = QPropertyAnimation(self, b"pos")
        self.opa_anim = QPropertyAnimation(self, b"windowOpacity")
        self.anim_group.addAnimation(self.pos_anim)
        self.anim_group.addAnimation(self.opa_anim)

    def show_smart(self):
        screen = QApplication.primaryScreen().availableGeometry()
        sx = screen.x() + screen.width() - 320
        # 初始位置在屏幕下方，结束位置在右下角上方
        start_y = screen.y() + screen.height()
        end_y = screen.y() + screen.height() - 115
        
        self.move(sx, start_y)
        self.show()
        
        self.pos_anim.setDuration(600)
        self.pos_anim.setEasingCurve(QEasingCurve.OutCubic)
        self.pos_anim.setStartValue(QPoint(sx, start_y))
        self.pos_anim.setEndValue(QPoint(sx, end_y))
        
        self.opa_anim.setDuration(600)
        self.opa_anim.setStartValue(0.0)
        self.opa_anim.setEndValue(1.0)
        
        self.anim_group.start()
        # 4秒后自动关闭
        QTimer.singleShot(4000, self.close)