import { useState } from "react";
import { ChevronDown, Copy, Check, Link2 } from "lucide-react";
import { MOCK_INVITE_LINK, type MockState } from "./mock";

type Patch = (p: Partial<MockState>) => void;

export function AddDevicesBlock({ state, patch }: { state: MockState; patch: Patch }) {
  const [remoteOpen, setRemoteOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const [inviteDraft, setInviteDraft] = useState("");

  const handleCopy = () => {
    setCopied(true);
    patch({ toast: "邀请链接已复制" });
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="rd-add-block">
      {/* 局域网配对常驻控制 */}
      <div className="pairing-window-row rd-pair-bar">
        <div>
          <strong>设备配对</strong>
          <span>
            {state.pairingOpen
              ? `等待连接 · ${state.pairingSecondsLeft}s`
              : "允许附近设备发现并申请配对"}
          </span>
        </div>
        <button
          type="button"
          className={state.pairingOpen ? "pairing-window-close" : "pair-device-action"}
          onClick={() => patch({ pairingOpen: !state.pairingOpen })}
        >
          {state.pairingOpen ? "关闭配对" : "开启配对"}
        </button>
      </div>

      {/* 远程 Iroh 配对：紧凑折叠设计，解决原版占用半屏问题 */}
      <div className="rd-collapsible">
        <button
          type="button"
          className="rd-collapsible-trigger"
          onClick={() => setRemoteOpen(!remoteOpen)}
          aria-expanded={remoteOpen}
        >
          <div className="rd-collapsible-title">
            <Link2 size={14} strokeWidth={1.8} />
            <span>异地/公网配对 (Iroh 中继)</span>
          </div>
          <ChevronDown
            size={14}
            className={`rd-chevron ${remoteOpen ? "open" : ""}`}
          />
        </button>

        {remoteOpen && (
          <div className="rd-collapsible-content">
            <p className="rd-hint-text">
              两台设备不在同一局域网时，生成或输入短效邀请链接完成端到端直连。
            </p>
            <div className="rd-remote-grid">
              <div className="rd-remote-card">
                <span className="rd-sublabel">创建一次性邀请</span>
                <button
                  type="button"
                  className="pair-device-action rd-btn-block"
                  onClick={handleCopy}
                >
                  {copied ? <Check size={14} /> : <Copy size={14} />}
                  <span>{copied ? "已复制链接" : "复制邀请链接 (15分钟有效)"}</span>
                </button>
              </div>

              <div className="rd-remote-card">
                <span className="rd-sublabel">加入远程邀请</span>
                <div className="rd-input-group">
                  <input
                    type="text"
                    className="rd-input"
                    placeholder="粘贴 tailsync://pair/..."
                    value={inviteDraft}
                    onChange={(e) => setInviteDraft(e.target.value)}
                  />
                  <button
                    type="button"
                    className="pair-device-action"
                    disabled={!inviteDraft.trim()}
                    onClick={() => {
                      patch({ toast: "已开始建立 Iroh 握手" });
                      setInviteDraft("");
                    }}
                  >
                    连接
                  </button>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
