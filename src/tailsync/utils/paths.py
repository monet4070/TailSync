import sys
import os

def get_resource(rel_path):
    """处理 PyInstaller 打包后的路径"""
    base = getattr(sys, '_MEIPASS', os.path.abspath("."))
    return os.path.join(base, rel_path)