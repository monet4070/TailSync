import { useState } from "react";
import { Folder, HardDrive, Sparkles, RefreshCw, CheckCircle2 } from "lucide-react";
import { SectionShell, Row, type Chrome } from "./primitives";
import { ZH, type MockState } from "./mock";

type Patch = (p: Partial<MockState>) => void;

export function HistoryAndStorageSection({
  chrome,
  state,
  patch,
}: {
  chrome: Chrome;
  state: MockState;
  patch: Patch;
}) {
  const [limit, setLimit] = useState(100);
  const [quota, setQuota] = useState(10);

  return (
    <>
      <SectionShell
        chrome={chrome}
        title={ZH.history}
        description="管理剪贴板历史记录条数"
      >
        <Row
          title="历史记录条数"
          desc={`保留最近 ${limit} 条剪贴板内容`}
          control={
            <div className="rd-stepper">
              <input
                type="number"
                min={20}
                max={1000}
                step={20}
                value={limit}
                className="rd-num-input"
                onChange={(e) => setLimit(Number(e.target.value) || 20)}
              />
              <span className="rd-unit">条</span>
            </div>
          }
        />
      </SectionShell>

      <SectionShell
        chrome={chrome}
        title={ZH.storage}
        description="历史记录与文件传输缓存路径"
      >
        <Row
          title="存储目录"
          desc="/Users/monet/Library/Application Support/TailSync/data"
          control={
            <button
              type="button"
              className="pair-device-action"
              onClick={() => patch({ toast: "演示模式：打开目录选择" })}
            >
              <Folder size={14} />
              <span>更改</span>
            </button>
          }
        />
        <Row
          title="存储配额"
          desc="文件传输和多媒体缓存上限"
          control={
            <div className="rd-stepper">
              <input
                type="number"
                min={1}
                max={100}
                value={quota}
                className="rd-num-input"
                onChange={(e) => setQuota(Number(e.target.value) || 1)}
              />
              <span className="rd-unit">GB</span>
            </div>
          }
        />
      </SectionShell>
    </>
  );
}

export function UpdatesSection({
  chrome,
  state,
  patch,
}: {
  chrome: Chrome;
  state: MockState;
  patch: Patch;
}) {
  const handleCheck = () => {
    patch({ updatePhase: "checking" });
    setTimeout(() => {
      patch({ updatePhase: "current", toast: ZH.updateCurrent });
    }, 1000);
  };

  return (
    <SectionShell
      chrome={chrome}
      title={ZH.updates}
      description={ZH.updatesDescription}
    >
      <Row
        title="当前版本 2.2.2"
        desc={
          state.updatePhase === "checking"
            ? ZH.updateChecking
            : state.updatePhase === "current"
              ? ZH.updateCurrent
              : ZH.updateReady
        }
        control={
          <button
            type="button"
            className="pair-device-action"
            disabled={state.updatePhase === "checking"}
            onClick={handleCheck}
          >
            {state.updatePhase === "checking" ? (
              <RefreshCw size={14} className="spin" />
            ) : state.updatePhase === "current" ? (
              <CheckCircle2 size={14} />
            ) : null}
            <span>
              {state.updatePhase === "checking" ? "检查中…" : ZH.updateCheck}
            </span>
          </button>
        }
      />
    </SectionShell>
  );
}
