import { useEffect, useMemo, useState } from "react";
import {
  ArrowDown,
  ArrowRight,
  ArrowUpRight,
  CalendarDays,
  Check,
  ClipboardCopy,
  Clock3,
  CloudOff,
  Code2,
  Download,
  File,
  FileText,
  GitFork,
  Image as ImageIcon,
  Laptop,
  LockKeyhole,
  Menu,
  Monitor,
  Moon,
  Network,
  RadioTower,
  Route,
  Sun,
  Tags,
  X,
  Zap,
} from "lucide-react";
import { ClipboardPreview } from "./components/ClipboardPreview";
import { HistoryIntelligence } from "./components/HistoryIntelligence";
import { ProductWindow } from "./components/ProductWindow";
import { RecoverySequence } from "./components/RecoverySequence";
import { SecurityHandshake } from "./components/SecurityHandshake";
import { SyncField } from "./components/SyncField";

const tailsyncIcon = "/tailsync-icon.png";

type RouteMode = "auto" | "lan" | "tailscale";
type ClipboardKind = "text" | "image" | "file";
type TimeTheme = "light" | "dark";
type ThemePreference = "auto" | TimeTheme;

const GITHUB_URL = "https://github.com/monet4070/TailSync";
const RELEASE_URL = `${GITHUB_URL}/releases`;
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
    title: "先走最快的路。",
    description:
      "TailSync 持续验证 LAN 与 Tailscale 路径。局域网可达时优先直连，离开同一网络后自动切换。",
    interface: "LAN 优先 · Tailscale 待命",
  },
  lan: {
    eyebrow: "LAN ONLY / ROUTE 02",
    title: "留在你的网络里。",
    description:
      "仅在局域网发现设备与传输内容。路径更短，也不会把数据交给一个额外的云端中转层。",
    interface: "mDNS / DNS-SD · TCP 19890",
  },
  tailscale: {
    eyebrow: "TAILSCALE / ROUTE 03",
    title: "跨过网络边界。",
    description:
      "通过同一 Tailnet 连接远端设备。TailSync 仍会主动检查应用服务，而不是只相信设备在线状态。",
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
    title: "一段想法，瞬间接力。",
    meta: "ACK · 去重 · 本地历史",
    description:
      "复制代码、链接或段落。另一台设备收到确认后写入剪贴板，并保留可搜索、可恢复的本地历史。",
  },
  image: {
    label: "图片",
    index: "02",
    title: "像素保持完整。",
    meta: "原图同步 · 本地缩略图",
    description:
      "截图和图片以原始内容同步，历史预览在本地生成。无需先保存文件，也无需经过聊天窗口。",
  },
  file: {
    label: "文件",
    index: "03",
    title: "大文件也知道从哪继续。",
    meta: "1 MiB 分块 · Blake3 校验",
    description:
      "文件按块传输、逐段确认。运行期间短暂断线后可以从已确认偏移继续，而不是重新开始。",
  },
};

const heroStats = [
  { value: "4 ms", label: "局域网直达延迟" },
  { value: "0", label: "云端中转 · 数据不出网" },
  { value: "08", label: "本地智能内容分类" },
  { value: "E2E", label: "Noise XX 端到端加密" },
];

