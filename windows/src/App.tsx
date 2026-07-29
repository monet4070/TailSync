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
  Copy,
  Download,
  File,
  FileText,
  Fingerprint,
  GitFork,
  History,
  Image as ImageIcon,
  Laptop,
  LockKeyhole,
  Menu,
  Monitor,
  Moon,
  Network,
  RadioTower,
  Route,
  ScanLine,
  Search,
  ShieldCheck,
  Sun,
  X,
  Zap,
} from "lucide-react";
import tailsyncIcon from "../src-tauri/icons/icon.png";

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
    meta: "ACK · 去重 · 加密历史",
    description:
      "复制代码、链接或段落。另一台设备收到确认后写入剪贴板，并保留可搜索、可恢复的本地历史。",
  },
  image: {
    label: "图片",
    index: "02",
    title: "像素保持完整。",
    meta: "加密落盘 · 本地缩略图",
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

function SyncField({ theme }: { theme: TimeTheme }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const context = canvas.getContext("2d");
    if (!context) return;

    const reducedMotion = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    const pointer = { x: -1000, y: -1000, active: false };
    const lightTheme = theme === "light";
    const colors = lightTheme
      ? ["#789600", "#16858c", "#df4f3b", "#191b17"]
      : ["#d8ff54", "#58dfe5", "#ff7158", "#f3f0e8"];
    const particles = Array.from({ length: 52 }, (_, index) => ({
      offset: index / 52,
      lane: (index % 7) - 3,
      speed: 0.000035 + (index % 5) * 0.000004,
      size: 1.4 + (index % 4) * 0.7,
      color: colors[index % colors.length],
    }));

    let width = 0;
    let height = 0;
    let frame = 0;
    let start = performance.now();

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      width = rect.width;
      height = rect.height;
      canvas.width = Math.max(1, Math.round(width * ratio));
      canvas.height = Math.max(1, Math.round(height * ratio));
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
    };

    const onPointerMove = (event: PointerEvent) => {
      const rect = canvas.getBoundingClientRect();
      pointer.x = event.clientX - rect.left;
      pointer.y = event.clientY - rect.top;
      pointer.active = true;
    };

    const onPointerLeave = () => {
      pointer.active = false;
    };

    const cubicPoint = (
      t: number,
      startX: number,
      startY: number,
      controlX1: number,
      controlY1: number,
      controlX2: number,
      controlY2: number,
      endX: number,
      endY: number,
    ) => {
      const inverse = 1 - t;
      return {
        x:
          inverse ** 3 * startX +
          3 * inverse ** 2 * t * controlX1 +
          3 * inverse * t ** 2 * controlX2 +
          t ** 3 * endX,
        y:
          inverse ** 3 * startY +
          3 * inverse ** 2 * t * controlY1 +
          3 * inverse * t ** 2 * controlY2 +
          t ** 3 * endY,
      };
    };

    const drawEndpoint = (x: number, y: number, time: number, reverse = false) => {
      const pulse = reducedMotion ? 0 : Math.sin(time * 0.002 + (reverse ? 2 : 0)) * 4;
      context.save();
      context.translate(x, y);
      context.strokeStyle = reverse
        ? lightTheme ? "rgba(22,133,140,.76)" : "rgba(88,223,229,.78)"
        : lightTheme ? "rgba(120,150,0,.8)" : "rgba(216,255,84,.82)";
      context.lineWidth = 1;
      context.strokeRect(-34 - pulse, -34 - pulse, 68 + pulse * 2, 68 + pulse * 2);
      context.strokeStyle = lightTheme
        ? "rgba(17,18,15,.16)"
        : "rgba(243,240,232,.16)";
      context.strokeRect(-50 + pulse, -50 + pulse, 100 - pulse * 2, 100 - pulse * 2);
      context.fillStyle = reverse
        ? lightTheme ? "#16858c" : "#58dfe5"
        : lightTheme ? "#789600" : "#d8ff54";
      context.fillRect(-3, -3, 6, 6);
      context.restore();
    };

    const draw = (time: number) => {
      context.clearRect(0, 0, width, height);
      const elapsed = reducedMotion ? 0 : time - start;
      const leftX = Math.max(88, width * 0.16);
      const rightX = Math.min(width - 88, width * 0.84);
      const centerY = height * 0.54;

      context.save();
      context.strokeStyle = lightTheme
        ? "rgba(17,18,15,.065)"
        : "rgba(243,240,232,.055)";
      context.lineWidth = 1;
      for (let x = 0; x < width; x += 72) {
        context.beginPath();
        context.moveTo(x, 0);
        context.lineTo(x, height);
        context.stroke();
      }
      for (let y = 0; y < height; y += 72) {
        context.beginPath();
        context.moveTo(0, y);
        context.lineTo(width, y);
        context.stroke();
      }
      context.restore();

      for (let lane = -3; lane <= 3; lane += 1) {
        const bend = lane * 28;
        context.beginPath();
        context.moveTo(leftX, centerY + lane * 8);
        context.bezierCurveTo(
          width * 0.36,
          centerY - 150 + bend,
          width * 0.64,
          centerY + 150 + bend,
          rightX,
          centerY - lane * 8,
        );
        context.strokeStyle = lane === 0
          ? lightTheme ? "rgba(120,150,0,.3)" : "rgba(216,255,84,.26)"
          : lightTheme ? "rgba(17,18,15,.12)" : "rgba(243,240,232,.10)";
        context.lineWidth = lane === 0 ? 1.4 : 0.8;
        context.stroke();
      }

      context.globalCompositeOperation = "lighter";
      particles.forEach((particle, index) => {
        const progress = reducedMotion
          ? particle.offset
          : (particle.offset + elapsed * particle.speed) % 1;
        const laneOffset = particle.lane * 8;
        const point = cubicPoint(
          progress,
          leftX,
          centerY + laneOffset,
          width * 0.36,
          centerY - 150 + particle.lane * 28,
          width * 0.64,
          centerY + 150 + particle.lane * 28,
          rightX,
          centerY - laneOffset,
        );
        let drawX = point.x;
        let drawY = point.y;

        if (pointer.active) {
          const deltaX = drawX - pointer.x;
          const deltaY = drawY - pointer.y;
          const distance = Math.hypot(deltaX, deltaY);
          if (distance < 110 && distance > 0) {
            const force = (110 - distance) / 110;
            drawX += (deltaX / distance) * force * 24;
            drawY += (deltaY / distance) * force * 24;
          }
        }

        const alpha = Math.sin(progress * Math.PI) * 0.92;
        context.globalAlpha = Math.max(0.12, alpha);
        context.fillStyle = particle.color;
        if (index % 3 === 0) {
          context.fillRect(
            drawX - particle.size * 1.6,
            drawY - particle.size,
            particle.size * 3.2,
            particle.size * 2,
          );
        } else {
          context.beginPath();
          context.arc(drawX, drawY, particle.size, 0, Math.PI * 2);
          context.fill();
        }
      });
      context.globalAlpha = 1;
      context.globalCompositeOperation = "source-over";

      drawEndpoint(leftX, centerY, time);
      drawEndpoint(rightX, centerY, time, true);

      if (pointer.active) {
        context.beginPath();
        context.arc(pointer.x, pointer.y, 18, 0, Math.PI * 2);
        context.strokeStyle = lightTheme
          ? "rgba(17,18,15,.28)"
          : "rgba(243,240,232,.28)";
        context.stroke();
      }

      frame = window.requestAnimationFrame(draw);
    };

    resize();
    window.addEventListener("resize", resize);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerleave", onPointerLeave);
    frame = window.requestAnimationFrame(draw);

    return () => {
      window.removeEventListener("resize", resize);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerleave", onPointerLeave);
      window.cancelAnimationFrame(frame);
      start = 0;
    };
  }, [theme]);

  return <canvas ref={canvasRef} className="sync-field" aria-hidden="true" />;
}

