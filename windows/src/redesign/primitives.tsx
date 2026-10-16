import { useEffect, useState, type ReactNode } from "react";
import { Keyboard, Pencil, RotateCcw, X } from "lucide-react";

export type Chrome = "mac" | "win";

/* ── 分区外壳：mac 用「衬线大标题 + 卡片」，win 沿用现有 setting-group 头部 ─ */
export function SectionShell({
  chrome,
  title,
  description,
  headerAction,
  children,
}: {
  chrome: Chrome;
  title: string;
  description?: string;
  headerAction?: ReactNode;
  children: ReactNode;
}) {
  if (chrome === "mac") {
    return (
      <section className="rd-mac-sec">
        <div className="rd-mac-sec-head">
          <h2>{title}</h2>
          {headerAction}
        </div>
        <div className="rd-mac-card">{children}</div>
      </section>
    );
  }
  return (
    <section className="setting-group">
      <div className={`setting-group-header${headerAction ? " section-header-with-action" : ""}`}>
        <div>
          <h3>{title}</h3>
          {description && <p>{description}</p>}
        </div>
        {headerAction}
      </div>
      <div className="rd-win-card">{children}</div>
    </section>
  );
}

/* ── 通用行 ─ */
export function Row({
  title,
  desc,
  control,
  onClick,
  asButton = false,
}: {
  title: ReactNode;
  desc?: string;
  control: ReactNode;
  onClick?: () => void;
  asButton?: boolean;
}) {
  const cls = `rd-row${asButton ? " rd-row--button" : ""}`;
  const inner = (
    <>
      <div className="rd-row-info">
        <span>{title}</span>
        {desc && <small>{desc}</small>}
      </div>
      {control}
    </>
  );
  return onClick ? (
    <div className={cls} role="button" tabIndex={0} onClick={onClick}>
      {inner}
    </div>
  ) : (
    <div className={cls}>{inner}</div>
  );
}

/* ── 开关（复用现有 .toggle 样式） ─ */
export function RdToggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <label className="toggle rd-toggle" onClick={(e) => e.stopPropagation()}>
      <input
        type="checkbox"
        aria-label={label}
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
      />
      <div className="toggle-track" />
    </label>
  );
}

/* ── 键帽 ─ */
export function Keycaps({ caps }: { caps: string[] }) {
  if (caps.length === 0) return <span className="shortcut-empty">未设置</span>;
  return (
    <span className="shortcut-keycaps">
      {caps.map((k, i) => (
        <kbd key={`${k}-${i}`}>{k}</kbd>
      ))}
    </span>
  );
}

/* ── 快捷键控件：两端统一为「键帽 + 编辑/重置/清除」。
   演示版支持真实按键录制：点击后按组合键即可更新键帽。 ─ */
const MOD_ORDER: Array<[string, string, string]> = [
  ["ctrlKey", "Ctrl", "⌃"],
  ["altKey", "Alt", "⌥"],
  ["shiftKey", "Shift", "⇧"],
  ["metaKey", "Win", "⌘"],
];

export function ShortcutControl({
  chrome,
  caps,
  defaultCaps,
  onChange,
  recordLabel,
}: {
  chrome: Chrome;
  caps: string[];
  defaultCaps: string[];
  onChange: (caps: string[]) => void;
  recordLabel: string;
}) {
  const [recording, setRecording] = useState(false);

  useEffect(() => {
    if (!recording) return;
    const handler = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(false);
        return;
      }
      const mods = MOD_ORDER.flatMap(([flag, winCap, macCap]) =>
        e[flag as keyof KeyboardEvent] ? [chrome === "mac" ? macCap : winCap] : [],
      );
      if (["Control", "Alt", "Shift", "Meta"].includes(e.key)) return;
      if (mods.length === 0) return;
      const key = e.key.length === 1 ? e.key.toUpperCase() : e.key;
      onChange([...mods, key]);
      setRecording(false);
    };
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [recording, chrome, onChange]);

  const isDefault = caps.join("+") === defaultCaps.join("+");
  return (
    <div className="shortcut-control rd-shortcut-control">
      <button
        type="button"
        className={`shortcut-recorder${recording ? " rd-recording" : ""}`}
        onClick={() => setRecording((v) => !v)}
        aria-label={recordLabel}
        title={recordLabel}
      >
        <Keyboard size={16} strokeWidth={1.7} aria-hidden="true" />
        {recording ? <span className="shortcut-empty">按下新的组合键…</span> : <Keycaps caps={caps} />}
        <Pencil className="shortcut-edit-icon" size={13} strokeWidth={1.8} aria-hidden="true" />
      </button>
      <button
        type="button"
        className="shortcut-icon-button"
        disabled={recording || isDefault}
        onClick={() => onChange(defaultCaps)}
        title="恢复默认"
        aria-label="恢复默认"
      >
        <RotateCcw size={15} strokeWidth={1.8} aria-hidden="true" />
      </button>
      <button
        type="button"
        className="shortcut-icon-button"
        disabled={recording || caps.length === 0}
        onClick={() => onChange([])}
        title="清除快捷键"
        aria-label="清除快捷键"
      >
        <X size={15} strokeWidth={1.8} aria-hidden="true" />
      </button>
    </div>
  );
}
