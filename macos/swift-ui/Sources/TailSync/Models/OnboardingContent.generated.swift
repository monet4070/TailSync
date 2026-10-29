// Generated from shared/onboarding/guide.json. Do not edit.
import Foundation

enum OnboardingContent {
    static let json = #"""
{
  "version": 3,
  "ui": {
    "title": {
      "zh-CN": "新手引导",
      "en": "Getting started"
    },
    "skip": {
      "zh-CN": "结束引导",
      "en": "End tour"
    },
    "back": {
      "zh-CN": "上一步",
      "en": "Back"
    },
    "next": {
      "zh-CN": "下一步",
      "en": "Next"
    },
    "finish": {
      "zh-CN": "完成",
      "en": "Finish"
    },
    "error": {
      "zh-CN": "操作未完成，请重试。",
      "en": "Could not complete this action. Try again."
    },
    "reopen": {
      "zh-CN": "跟随高亮提示，在实际界面中一步步上手。",
      "en": "Follow the highlights through the actual app."
    },
    "waiting": {
      "zh-CN": "正在定位演示控件；页面加载后会自动继续。",
      "en": "Locating the demo control; it will continue when the page is ready."
    },
    "hint": {
      "zh-CN": "可以直接操作高亮区域",
      "en": "You can use the highlighted controls"
    },
    "progress": {
      "zh-CN": "引导进度",
      "en": "Tour progress"
    },
    "never": {
      "zh-CN": "不再显示（永久）",
      "en": "Don't show again"
    },
    "neverHint": {
      "zh-CN": "之后不会在设置中再次提供，除非重新安装 TailSync。",
      "en": "This walkthrough will no longer be available in Settings unless TailSync is reinstalled."
    },
    "demo": {
      "zh-CN": "演示数据",
      "en": "Demo data"
    },
    "demoRestore": {
      "zh-CN": "演示：已模拟回溯。实际使用时双击记录，再到目标应用粘贴。",
      "en": "Demo: restore simulated. In normal use, double-click a row and paste into the target app."
    },
    "demoDelete": {
      "zh-CN": "演示：记录已删除。实际使用时右键记录即可删除本机这一条。",
      "en": "Demo: row deleted. In normal use, right-click a row to delete it from this device."
    },
    "demoClear": {
      "zh-CN": "演示历史已清空。真实历史没有被修改。",
      "en": "Demo history cleared. Real history was not changed."
    },
    "demoFavorite": {
      "zh-CN": "演示：已加入收藏。实际使用时长按记录即可收藏。",
      "en": "Demo: added to Favorites. In normal use, long-press a row to favorite it."
    },
    "demoUnfavorite": {
      "zh-CN": "演示：已取消收藏。",
      "en": "Demo: removed from Favorites."
    },
    "demoPeerHint": {
      "zh-CN": "仅用于引导的虚拟设备，不会连接网络或建立信任。",
      "en": "A virtual device used only for this tour; it never connects or establishes trust."
    },
    "demoPair": {
      "zh-CN": "模拟配对",
      "en": "Try pairing"
    },
    "demoPairingTitle": {
      "zh-CN": "演示配对",
      "en": "Pairing demo"
    },
    "demoCode": {
      "zh-CN": "实际配对时，两台设备会显示相同验证码。请逐位核对，再分别确认。",
      "en": "In a real pairing both devices show this code. Compare every digit, then confirm on each device."
    },
    "demoContinue": {
      "zh-CN": "查看验证码",
      "en": "Show code"
    },
    "demoConfirm": {
      "zh-CN": "验证码一致",
      "en": "Codes match"
    },
    "demoPaired": {
      "zh-CN": "演示配对完成。这里没有写入真实设备、密钥或信任状态。",
      "en": "Demo pairing complete. No real device, key, or trust state was written."
    },
    "demoSyncToggle": {
      "zh-CN": "演示：控制是否向虚拟设备发送内容",
      "en": "Demo: control delivery to the virtual device"
    }
  },
  "steps": [
    {
      "id": "connections-1",
      "window": "connections",
      "target": "connection-mode",
      "title": {
        "zh-CN": "先选择「自动」",
        "en": "Choose Automatic"
      },
      "body": {
        "zh-CN": "点击高亮区域中的「自动」。TailSync 会优先寻找可用的局域网连接；需要时也可选择 Iroh 或 Tailscale。",
        "en": "Click Automatic in the highlighted controls. TailSync prefers an available LAN route; Iroh and Tailscale are also available."
      },
      "tip": {
        "zh-CN": "在另一台设备上也打开 TailSync。",
        "en": "Open TailSync on your other device too."
      }
    },
    {
      "id": "connections-2",
      "window": "connections",
      "target": "pairing",
      "title": {
        "zh-CN": "点击「允许配对」",
        "en": "Allow pairing"
      },
      "body": {
        "zh-CN": "只需在一台设备点击「允许配对」，开启 120 秒配对窗口。另一台设备无需开启「允许配对」，直接在设备列表点击这台设备旁的「配对」。",
        "en": "Click Allow pairing on just one device to open a 120-second pairing window. On the other device, click Pair beside it in the device list; that device does not need to enable Allow pairing."
      },
      "tip": {
        "zh-CN": "只有你主动确认，才会信任新设备。",
        "en": "A new device is trusted only after you confirm."
      }
    },
    {
      "id": "connections-3",
      "window": "connections",
      "target": "peers",
      "title": {
        "zh-CN": "在设备列表中找到对方",
        "en": "Find your other device"
      },
      "body": {
        "zh-CN": "这里有一台仅用于演示的虚拟设备。在真实设备上：一端点击「允许配对」，另一端在列表里点击那台设备旁的「配对」。然后逐位核对两端相同的验证码，再分别确认。演示里的「模拟配对」不会连接网络。",
        "en": "This list includes a virtual demo device. With real devices, click Allow pairing on one device, then click Pair beside that device on the other. Compare the same code digit by digit and confirm on both. The demo button never connects to the network."
      },
      "tip": {
        "zh-CN": "设备开关控制是否向该设备发送内容；撤销配对会删除本机对它的信任。",
        "en": "The switch controls delivery to that peer; removing a peer deletes trust on this device."
      }
    },
    {
      "id": "connections-4",
      "window": "connections",
      "target": "remote-pairing",
      "title": {
        "zh-CN": "不在同一网络？从这里邀请",
        "en": "Connect across networks"
      },
      "body": {
        "zh-CN": "展开「远程配对」，创建邀请链接，或粘贴另一端的链接。成功连接后仍需在两端确认验证码。",
        "en": "Expand Remote pairing to create an invite or paste one from the other device. You still need to verify the code on both devices."
      },
      "tip": {
        "zh-CN": "此入口在「自动」或「仅 Iroh」模式显示；邀请 120 秒有效。",
        "en": "Available in Automatic or Iroh only mode. Invites expire in 120 seconds."
      }
    },
    {
      "id": "history-5",
      "window": "history",
      "target": "history-items",
      "title": {
        "zh-CN": "从一次复制开始",
        "en": "Start with one copy"
      },
      "body": {
        "zh-CN": "引导期间这里显示安全的演示记录，覆盖文字、网站、代码、路径、图片和文件。真实使用时，在一台设备复制一次，已连接的另一台设备就会收到并进入各自的本机历史。",
        "en": "During the tour these are safe samples covering text, websites, code, paths, images, and files. In normal use, copy once on one device; connected devices receive it and each keeps its own local history."
      },
      "tip": {
        "zh-CN": "演示记录不会写入数据库、剪贴板或另一台设备；结束引导并重新打开历史后恢复真实记录。",
        "en": "Samples never touch your database, clipboard, or other device. After the tour, reopen History to see your real entries."
      }
    },
    {
      "id": "history-restore",
      "window": "history",
      "target": "history-items",
      "title": {
        "zh-CN": "双击回溯，右键删除",
        "en": "Double-click to restore; right-click to delete"
      },
      "body": {
        "zh-CN": "双击任意演示记录：内容会回到剪贴板，随后到目标应用按 ⌘V 粘贴。右键同一条记录可以只删除这台设备上的副本，不影响另一台设备的历史。",
        "en": "Double-click any sample to restore it to the clipboard, then press ⌘V in the target app. Right-click a row to delete only this device's copy; the other device's history is unaffected."
      },
      "tip": {
        "zh-CN": "也可以选中记录后按 Enter 回溯、Delete 删除。",
        "en": "With a row selected, Enter restores and Delete removes it."
      }
    },
    {
      "id": "history-favorite-gesture",
      "window": "history",
      "target": "history-items",
      "title": {
        "zh-CN": "长按一条记录即可收藏",
        "en": "Long-press a row to favorite it"
      },
      "body": {
        "zh-CN": "按住一条演示记录约一秒，进度环完成后会加入收藏；对已收藏记录长按可取消。清空普通历史不会删除收藏。",
        "en": "Hold a sample row for about one second until the progress ring completes. Hold a favorite again to remove it. Clearing ordinary history keeps favorites."
      },
      "tip": {
        "zh-CN": "也可以用行尾操作菜单收藏。收藏仍占用存储配额。",
        "en": "The row actions menu can favorite too. Favorites still use storage quota."
      }
    },
    {
      "id": "history-6",
      "window": "history",
      "target": "history-search",
      "title": {
        "zh-CN": "用搜索与筛选找回内容",
        "en": "Search your history"
      },
      "body": {
        "zh-CN": "在高亮区域输入关键词，或按分类、日期筛选。找不到记录时先清除筛选，回到「全部」。",
        "en": "Enter a keyword here, or filter by content category and date. Clear filters and choose All if an item is missing."
      },
      "tip": {
        "zh-CN": "历史只保存在本机；同步内容会进入双方各自的历史。",
        "en": "History is stored locally on each device."
      }
    },
    {
      "id": "history-7",
      "window": "history",
      "target": "history-items",
      "title": {
        "zh-CN": "选中记录，按空格预览",
        "en": "Select a row, then press Space"
      },
      "body": {
        "zh-CN": "点选不同演示记录，按空格查看文字、代码、Markdown、图片、PDF、Word、PowerPoint、SVG 和文件批次；用预览窗口左右键逐项浏览。",
        "en": "Select different samples and press Space to preview text, code, Markdown, images, PDF, Word, PowerPoint, SVG, and file batches. Use the preview window's arrow controls to browse items."
      },
      "tip": {
        "zh-CN": "空格或 Esc 关闭预览；双击恢复，之后到目标应用粘贴。",
        "en": "Space or Esc closes the preview. Double-click to restore, then paste in the target app."
      }
    },
    {
      "id": "history-8",
      "window": "history",
      "target": "favorites",
      "title": {
        "zh-CN": "把常用内容留在收藏里",
        "en": "Keep frequently used content"
      },
      "body": {
        "zh-CN": "点击顶部星标打开收藏窗口。上面两种方式都能收藏：长按记录，或使用行尾操作菜单。",
        "en": "Click the star above to open Favorites. You can favorite by long-pressing a row or using its actions menu."
      },
      "tip": {
        "zh-CN": "收藏仍占用配额；要删除收藏，请在收藏窗口操作。",
        "en": "Favorites still use storage quota. Delete them from the Favorites window."
      }
    },
    {
      "id": "settings-9",
      "window": "settings",
      "target": "general",
      "title": {
        "zh-CN": "掌控同步和快捷键",
        "en": "Control sync and shortcuts"
      },
      "body": {
        "zh-CN": "这里可暂停本机剪贴板广播，设置开机启动、通知和两个全局快捷键。关闭窗口仍会后台运行；退出需用托盘菜单。",
        "en": "Pause local clipboard broadcasting here and configure launch at login, notifications, and global shortcuts. Closing windows keeps TailSync running; quit from the tray menu."
      },
      "tip": {
        "zh-CN": "暂停同步不会清空历史，也不等于撤销设备配对。",
        "en": "Pausing does not clear history or unpair devices."
      }
    },
    {
      "id": "settings-10",
      "window": "settings",
      "target": "history-settings",
      "title": {
        "zh-CN": "设置历史保留方式",
        "en": "Choose what history retains"
      },
      "body": {
        "zh-CN": "在这里调整保留条数（10–500），按需启用网络图片自动保存。macOS 的「第三方应用图片」可查看微信、QQ 图片支持情况和授权开关。",
        "en": "Adjust the history limit (10–500) and automatic network-image saving here. On macOS, expand Third-party app images for WeChat and QQ image support and access controls."
      },
      "tip": {
        "zh-CN": "网络图片保存取决于内容是否可访问。",
        "en": "Saving network images depends on resource availability."
      }
    },
    {
      "id": "settings-11",
      "window": "settings",
      "target": "storage",
      "title": {
        "zh-CN": "查看空间与存储位置",
        "en": "Manage storage"
      },
      "body": {
        "zh-CN": "这里显示空间用量，可调整配额、迁移存储目录。",
        "en": "Inspect storage usage, adjust the quota, or migrate the storage folder here."
      },
      "tip": {
        "zh-CN": "存储不可用时先恢复原路径，再迁移。文件续传状态最多保留 24 小时。",
        "en": "Restore unavailable storage before migrating. Unfinished file transfers are retained for up to 24 hours."
      }
    },
    {
      "id": "settings-12",
      "window": "settings",
      "target": "appearance",
      "title": {
        "zh-CN": "挑选主题与界面语言",
        "en": "Choose a theme and language"
      },
      "body": {
        "zh-CN": "在这里切换浅色、深色、跟随系统及内置配色；也可导入 .tailsync-theme 主题包，管理主题更新、回退或删除。",
        "en": "Choose Light, Dark, System, and built-in palettes here. Import .tailsync-theme packages and manage updates, rollback, or removal."
      },
      "tip": {
        "zh-CN": "外观与语言偏好在当前设备设置。",
        "en": "Appearance and language are configured on this device."
      }
    },
    {
      "id": "settings-13",
      "window": "settings",
      "target": "updates",
      "title": {
        "zh-CN": "最后，认识更新入口",
        "en": "Find updates when you need them"
      },
      "body": {
        "zh-CN": "这里显示当前版本，并可检查可用更新。以后想重走一遍，在设置顶部点击「新手引导」。",
        "en": "Check your installed version and available updates here. Restart this walkthrough from Getting started at the top of Settings."
      },
      "tip": {
        "zh-CN": "无法同步时先检查设备在线状态、两个同步开关和存储空间。完成引导不会自动完成配对。",
        "en": "If sync fails, check peer status, both sync switches, and storage. Finishing the tour does not pair devices."
      }
    }
  ]
}
"""#
}
