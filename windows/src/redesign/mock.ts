// 静态原型专用 mock 数据 —— 不接任何后端，仅演示布局与交互。

export interface MockRoute {
  interface: "lan" | "iroh" | "tailscale";
  address: string;
  status: "connected" | "discovered" | "offline";
  latencyMs: number | null;
}

export interface MockPeer {
  hostname: string;
  fingerprint: string;
  trusted: boolean;
  enabled: boolean;
  routes: MockRoute[];
}

export const MOCK_SELF = {
  hostname: "MONET-MACBOOK",
  fingerprint: "sha256:9f2c 41ab 77e0 1d5f",
  routes: [
    { interface: "lan", address: "192.168.31.5", status: "connected", latencyMs: null },
    { interface: "iroh", address: "k9w3…x2qa", status: "connected", latencyMs: null },
  ] satisfies MockRoute[],
};

export const MOCK_PEERS: MockPeer[] = [
  {
    hostname: "DESKTOP-5K2P9QM",
    fingerprint: "sha256:1b8d 03cc 5aa2 f047",
    trusted: true,
    enabled: true,
    routes: [
      { interface: "lan", address: "192.168.31.8", status: "connected", latencyMs: 8 },
      { interface: "iroh", address: "m74d…p0zq", status: "discovered", latencyMs: null },
    ],
  },
  {
    hostname: "SURFACE-GO",
    fingerprint: "",
    trusted: false,
    enabled: false,
    routes: [{ interface: "lan", address: "192.168.31.20", status: "discovered", latencyMs: null }],
  },
];

export const MOCK_INVITE_LINK =
  "tailsync://pair/v1/eyJlbmRwb2ludCI6Ims5dzMiLCJleHBpcmVzIjoxNjl9";

export type ConnectionMode = "auto" | "lan_only" | "iroh_only" | "tailscale_only";

export interface MockState {
  syncEnabled: boolean;
  launchAtLogin: boolean;
  notifications: boolean;
  progressBar: boolean;
  syncShortcut: string[];
  historyShortcut: string[];
  connectionMode: ConnectionMode;
  pairingOpen: boolean;
  pairingSecondsLeft: number;
  remoteExpanded: boolean;
  inviteCreated: boolean;
  inviteCopied: boolean;
  inviteLinkDraft: string;
  peerEnabled: Record<string, boolean>;
  updatePhase: "ready" | "checking" | "current";
  toast: string | null;
}

export const INITIAL_MOCK_STATE: MockState = {
  syncEnabled: true,
  launchAtLogin: true,
  notifications: true,
  progressBar: true,
  syncShortcut: ["Win", "Alt", "/"],
  historyShortcut: ["Win", "Alt", ","],
  connectionMode: "auto",
  pairingOpen: false,
  pairingSecondsLeft: 120,
  remoteExpanded: false,
  inviteCreated: false,
  inviteCopied: false,
  inviteLinkDraft: "",
  peerEnabled: { "DESKTOP-5K2P9QM": true },
  updatePhase: "ready",
  toast: null,
};

export const MAC_SYNC_CAPS = ["⇧", "⌘", "S"];
export const MAC_HISTORY_CAPS = ["⇧", "⌘", "H"];

export const INTERFACE_LABEL: Record<MockRoute["interface"], string> = {
  lan: "局域网",
  iroh: "iroh",
  tailscale: "tailscale",
};

// 与 windows/src/i18n/zh-CN.json 严格一致的文案键值（原型直接内联）。
export const ZH = {
  connectionsTitle: "连接与设备",
  connectionsDescription: "选择 TailSync 发现设备的方式，并管理设备配对",
  modeAuto: "自动",
  modeLan: "局域网",
  modeIroh: "Iroh",
  modeTailscale: "Tailscale",
  general: "通用",
  generalDescription: "控制桌面通知和传输反馈",
  syncEnabled: "剪贴板同步",
  syncEnabledDescription: "把本机新复制的内容发送到已启用且已配对的设备",
  launchAtLogin: "开机时自动启动",
  launchAtLoginDescription: "登录后自动启动 TailSync",
  syncShortcut: "同步快捷键",
  syncShortcutDescription: "全局暂停或恢复剪贴板同步",
  historyShortcut: "历史记录快捷键",
  historyShortcutDescription: "从任意应用全局打开历史记录窗口",
  notifications: "通知",
  notificationsDescription: "远端内容到达时显示系统通知",
  progressBar: "进度条",
  progressDescription: "在历史窗口显示文件传输进度",
  history: "历史记录",
  storage: "存储",
  appearance: "外观",
  updates: "更新",
  updatesDescription: "版本信息与签名更新",
  updateCheck: "检查更新",
  updateChecking: "正在检查更新...",
  updateReady: "可以检查签名更新",
  updateCurrent: "TailSync 已是最新版",
  saved: "设置已保存",
};
