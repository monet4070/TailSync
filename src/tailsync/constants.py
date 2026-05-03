import os

# 基础路径：用户目录下 TailSync_History 文件夹
BASE_DIR = os.path.normpath(os.path.join(os.path.expanduser("~"), "TailSync_History"))
DB_PATH = os.path.join(BASE_DIR, "history.db")

# 业务逻辑常量
IMAGE_EXTS = {'.png', '.jpg', '.jpeg', '.bmp', '.webp', '.ico'}
PROGRESS_THRESHOLD = 24 * 1024 * 1024  # 24MB 以上显示进度条
TCP_PORT = 8888

# 应用运行时配置
GLOBAL_SETTINGS = {
    "show_notifications": True,
    "show_progress": True,
    "history_limit": 20,
    "enabled_remotes": {}  
}