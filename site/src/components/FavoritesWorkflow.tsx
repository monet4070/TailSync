import {
  ArrowRight,
  FileText,
  History,
  RotateCcw,
  Search,
  ShieldCheck,
  Star,
  Terminal,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

const LONG_PRESS_GRACE_MS = 220;
const LONG_PRESS_CHARGE_MS = 420;

type FavoriteRecord = {
  id: string;
  kind: string;
  title: string;
  meta: string;
  icon: typeof FileText;
};

const favoriteRecords: FavoriteRecord[] = [
  {
    id: "release-note",
    kind: "TEXT",
    title: "发布前检查：Windows 与 macOS 安装包均已验证",
    meta: "MacBook Pro · 刚刚",
    icon: FileText,
  },
  {
    id: "release-link",
    kind: "WEBSITE",
    title: "github.com/monet4070/TailSync/releases/latest",
    meta: "Windows Studio · 1 分钟前",
    icon: Search,
  },
  {
    id: "test-command",
    kind: "COMMAND",
    title: "cargo test --workspace --locked",
    meta: "MacBook Pro · 3 分钟前",
    icon: Terminal,
  },
];

const initialFavorites = ["release-note", "test-command"];

export function FavoritesWorkflow() {
  const [favorites, setFavorites] = useState(() => new Set(initialFavorites));
  const [deleted, setDeleted] = useState(() => new Set<string>());
  const [pressingId, setPressingId] = useState<string | null>(null);
  const [chargingId, setChargingId] = useState<string | null>(null);
  const [lastToggledId, setLastToggledId] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("按住任意记录约 0.6 秒即可收藏");
  const graceTimer = useRef<number | null>(null);
  const commitTimer = useRef<number | null>(null);
  const stampTimer = useRef<number | null>(null);
  const pressStart = useRef<{ id: string; x: number; y: number } | null>(null);

  const visibleRecords = useMemo(
    () => favoriteRecords.filter((record) => !deleted.has(record.id)),
    [deleted],
  );
  const favoriteList = useMemo(
    () => visibleRecords.filter((record) => favorites.has(record.id)),
    [favorites, visibleRecords],
  );

  const clearPressTimers = useCallback(() => {
    if (graceTimer.current !== null) window.clearTimeout(graceTimer.current);
    if (commitTimer.current !== null) window.clearTimeout(commitTimer.current);
    graceTimer.current = null;
    commitTimer.current = null;
  }, []);

  const endPress = useCallback(() => {
    clearPressTimers();
    pressStart.current = null;
    setPressingId(null);
    setChargingId(null);
  }, [clearPressTimers]);

  const toggleFavorite = useCallback((id: string) => {
    const next = new Set(favorites);
    const willFavorite = !next.has(id);
    if (willFavorite) next.add(id);
    else next.delete(id);
    setFavorites(next);
    setAnnouncement(
      willFavorite
        ? "已收藏。这条记录不会被历史页删除或自动清理。"
        : "已取消收藏。它现在是一条普通历史记录。",
    );
    setLastToggledId(id);
    if (stampTimer.current !== null) window.clearTimeout(stampTimer.current);
    stampTimer.current = window.setTimeout(() => setLastToggledId(null), 520);
  }, [favorites]);

  const beginPress = (id: string, event: React.PointerEvent<HTMLButtonElement>) => {
    if (!event.isPrimary || event.button !== 0) return;
    endPress();
    pressStart.current = { id, x: event.clientX, y: event.clientY };
    setPressingId(id);
    setAnnouncement("继续按住；短按仍然只会选中记录");

    graceTimer.current = window.setTimeout(() => {
      setChargingId(id);
      setAnnouncement("正在写入收藏…");
      commitTimer.current = window.setTimeout(() => {
        toggleFavorite(id);
        pressStart.current = null;
        setPressingId(null);
        setChargingId(null);
      }, LONG_PRESS_CHARGE_MS);
    }, LONG_PRESS_GRACE_MS);
  };

  const trackPress = (event: React.PointerEvent<HTMLButtonElement>) => {
    const start = pressStart.current;
    if (!start) return;
    if (Math.hypot(event.clientX - start.x, event.clientY - start.y) > 8) {
      endPress();
      setAnnouncement("手指或鼠标移动后已取消长按");
    }
  };

  const deleteFavorite = (id: string) => {
    setDeleted((current) => new Set(current).add(id));
    setFavorites((current) => {
      const next = new Set(current);
      next.delete(id);
      return next;
    });
    setAnnouncement("已从收藏窗口删除这条记录");
  };

  const resetDemo = () => {
    setDeleted(new Set());
    setFavorites(new Set(initialFavorites));
    setAnnouncement("演示已重置，可以再次长按记录");
  };

  useEffect(
    () => () => {
      clearPressTimers();
      if (stampTimer.current !== null) window.clearTimeout(stampTimer.current);
    },
    [clearPressTimers],
  );

  return (
    <section className="favorites-section" id="favorites">
      <div className="favorites-kinetic-word" aria-hidden="true">KEEP</div>

      <div className="favorites-heading" data-reveal>
        <div className="section-marker">
          <span>05</span>
          <small>FAVORITES / LONG PRESS</small>
        </div>
        <div>
          <span className="favorites-eyebrow"><Star size={15} /> KEEP WHAT MATTERS</span>
          <h2>长按一条记录，<br /><strong>就能把它收藏起来。</strong></h2>
        </div>
        <p>
          按住约 0.6 秒，主题色会从左到右铺满整行。收藏完成后颜色和星标会保留；
          再次长按即可取消收藏，记录仍留在原来的时间位置。
        </p>
      </div>

      <div className="favorites-workbench" data-reveal>
        <div className="favorite-demo-window favorite-history-window">
          <div className="favorite-window-titlebar">
            <span className="favorite-window-controls" aria-hidden="true"><i /><i /><i /></span>
            <strong><History size={14} /> History</strong>
            <button type="button" aria-label="收藏窗口在右侧演示中">
              <Star size={14} /> {favoriteList.length}
            </button>
          </div>
          <div className="favorite-window-toolbar">
            <span><Search size={14} /> 搜索历史…</span>
            <small>长按收藏 · 双击恢复</small>
          </div>
          <div className="favorite-demo-list" aria-label="可交互的长按收藏演示">
            {visibleRecords.map((record) => {
              const Icon = record.icon;
              const isFavorite = favorites.has(record.id);
              const isCharging = chargingId === record.id;
              const isPressing = pressingId === record.id;
              const wasToggled = lastToggledId === record.id;
              return (
                <button
                  className={[
                    "favorite-demo-row",
                    isFavorite ? "is-favorite" : "",
                    isPressing ? "is-pressing" : "",
                    isCharging ? "is-charging" : "",
                    wasToggled ? "just-toggled" : "",
                  ].filter(Boolean).join(" ")}
                  type="button"
                  aria-pressed={isFavorite}
                  key={record.id}
                  onPointerDown={(event) => beginPress(record.id, event)}
                  onPointerMove={trackPress}
                  onPointerUp={endPress}
                  onPointerCancel={endPress}
                  onPointerLeave={endPress}
                  onContextMenu={(event) => event.preventDefault()}
                  onKeyDown={(event) => {
                    if ((event.key === "Enter" || event.key === " ") && !event.repeat) {
                      event.preventDefault();
                      toggleFavorite(record.id);
                    }
                  }}
                  onClick={(event) => event.preventDefault()}
                >
                  <span className="favorite-demo-fill" aria-hidden="true" />
                  <span className="favorite-demo-icon"><Icon size={17} /></span>
                  <span className="favorite-demo-copy">
                    <span><b>{record.kind}</b><small>{record.meta}</small></span>
                    <strong>{record.title}</strong>
                    <em>{isCharging ? "正在收藏…" : isFavorite ? "已收藏 · 历史页不可删除" : "按住约 0.6 秒收藏"}</em>
                  </span>
                  <span className="favorite-demo-state">
                    {isFavorite ? <ShieldCheck size={15} /> : <Trash2 size={15} />}
                    <small>{isFavorite ? "保护" : "可删"}</small>
                  </span>
                  <span className="favorite-demo-stamp" aria-hidden="true"><Star size={15} fill="currentColor" /></span>
                </button>
              );
            })}
          </div>
          <div className="favorite-window-status" aria-live="polite">
            <i /> {announcement}
          </div>
        </div>

        <div className="favorites-bridge" aria-hidden="true">
          <span>独立窗口</span>
          <ArrowRight size={22} />
        </div>

        <div className="favorite-demo-window favorite-collection-window">
          <div className="favorite-window-titlebar">
            <span className="favorite-window-controls" aria-hidden="true"><i /><i /><i /></span>
            <strong><Star size={14} fill="currentColor" /> Favorites</strong>
            <small>{String(favoriteList.length).padStart(2, "0")} SAVED</small>
          </div>
          <div className="favorite-collection-note">
            <ShieldCheck size={18} />
            <span><b>已收藏的记录</b><small>历史页的清空和自动清理不会删除它们</small></span>
          </div>
          <div className="favorite-collection-list">
            {favoriteList.length > 0 ? favoriteList.map((record) => {
              const Icon = record.icon;
              return (
                <div key={`saved-${record.id}`}>
                  <span><Icon size={16} /></span>
                  <p><b>{record.kind}</b><strong>{record.title}</strong><small>{record.meta}</small></p>
                  <button type="button" aria-label={`从收藏窗口删除 ${record.title}`} onClick={() => deleteFavorite(record.id)}>
                    <Trash2 size={15} />
                  </button>
                </div>
              );
            }) : (
              <div className="favorite-empty-state">
                <Star size={22} />
                <p><strong>还没有收藏记录</strong><small>收藏内容只能在这个窗口删除</small></p>
              </div>
            )}
          </div>
          <button className="favorite-reset" type="button" onClick={resetDemo}>
            <RotateCcw size={13} /> 重置演示
          </button>
        </div>
      </div>

      <div className="favorites-policy" data-reveal data-cascade>
        <article>
          <span>01</span><Star size={19} />
          <h3>不影响原来的操作</h3>
          <p>短按仍然选中记录，双击仍然恢复到剪贴板；只有完整长按才会收藏。</p>
        </article>
        <article>
          <span>02</span><ShieldCheck size={19} />
          <h3>收藏内容不会被清理</h3>
          <p>收藏后不能在历史页右键删除，清空历史和后台自动清理也会跳过它。</p>
        </article>
        <article>
          <span>03</span><Trash2 size={19} />
          <h3>在收藏窗口删除</h3>
          <p>需要永久删除收藏内容时，到收藏窗口操作，避免在历史列表里误删。</p>
        </article>
      </div>
    </section>
  );
}