function ClipboardPreview({ active }: { active: ClipboardKind }) {
  if (active === "image") {
    return (
      <div className="preview-art preview-image" aria-label="图片同步预览">
        <div className="image-scan" aria-hidden="true" />
        <div className="image-plane image-plane-a" />
        <div className="image-plane image-plane-b" />
        <div className="image-pixels" aria-hidden="true">
          {Array.from({ length: 8 }, (_, index) => <span key={`pixel-${index}`} />)}
        </div>
        <div className="image-status">
          <ScanLine size={14} />
          PIXEL MAP / LIVE
        </div>
        <div className="image-caption">
          <ImageIcon size={15} />
          screenshot-0728.png
        </div>
      </div>
    );
  }

  if (active === "file") {
    return (
      <div className="preview-art preview-file" aria-label="文件传输预览">
        <div className="file-icon-wrap">
          <File size={42} strokeWidth={1.25} />
          <span>84</span>
        </div>
        <div className="file-preview-copy">
          <strong>prototype-v2.fig</strong>
          <span>84.2 MB / 84.2 MB · 84 CHUNKS</span>
        </div>
        <div className="file-progress-track">
          <span />
        </div>
        <div className="file-chunk-rail" aria-hidden="true">
          {Array.from({ length: 7 }, (_, index) => <span key={`chunk-${index}`} />)}
        </div>
        <div className="file-metrics">
          <span>OFFSET / 88,289,075</span>
          <span>1 MiB BLOCKS</span>
        </div>
        <div className="file-check">
          <Check size={13} />
          BLAKE3 VERIFIED
        </div>
      </div>
    );
  }

  return (
    <div className="preview-art preview-text" aria-label="文本同步预览">
      <div className="text-scan" aria-hidden="true" />
      <div className="text-preview-head">
        <span><Code2 size={16} /> payload.ts</span>
        <small>LIVE WRITE</small>
      </div>
      <pre className="text-code">
        <span className="code-line"><i>01</i><code><em>const</em> clipboard = <b>await</b> sync&#40;&#123;</code></span>
        <span className="code-line"><i>02</i><code>&nbsp;&nbsp;from: <strong>&quot;macOS&quot;</strong>,</code></span>
        <span className="code-line"><i>03</i><code>&nbsp;&nbsp;to: <strong>&quot;Windows&quot;</strong>,</code></span>
        <span className="code-line"><i>04</i><code>&nbsp;&nbsp;encrypted: <em>true</em></code></span>
        <span className="code-line"><i>05</i><code>&#125;&#41;;<span className="code-caret" /></code></span>
      </pre>
      <div className="text-ack">
        <Check size={13} />
        ACK / 11:42:08
      </div>
    </div>
  );
}

