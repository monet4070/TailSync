import sys
import os

def get_resource(rel_path):
    """同时处理直接运行、PyInstaller 和 Nuitka 打包后的路径"""
    # 1. 优先检测 PyInstaller
    if hasattr(sys, '_MEIPASS'):
        base = sys._MEIPASS
    # 2. 其次检测 Nuitka 编译环境
    elif '__compiled__' in globals():
        base = os.path.dirname(os.path.abspath(sys.argv[0]))
    # 3. 最后是普通 Python 运行环境
    else:
        base = os.path.abspath(".")
        
    return os.path.join(base, rel_path)


def fix_system_path():
    """补全不同系统的环境变量，确保能找到 tailscale 等外部命令"""
    if sys.platform == "darwin":  # macOS
        extra_paths = ["/opt/homebrew/bin", "/usr/local/bin", "/Applications/Tailscale.app/Contents/MacOS"]
        separator = ":"
    elif sys.platform == "win32":  # Windows
        extra_paths = [r"C:\Program Files\Tailscale"]
        separator = ";"
    else:
        return

    current_path = os.environ.get("PATH", "")
    for p in extra_paths:
        if p not in current_path and os.path.exists(p):
            current_path = f"{current_path}{separator}{p}" if current_path else p
    
    os.environ["PATH"] = current_path