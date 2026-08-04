import { useEffect, useState } from "react";
import { Check, Fingerprint, Laptop, LockKeyhole, Monitor, ScanLine, ShieldCheck } from "lucide-react";

const handshakeSteps = [
  { label: "IDENTITY", icon: Fingerprint },
  { label: "VERIFY", icon: ScanLine },
  { label: "HANDSHAKE", icon: LockKeyhole },
  { label: "TRUSTED", icon: ShieldCheck },
];

const handshakePhaseLabels = ["IDENTITY PROOF", "CODE MATCH", "NOISE XX", "TRUST PINNED"];

export function SecurityHandshake() {
  const [phase, setPhase] = useState(0);

  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const timer = window.setInterval(
      () => setPhase((current) => (current + 1) % handshakeSteps.length),
      1_150,
    );
    return () => window.clearInterval(timer);
  }, []);

  return (
    <div className={`handshake handshake-phase-${phase}`} data-reveal>
      <div className="handshake-head">
        <span>SECURE PAIRING / <b>{handshakePhaseLabels[phase]}</b></span>
        <span className="handshake-live-status">
          <i />
          0{phase + 1} / 04
          <ShieldCheck size={19} />
        </span>
      </div>

      <div className="crypto-stage" aria-label="实时加密握手演示">
        <div className="crypto-sweep" aria-hidden="true" />
        <div className="crypto-peer crypto-peer-local">
          <Laptop size={20} />
          <strong>MAC</strong>
          <small>X25519 ID</small>
        </div>
        <div className="crypto-channel" aria-hidden="true">
          <i />
          <span className="crypto-signal crypto-signal-a"><Fingerprint size={12} /></span>
          <span className="crypto-signal crypto-signal-b"><LockKeyhole size={12} /></span>
          <span className="crypto-signal crypto-signal-c"><Check size={12} /></span>
        </div>
        <div className="crypto-core">
          <span className="crypto-ring crypto-ring-a" aria-hidden="true" />
          <span className="crypto-ring crypto-ring-b" aria-hidden="true" />
          <div><LockKeyhole size={22} /><small>NOISE XX</small></div>
        </div>
        <div className="crypto-peer crypto-peer-remote">
          <Monitor size={20} />
          <strong>PC</strong>
          <small>KEY PINNED</small>
        </div>
        <div className="entropy-stream" aria-hidden="true">
          {Array.from({ length: 12 }, (_, index) => <i key={`entropy-${index}`} />)}
        </div>
        <span className="crypto-caption crypto-caption-left">EPHEMERAL KEY</span>
        <span className="crypto-caption crypto-caption-right">AUTHENTICATED</span>
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

      <div className="handshake-steps">
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