const handshakeSteps = [
  { label: "IDENTITY", icon: Fingerprint },
  { label: "VERIFY", icon: ScanLine },
  { label: "HANDSHAKE", icon: LockKeyhole },
  { label: "TRUSTED", icon: ShieldCheck },
];

const handshakePhaseLabels = ["IDENTITY PROOF", "CODE MATCH", "NOISE XX", "TRUST PINNED"];

function SecurityHandshake() {
  const [phase, setPhase] = useState(0);

  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const timer = window.setInterval(
      () => setPhase((current) => (current + 1) % handshakeSteps.length),
      1_150,
    );
    return () => window.clearInterval(timer);
  }, []);

  return (
    <div className={`handshake handshake-phase-${phase}`} data-reveal>
      <div className="handshake-head">
        <span>SECURE PAIRING / <b>{handshakePhaseLabels[phase]}</b></span>
        <span className="handshake-live-status">
          <i />
          0{phase + 1} / 04
          <ShieldCheck size={19} />
        </span>
      </div>

      <div className="crypto-stage" aria-label="实时加密握手演示">
        <div className="crypto-sweep" aria-hidden="true" />
        <div className="crypto-peer crypto-peer-local">
          <Laptop size={20} />
          <strong>MAC</strong>
          <small>X25519 ID</small>
        </div>
        <div className="crypto-channel" aria-hidden="true">
          <i />
          <span className="crypto-signal crypto-signal-a"><Fingerprint size={12} /></span>
          <span className="crypto-signal crypto-signal-b"><LockKeyhole size={12} /></span>
          <span className="crypto-signal crypto-signal-c"><Check size={12} /></span>
        </div>
        <div className="crypto-core">
          <span className="crypto-ring crypto-ring-a" aria-hidden="true" />
          <span className="crypto-ring crypto-ring-b" aria-hidden="true" />
          <div><LockKeyhole size={22} /><small>NOISE XX</small></div>
        </div>
        <div className="crypto-peer crypto-peer-remote">
          <Monitor size={20} />
          <strong>PC</strong>
          <small>KEY PINNED</small>
        </div>
        <div className="entropy-stream" aria-hidden="true">
          {Array.from({ length: 12 }, (_, index) => <i key={`entropy-${index}`} />)}
        </div>
        <span className="crypto-caption crypto-caption-left">EPHEMERAL KEY</span>
        <span className="crypto-caption crypto-caption-right">AUTHENTICATED</span>
      </div>

      <div className="pair-code-panel">
        <div className="pair-code-meta">
          <span>ONE-TIME VERIFICATION</span>
          <small>CODE MATCH / BOTH DEVICES</small>
        </div>
        <div className="pair-code" aria-label="示例配对验证码">
          {["4", "8", "1", "6", "0", "2"].map((digit) => <span key={digit}>{digit}</span>)}
        </div>
      </div>

      <div className="handshake-steps">
        {handshakeSteps.map((step, index) => {
          const Icon = step.icon;
          const state = index < phase ? "complete" : index === phase ? "active" : "pending";
          return (
            <div className={state} key={step.label}>
              <span><Icon size={18} /></span>
              <small>0{index + 1}</small>
              <strong>{step.label}</strong>
            </div>
          );
        })}
      </div>
      <div className="fingerprint-line">
        <span>DEVICE FINGERPRINT</span>
        <code>7A:4C:91:EF:2D:08:AA:61</code>
        <strong><Check size={12} /> MATCH</strong>
      </div>
    </div>
  );
}

