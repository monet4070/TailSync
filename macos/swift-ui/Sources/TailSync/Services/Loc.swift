import Foundation
import AppKit
import SwiftUI

extension Notification.Name {
    static let tailSyncLocaleChanged = Notification.Name("TailSyncLocaleChanged")
}

/// Observable localization service.  Reads/watches the language and theme
/// settings saved by the Rust backend.
final class Loc: ObservableObject {
    static let shared = Loc()

    @Published var lang: String = "en" {
        didSet {
            guard lang != oldValue else { return }
            NotificationCenter.default.post(name: .tailSyncLocaleChanged, object: nil)
        }
    }
    @Published var theme: String = "system"
    @Published var colorTheme: String = TailSyncColorTheme.tailsync.rawValue
    @Published var notificationsEnabled: Bool = true

    private static let configURL: URL = {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("com.tailsync.TailSync")
        return dir.appendingPathComponent("config-v2.json")
    }()

    private init() {
        reload()
    }

    func reload() {
        if let data = try? Data(contentsOf: Self.configURL),
           let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            lang = obj["language"] as? String ?? fallbackLang()
            theme = obj["theme"] as? String ?? "system"
            colorTheme = TailSyncColorTheme(
                storedValue: obj["color_theme"] as? String ?? "tailsync"
            ).rawValue
        } else {
            lang = fallbackLang()
            theme = "system"
            colorTheme = TailSyncColorTheme.tailsync.rawValue
        }
        applyTheme()
    }

    private func fallbackLang() -> String {
        Locale.current.language.languageCode?.identifier == "zh" ? "zh-CN" : "en"
    }

    // ── Dictionary ──────────────────────────────────────────────

    private static let strings: [String: [String: String]] = [
        "en": [
            "history.loadError": "Could not load history",
            "history.title": "History",
            "history.search": "Search history...",
            "history.empty": "No entries",
            "history.restored": "Restored to clipboard",
            "history.restore": "Restore",
            "history.delete": "Delete",
            "history.clearAll": "Clear All History",
            "history.confirmClear": "Delete all clipboard history?",
            "history.categoryFilter": "Filter by category",
            "history.category.all": "All categories",
            "history.category.text": "Text",
            "history.category.website": "Website",
            "history.category.code": "Code",
            "history.category.command": "Command",
            "history.category.structured_data": "Structured data",
            "history.category.path": "Path",
            "history.category.image": "Image",
            "history.category.file": "File",
            "settings.title": "Settings",
            "settings.general": "General",
            "settings.notifications": "Notifications",
            "settings.progressBar": "Progress bar",
            "settings.history": "History",
            "settings.limit": "Limit",
            "settings.appearance": "Appearance",
            "settings.theme": "Theme",
            "settings.themeSystem": "System",
            "settings.themeLight": "Light",
            "settings.themeDark": "Dark",
            "settings.colorTheme": "Visual theme",
            "settings.colorThemeDescription": "Type, material, colour, and density",
            "settings.colorTheme.tailsync": "Canvas",
            "settings.colorTheme.ocean": "Flux",
            "settings.colorTheme.forest": "Ledger",
            "settings.colorTheme.rose": "Aura",
            "settings.colorTheme.high-contrast": "Mono",
            "settings.selected": "Selected",
            "settings.language": "Language",
            "settings.saved": "Settings saved",
            "settings.loading": "Connecting to backend...",
            "settings.error": "Failed to connect",
            "settings.retry": "Retry",
            "menu.history": "History",
            "menu.settings": "Settings",
            "menu.quit": "Quit TailSync",
            "settings.network": "Network",
            "settings.connectionMode": "Connection",
            "settings.modeAuto": "Automatic",
            "settings.modeTailscale": "Tailscale",
            "settings.modeLan": "Local network",
            "settings.localDevice": "This device",
            "settings.identityLoading": "Loading identity...",
            "settings.copyPublicKey": "Copy public key",
            "settings.pairDevice": "Pair a device",
            "settings.peerHostname": "Device name",
            "settings.peerPublicKey": "Remote public key (Base64)",
            "settings.pair": "Pair",
            "settings.allowPairing": "Allow pairing",
            "settings.closePairing": "Close",
            "settings.pairingClosed": "Currently closed",
            "settings.waitingPairing": "Waiting for another device",
            "settings.secureHandshake": "Establishing a secure connection",
            "settings.compareCode": "Confirm that the other device shows the same code",
            "settings.waitingPeerConfirm": "Confirmed, waiting for the other device...",
            "settings.codesMatch": "Codes match",
            "settings.confirmed": "Confirmed",
            "settings.cancel": "Cancel",
            "settings.unpair": "Revoke pairing",
            "settings.removeDevice": "Remove",
            "settings.loadingDevices": "Discovering devices...",
            "settings.noDevices": "No devices found",
            "settings.devices": "devices",
            "settings.pairedOffline": "Paired · waiting for device",
            "settings.connected": "Connected",
            "settings.disconnected": "Not connected",
            "settings.online": "Online",
            "settings.offline": "Offline",
            "settings.confirming": "Confirming…",
            "settings.discovered": "Discovered",
            "settings.paired": "Paired",
            "settings.notPaired": "Not paired",
            "settings.refresh": "Refresh",
            "settings.testConnection": "Test connection",
        ],
        "zh-CN": [
            "history.loadError": "无法加载历史记录",
            "history.title": "历史记录",
            "history.search": "搜索历史...",
            "history.empty": "暂无记录",
            "history.restored": "历史已回溯至剪切板",
            "history.restore": "回溯",
            "history.delete": "删除",
            "history.clearAll": "清空所有记录",
            "history.confirmClear": "确定要删除所有记录吗？",
            "history.categoryFilter": "按分类筛选",
            "history.category.all": "全部分类",
            "history.category.text": "文本",
            "history.category.website": "网站",
            "history.category.code": "代码",
            "history.category.command": "命令",
            "history.category.structured_data": "结构化数据",
            "history.category.path": "路径",
            "history.category.image": "图片",
            "history.category.file": "文件",
            "settings.title": "设置",
            "settings.general": "通用",
            "settings.notifications": "通知",
            "settings.progressBar": "进度条",
            "settings.history": "历史记录",
            "settings.limit": "上限",
            "settings.appearance": "外观",
            "settings.theme": "主题",
            "settings.themeSystem": "跟随系统",
            "settings.themeLight": "浅色",
            "settings.themeDark": "深色",
            "settings.colorTheme": "视觉主题",
            "settings.colorThemeDescription": "排版、材质、色彩与界面密度",
            "settings.colorTheme.tailsync": "画布 Canvas",
            "settings.colorTheme.ocean": "流光 Flux",
            "settings.colorTheme.forest": "书页 Ledger",
            "settings.colorTheme.rose": "柔光 Aura",
            "settings.colorTheme.high-contrast": "单色 Mono",
            "settings.selected": "已选择",
            "settings.language": "语言",
            "settings.saved": "设置已保存",
            "settings.loading": "连接后端中...",
            "settings.error": "连接失败",
            "settings.retry": "重试",
            "menu.history": "历史记录",
            "menu.settings": "设置",
            "menu.quit": "退出 TailSync",
            "settings.network": "网络",
            "settings.connectionMode": "连接方式",
            "settings.modeAuto": "自动",
            "settings.modeTailscale": "Tailscale",
            "settings.modeLan": "局域网",
            "settings.localDevice": "本机设备",
            "settings.identityLoading": "正在加载设备身份…",
            "settings.copyPublicKey": "复制公钥",
            "settings.pairDevice": "配对设备",
            "settings.peerHostname": "设备名称",
            "settings.peerPublicKey": "远端公钥（Base64）",
            "settings.pair": "配对",
            "settings.allowPairing": "允许配对",
            "settings.closePairing": "关闭",
            "settings.pairingClosed": "当前关闭",
            "settings.waitingPairing": "等待另一台设备",
            "settings.secureHandshake": "正在建立安全连接",
            "settings.compareCode": "请确认另一台设备显示相同验证码",
            "settings.waitingPeerConfirm": "已确认，等待对端确认...",
            "settings.codesMatch": "验证码一致",
            "settings.confirmed": "已确认",
            "settings.cancel": "取消",
            "settings.unpair": "撤销配对",
            "settings.removeDevice": "删除",
            "settings.loadingDevices": "正在发现设备…",
            "settings.noDevices": "未发现设备",
            "settings.devices": "台设备",
            "settings.pairedOffline": "已配对 · 等待设备上线",
            "settings.connected": "已连接",
            "settings.disconnected": "未连接",
            "settings.online": "在线",
            "settings.offline": "离线",
            "settings.confirming": "正在确认…",
            "settings.discovered": "已发现",
            "settings.paired": "已配对",
            "settings.notPaired": "未配对",
            "settings.refresh": "刷新",
            "settings.testConnection": "测试连接",
        ],
    ]

    static func t(_ key: String) -> String {
        strings[shared.lang]?[key] ?? strings["en"]?[key] ?? key
    }

    // ── Theme ───────────────────────────────────────────────────

    func applyTheme() {
        DispatchQueue.main.async {
            switch self.theme {
            case "dark":  NSApp.appearance = NSAppearance(named: .darkAqua)
            case "light": NSApp.appearance = NSAppearance(named: .aqua)
            default:      NSApp.appearance = nil
            }
        }
    }
}
