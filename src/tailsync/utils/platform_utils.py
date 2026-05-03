import sys
import os
from PyQt5.QtCore import QUrl, QMimeData
from PyQt5.QtWidgets import QApplication

def set_clipboard_file(file_path):
    """
    使用 QMimeData 注入文件，确保 Windows 粘贴出的是文件对象而非文本
    """
    if not os.path.exists(file_path):
        return False
    try:
        path = os.path.abspath(file_path)
        file_url = QUrl.fromLocalFile(path)
        
        m = QMimeData()
        m.setUrls([file_url])
        
        # 必须确保在主线程或通过 QApplication 获取剪贴板
        QApplication.clipboard().setMimeData(m)
        return True
    except Exception as e:
        print(f"注入失败: {e}")
        return False