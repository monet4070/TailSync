import { ArrowRight, Braces, CalendarDays, Check, Copy, Database, File, Globe2, Search, Tags, Terminal } from "lucide-react";
import { usePhaseCycle } from "../hooks/usePhaseCycle";
import { MAC_INSTALLER_NAME } from "../product";
const tailsyncIcon = "/tailsync-icon.png";

// Module scope, matching the other three sequences. Declared inside the
// component body this was a fresh array identity every render, which made the
// old `[entries.length]` effect dep correct only because `.length` is a
// primitive rather than because the dep was right.
const entries = [
  {
    icon: Globe2,
    type: "WEBSITE",
    title: "github.com/monet4070/TailSync",
    meta: "MacBook Pro · 刚刚",
    tags: ["网站", "文本"],
    confidence: 98,
    color: "lime",
  },
  {
    icon: Terminal,
    type: "COMMAND",
    title: "cargo test --workspace",
    meta: "Windows Studio · 1 分钟前",
    color: "coral",
    tags: ["命令", "代码"],
    confidence: 97,
  },
  {
    icon: Braces,
    type: "DATA",
    title: '{"trusted":true,"route":"lan"}',
    meta: "MacBook Pro · 4 分钟前",
    color: "cyan",
    tags: ["结构化数据", "代码"],
    confidence: 94,
  },
  {
    icon: File,
    type: "FILE",
    title: MAC_INSTALLER_NAME,
    meta: "Windows Studio · 8 分钟前",
    color: "paper",
    tags: ["文件"],
    confidence: 100,
  },
];

export function ProductWindow() {
  const {
    phase: activeEntry,
    setPhase: setActiveEntry,
    ref,
  } = usePhaseCycle<HTMLDivElement>(entries.length, 1_650);

  const active = entries[activeEntry];

  return (
    <div className="product-window" ref={ref}>
      <div className="product-window-scan" aria-hidden="true" />
      <div className="product-titlebar">
        <div className="product-title">
          <img src={tailsyncIcon} alt="" />
          <span>TailSync</span>
          <small>main</small>
        </div>
        <div className="product-live-state">
          <i /> CLASSIFIER V4 / {active.type}
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
        <button type="button" aria-label="选择日期范围">
          <CalendarDays size={15} />
        </button>
      </div>
      <div className="product-filterbar">
        <button className="active" type="button"><CalendarDays size={12} /> 今天</button>
        <button type="button"><Tags size={12} /> 全部分类</button>
        <span><Database size={12} /> 16 RESULTS</span>
      </div>
      <div className="product-date">
        <span>今天 / TODAY</span>
        <small>SMART MATCH 0{activeEntry + 1} / 04</small>
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
                <span className="product-row-tags">
                  {entry.tags.map((tag, tagIndex) => (
                    <i className={tagIndex === 0 ? "primary" : ""} key={tag}>{tag}</i>
                  ))}
                  <em>{entry.confidence}%</em>
                </span>
              </span>
              <span className="row-action">
                {index === activeEntry ? <Check size={15} /> : index === 0 ? <Copy size={15} /> : <ArrowRight size={15} />}
              </span>
            </button>
          );
        })}
      </div>
      <div className="product-statusbar">
        <span><i /> {active.type} 已分类</span>
        <span>LOCAL DB / {activeEntry + 1} OF 4 / INDEXED</span>
      </div>
    </div>
  );
}
