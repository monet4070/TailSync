import os
from PyQt5.QtWidgets import QWidget, QVBoxLayout, QHBoxLayout, QLabel, QApplication, QFileIconProvider
from PyQt5.QtCore import Qt, QTimer, QFileInfo, QPropertyAnimation, QPoint, QEasingCurve, QParallelAnimationGroup
from tailsync.ui.styles import is_dark_mode

_SHOW_FILE_ICON = {
    '.pdf', '.doc', '.docx', '.xls', '.xlsx', '.ppt', '.pptx',
    '.zip', '.rar', '.txt', '.mp3', '.mp4', '.mov', '.avi',
    '.html', '.htm', '.csv',
}

class NotificationWindow(QWidget):
    def __init__(self, title, message, file_path=None):
        super().__init__()
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
        inner.addWidget(t_label)

        msg_row = QHBoxLayout()
        msg_row.setContentsMargins(0, 0, 0, 0)

        show_icon = (
            file_path and os.path.exists(file_path) and
            os.path.splitext(file_path)[1].lower() in _SHOW_FILE_ICON
        )
        if show_icon:
            icon_label = QLabel()
            icon_label.setPixmap(QFileIconProvider().icon(QFileInfo(file_path)).pixmap(24, 24))
            icon_label.setFixedSize(24, 24)
            icon_label.setStyleSheet("border: none; background: transparent;")
            msg_row.addWidget(icon_label)

        m_label = QLabel(message)
        m_label.setWordWrap(True)
        m_label.setStyleSheet("font-size: 11px; border: none; background: transparent;")
        msg_row.addWidget(m_label, 1)
        inner.addLayout(msg_row)

        layout.addWidget(self.container)

        self.anim_group = QParallelAnimationGroup()
        self.pos_anim = QPropertyAnimation(self, b"pos")
        self.opa_anim = QPropertyAnimation(self, b"windowOpacity")
        self.anim_group.addAnimation(self.pos_anim)
        self.anim_group.addAnimation(self.opa_anim)

    def show_smart(self):
        screen = QApplication.primaryScreen().availableGeometry()
        sx = screen.x() + screen.width() - 320
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
        QTimer.singleShot(4000, self.close)