import { useState } from "react";
import { Activity, ArrowRight, Braces, CalendarDays, Check, Code2, Database, File, Folder, Globe2, ImageIcon, Search, Tags, Terminal, Type } from "lucide-react";
import { usePhaseCycle } from "../hooks/usePhaseCycle";
import { MAC_INSTALLER_NAME, RELEASE_TAG_URL } from "../product";

type HistoryCategory =
  | "text"
  | "website"
  | "code"
  | "command"
  | "structured_data"
  | "path"
  | "image"
  | "file";

const historyCategories = [
  { id: "text" as HistoryCategory, label: "文本", code: "TEXT", icon: Type },
  { id: "website" as HistoryCategory, label: "网站", code: "WEB", icon: Globe2 },
  { id: "code" as HistoryCategory, label: "代码", code: "CODE", icon: Code2 },
  { id: "command" as HistoryCategory, label: "命令", code: "CMD", icon: Terminal },
  { id: "structured_data" as HistoryCategory, label: "结构化数据", code: "DATA", icon: Braces },
  { id: "path" as HistoryCategory, label: "路径", code: "PATH", icon: Folder },
  { id: "image" as HistoryCategory, label: "图片", code: "IMAGE", icon: ImageIcon },
  { id: "file" as HistoryCategory, label: "文件", code: "FILE", icon: File },
];

const historySamples: Array<{
  category: HistoryCategory;
  value: string;
  source: string;
  time: string;
  tags: string[];
  confidence: number;
  tone: "lime" | "cyan" | "coral" | "paper";
}> = [
  {
    category: "website",
    value: RELEASE_TAG_URL,
    source: "MacBook Pro / Safari",
    time: "刚刚",
    tags: ["网站", "文本"],
    confidence: 98,
    tone: "lime",
  },
  {
    category: "code",
    value: "const route = await probe({ lan: true, tailnet: true });",
    source: "Windows Studio / VS Code",
    time: "1 分钟前",
    tags: ["代码", "文本"],
    confidence: 96,
    tone: "cyan",
  },
  {
    category: "command",
    value: "cargo test --manifest-path windows/src-tauri/Cargo.toml",
    source: "MacBook Pro / Terminal",
    time: "3 分钟前",
    tags: ["命令", "代码"],
    confidence: 97,
    tone: "coral",
  },
  {
    category: "structured_data",
    value: '{"device":"MacBook Pro","trusted":true,"route":"lan"}',
    source: "Windows Studio / Console",
    time: "8 分钟前",
    tags: ["结构化数据", "代码"],
    confidence: 94,
    tone: "paper",
  },
  {
    category: "path",
    value: "C:\\Users\\monet\\Documents\\TailSync\\release-notes.md",
    source: "Windows Studio / Explorer",
    time: "12 分钟前",
    tags: ["路径", "文本"],
    confidence: 99,
    tone: "lime",
  },
  {
    category: "text",
    value: "设计评审改到 14:30，main 分支唤醒恢复验证已经通过。",
    source: "MacBook Pro / Notes",
    time: "18 分钟前",
    tags: ["文本"],
    confidence: 91,
    tone: "cyan",
  },
  {
    category: "image",
    value: "history-classification-preview.png / 2880 x 1800",
    source: "MacBook Pro / Screenshot",
    time: "26 分钟前",
    tags: ["图片", "文件"],
    confidence: 100,
    tone: "coral",
  },
  {
    category: "file",
    value: `${MAC_INSTALLER_NAME} / 18.4 MB`,
    source: "Windows Studio / Downloads",
    time: "31 分钟前",
    tags: ["文件"],
    confidence: 100,
    tone: "paper",
  },
];

const historyDateFilters = [
  { key: "all", label: "全部", count: 128 },
  { key: "today", label: "今天", count: 16 },
  { key: "yesterday", label: "昨天", count: 11 },
  { key: "week", label: "最近 7 天", count: 63 },
  { key: "month", label: "最近 30 天", count: 104 },
  { key: "this-month", label: "本月", count: 89 },
  { key: "custom", label: "自定义", count: 42 },
];

