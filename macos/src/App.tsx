import { useEffect, useRef, useState } from "react";
import appIcon from "../assets/icons/tailsync_1024.png";
import historyShot from "../assets/tailsync-history.png";

type IconName =
  | "arrow"
  | "check"
  | "copy"
  | "file"
  | "image"
  | "lock"
  | "menu"
  | "moon"
  | "network"
  | "sun"
  | "x";

function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  const paths: Record<IconName, React.ReactNode> = {
    arrow: <><path d="M5 12h14"/><path d="m13 6 6 6-6 6"/></>,
    check: <path d="m5 12 4 4L19 6"/>,
    copy: <><rect width="13" height="13" x="9" y="9" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></>,
    file: <><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z"/><path d="M14 2v6h6"/></>,
    image: <><rect width="18" height="18" x="3" y="3" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/></>,
    lock: <><rect width="18" height="11" x="3" y="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></>,
    menu: <><path d="M4 7h16"/><path d="M4 17h16"/></>,
    moon: <path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8Z"/>,
    network: <><rect width="6" height="6" x="9" y="2" rx="1"/><rect width="6" height="6" x="2" y="16" rx="1"/><rect width="6" height="6" x="16" y="16" rx="1"/><path d="M5 16v-3a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v3"/><path d="M12 8v3"/></>,
    sun: <><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.42 1.42M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.35 17.66l-1.42 1.41M19.07 4.93l-1.41 1.42"/></>,
    x: <><path d="M18 6 6 18"/><path d="m6 6 12 12"/></>,
  };

  return (
    <svg aria-hidden="true" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      {paths[name]}
    </svg>
  );
}

const featureItems = [
  { number: "01", icon: "copy" as const, title: "不止文字", copy: "图片、文件、代码片段，都能在设备间自然流转。" },
  { number: "02", icon: "network" as const, title: "两种网络，一种体验", copy: "局域网优先，Tailscale 随行，自动选择更合适的链路。" },
  { number: "03", icon: "lock" as const, title: "信任由你确认", copy: "Noise XX 加密握手与六位验证码，设备身份清晰可见。" },
];

