import json
import os
# 从常量模块导入存储路径
from tailsync.constants import BASE_DIR, GLOBAL_SETTINGS

CONFIG_PATH = os.path.join(BASE_DIR, "config.json")

class Settings:
    """
    负责项目配置的持久化管理。
    将配置存储在用户目录下的 config.json 中。
    """
    def __init__(self):
        # 默认配置副本，确保新版本增加配置项时不崩溃
        self.defaults = GLOBAL_SETTINGS.copy()
        self.data = self.defaults.copy()
        self.load()

    def load(self):
        """从硬盘读取 JSON 配置，如果文件不存在或损坏则使用默认值"""
        if not os.path.exists(CONFIG_PATH):
            self.save() # 第一次运行，生成默认配置文件
            return

        try:
            with open(CONFIG_PATH, 'r', encoding='utf-8') as f:
                loaded_data = json.load(f)
                self.data.update(loaded_data)
                # 同步回模块级全局配置，确保其他模块直接引用 GLOBAL_SETTINGS 时也拿到最新值
                GLOBAL_SETTINGS.update(self.data)
        except (json.JSONDecodeError, IOError) as e:
            print(f"⚠️ 配置文件读取失败，将使用默认设置: {e}")
            self.data = self.defaults.copy()

    def save(self):
        """将当前配置持久化到硬盘"""
        try:
            # 确保存储文件夹存在
            if not os.path.exists(BASE_DIR):
                os.makedirs(BASE_DIR)
                
            with open(CONFIG_PATH, 'w', encoding='utf-8') as f:
                json.dump(self.data, f, indent=4, ensure_ascii=False)
        except Exception as e:
            print(f"❌ 无法保存配置文件: {e}")

    def get(self, key, default=None):
        """获取配置项的值"""
        return self.data.get(key, default)

    def set(self, key, value):
        """
        设置配置项的值并立即自动保存。
        适用于 UI 上的开关操作（如切换通知开关）。
        """
        self.data[key] = value
        self.save()
        # 同步更新常量内存中的值，确保程序逻辑立即生效
        GLOBAL_SETTINGS[key] = value