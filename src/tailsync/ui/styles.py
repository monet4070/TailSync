from PyQt5.QtWidgets import QApplication
from PyQt5.QtGui import QPalette

def is_dark_mode():
    """检测系统是否处于深色模式"""
    return QApplication.palette().color(QPalette.Window).lightness() < 128

def get_theme_qss():
    """动态生成样式表"""
    dark = is_dark_mode()
    bg = "#2D2D2D" if dark else "#FFFFFF"
    item_bg = "#3D3D3D" if dark else "#F0F0F0"
    text = "#FFFFFF" if dark else "#202020" 
    accent = "#00ADFF"
    border = "#444444" if dark else "#DDDDDD"
    
    return f"""
        QWidget {{ color: {text}; font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei"; }}
        QMenu {{ background-color: {bg}; border: 1px solid {border}; border-radius: 6px; padding: 4px; }}
        QMenu::item {{ padding: 8px 30px 8px 25px; border-radius: 4px; }}
        QMenu::item:selected {{ background-color: {accent}; color: white; }}
        QProgressBar {{ background: {border}; border-radius: 6px; text-align: center; height: 12px; font-size: 10px; }}
        QProgressBar::chunk {{ background: {accent}; border-radius: 6px; }}
        QListWidget {{ background: transparent; border: none; outline: none; }}
        QListWidget::item {{ background-color: {item_bg}; border: 1px solid {border}; border-radius: 8px; margin-bottom: 5px; padding: 10px; }}
        QListWidget::item:selected {{ border: 2px solid {accent}; color: {text}; }}
        QLineEdit {{ background: {item_bg}; border: 1px solid {border}; border-radius: 6px; padding: 5px; color: {text}; }}
    """