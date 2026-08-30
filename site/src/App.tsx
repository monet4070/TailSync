import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDown,
  ArrowRight,
  ArrowUpRight,
  Check,
  ClipboardCopy,
  Clock3,
  CloudOff,
  Code2,
  Download,
  Eye,
  File,
  FileText,
  GitFork,
  Image as ImageIcon,
  Keyboard,
  Laptop,
  Menu,
  Monitor,
  Moon,
  Network,
  RadioTower,
  Route,
  ShieldCheck,
  Sun,
  Tags,
  X,
  Zap,
} from "lucide-react";
import { ClipboardPreview } from "./components/ClipboardPreview";
import { FavoritesWorkflow } from "./components/FavoritesWorkflow";
import { HistoryIntelligence } from "./components/HistoryIntelligence";
import { ProductWindow } from "./components/ProductWindow";
import { RecoverySequence } from "./components/RecoverySequence";
import { RichPreview } from "./components/RichPreview";
import { SecurityHandshake } from "./components/SecurityHandshake";
import { SyncField } from "./components/SyncField";
import { useOffstageGate } from "./hooks/useOffstageGate";
import { useScrollDriver } from "./hooks/useScrollDriver";
import {
  GITHUB_URL,
  MAC_INSTALLER_NAME,
  PRODUCT_FACTS,
  PRODUCT_VERSION,
  RELEASE_URL,
} from "./product";

const tailsyncIcon = "/tailsync-icon.png";

type RouteMode = "auto" | "lan" | "tailscale";
type ClipboardKind = "text" | "image" | "file";
type TimeTheme = "light" | "dark";
type ThemePreference = "auto" | TimeTheme;

const DAY_START_HOUR = 7;
const NIGHT_START_HOUR = 19;
const THEME_STORAGE_KEY = "tailsync-theme-preference";

function getAutomaticTheme(): TimeTheme {
  const hour = new Date().getHours();
  return hour >= DAY_START_HOUR && hour < NIGHT_START_HOUR ? "light" : "dark";
}

function isThemePreference(value: string | null): value is ThemePreference {
  return value === "auto" || value === "light" || value === "dark";
}

function getInitialThemePreference(): ThemePreference {
  if (typeof window === "undefined") return "auto";

  const override = new URLSearchParams(window.location.search).get("theme");
  if (isThemePreference(override)) return override;

  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (isThemePreference(stored)) return stored;
  } catch {
    // Storage can be unavailable in hardened browser contexts.
  }

  return "auto";
}

const routeCopy: Record<
  RouteMode,
  { eyebrow: string; title: string; description: string; interface: string }
> = {
  auto: {
    eyebrow: "AUTO / ROUTE 01",
    title: "自动选择连接方式",
    description:
      "两台设备在同一局域网时优先直连；局域网不可用时，如果它们在同一 Tailnet，TailSync 会自动改走 Tailscale。",
    interface: "LAN 优先 · Tailscale 待命",
  },
  lan: {
    eyebrow: "LAN ONLY / ROUTE 02",
    title: "只在局域网内同步",
    description:
      "设备发现和内容传输都留在当前局域网，适合只在家里或办公室使用。",
    interface: "mDNS / DNS-SD · TCP 19890",
  },
  tailscale: {
    eyebrow: "TAILSCALE / ROUTE 03",
    title: "不在同一网络也能同步",
    description:
      "两台设备加入同一 Tailnet 后即可连接。TailSync 会检查应用是否真的可用，不只看设备是否在线。",
    interface: "Tailnet · 主动健康检查",
  },
};

const flowData: Record<
  ClipboardKind,
  {
    label: string;
    index: string;
    title: string;
    meta: string;
    description: string;
  }
> = {
  text: {
    label: "文本",
    index: "01",
    title: "文本直接到另一台设备",
    meta: "ACK · 去重 · 本地历史",
    description:
      "复制代码、链接或段落。另一台设备收到确认后写入剪贴板，并保留可搜索、可恢复的本地历史。",
  },
  image: {
    label: "图片",
    index: "02",
    title: "截图和图片按原图同步",
    meta: "原图同步 · 本地缩略图",
    description:
      "截图和图片以原始内容同步，历史预览在本地生成。无需先保存文件，也无需经过聊天窗口。",
  },
  file: {
    label: "文件",
    index: "03",
    title: "文件断线后可以继续传",
    meta: "1 MiB 分块 · Blake3 校验",
    description:
      "文件按块传输、逐段确认。运行期间短暂断线后可以从已确认偏移继续，而不是重新开始。",
  },
};

