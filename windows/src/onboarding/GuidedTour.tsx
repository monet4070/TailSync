import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { openConnectionsWindow, openHistoryWindow, openSettingsWindow } from "../tailsyncClient";
import { ArrowLeft, ArrowRight, MousePointer2, X } from "lucide-react";
import { useI18n } from "../hooks/useI18n";
import guide from "../../../shared/onboarding/guide.json";
import { endTour, endTourPermanently, moveTour, readTour, saveTour, TOUR_EVENT, TOUR_KEY, TOUR_SUPPRESSED_KEY, type TourWindow } from "./tourState";
import "./onboarding.css";

type Rect = { left: number; top: number; width: number; height: number };
const openTourWindow = (name: TourWindow) => ({ connections: openConnectionsWindow, history: openHistoryWindow, settings: openSettingsWindow })[name]();
export function GuidedTour({ window: windowName, navigate = openTourWindow, dismiss = endTour, suppress = endTourPermanently }: {
  window: TourWindow; navigate?: (window: TourWindow) => Promise<unknown>; dismiss?: () => Promise<unknown>; suppress?: () => Promise<unknown>;
}) {
  const { locale } = useI18n();
  const [state, setState] = useState(readTour);
  const [rect, setRect] = useState<Rect | null>(null);
  const [modal, setModal] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [cardHeight, setCardHeight] = useState(240);
  const card = useRef<HTMLDivElement>(null);
  const title = useRef<HTMLHeadingElement>(null);
  const step = guide.steps[state.index];
  const active = state.active && step.window === windowName;
  const lang = (locale ?? "en").startsWith("zh") ? "zh-CN" : "en";
  const label = (key: keyof typeof guide.ui) => guide.ui[key][lang];
  useEffect(() => {
    const sync = () => setState(readTour());
    const storage = (event: StorageEvent) => { if (event.key === null || event.key === TOUR_KEY) sync(); };
    window.addEventListener(TOUR_EVENT, sync);
    window.addEventListener("storage", storage);
    // The native startup route carries this only when no pairing invite is pending.
    if (windowName === "connections" && new URLSearchParams(location.search).get("tour") === "start") {
      // This route is emitted once for a fresh install or an explicit restart.
      // WebView storage can survive uninstall, so discard stale tour progress.
      const index = 0;
      try {
        localStorage.removeItem(TOUR_SUPPRESSED_KEY);
        saveTour({ active: true, index });
        const destination = guide.steps[index].window as TourWindow;
        if (destination !== windowName) void navigate(destination).catch(() => {
          saveTour({ active: true, index: 0 }); setError(true);
        });
      }
      catch { setError(true); }
      history.replaceState(null, "", location.pathname);
    }
    return () => { window.removeEventListener(TOUR_EVENT, sync); window.removeEventListener("storage", storage); };
  }, [windowName, navigate]);
  useLayoutEffect(() => {
    if (!active || modal) return;
    const app = document.querySelector<HTMLElement>(".app");
    if (!app) return;
    app.style.setProperty("--tour-panel-height", `${cardHeight + 28}px`);
    app.classList.add("has-guided-tour");
    return () => { app.classList.remove("has-guided-tour"); app.style.removeProperty("--tour-panel-height"); };
  }, [active, modal, cardHeight]);
  useLayoutEffect(() => {
    if (!active) return;
    let target: HTMLElement | null = null;
    let targetHeight = 0;
    let frame = 0;
    const measure = () => {
      const next = document.querySelector<HTMLElement>(`[data-tour="${step.target}"]`);
      const nextHeight = next?.getBoundingClientRect().height ?? 0;
      if (next && (next !== target || nextHeight !== targetHeight)) {
        target = next;
        targetHeight = nextHeight;
        next.scrollIntoView?.({ block: "start", behavior: "instant" });
      }
      const bounds = next?.getBoundingClientRect();
      if (!bounds || bounds.width === 0 || bounds.height === 0) setRect(null);
      else {
        let top = Math.max(2, bounds.top - 5);
        let left = Math.max(2, bounds.left - 5);
        let right = Math.min(window.innerWidth - 2, bounds.right + 5);
        let bottom = Math.min(window.innerHeight - cardHeight - 28, bounds.bottom + 5);
        // Clip only at actual scroll/container boundaries, never to make room
        // for the card. Its space is reserved in the application's layout.
        for (let parent = next?.parentElement; parent; parent = parent.parentElement) {
          const style = getComputedStyle(parent);
          const clip = parent.getBoundingClientRect();
          if (/(auto|scroll|hidden|clip)/.test(style.overflowY)) {
            top = Math.max(top, clip.top); bottom = Math.min(bottom, clip.bottom);
          }
          if (/(auto|scroll|hidden|clip)/.test(style.overflowX)) {
            left = Math.max(left, clip.left); right = Math.min(right, clip.right);
          }
        }
        setRect(bottom > top && right > left ? { left, top, width: right - left, height: bottom - top } : null);
      }
      // Let native workflows and menus opened from a highlighted control finish first.
      setModal(Boolean(document.querySelector('[role="dialog"]:not([data-tour-card]), [role="listbox"], [role="menu"]')));
    };
    const queue = () => { cancelAnimationFrame(frame); frame = requestAnimationFrame(measure); };
    const observer = new MutationObserver(records => {
      if (records.some(record => !(record.target instanceof Element && record.target.closest(".guided-tour")))) queue();
    });
    observer.observe(document.querySelector(".app") ?? document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "style", "hidden", "open"] });
    window.addEventListener("resize", queue);
    document.addEventListener("scroll", queue, true);
    measure(); title.current?.focus();
    return () => { observer.disconnect(); cancelAnimationFrame(frame); window.removeEventListener("resize", queue); document.removeEventListener("scroll", queue, true); };
  }, [active, step.target, state.index, cardHeight, modal]);
  useLayoutEffect(() => {
    if (!card.current || !active) return;
    const measure = () => setCardHeight(card.current?.offsetHeight || 240);
    const observer = new ResizeObserver(measure);
    observer.observe(card.current);
    measure();
    return () => observer.disconnect();
  }, [active, modal]);
  const run = async (action: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true); setError(false);
    try { await action(); } catch { setError(true); }
    finally { setBusy(false); }
  };
  const move = (index: number) => moveTour(index, name => name === windowName ? Promise.resolve() : navigate(name));
  if (!active || modal) return null;
  const width = Math.min(440, window.innerWidth - 24);
  const maxHeight = window.innerHeight * 0.46;
  const left = (window.innerWidth - width) / 2;
  return <div className="guided-tour" data-testid="guided-tour">
    <svg className="guided-tour-mask" aria-hidden="true" width="100%" height="100%">
      <path fillRule="evenodd" style={rect ? undefined : { pointerEvents: "none" }} d={`M0 0H${window.innerWidth}V${window.innerHeight}H0Z ${rect ? `M${rect.left} ${rect.top}v${rect.height}h${rect.width}v-${rect.height}Z` : ""}`} />
    </svg>
    {rect && <div className="guided-tour-highlight" style={{left:rect.left, top:rect.top, width:rect.width, height:rect.height}} />}
    <div ref={card} className="guided-tour-card" data-tour-card role="dialog" aria-modal="false" aria-labelledby="tour-title" style={{left,bottom:12,width,maxHeight}} onKeyDown={event => {
      event.stopPropagation();
      if (event.key === "Escape") { void run(dismiss); }
    }}>
      <div className="guided-tour-caption"><span>TAILSYNC · {label("title")}</span><button aria-label={label("skip")} disabled={busy} onClick={() => void run(dismiss)}><X size={15} /></button></div>
      <div className="guided-tour-copy"><h2 ref={title} tabIndex={-1} id="tour-title">{step.title[lang]}</h2>
      <p>{rect ? step.body[lang] : label("waiting")}</p>
      <p className="guided-tour-tip">{step.tip[lang]}</p>
      {rect && <span className="guided-tour-hint"><MousePointer2 size={12} />{label("hint")}</span>}
      {error && <p role="alert">{label("error")}</p>}
      </div>
      <button className="guided-tour-never" disabled={busy} onClick={() => void run(suppress)}>{label("never")}</button>
      <div className="guided-tour-controls"><span aria-label={label("progress")}>{state.index + 1} / {guide.steps.length}</span><div>
        <button aria-label={label("back")} disabled={busy || state.index === 0} onClick={() => void run(() => move(state.index - 1))}><ArrowLeft size={16} /></button>
        <button className="guided-tour-next" disabled={busy} onClick={() => void run(() => state.index === guide.steps.length - 1 ? dismiss() : move(state.index + 1))}>{label(state.index === guide.steps.length - 1 ? "finish" : "next")}<ArrowRight size={14} /></button>
      </div></div>
    </div>
  </div>;
}
