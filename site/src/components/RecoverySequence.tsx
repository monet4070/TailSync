import { Activity, Power, RefreshCw, ShieldCheck, Waves, WifiOff, Zap } from "lucide-react";
import { usePhaseCycle } from "../hooks/usePhaseCycle";

const recoverySteps = [
  { label: "SLEEP", title: "系统休眠", detail: "SOCKET SUSPENDED", icon: WifiOff },
  { label: "WAKE", title: "设备唤醒", detail: "POWER EVENT", icon: Power },
  { label: "PROBE", title: "主动探测", detail: "PEER HEALTH", icon: Activity },
  { label: "RECONNECT", title: "重建连接", detail: "NOISE SESSION", icon: RefreshCw },
  { label: "RESUME", title: "恢复同步", detail: "CLIPBOARD LIVE", icon: Zap },
];

export function RecoverySequence() {
  const { phase, setPhase, ref } = usePhaseCycle<HTMLDivElement>(recoverySteps.length, 1_300);

  const current = recoverySteps[phase];

  return (
    <section className={`recovery-section recovery-phase-${phase}`} id="recovery">
      <div className="recovery-copy" data-reveal>
        <div className="section-marker">
          <span>09</span>
          <small>WAKE RECOVERY / MAIN</small>
        </div>
        <span className="recovery-eyebrow"><RefreshCw size={15} /> RESILIENT SESSION</span>
        <h2>设备唤醒后，<br /><strong>同步会自动恢复。</strong></h2>
        <p>
          Windows 或 macOS 从休眠中唤醒后，TailSync 主动探测对端、重建加密会话并恢复剪贴板监听。恢复的文件不会再回传给原发送端，链路重新上线，也不会形成回环。
        </p>
        <div className="recovery-facts" data-cascade>
          <span><Activity size={16} /> 唤醒后主动健康检查</span>
          <span><RefreshCw size={16} /> 自动重建安全会话</span>
          <span><ShieldCheck size={16} /> 来源标记阻止文件回传</span>
        </div>
      </div>

      <div className="recovery-console" data-reveal ref={ref}>
        <div className="recovery-console-head">
          <span><Waves size={15} /> SESSION RECOVERY MONITOR</span>
          <strong><i /> {current.label}</strong>
          <small>MAIN / CLASSIFIER V4</small>
        </div>

        <div className={`recovery-trace status-${phase === 0 ? "flat" : phase === 4 ? "live" : "revive"}`} aria-hidden="true">
          <svg className="ecg" viewBox="0 0 480 96" preserveAspectRatio="none">
            <path className="ecg-flat" d="M0,48 H480" vectorEffect="non-scaling-stroke" />
            <path
              className="ecg-line"
              pathLength={100}
              vectorEffect="non-scaling-stroke"
              d="M0,48 H36 L44,48 L47,41 L50,48 L53,61 L56,15 L59,66 L62,48 H132 L140,48 L143,41 L146,48 L149,61 L152,15 L155,66 L158,48 H228 L236,48 L239,41 L242,48 L245,61 L248,15 L251,66 L254,48 H324 L332,48 L335,41 L338,48 L341,61 L344,15 L347,66 L350,48 H420 L428,48 L431,41 L434,48 L437,61 L440,15 L443,66 L446,48 H480"
            />
          </svg>
          <span className="ecg-sweep" />
          <div className="ecg-readout">
            <i />
            <b>{current.detail}</b>
          </div>
        </div>

        <div className="recovery-timeline" role="group" aria-label="休眠唤醒恢复阶段" data-cascade>
          {recoverySteps.map((step, index) => {
            const Icon = step.icon;
            const state = index < phase ? "complete" : index === phase ? "active" : "pending";
            return (
              <button
                className={state}
                type="button"
                aria-pressed={index === phase}
                key={step.label}
                onClick={() => setPhase(index)}
              >
                <span><Icon size={18} /></span>
                <small>0{index + 1}</small>
                <strong>{step.label}</strong>
                <em>{step.title}</em>
              </button>
            );
          })}
        </div>

        <div className="recovery-log">
          <div className="recovery-log-head">
            <span>EVENT STREAM</span>
            <small>AUTOMATIC / NO USER ACTION</small>
          </div>
          {recoverySteps.map((step, index) => {
            const Icon = step.icon;
            return (
              <div className={index === phase ? "active" : index < phase ? "complete" : ""} key={`log-${step.label}`}>
                <span>14:32:{String(index * 2 + 1).padStart(2, "0")}</span>
                <Icon size={14} />
                <strong>{step.detail}</strong>
                <small>{index <= phase ? index === phase ? "RUNNING" : "OK" : "QUEUED"}</small>
              </div>
            );
          })}
        </div>

        <div className="recovery-guard">
          <ShieldCheck size={17} />
          <span><strong>ORIGIN GUARD</strong> / RECEIVED FILE WILL NOT RETURN TO SENDER</span>
          <b>NO LOOPBACK</b>
        </div>
      </div>
    </section>
  );
}