function App() {
  const [dark, setDark] = useState(true);
  const [menuOpen, setMenuOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [windowsNotice, setWindowsNotice] = useState(false);
  const heroRef = useRef<HTMLElement>(null);

  useEffect(() => {
    document.body.classList.add("marketing-body");
    return () => document.body.classList.remove("marketing-body");
  }, []);

  useEffect(() => {
    const hero = heroRef.current;
    if (!hero || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const move = (event: PointerEvent) => {
      const x = (event.clientX / window.innerWidth - 0.5) * 16;
      const y = (event.clientY / window.innerHeight - 0.5) * 12;
      hero.style.setProperty("--drift-x", `${x}px`);
      hero.style.setProperty("--drift-y", `${y}px`);
    };
    window.addEventListener("pointermove", move);
    return () => window.removeEventListener("pointermove", move);
  }, []);

  const runDemo = () => {
    setCopied(true);
    window.setTimeout(() => setCopied(false), 2200);
  };

  return (
    <div className={`marketing-site ${dark ? "is-dark" : "is-light"}`}>
      <header className="site-header">
        <a className="site-brand" href="#top" aria-label="TailSync 首页">
          <img src={appIcon} alt="" />
          <span>TailSync</span>
        </a>
        <nav className={menuOpen ? "is-open" : ""} aria-label="主导航">
          <a href="#features" onClick={() => setMenuOpen(false)}>功能</a>
          <a href="#security" onClick={() => setMenuOpen(false)}>安全</a>
          <a href="#download" onClick={() => setMenuOpen(false)}>下载</a>
        </nav>
        <div className="header-actions">
          <button className="theme-button" onClick={() => setDark(!dark)} aria-label={dark ? "切换浅色模式" : "切换深色模式"} title={dark ? "浅色模式" : "深色模式"}>
            <Icon name={dark ? "sun" : "moon"} size={17} />
          </button>
          <a className="header-download" href="#download">获取 TailSync <Icon name="arrow" size={16} /></a>
          <button className="menu-button" onClick={() => setMenuOpen(!menuOpen)} aria-label="打开导航菜单">
            <Icon name={menuOpen ? "x" : "menu"} />
          </button>
        </div>
      </header>

      <main>
        <section className="hero" id="top" ref={heroRef}>
          <div className="hero-scene" aria-hidden="true">
            <div className="scene-line line-one" />
            <div className="scene-line line-two" />
            <div className="device device-mac">
              <div className="device-bar"><i/><i/><i/><span>MacBook Pro</span></div>
              <div className="clipboard-card">
                <span className="clip-type"><Icon name="copy" size={14}/> TEXT</span>
                <p>设计不是装饰，<br/>而是让复杂变得自然。</p>
                <small>刚刚复制</small>
              </div>
            </div>
            <div className="device device-pc">
              <div className="device-bar"><span>Windows Studio</span><b>● 在线</b></div>
              <div className={`receive-card ${copied ? "is-received" : ""}`}>
                <span><Icon name={copied ? "check" : "network"} size={15}/>{copied ? " 已同步" : " 等待内容"}</span>
                <p>{copied ? "设计不是装饰，而是让复杂变得自然。" : "你的剪贴板将在这里出现"}</p>
              </div>
            </div>
            <span className="signal-dot dot-one" />
            <span className="signal-dot dot-two" />
          </div>

          <div className="hero-content">
            <p className="hero-kicker"><span/> 为多设备工作流而生</p>
            <h1>TailSync</h1>
            <p className="hero-statement">复制一次，<strong>到处粘贴。</strong></p>
            <p className="hero-copy">让文字、图片与文件安全穿过你的设备。<br/>没有云端中转，没有多余动作。</p>
            <div className="hero-actions">
              <a className="primary-button" href="#download">免费下载 <Icon name="arrow" /></a>
              <button className="demo-button" onClick={runDemo}><span className={copied ? "demo-indicator is-live" : "demo-indicator"}/>{copied ? "已发送到 Windows" : "试试同步"}</button>
            </div>
            <p className="compatibility">适用于 macOS 13+ 与 Windows 10/11</p>
          </div>
          <a className="scroll-cue" href="#features" aria-label="查看功能"><span>SCROLL</span><i/></a>
        </section>

        <section className="trust-strip" aria-label="产品特点">
          <span>LOCAL FIRST</span><i/> <span>END-TO-END ENCRYPTED</span><i/> <span>MAC + WINDOWS</span><i/> <span>NO CLOUD REQUIRED</span>
        </section>

        <section className="features-section" id="features">
          <div className="section-intro">
            <p className="section-label">为何是 TailSync</p>
            <h2>设备很多，<br/>工作流只有一个。</h2>
            <p>你不该把时间花在给自己发消息、上传临时文件，或反复寻找“刚才复制的那段”。</p>
          </div>
          <div className="feature-list">
            {featureItems.map((item) => (
              <article className="feature-row" key={item.number}>
                <span className="feature-number">{item.number}</span>
                <span className="feature-icon"><Icon name={item.icon} size={22}/></span>
                <div><h3>{item.title}</h3><p>{item.copy}</p></div>
                <Icon name="arrow" size={20}/>
              </article>
            ))}
          </div>
        </section>

        <section className="flow-section">
          <div className="flow-copy">
            <p className="section-label">像本来就该这样</p>
            <h2>复制。切换设备。粘贴。</h2>
            <p>TailSync 常驻菜单栏，连接可信设备，并把同步留在后台。你只需要继续手上的事。</p>
            <div className="flow-steps">
              <span><b>01</b> 复制内容</span><span><b>02</b> 加密传输</span><span><b>03</b> 即刻粘贴</span>
            </div>
          </div>
          <div className="history-visual">
            <img src={historyShot} alt="TailSync 剪贴板历史界面" />
            <div className="history-overlay overlay-text"><Icon name="copy"/><span><b>一段会议摘要</b><small>来自 MacBook Pro · 刚刚</small></span></div>
            <div className="history-overlay overlay-image"><Icon name="image"/><span><b>design-preview.png</b><small>2.4 MB · 已同步</small></span></div>
            <div className="history-overlay overlay-file"><Icon name="file"/><span><b>proposal-final.pdf</b><small>来自 Windows Studio</small></span></div>
          </div>
        </section>

        <section className="security-section" id="security">
          <div className="security-mark"><Icon name="lock" size={44}/><span>NO CLOUD</span></div>
          <div className="security-copy">
            <p className="section-label">安全不是附加项</p>
            <h2>内容属于你，<br/>路径也应该。</h2>
            <p>设备通过六位验证码相互确认，数据使用 Noise XX 加密通道传输。局域网直连或经由你的 Tailscale 网络，不经过 TailSync 云端。</p>
            <div className="security-points"><span><Icon name="check"/> 固定设备身份</span><span><Icon name="check"/> 本地历史记录</span><span><Icon name="check"/> 断点续传</span></div>
          </div>
        </section>

        <section className="download-section" id="download">
          <img src={appIcon} alt="TailSync 应用图标" />
          <p className="section-label">开始保持同步</p>
          <h2>少一次中断，<br/>多一点专注。</h2>
          <p>TailSync v2 · 免费使用</p>
          <div className="download-actions">
            <a className="primary-button" href="https://github.com/monet4070/TailSync/releases/latest">下载 macOS 版 <Icon name="arrow"/></a>
            <button type="button" onClick={() => setWindowsNotice(true)}>{windowsNotice ? "已记下，敬请期待" : "Windows 版即将推出"}</button>
          </div>
          <small>macOS 13+ · Apple Silicon</small>
        </section>
      </main>

      <footer>
        <a className="site-brand" href="#top"><img src={appIcon} alt=""/><span>TailSync</span></a>
        <p>让设备之间，少一点边界。</p>
        <span>© 2026 TailSync</span>
      </footer>
    </div>
  );
}

export default App;
