import { useEffect, useState } from "react";
import { Activity, Check, ClipboardCopy, Laptop, LockKeyhole, Monitor, Power, RefreshCw, ShieldCheck, Waves, WifiOff, Zap } from "lucide-react";

const recoverySteps = [
  { label: "SLEEP", title: "系统休眠", detail: "SOCKET SUSPENDED", icon: WifiOff },
  { label: "WAKE", title: "设备唤醒", detail: "POWER EVENT", icon: Power },
  { label: "PROBE", title: "主动探测", detail: "PEER HEALTH", icon: Activity },
  { label: "RECONNECT", title: "重建连接", detail: "NOISE SESSION", icon: RefreshCw },
  { label: "RESUME", title: "恢复同步", detail: "CLIPBOARD LIVE", icon: Zap },
];

export function RecoverySequence() {
  const [phase, setPhase] = useState(0);

  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const timer = window.setInterval(
      () => setPhase((current) => (current + 1) % recoverySteps.length),
      1_300,
    );
    return () => window.clearInterval(timer);
  }, []);

  const current = recoverySteps[phase];

  return (
    <section className={`recovery-section recovery-phase-${phase}`} id="recovery">
      <div className="recovery-copy" data-reveal>
        <div className="section-marker">
          <span>07</span>
          <small>WAKE RECOVERY / MAIN</small>
        </div>
        <span className="recovery-eyebrow"><RefreshCw size={15} /> RESILIENT SESSION</span>
        <h2>睡一觉，醒来<br /><strong>同步仍在继续。</strong></h2>
        <p>
          Windows 或 macOS 从休眠中唤醒后，TailSync 主动探测对端、重建加密会话并恢复剪贴板监听。恢复的文件不会再回传给原发送端，链路重新上线，也不会形成回环。
        </p>
        <div className="recovery-facts">
          <span><Activity size={16} /> 唤醒后主动健康检查</span>
          <span><RefreshCw size={16} /> 自动重建安全会话</span>
          <span><ShieldCheck size={16} /> 来源标记阻止文件回传</span>
        </div>
      </div>

      <div className="recovery-console" data-reveal>
        <div className="recovery-console-head">
          <span><Waves size={15} /> SESSION RECOVERY MONITOR</span>
          <strong><i /> {current.label}</strong>
          <small>MAIN / CLASSIFIER V4</small>
        </div>

        <div className="recovery-wave" aria-hidden="true">
          {Array.from({ length: 32 }, (_, index) => (
            <i className={`wave-${(index % 6) + 1}`} key={`wave-${index}`} />
          ))}
          <span>CONNECTION SIGNAL</span>
        </div>

        <div className="recovery-timeline" role="group" aria-label="休眠唤醒恢复阶段">
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

        <div className="recovery-network">
          <div className="recovery-device recovery-device-source">
            <Laptop size={23} />
            <strong>MAC</strong>
            <small>SOURCE / TRUSTED</small>
          </div>
          <div className="recovery-link" aria-hidden="true">
            <span className="recovery-link-base" />
            <span className="recovery-link-live" />
            <i className="recovery-packet packet-a"><ClipboardCopy size={12} /></i>
            <i className="recovery-packet packet-b"><LockKeyhole size={12} /></i>
            <b>{current.detail}</b>
          </div>
          <div className="recovery-core">
            <span aria-hidden="true" />
            <RefreshCw size={23} />
            <strong>SESSION</strong>
            <small>{phase < 2 ? "PAUSED" : phase < 4 ? "REBUILD" : "HEALTHY"}</small>
          </div>
          <div className="recovery-link recovery-link-right" aria-hidden="true">
            <span className="recovery-link-base" />
            <span className="recovery-link-live" />
            <i className="recovery-packet packet-a"><Check size={12} /></i>
            <i className="recovery-packet packet-b"><Zap size={12} /></i>
            <b>{phase === 4 ? "SYNC RESUMED" : "WAITING ACK"}</b>
          </div>
          <div className="recovery-device recovery-device-target">
            <Monitor size={23} />
            <strong>PC</strong>
            <small>TARGET / {phase === 0 ? "ASLEEP" : "ONLINE"}</small>
          </div>
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