// Every figure here must be traceable to source via PRODUCT_FACTS. An earlier
// version of this band led with "4 ms 局域网直达延迟", which nothing in the
// codebase supports — LAN latency depends entirely on the user's network, so
// publishing a fixed number was a falsifiable claim for no benefit.
const heroStats = [
  { value: "03", label: "历史 · 收藏 · 预览窗口" },
  { value: "0", label: "云端中转 · 数据不出网" },
  {
    value: String(PRODUCT_FACTS.categoryCount).padStart(2, "0"),
    label: "本地内容分类",
  },
  { value: "E2E", label: "Noise XX 端到端加密" },
];

function App() {
  const [menuOpen, setMenuOpen] = useState(false);
  const [routeMode, setRouteMode] = useState<RouteMode>("auto");
  const [activeFlow, setActiveFlow] = useState<ClipboardKind>("text");
  const [themePreference, setThemePreference] = useState<ThemePreference>(getInitialThemePreference);
  const [automaticTheme, setAutomaticTheme] = useState<TimeTheme>(getAutomaticTheme);
  const headerRef = useRef<HTMLElement>(null);
  const currentRoute = routeCopy[routeMode];
  const currentFlow = flowData[activeFlow];
  const timeTheme = themePreference === "auto" ? automaticTheme : themePreference;

  useScrollDriver(headerRef);
  useOffstageGate();

  const year = useMemo(() => new Date().getFullYear(), []);

  useEffect(() => {
    if (themePreference !== "auto") return;
    const updateAutomaticTheme = () => setAutomaticTheme(getAutomaticTheme());
    updateAutomaticTheme();
    const timer = window.setInterval(updateAutomaticTheme, 60_000);
    document.addEventListener("visibilitychange", updateAutomaticTheme);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", updateAutomaticTheme);
    };
  }, [themePreference]);

  useEffect(() => {
    document.documentElement.dataset.theme = timeTheme;
    document.documentElement.dataset.themePreference = themePreference;
    document.documentElement.style.colorScheme = timeTheme;
    document
      .querySelector<HTMLMetaElement>('meta[name="theme-color"]')
      ?.setAttribute("content", timeTheme === "light" ? "#f2f0e9" : "#14130e");

    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, themePreference);
    } catch {
      // The selected theme still works for the current session without storage.
    }
  }, [themePreference, timeTheme]);

  useEffect(() => {
    const syncThemeAcrossTabs = (event: StorageEvent) => {
      if (event.key === THEME_STORAGE_KEY && isThemePreference(event.newValue)) {
        setThemePreference(event.newValue);
      }
    };
    window.addEventListener("storage", syncThemeAcrossTabs);
    return () => window.removeEventListener("storage", syncThemeAcrossTabs);
  }, []);

  useEffect(() => {
    const elements = [...document.querySelectorAll<HTMLElement>("[data-reveal]")];
    const timers = new Set<number>();

    // `will-change` on 20 large subtrees is worth it for the ~1s the reveal
    // runs and pure cost forever after, so retire the hint once each element
    // has landed. The timer also covers child-owned reveals such as stat-band,
    // whose container deliberately has no transition of its own.
    const settle = (element: HTMLElement) => {
      element.dataset.settled = "true";
    };

    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (!entry.isIntersecting) return;
          const element = entry.target as HTMLElement;
          element.setAttribute("data-visible", "true");
          observer.unobserve(element);
          element.addEventListener("transitionend", () => settle(element), { once: true });
          timers.add(window.setTimeout(() => settle(element), 1_600));
        });
      },
      { threshold: 0.14 },
    );
    elements.forEach((element) => observer.observe(element));
    return () => {
      observer.disconnect();
      timers.forEach((timer) => window.clearTimeout(timer));
    };
  }, []);

  const closeMenu = () => setMenuOpen(false);
  const selectTheme = (preference: ThemePreference) => {
    setThemePreference(preference);

    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, preference);
    } catch {
      // The in-memory preference remains available for this session.
    }

    const url = new URL(window.location.href);
    if (url.searchParams.has("theme")) {
      url.searchParams.delete("theme");
      window.history.replaceState(window.history.state, "", `${url.pathname}${url.search}${url.hash}`);
    }
  };

  return (
    <div className={`landing-shell theme-${timeTheme}`} id="top">
      <div className="scroll-progress" aria-hidden="true" />

      <header className="site-header" ref={headerRef}>
        <a className="brand" href="#top" onClick={closeMenu}>
          <img src={tailsyncIcon} alt="TailSync" />
          <span>TailSync</span>
          <small>V2</small>
        </a>

        <button
          className="menu-toggle"
          type="button"
          aria-label={menuOpen ? "关闭导航" : "打开导航"}
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen((value) => !value)}
        >
          {menuOpen ? <X size={20} /> : <Menu size={20} />}
        </button>

        <nav className={menuOpen ? "site-nav is-open" : "site-nav"}>
          <div
            className={`theme-switcher preference-${themePreference}`}
            role="radiogroup"
            aria-label="显示模式"
          >
            <i className="theme-switch-indicator" aria-hidden="true" />
            <button
              className={themePreference === "auto" ? "active" : ""}
              type="button"
              role="radio"
              aria-label={`自动模式，当前显示${timeTheme === "light" ? "浅色" : "深色"}`}
              aria-checked={themePreference === "auto"}
              title={`自动：07:00–19:00 浅色，当前${timeTheme === "light" ? "浅色" : "深色"}`}
              onClick={() => selectTheme("auto")}
            >
              <Clock3 size={14} />
              <span>自动</span>
            </button>
            <button
              className={themePreference === "light" ? "active" : ""}
              type="button"
              role="radio"
              aria-label="浅色模式"
              aria-checked={themePreference === "light"}
              title="浅色模式"
              onClick={() => selectTheme("light")}
            >
              <Sun size={14} />
              <span>浅色</span>
            </button>
            <button
              className={themePreference === "dark" ? "active" : ""}
              type="button"
              role="radio"
              aria-label="深色模式"
              aria-checked={themePreference === "dark"}
              title="深色模式"
              onClick={() => selectTheme("dark")}
            >
              <Moon size={14} />
              <span>深色</span>
            </button>
          </div>
          <a href="#routing" onClick={closeMenu}>连接</a>
          <a href="#history" onClick={closeMenu}>历史</a>
          <a href="#favorites" onClick={closeMenu}>收藏</a>
          <a href="#security" onClick={closeMenu}>安全</a>
          <a href="/themes.html">主题工坊</a>
          <a className="nav-source" href={GITHUB_URL} target="_blank" rel="noreferrer">
            <GitFork size={14} />
            源码
          </a>
          <a className="nav-cta" href={RELEASE_URL} target="_blank" rel="noreferrer">
            获取
            <ArrowUpRight size={13} />
          </a>
        </nav>
      </header>

      <main>
        <section className="hero" aria-labelledby="hero-title">
          <SyncField theme={timeTheme} />
          <div className="hero-copy">
            <div className="hero-kicker">
              <span className="live-dot" />
              TailSync {PRODUCT_VERSION} · Mac 与 Windows 剪贴板同步
            </div>
            <h1 id="hero-title">
              在一台设备复制，
              <br />
              <span>另一台直接粘贴。</span>
            </h1>
            <p>
              TailSync 在 Mac 和 Windows 之间同步文本、图片和文件。内容可以从本地历史找回，也可以长按收藏。
              局域网可用时直连，远程时通过 Tailscale，数据不经过 TailSync 的云端服务器。
            </p>
            <div className="hero-actions">
              <a className="button button-primary" href={RELEASE_URL} target="_blank" rel="noreferrer">
                <Download size={17} />
                下载 TailSync
              </a>
              <a className="button button-quiet" href="#favorites">
                了解收藏功能
                <ArrowDown size={15} />
              </a>
            </div>
          </div>

          <a className="scroll-cue" href="#manifesto" aria-label="继续浏览">
            <span>SCROLL</span>
            <ArrowDown size={14} />
          </a>
        </section>

        <div className="stat-band" data-reveal data-cascade>
          {heroStats.map((stat) => (
            <div key={stat.label}>
              <strong>{stat.value}</strong>
              <span>{stat.label}</span>
            </div>
          ))}
        </div>

        <section className="manifesto" id="manifesto">
          <div className="manifesto-inner" data-reveal>
            <div className="manifesto-copy">
              <div className="section-marker">
                <span>01</span>
                <small>WHY TAILSYNC</small>
              </div>
              <p className="manifesto-lead">
                如果你经常在 Mac 和 Windows 之间切换，
                <strong>复制内容不该还要靠聊天软件或临时文件。</strong>
              </p>
              <div className="manifesto-note">
                <CloudOff size={26} strokeWidth={1.5} />
                <p>
                  TailSync 不提供云端收件箱。内容只在你的设备之间传输，
                  同一网络时走局域网，远程连接时使用你自己的 Tailscale 网络。
                </p>
              </div>
            </div>

            <div className="relay-visual" aria-label="TailSync 实时剪贴板接力演示">
              <div className="relay-head">
                <span><RadioTower size={14} /> LIVE CLIPBOARD RELAY</span>
                <small>LOCAL / END-TO-END</small>
              </div>
              <div className="relay-stage">
                <div className="orrery" aria-hidden="true">
                  <div className="orrery-rings"><i /><i /><i /></div>

                  <div className="orrery-arm orrery-arm-1">
                    <div className="orrery-hold">
                      <span className="orrery-chip"><FileText size={14} /></span>
                    </div>
                  </div>
                  <div className="orrery-arm orrery-arm-2">
                    <div className="orrery-hold">
                      <span className="orrery-chip"><ImageIcon size={14} /></span>
                    </div>
                  </div>
                  <div className="orrery-arm orrery-arm-3 orrery-arm-rev">
                    <div className="orrery-hold">
                      <span className="orrery-chip"><File size={14} /></span>
                    </div>
                  </div>

                  <div className="orrery-center">
                    <ClipboardCopy size={24} />
                    <em>现在</em>
                  </div>
                </div>
              </div>
              <div className="relay-footer" data-cascade>
                <span><Check size={13} /> VERIFIED PATH</span>
                <span>ACK / RECEIPT</span>
                <span>NO CLOUD HOP</span>
              </div>
            </div>
          </div>
        </section>

        <section className="routing-section" id="routing">
          <div className="routing-copy" data-reveal>
            <div className="section-marker">
              <span>02</span>
              <small>ADAPTIVE ROUTING</small>
            </div>
            <div className="eyebrow">{currentRoute.eyebrow}</div>
            <h2>{currentRoute.title}</h2>
            <p>{currentRoute.description}</p>
            <div className="route-interface">
              <RadioTower size={17} />
              <span>{currentRoute.interface}</span>
            </div>
            <div className="route-switcher" role="group" aria-label="连接路径" data-cascade>
              <button
                className={routeMode === "auto" ? "active" : ""}
                type="button"
                onClick={() => setRouteMode("auto")}
              >
                <Zap size={15} /> 自动
              </button>
              <button
                className={routeMode === "lan" ? "active" : ""}
                type="button"
                onClick={() => setRouteMode("lan")}
              >
                <Network size={15} /> 仅 LAN
              </button>
              <button
                className={routeMode === "tailscale" ? "active" : ""}
                type="button"
                onClick={() => setRouteMode("tailscale")}
              >
                <Route size={15} /> Tailscale
              </button>
            </div>
          </div>

          <div className={`route-lab mode-${routeMode}`} data-reveal>
            <div className="lab-header">
              <span>LIVE ROUTE LAB</span>
              <small>LAN 优先 · TAILNET 兜底</small>
            </div>
            <div className="route-stage" aria-label="LAN 与 Tailscale 路径选择演示">
              <svg className="route-map" viewBox="0 0 460 220" preserveAspectRatio="xMidYMid meet" aria-hidden="true">
                <path className="route-sketch route-sketch-lan" d="M42,110 C150,34 310,34 418,110" />
                <path className="route-sketch route-sketch-tail" d="M42,110 C150,186 310,186 418,110" />
                <path className="route-ink route-ink-lan" d="M42,110 C150,34 310,34 418,110" />
                <path className="route-ink route-ink-tail" d="M42,110 C150,186 310,186 418,110" />
                <circle className="route-node" cx="42" cy="110" r="5.5" />
                <circle className="route-node" cx="418" cy="110" r="5.5" />
                <text className="route-node-label" x="40" y="136">本机</text>
                <text className="route-node-label" x="420" y="136" textAnchor="end">对端</text>
              </svg>
              <span className="route-tag route-tag-lan">
                <b>LAN</b>
                <small>{routeMode === "tailscale" ? "STANDBY" : "ACTIVE"}</small>
              </span>
              <span className="route-tag route-tag-tail">
                <b>TAILNET</b>
                <small>{routeMode === "lan" ? "DISABLED" : routeMode === "tailscale" ? "ACTIVE" : "READY"}</small>
              </span>
            </div>
            <div className="lab-status" data-cascade>
              <span><i /> AUTHENTICATED</span>
              <span>NOISE XX</span>
              <span>HEALTH CHECK / 5S</span>
            </div>
          </div>
        </section>

        <section className="flow-section" id="flow">
          <div className="flow-heading" data-reveal>
            <div className="section-marker">
              <span>03</span>
              <small>ONE CLIPBOARD</small>
            </div>
            <h2>
              文本、图片和文件，
              <br />
              <span>使用各自合适的同步方式。</span>
            </h2>
          </div>

          <div className="flow-workbench" data-reveal>
            <div className="flow-tabs" role="tablist" aria-label="同步内容类型" data-cascade>
              {(Object.keys(flowData) as ClipboardKind[]).map((kind) => {
                const item = flowData[kind];
                const Icon = kind === "text" ? FileText : kind === "image" ? ImageIcon : File;
                return (
                  <button
                    key={kind}
                    className={activeFlow === kind ? "active" : ""}
                    type="button"
                    role="tab"
                    aria-selected={activeFlow === kind}
                    onClick={() => setActiveFlow(kind)}
                  >
                    <span className="flow-tab-index">{item.index}</span>
                    <span className="flow-tab-icon"><Icon size={19} /></span>
                    <span className="flow-tab-name">{item.label}</span>
                    <ArrowRight size={18} />
                  </button>
                );
              })}
            </div>

            <div className={`flow-preview flow-preview-${activeFlow}`}>
              <div className="preview-topline">
                <span>CLIP / {currentFlow.index}</span>
                <small>END-TO-END ENCRYPTED</small>
              </div>
              <ClipboardPreview key={activeFlow} active={activeFlow} />
              <div className="preview-copy">
                <span>{currentFlow.meta}</span>
                <h3>{currentFlow.title}</h3>
                <p>{currentFlow.description}</p>
              </div>
            </div>
          </div>
        </section>

        <HistoryIntelligence />

        <FavoritesWorkflow />

        <section className="preview-section" id="preview">
          <div className="preview-heading" data-reveal>
            <div className="section-marker">
              <span>06</span>
              <small>RICH PREVIEW</small>
            </div>
            <h2>
              从历史记录直接打开预览
              <br />
              <span>支持六种常用格式。</span>
            </h2>
            <p>
              在历史或收藏中选中一条记录，按空格即可打开独立预览窗口，原来的列表仍可继续使用。
              支持图片、文本、代码、Markdown、PDF 和 docx。
            </p>
            <div className="preview-facts" data-cascade>
              <span><Eye size={15} /> 独立非模态窗口</span>
              <span><ShieldCheck size={15} /> 负载上限 64 MiB</span>
              <span><Check size={15} /> Markdown 净化渲染</span>
            </div>
          </div>

          <RichPreview />

          <div className="preview-more" data-reveal>
            <div className="preview-keys" data-cascade>
              <Keyboard size={16} />
              <span><kbd>空格</kbd> 打开 / 关闭</span>
              <span><kbd>双击</kbd> 恢复到剪贴板</span>
              <span><kbd>Alt</kbd> + <kbd>←/→</kbd> 同批翻看</span>
              <span><kbd>Ctrl/⌘</kbd> + 滚轮 缩放</span>
            </div>
            <a className="button button-quiet" href="/preview.html">
              查看完整预览能力
              <ArrowUpRight size={15} />
            </a>
          </div>
        </section>

        <section className="security-section" id="security">
          <div className="security-word" aria-hidden="true">PRIVATE</div>
          <div className="security-inner">
            <div className="security-copy" data-reveal>
              <div className="section-marker section-marker-light">
                <span>07</span>
                <small>TRUST, EXPLICITLY</small>
              </div>
              <h2>
                首次配对需要两台设备确认
                <br />
                <span>之后的连接全程加密。</span>
              </h2>
              <p>
                每台设备生成持久 X25519 身份。首次连接需要限时配对、六位验证码和双端确认，之后通过 Noise XX 建立加密会话。
              </p>
              <div className="security-facts" data-cascade>
                <span><Check size={14} /> 不降级到明文协议</span>
                <span><Check size={14} /> 固定设备公钥</span>
                <span><Check size={14} /> ChaCha20-Poly1305</span>
              </div>
            </div>

            <SecurityHandshake />
          </div>
        </section>

        <section className="product-section" id="product">
          <div className="product-copy" data-reveal>
            <div className="section-marker">
              <span>08</span>
              <small>NATIVE WORKSPACE</small>
            </div>
            <h2>历史、收藏和预览<br />分别使用独立窗口。</h2>
            <p>
              TailSync 平时在后台同步。需要找内容时打开历史，长期保留的内容放进收藏，查看图片或文档时再打开预览；三个窗口可以单独关闭和移动。
            </p>
            <div className="product-points" data-cascade>
              <span><Tags size={17} /> 历史：搜索、分类与日期筛选</span>
              <span><ShieldCheck size={17} /> 收藏：保护记录与明确删除出口</span>
              <span><Eye size={17} /> 预览：独立非模态阅读窗口</span>
            </div>
          </div>

          <div className="product-stage" data-reveal>
            <div className="product-stage-label label-top">WINDOWS / REACT + TAURI</div>
            <ProductWindow />
            <div className="route-float">
              <div className="route-float-head">
                <span><RadioTower size={14} /> 当前路径</span>
                <i />
              </div>
              <strong>MacBook Pro</strong>
              <small>LAN · 192.168.1.24</small>
              <div className="route-float-meta">
                <span>CONNECTED</span>
                <b>直连</b>
              </div>
              <div className="route-signal" aria-hidden="true">
                {Array.from({ length: 7 }, (_, index) => <i key={`signal-${index}`} />)}
              </div>
            </div>
            <div className="transfer-float">
              <File size={17} />
              <div className="transfer-float-copy">
                <strong>{MAC_INSTALLER_NAME}</strong>
                <small>分块写入 / 来源已标记</small>
                <span className="transfer-progress"><i /></span>
              </div>
              <Check size={15} />
            </div>
            <div className="product-stage-label label-bottom">NATIVE FEEL / SHARED RUST CORE</div>
          </div>
        </section>

        <section className="architecture-strip">
          <div className="architecture-copy" data-reveal>
            <span>ONE PROTOCOL / TWO NATIVE EXPERIENCES</span>
            <h2>SwiftUI on Mac.<br />Tauri on Windows.<br /><b>Rust at the core.</b></h2>
          </div>
          <div className="architecture-diagram" data-reveal data-cascade>
            <div><Laptop size={26} /><span>macOS</span><small>SwiftUI</small></div>
            <i />
            <div className="core-node"><Code2 size={28} /><span>Core</span><small>Rust / main</small></div>
            <i />
            <div><Monitor size={26} /><span>Windows</span><small>Tauri</small></div>
          </div>
        </section>

        <RecoverySequence />

        <section className="download-section" id="download">
          <div className="download-grid" aria-hidden="true" />
          <div className="download-copy" data-reveal>
            <img src={tailsyncIcon} alt="" />
            <span>LATEST RELEASE / {PRODUCT_VERSION}</span>
            <h2>下载 TailSync，<br />开始在两台设备间复制粘贴。</h2>
            <p>支持 macOS 与 Windows，代码开源，使用 MIT License。</p>
            <div className="download-actions" data-cascade>
              <a className="button button-download" href={RELEASE_URL} target="_blank" rel="noreferrer">
                <Monitor size={18} />
                下载 Windows
                <Download size={16} />
              </a>
              <a className="button button-download button-download-alt" href={RELEASE_URL} target="_blank" rel="noreferrer">
                <Laptop size={18} />
                下载 macOS
                <Download size={16} />
              </a>
            </div>
            <p className="download-note">
              <ShieldCheck size={14} />
              这是社区版本。更新包会验证 TailSync 签名和 SHA-256；但 macOS 包尚未公证，Windows 包也没有商业代码签名，因此首次启动时系统会显示 Gatekeeper 或 SmartScreen 提醒。
            </p>
            <a className="source-link" href={GITHUB_URL} target="_blank" rel="noreferrer">
              <GitFork size={16} />
              在 GitHub 查看源码
              <ArrowUpRight size={14} />
            </a>
          </div>
        </section>
      </main>

      <footer className="site-footer">
        <div className="brand footer-brand">
          <img src={tailsyncIcon} alt="" />
          <span>TailSync</span>
        </div>
        <p>Mac 与 Windows 剪贴板同步 · 本地历史 · 端到端加密</p>
        <span>© {year} TAILSYNC · MIT</span>
      </footer>

    </div>
  );
}

export default App;
