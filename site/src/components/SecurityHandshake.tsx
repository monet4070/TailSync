import { Check, Fingerprint, LockKeyhole, ScanLine, ShieldCheck } from "lucide-react";
import { usePhaseCycle } from "../hooks/usePhaseCycle";

const handshakeSteps = [
  { label: "IDENTITY", icon: Fingerprint },
  { label: "VERIFY", icon: ScanLine },
  { label: "HANDSHAKE", icon: LockKeyhole },
  { label: "TRUSTED", icon: ShieldCheck },
];

const handshakePhaseLabels = ["IDENTITY PROOF", "CODE MATCH", "NOISE XX", "TRUST PINNED"];

export function SecurityHandshake() {
  const { phase, ref } = usePhaseCycle<HTMLDivElement>(handshakeSteps.length, 1_150);

  return (
    <div className={`handshake handshake-phase-${phase}`} data-reveal ref={ref}>
      <div className="handshake-head">
        <span>SECURE PAIRING / <b>{handshakePhaseLabels[phase]}</b></span>
        <span className="handshake-live-status">
          <i />
          0{phase + 1} / 04
          <ShieldCheck size={19} />
        </span>
      </div>

      <div className="crypto-stage" aria-label="加密握手演示：双方身份合拢成印">
        <div className="seal" aria-hidden="true">
          <svg className="seal-svg" viewBox="0 0 200 200">
            <circle className="seal-guide" cx="100" cy="100" r="84" />
            <circle className="seal-guide seal-guide-dash" cx="100" cy="100" r="64" />
            <path className="seal-arc seal-arc-a" d="M100,26 A74,74 0 0 1 100,174" />
            <path className="seal-arc seal-arc-b" d="M100,26 A74,74 0 0 0 100,174" />
            <circle className="seal-disc" cx="100" cy="100" r="46" />
          </svg>
          <div className="seal-core">
            <LockKeyhole size={22} />
            <small>NOISE XX</small>
          </div>
        </div>
        <span className="seal-caption seal-caption-a">X25519 · 本机</span>
        <span className="seal-caption seal-caption-b">X25519 · 对端</span>
      </div>

      <div className="pair-code-panel">
        <div className="pair-code-meta">
          <span>ONE-TIME VERIFICATION</span>
          <small>CODE MATCH / BOTH DEVICES</small>
        </div>
        <div className="pair-code" aria-label="示例配对验证码">
          {["4", "8", "1", "6", "0", "2"].map((digit) => <span key={digit}>{digit}</span>)}
        </div>
      </div>

      <div className="handshake-steps" data-cascade>
        {handshakeSteps.map((step, index) => {
          const Icon = step.icon;
          const state = index < phase ? "complete" : index === phase ? "active" : "pending";
          return (
            <div className={state} key={step.label}>
              <span><Icon size={18} /></span>
              <small>0{index + 1}</small>
              <strong>{step.label}</strong>
            </div>
          );
        })}
      </div>
      <div className="fingerprint-line">
        <span>DEVICE FINGERPRINT</span>
        <code>7A:4C:91:EF:2D:08:AA:61</code>
        <strong><Check size={12} /> MATCH</strong>
      </div>
    </div>
  );
}