export function HistoryIntelligence() {
  const {
    phase: activeSample,
    setPhase: setActiveSample,
    ref,
  } = usePhaseCycle<HTMLDivElement>(historySamples.length, 2_400);
  const [activeDate, setActiveDate] = useState("today");

  const sample = historySamples[activeSample];
  const category = historyCategories.find((item) => item.id === sample.category) ?? historyCategories[0];
  const activeFilter = historyDateFilters.find((filter) => filter.key === activeDate) ?? historyDateFilters[1];
  const CategoryIcon = category.icon;
  const visibleResults = [0, 1, 2].map(
    (offset) => historySamples[(activeSample + offset) % historySamples.length],
  );

  const selectCategory = (categoryId: HistoryCategory) => {
    const nextIndex = historySamples.findIndex((item) => item.category === categoryId);
    if (nextIndex >= 0) setActiveSample(nextIndex);
  };

  return (
    <section className="history-intelligence" id="history">
      <div className="history-kinetic-word" aria-hidden="true">CLASSIFY</div>
      <div className="history-pulse-field" aria-hidden="true">
        {Array.from({ length: 18 }, (_, index) => <i key={`history-pulse-${index}`} />)}
      </div>

      <div className="history-intro" data-reveal>
        <div className="section-marker">
          <span>04</span>
          <small>SMART HISTORY / V4</small>
        </div>
        <div className="history-intro-copy">
          <span className="history-eyebrow"><Tags size={15} /> LOCAL CLASSIFIER / MULTI-LABEL</span>
          <h2>历史不再只是<strong>按时间堆叠。</strong></h2>
          <p>
            TailSync 在本地识别八类剪贴板内容，为一条记录保留主标签、次标签与置信度；再用完整日期范围，把需要的那一条迅速找回来。
          </p>
        </div>
        <div className="history-intro-stats" aria-label="智能历史能力摘要" data-cascade>
          <div><strong>08</strong><span>内容分类</span></div>
          <div><strong>V4</strong><span>分类器版本</span></div>
          <div><strong>100%</strong><span>本地处理</span></div>
        </div>
      </div>

      <div className="history-console" data-reveal ref={ref}>
        <div className="history-console-head">
          <span><Database size={15} /> HISTORY INTELLIGENCE</span>
          <div className="history-console-live"><i /> INDEX ONLINE</div>
          <small>DATABASE / LOCAL / INDEXED</small>
        </div>

        <div className="history-date-filter" role="group" aria-label="历史日期范围">
          <span className="history-date-label"><CalendarDays size={15} /> RANGE</span>
          <div className="history-date-options">
            {historyDateFilters.map((filter) => (
              <button
                className={activeDate === filter.key ? "active" : ""}
                type="button"
                aria-pressed={activeDate === filter.key}
                key={filter.key}
                onClick={() => setActiveDate(filter.key)}
              >
                {filter.label}
              </button>
            ))}
          </div>
          <strong>{String(activeFilter.count).padStart(3, "0")} RESULTS</strong>
        </div>

        <div className="history-console-grid">
          <aside className="history-category-rail" aria-label="内容分类">
            <div className="history-rail-title">CLASS / 08</div>
            {historyCategories.map((item, index) => {
              const Icon = item.icon;
              const isActive = item.id === sample.category;
              return (
                <button
                  className={isActive ? "active" : ""}
                  type="button"
                  aria-pressed={isActive}
                  key={item.id}
                  onClick={() => selectCategory(item.id)}
                >
                  <span>0{index + 1}</span>
                  <Icon size={16} />
                  <b>{item.label}</b>
                  <small>{item.code}</small>
                </button>
              );
            })}
          </aside>

          <div className={`history-analysis tone-${sample.tone}`}>
            <div className="history-analysis-scan" aria-hidden="true" />
            <div className="history-vector-field" aria-hidden="true">
              {Array.from({ length: 24 }, (_, index) => <i key={`vector-${index}`} />)}
            </div>
            <div className="history-sample" key={sample.category}>
              <div className="history-sample-head">
                <span><CategoryIcon size={18} /> INPUT / {category.code}</span>
                <small>{sample.time}</small>
              </div>
              <p>{sample.value}</p>
              <div className="history-sample-source">{sample.source}</div>
              <div className="history-labels">
                <span>LABELS</span>
                {sample.tags.map((tag, index) => (
                  <b className={index === 0 ? "primary" : "secondary"} key={tag}>
                    {index === 0 ? <Check size={11} /> : <Tags size={11} />}
                    {tag}
                  </b>
                ))}
              </div>
              <div className="history-confidence">
                <div>
                  <span>CONFIDENCE</span>
                  <strong>{sample.confidence}%</strong>
                </div>
                <span className="history-confidence-track">
                  <i style={{ "--confidence": sample.confidence / 100 } as React.CSSProperties} />
                </span>
              </div>
              <div className="history-feature-strip" aria-hidden="true">
                <span>SCHEME</span><i />
                <span>TOKEN</span><i />
                <span>SHAPE</span><i />
                <span>CONTEXT</span><i />
              </div>
            </div>
            <div className="history-orbit-tags" aria-hidden="true">
              <span>01</span><span>V4</span><span>LOCAL</span><span>ML</span>
            </div>
          </div>

          <div className="history-results">
            <div className="history-results-head">
              <span><Search size={14} /> MATCHES</span>
              <small>{activeFilter.label.toUpperCase()}</small>
            </div>
            {visibleResults.map((entry, index) => {
              const itemCategory = historyCategories.find((item) => item.id === entry.category) ?? historyCategories[0];
              const Icon = itemCategory.icon;
              return (
                <button
                  className={index === 0 ? "active" : ""}
                  type="button"
                  key={`${entry.category}-${index}`}
                  onClick={() => selectCategory(entry.category)}
                >
                  <span className={`history-result-icon tone-${entry.tone}`}><Icon size={16} /></span>
                  <span className="history-result-copy">
                    <b>{itemCategory.label}</b>
                    <strong>{entry.value}</strong>
                    <small>{entry.source} / {entry.time}</small>
                  </span>
                  <ArrowRight size={14} />
                </button>
              );
            })}
            <div className="history-query-state">
              <Activity size={15} />
              <span>LOCAL QUERY</span>
              <strong>INDEXED</strong>
            </div>
          </div>
        </div>

        <div className="history-console-foot">
          <span><Check size={13} /> CLASSIFIED LOCALLY</span>
          <span>MULTI-LABEL / CONFIDENCE STORED</span>
          <span>FILTER / {activeFilter.label.toUpperCase()}</span>
        </div>
      </div>

      <div className="history-category-marquee" aria-hidden="true">
        <div>
          {[...historyCategories, ...historyCategories].map((item, index) => (
            <span key={`${item.id}-${index}`}>{item.code}<i /></span>
          ))}
        </div>
      </div>
    </section>
  );
}