function ProductWindow() {
  const entries = [
    {
      icon: FileText,
      type: "TEXT",
      title: "Design review moved to 14:30",
      meta: "MacBook Pro · 刚刚",
      color: "lime",
    },
    {
      icon: ImageIcon,
      type: "IMAGE",
      title: "dashboard-final.png",
      meta: "Windows Studio · 1 分钟前",
      color: "coral",
    },
    {
      icon: File,
      type: "FILE",
      title: "TailSync-v2-spec.pdf",
      meta: "MacBook Pro · 4 分钟前",
      color: "cyan",
    },
  ];
  const [activeEntry, setActiveEntry] = useState(0);

  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const timer = window.setInterval(
      () => setActiveEntry((current) => (current + 1) % entries.length),
      1_650,
    );
    return () => window.clearInterval(timer);
  }, [entries.length]);

  const active = entries[activeEntry];

  return (
    <div className="product-window">
      <div className="product-window-scan" aria-hidden="true" />
      <div className="product-titlebar">
        <div className="product-title">
          <img src={tailsyncIcon} alt="" />
          <span>TailSync</span>
          <small>v2</small>
        </div>
        <div className="product-live-state">
          <i /> LIVE / {active.type}
        </div>
        <div className="window-controls" aria-hidden="true">
          <span />
          <span />
          <span />
        </div>
      </div>
      <div className="product-toolbar">
        <div className="product-search">
          <Search size={14} />
          <span>搜索历史记录</span>
        </div>
        <button type="button" aria-label="历史记录">
          <History size={15} />
        </button>
      </div>
      <div className="product-date">
        <span>今天 / TODAY</span>
        <small>SYNC EVENT 0{activeEntry + 1} / 03</small>
      </div>
      <div className="product-list">
        {entries.map((entry, index) => {
          const Icon = entry.icon;
          return (
            <button
              className={index === activeEntry ? "product-row active" : "product-row"}
              type="button"
              key={entry.type}
              aria-pressed={index === activeEntry}
              onClick={() => setActiveEntry(index)}
            >
              <span className={`product-row-icon ${entry.color}`}>
                <Icon size={17} />
              </span>
              <span className="product-row-copy">
                <span className="product-row-meta">
                  <b>{entry.type}</b>
                  <small>{entry.meta}</small>
                </span>
                <strong>{entry.title}</strong>
              </span>
              <span className="row-action">
                {index === activeEntry ? <Check size={15} /> : index === 0 ? <Copy size={15} /> : <ArrowRight size={15} />}
              </span>
            </button>
          );
        })}
      </div>
      <div className="product-statusbar">
        <span><i /> {active.type} 已同步</span>
        <span>LAN / {activeEntry + 1} OF 3 / ENCRYPTED</span>
      </div>
    </div>
  );
}

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
    const updateAutomaticTheme = () => setAutomaticTheme(getAutomaticTheme());
    const timer = window.setInterval(updateAutomaticTheme, 60_000);
    document.addEventListener("visibilitychange", updateAutomaticTheme);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", updateAutomaticTheme);
    };
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = timeTheme;
    document.documentElement.dataset.themePreference = themePreference;
    document.documentElement.style.colorScheme = timeTheme;
    document
      .querySelector<HTMLMetaElement>('meta[name="theme-color"]')
      ?.setAttribute("content", timeTheme === "light" ? "#f1efe7" : "#11120f");

    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, themePreference);
    } catch {
      // The selected theme still works for the current session without storage.
    }
  }, [themePreference, timeTheme]);

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
          <span>TAILSYNC</span>
          <small>2.0</small>
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
          <div className="theme-switcher" role="group" aria-label="显示模式">
            <button
              className={themePreference === "auto" ? "active" : ""}
              type="button"
              aria-label={`自动模式，当前显示${timeTheme === "light" ? "浅色" : "深色"}`}
              aria-pressed={themePreference === "auto"}
              title="自动模式"
              onClick={() => setThemePreference("auto")}
            >
              <Clock3 size={14} />
              <span>自动</span>
            </button>
            <button
              className={themePreference === "light" ? "active" : ""}
              type="button"
              aria-label="浅色模式"
              aria-pressed={themePreference === "light"}
              title="浅色模式"
              onClick={() => setThemePreference("light")}
            >
              <Sun size={14} />
              <span>浅色</span>
            </button>
            <button
              className={themePreference === "dark" ? "active" : ""}
              type="button"
              aria-label="深色模式"
              aria-pressed={themePreference === "dark"}
              title="深色模式"
              onClick={() => setThemePreference("dark")}
            >
              <Moon size={14} />
              <span>深色</span>
            </button>
          </div>
          <a href="#routing" onClick={closeMenu}>智能路由</a>
          <a href="#flow" onClick={closeMenu}>内容流</a>
          <a href="#security" onClick={closeMenu}>安全</a>
          <a className="nav-source" href={GITHUB_URL} target="_blank" rel="noreferrer">
            <GitFork size={15} />
            源码
            <ArrowUpRight size={13} />
          </a>
        </nav>
      </header>

      <main>
        <section className="hero" aria-labelledby="hero-title">
          <SyncField theme={timeTheme} />
          <div className="hero-noise" aria-hidden="true" />
          <div className="hero-copy">
            <div className="hero-kicker">
              <span className="live-dot" />
              macOS + Windows / v2.0.0
            </div>
            <h1 id="hero-title">
              复制。
              <br />
              <span>穿过设备边界。</span>
            </h1>
            <p>
              TailSync 让文本、图片和文件在 Mac 与 Windows 之间直接流动。
              局域网优先，Tailscale 兜底，全程加密，不依赖云端剪贴板。
            </p>
            <div className="hero-actions">
              <a className="button button-primary" href={RELEASE_URL} target="_blank" rel="noreferrer">
                <Download size={17} />
                获取 TailSync
                <ArrowUpRight size={15} />
              </a>
              <a className="button button-quiet" href="#routing">
                看它如何工作
                <ArrowDown size={16} />
              </a>
            </div>
          </div>

          <div className="hero-device hero-device-left" aria-hidden="true">
            <Laptop size={20} />
            <span>MACBOOK PRO</span>
            <small>LOCAL / READY</small>
          </div>
          <div className="hero-device hero-device-right" aria-hidden="true">
            <Monitor size={20} />
            <span>WINDOWS STUDIO</span>
            <small>SECURE / ONLINE</small>
          </div>

          <div className="hero-index" aria-hidden="true">
            <span>01</span>
            <span>DIRECT CLIPBOARD PROTOCOL</span>
          </div>
          <a className="scroll-cue" href="#manifesto" aria-label="继续浏览">
            <span>SCROLL TO SYNC</span>
            <ArrowDown size={15} />
          </a>
        </section>

        <section className="manifesto" id="manifesto">
          <div className="marquee" aria-hidden="true">
            <div className="marquee-track">
              <span>LOCAL FIRST</span><i />
              <span>NO CLOUD</span><i />
              <span>END TO END</span><i />
              <span>TEXT / IMAGE / FILE</span><i />
              <span>LOCAL FIRST</span><i />
              <span>NO CLOUD</span><i />
              <span>END TO END</span><i />
              <span>TEXT / IMAGE / FILE</span><i />
            </div>
          </div>
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
                <CloudOff size={28} strokeWidth={1.35} />
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
            <div className="section-marker section-marker-dark">
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

        <section className="security-section" id="security">
          <div className="security-word" aria-hidden="true">PRIVATE</div>
          <div className="security-inner">
            <div className="security-copy" data-reveal>
              <div className="section-marker section-marker-light">
                <span>04</span>
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
              <span>05</span>
              <small>QUIET BY DESIGN</small>
            </div>
            <h2>常驻托盘。<br />需要时才出现。</h2>
            <p>
              TailSync 不抢占桌面。它在后台监听、同步与确认；需要找回内容、查看路径或管理设备时，再打开精确而克制的工具界面。
            </p>
            <div className="product-points">
              <span><History size={17} /> 本地历史搜索与恢复</span>
              <span><RadioTower size={17} /> 真实在线状态与路径延迟</span>
              <span><ClipboardCopy size={17} /> 文本、图片、文件双向同步</span>
            </div>
          </div>

          <div className="product-stage" data-reveal>
            <div className="product-stage-motion" aria-hidden="true">
              <span className="stage-scan" />
              <span className="stage-rail stage-rail-a" />
              <span className="stage-rail stage-rail-b" />
              <span className="stage-packet stage-packet-text"><FileText size={15} /></span>
              <span className="stage-packet stage-packet-image"><ImageIcon size={15} /></span>
              <span className="stage-packet stage-packet-file"><File size={15} /></span>
            </div>
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
                <strong>prototype-v2.fig</strong>
                <small>分块写入 / 校验中</small>
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
            <div className="core-node"><Code2 size={28} /><span>Core</span><small>Rust / v2</small></div>
            <i />
            <div><Monitor size={26} /><span>Windows</span><small>Tauri</small></div>
          </div>
        </section>

        <section className="download-section" id="download">
          <div className="download-grid" aria-hidden="true" />
          <div className="download-copy" data-reveal>
            <img src={tailsyncIcon} alt="" />
            <span>TAILSYNC 2.0</span>
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
              <GitFork size={17} />
              在 GitHub 查看源码
              <ArrowUpRight size={15} />
            </a>
          </div>
        </section>
      </main>

      <footer className="site-footer">
        <div className="brand footer-brand">
          <img src={tailsyncIcon} alt="" />
          <span>TAILSYNC</span>
        </div>
        <p>DIRECT CLIPBOARD SYNC / BUILT FOR DEVICES YOU TRUST</p>
        <span>© {year} TAILSYNC · MIT</span>
      </footer>
    </div>
  );
}

export default App;
