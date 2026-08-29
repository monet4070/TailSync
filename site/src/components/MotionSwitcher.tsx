import { useState } from "react";
import { MOTION_VARIANTS, useMotionVariant } from "../hooks/useMotionVariant";

/**
 * Dev-only motion A/B panel.
 *
 * `import.meta.env.DEV` is substituted with a literal at build time, so the
 * early return makes the whole body — panel markup, styles, and the
 * `useMotionVariant` import — unreachable and it drops out of `vite build`.
 * Production therefore ships no switcher and no `data-motion` attribute, which
 * leaves the page on the `base` motion language.
 *
 * To promote a winning variant to production, set `data-motion` on `<html>` in
 * `index.html` (and the sibling entry points) rather than shipping this panel.
 */
export function MotionSwitcher() {
  if (!import.meta.env.DEV) return null;

  return <MotionSwitcherPanel />;
}

function MotionSwitcherPanel() {
  const { variant, selectVariant } = useMotionVariant();
  const [open, setOpen] = useState(true);

  return (
    <>
      <style>{PANEL_CSS}</style>
      <aside className={open ? "motion-switcher is-open" : "motion-switcher"}>
        <button
          className="motion-switcher-toggle"
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          <b>动效</b>
          <span>{MOTION_VARIANTS.find((item) => item.id === variant)?.name}</span>
        </button>

        {open ? (
          <div className="motion-switcher-list" role="radiogroup" aria-label="动效版本">
            {MOTION_VARIANTS.map((item) => (
              <button
                className={item.id === variant ? "active" : ""}
                key={item.id}
                type="button"
                role="radio"
                aria-checked={item.id === variant}
                onClick={() => selectVariant(item.id)}
              >
                <b>{item.name}</b>
                <small>{item.note}</small>
              </button>
            ))}
            <p>仅开发环境可见 · 选择会记入 localStorage</p>
          </div>
        ) : null}
      </aside>
    </>
  );
}

const PANEL_CSS = `
.motion-switcher {
  position: fixed;
  right: 18px;
  bottom: 18px;
  z-index: 500;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 6px;
  width: 232px;
  padding: 8px;
  border: 1px solid var(--hairline-strong, rgba(128,128,128,.3));
  border-radius: 12px;
  background: color-mix(in srgb, var(--elevated, #1c1b16) 88%, transparent);
  backdrop-filter: saturate(180%) blur(18px);
  -webkit-backdrop-filter: saturate(180%) blur(18px);
  box-shadow: 0 18px 44px rgba(0, 0, 0, 0.28);
  color: var(--ink, #f2ebdb);
  font-family: var(--font-sans);
}
.motion-switcher:not(.is-open) { width: auto; }
.motion-switcher-toggle {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 5px 7px;
  border-radius: 8px;
  color: inherit;
  text-align: left;
}
.motion-switcher-toggle b {
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.16em;
  color: var(--ink-3, #8a8578);
  text-transform: uppercase;
}
.motion-switcher-toggle span {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--brand, #c8703c);
}
.motion-switcher-list { display: flex; flex-direction: column; gap: 2px; }
.motion-switcher-list button {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 7px 9px;
  border-radius: 8px;
  color: inherit;
  text-align: left;
  transition: background 0.15s ease;
}
.motion-switcher-list button:hover { background: var(--chip-bg, rgba(128,128,128,.12)); }
.motion-switcher-list button.active {
  background: color-mix(in srgb, var(--brand, #c8703c) 18%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--brand, #c8703c) 42%, transparent);
}
.motion-switcher-list button b { font-size: 12.5px; font-weight: 600; }
.motion-switcher-list button small {
  font-size: 10.5px;
  line-height: 1.35;
  color: var(--ink-3, #8a8578);
}
.motion-switcher-list button.active small { color: var(--ink-2, #c4bdad); }
.motion-switcher-list p {
  margin-top: 4px;
  padding: 0 9px;
  font-family: var(--font-mono);
  font-size: 9px;
  letter-spacing: 0.04em;
  color: var(--ink-3, #8a8578);
}
@media (max-width: 720px) { .motion-switcher { display: none; } }
`;
