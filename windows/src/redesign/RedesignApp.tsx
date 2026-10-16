import { useState } from "react";
import { X, Moon, Sun, Laptop, Monitor } from "lucide-react";
import { ThemeLogo } from "../ThemeLogo";
import { ConnectionsSection } from "./connections";
import { GeneralSection } from "./general";
import { HistoryAndStorageSection, UpdatesSection } from "./otherSections";
import { INITIAL_MOCK_STATE, type MockState } from "./mock";
import type { Chrome } from "./primitives";

export function RedesignApp() {
  const [chrome, setChrome] = useState<Chrome>("mac");
  const [darkMode, setDarkMode] = useState(false);
  const [state, setState] = useState<MockState>(INITIAL_MOCK_STATE);

  const patch = (p: Partial<MockState>) => {
    setState((prev) => ({ ...prev, ...p }));
  };

  const appCls = `app settings-window ${darkMode ? "dark" : "light"}`;

  return (
    <div className="rd-root">
      {/* 顶部控制栏：支持一键在 macOS 与 Windows 两种设计风格间切换试用 */}
      <header className="rd-demo-toolbar">
        <div className="rd-demo-title">
          <span>TailSync 设置页重构试用</span>
          <span className="rd-demo-badge">静态原型 (同技术栈 React+TS)</span>
        </div>

        <div className="rd-demo-switches">
          <div className="rd-pill-group">
            <button
              type="button"
              className={`rd-pill-btn ${chrome === "mac" ? "active" : ""}`}
              onClick={() => setChrome("mac")}
            >
              <Laptop size={12} style={{ display: "inline", marginRight: 4 }} />
              macOS 视图 (修复空白+键帽)
            </button>
            <button
              type="button"
              className={`rd-pill-btn ${chrome === "win" ? "active" : ""}`}
              onClick={() => setChrome("win")}
            >
              <Monitor size={12} style={{ display: "inline", marginRight: 4 }} />
              Windows 视图 (重构连接+折叠Iroh)
            </button>
          </div>

          <div className="rd-pill-group">
            <button
              type="button"
              className={`rd-pill-btn ${!darkMode ? "active" : ""}`}
              onClick={() => setDarkMode(false)}
              title="浅色模式"
            >
              <Sun size={12} />
            </button>
            <button
              type="button"
              className={`rd-pill-btn ${darkMode ? "active" : ""}`}
              onClick={() => setDarkMode(true)}
              title="深色模式"
            >
              <Moon size={12} />
            </button>
          </div>
        </div>
      </header>

      {/* 居中展示窗口 */}
      <main className="rd-preview-stage">
        <div className={`rd-window-frame ${appCls}`}>
          {/* 标题栏 */}
          <div className="titlebar" data-tauri-drag-region>
            {chrome === "mac" && (
              <div className="rd-mac-traffic-lights">
                <span className="rd-dot red" />
                <span className="rd-dot yellow" />
                <span className="rd-dot green" />
              </div>
            )}
            <div className="titlebar-brand">
              <ThemeLogo />
              <span className="titlebar-text">TailSync 设置</span>
              <span className="titlebar-badge">v2.2.2</span>
            </div>
            {chrome === "win" && (
              <button
                type="button"
                className="titlebar-close"
                onClick={() => patch({ toast: "点击了关闭窗口" })}
                title="关闭"
                aria-label="关闭"
              >
                <X size={15} strokeWidth={1.8} aria-hidden="true" />
              </button>
            )}
          </div>

          {/* 设置主体内容区 */}
          <div className="settings-content">
            <ConnectionsSection chrome={chrome} state={state} patch={patch} />
            <GeneralSection chrome={chrome} state={state} patch={patch} />
            <HistoryAndStorageSection chrome={chrome} state={state} patch={patch} />
            <UpdatesSection chrome={chrome} state={state} patch={patch} />
          </div>

          {/* Toast 提示 */}
          {state.toast && (
            <div
              className="toast"
              role="status"
              onClick={() => patch({ toast: null })}
              style={{ cursor: "pointer" }}
            >
              {state.toast}
            </div>
          )}
        </div>
      </main>
    </div>
  );
}