function App() {
  const [menuOpen, setMenuOpen] = useState(false);
  const [routeMode, setRouteMode] = useState<RouteMode>("auto");
  const [activeFlow, setActiveFlow] = useState<ClipboardKind>("text");
  const [themePreference, setThemePreference] = useState<ThemePreference>(getInitialThemePreference);
  const [automaticTheme, setAutomaticTheme] = useState<TimeTheme>(getAutomaticTheme);
  const [scrollProgress, setScrollProgress] = useState(0);
  const currentRoute = routeCopy[routeMode];
  const currentFlow = flowData[activeFlow];
  const timeTheme = themePreference === "auto" ? automaticTheme : themePreference;

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
      ?.setAttribute("content", timeTheme === "light" ? "#fbfbfd" : "#000000");

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
    const updateProgress = () => {
      const maxScroll = document.documentElement.scrollHeight - window.innerHeight;
      setScrollProgress(maxScroll > 0 ? window.scrollY / maxScroll : 0);
    };
    updateProgress();
    window.addEventListener("scroll", updateProgress, { passive: true });
    window.addEventListener("resize", updateProgress);
    return () => {
      window.removeEventListener("scroll", updateProgress);
      window.removeEventListener("resize", updateProgress);
    };
  }, []);

  useEffect(() => {
    const elements = [...document.querySelectorAll<HTMLElement>("[data-reveal]")];
    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.setAttribute("data-visible", "true");
            observer.unobserve(entry.target);
          }
        });
      },
      { threshold: 0.14 },
    );
    elements.forEach((element) => observer.observe(element));
    return () => observer.disconnect();
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
      <div
        className="scroll-progress"
        style={{ transform: `scaleX(${scrollProgress})` }}
        aria-hidden="true"
      />

      <header className="site-header">
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
          <a href="#routing" onClick={closeMenu}>智能路由</a>
          <a href="#history" onClick={closeMenu}>智能历史</a>
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
              TailSync 2.0 · 本地优先的跨设备剪贴板
            </div>
            <h1 id="hero-title">
              复制。
              <br />
              <span>穿过设备边界。</span>
            </h1>
            <p>
              TailSync 让文本、图片和文件在 Mac 与 Windows 之间直接流动。
              局域网优先，Tailscale 兜底；智能历史自动分类，休眠唤醒后自动恢复，全程加密且不依赖云端剪贴板。
            </p>
            <div className="hero-actions">
              <a className="button button-primary" href={RELEASE_URL} target="_blank" rel="noreferrer">
                <Download size={17} />
                获取 TailSync
              </a>
              <a className="button button-quiet" href="#routing">
                了解如何工作
                <ArrowDown size={15} />
              </a>
            </div>
          </div>

          <a className="scroll-cue" href="#manifesto" aria-label="继续浏览">
            <span>SCROLL</span>
            <ArrowDown size={14} />
          </a>
        </section>

        <div className="stat-band" data-reveal>
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
                剪贴板本来就该像你的手一样，
                <strong>跟着你，而不是困在某一台设备里。</strong>
              </p>
              <div className="manifesto-note">
                <CloudOff size={26} strokeWidth={1.5} />
                <p>
                  没有云端收件箱，也没有把内容发给自己的临时聊天。
                  TailSync 在你拥有的网络与设备之间建立一条可信通道。
                </p>
              </div>
            </div>

            <div className="relay-visual" aria-label="TailSync 实时剪贴板接力演示">
              <div className="relay-head">
                <span><RadioTower size={14} /> LIVE CLIPBOARD RELAY</span>
                <small>LOCAL / END-TO-END</small>
              </div>
              <div className="relay-stage">
                <div className="relay-axis" aria-hidden="true" />
                <div className="relay-scanner" aria-hidden="true" />
                <div className="relay-device relay-device-mac">
                  <Laptop size={24} />
                  <strong>MAC</strong>
                  <small>SOURCE / 4 ms</small>
                </div>
                <div className="relay-core">
                  <div className="relay-orbit" aria-hidden="true"><i /><i /><i /></div>
                  <div className="relay-core-face">
                    <ClipboardCopy size={25} />
                    <strong>TAILSYNC</strong>
                    <small>ENCRYPTED</small>
                  </div>
                </div>
                <div className="relay-device relay-device-pc">
                  <Monitor size={24} />
                  <strong>PC</strong>
                  <small>TARGET / READY</small>
                </div>
                <span className="relay-packet relay-packet-text" aria-hidden="true"><FileText size={13} /></span>
                <span className="relay-packet relay-packet-image" aria-hidden="true"><ImageIcon size={13} /></span>
                <span className="relay-packet relay-packet-file" aria-hidden="true"><File size={13} /></span>
              </div>
              <div className="relay-footer">
                <span><Check size={13} /> VERIFIED PATH</span>
                <span>ACK / 00:00.004</span>
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
            <div className="route-switcher" role="group" aria-label="连接路径">
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
              <small>SIMULATION / 02 DEVICES</small>
            </div>
            <div className="route-stage">
              <div className="lab-device lab-device-a">
                <Laptop size={25} />
                <strong>MAC</strong>
                <small>192.168.1.24</small>
              </div>
              <div className="route-path route-path-lan">
                <span className="route-packet packet-lan"><ClipboardCopy size={13} /></span>
                <span className="route-path-label">
                  <b>LAN</b>
                  <small>{routeMode === "tailscale" ? "STANDBY" : "ACTIVE"}</small>
                </span>
              </div>
              <div className="route-path route-path-tail">
                <span className="route-packet packet-tail"><LockKeyhole size={13} /></span>
                <span className="route-path-label">
                  <b>TAILNET</b>
                  <small>{routeMode === "lan" ? "DISABLED" : routeMode === "tailscale" ? "ACTIVE" : "READY"}</small>
                </span>
              </div>
              <div className="lab-device lab-device-b">
                <Monitor size={25} />
                <strong>PC</strong>
                <small>100.72.18.9</small>
              </div>
              <div className="route-core">
                <div className="route-core-content">
                  <span><Route size={19} /></span>
                  <strong>{routeMode === "auto" ? "AUTO" : routeMode === "lan" ? "LAN" : "TAIL"}</strong>
                  <small>ROUTER</small>
                </div>
              </div>
            </div>
            <div className="lab-status">
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
              不只是一行字。
              <br />
              <span>每种内容，都有自己的传输逻辑。</span>
            </h2>
          </div>

          <div className="flow-workbench" data-reveal>
            <div className="flow-tabs" role="tablist" aria-label="同步内容类型">
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

        <section className="security-section" id="security">
          <div className="security-word" aria-hidden="true">PRIVATE</div>
          <div className="security-inner">
            <div className="security-copy" data-reveal>
              <div className="section-marker section-marker-light">
                <span>05</span>
                <small>TRUST, EXPLICITLY</small>
              </div>
              <h2>
                安全不是一个开关。
                <br />
                <span>它是整条路径。</span>
              </h2>
              <p>
                每台设备生成持久 X25519 身份。首次连接需要限时配对、六位验证码和双端确认，之后通过 Noise XX 建立加密会话。
              </p>
              <div className="security-facts">
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
              <span>06</span>
              <small>INTELLIGENCE, IN CONTEXT</small>
            </div>
            <h2>安静常驻。<br />需要时，历史已经整理好。</h2>
            <p>
              TailSync 在后台监听、同步、分类与确认。打开历史时，内容类型、多标签、置信度和日期范围都已经就位，找回记录不再依赖逐条翻看。
            </p>
            <div className="product-points">
              <span><Tags size={17} /> 八类内容与多标签识别</span>
              <span><CalendarDays size={17} /> 七种日期范围与自定义筛选</span>
              <span><RadioTower size={17} /> 真实在线状态与路径延迟</span>
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
                <b>4 ms</b>
              </div>
              <div className="route-signal" aria-hidden="true">
                {Array.from({ length: 7 }, (_, index) => <i key={`signal-${index}`} />)}
              </div>
            </div>
            <div className="transfer-float">
              <File size={17} />
              <div className="transfer-float-copy">
                <strong>TailSync-v2.0.1.dmg</strong>
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
          <div className="architecture-diagram" data-reveal>
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
            <span>LATEST RELEASE / 2.0.1</span>
            <h2>你的剪贴板，<br />应该跟着你。</h2>
            <p>macOS 与 Windows。开源。MIT License。</p>
            <div className="download-actions">
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
        <p>智能本地历史 · 弹性直连同步 · 只为你信任的设备而建</p>
        <span>© {year} TAILSYNC · MIT</span>
      </footer>
    </div>
  );
}

export default App;
