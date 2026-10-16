import { Activity, Trash2 } from "lucide-react";
import { RdToggle } from "./primitives";
import { MOCK_PEERS, MOCK_SELF, type MockState } from "./mock";

type Patch = (p: Partial<MockState>) => void;

function routeSummary(peer: (typeof MOCK_PEERS)[number]) {
  const route = peer.routes.find((r) => r.status === "connected") ?? peer.routes[0];
  if (!route) return "已配对 · 等待设备上线";
  const base = `${route.address} · ${route.status === "connected" ? "已连接" : "已发现"}`;
  return route.latencyMs != null ? `${base} · ${route.latencyMs} ms` : base;
}

export function DeviceList({ state, patch }: { state: MockState; patch: Patch }) {
  return (
    <div className="device-list rd-device-list" aria-live="polite">
      {/* 本机压缩为一行 */}
      <div className="device-row local-device rd-self-row">
        <div className="device-avatar self">{MOCK_SELF.hostname.slice(0, 1)}</div>
        <div className="device-info">
          <div className="device-name">
            <span className="device-name-text">{MOCK_SELF.hostname}</span>
            <span>本机</span>
          </div>
          <div className="device-fingerprint">
            {MOCK_SELF.routes.map((r) => r.address).join(" · ")} · 在线
          </div>
        </div>
      </div>

      {MOCK_PEERS.map((peer) => {
        const enabled = state.peerEnabled[peer.hostname] ?? false;
        return (
          <div className="device-row peer-device-row" key={peer.hostname}>
            <div className="device-avatar">{peer.hostname.slice(0, 1)}</div>
            <div className="device-info">
              <div className="device-name">
                <span className="device-name-text">{peer.hostname}</span>
                <span className={peer.trusted ? "peer-badge paired" : "peer-badge unpaired"}>
                  {peer.trusted ? "已配对" : "未配对"}
                </span>
              </div>
              <div className="device-fingerprint">
                {peer.trusted ? peer.fingerprint : "等待安全配对"}
              </div>
              <div className="rd-peer-meta">
                <span className={`rd-peer-route ${peer.trusted ? "ok" : ""}`}>
                  {routeSummary(peer)}
                </span>
                {peer.trusted && (
                  <button type="button" className="connection-test-button" title="测试连接" aria-label="测试连接">
                    <Activity size={16} strokeWidth={1.7} aria-hidden="true" />
                  </button>
                )}
              </div>
            </div>
            <div className="device-actions">
              {peer.trusted ? (
                <>
                  <RdToggle
                    checked={enabled}
                    onChange={(v) => patch({ peerEnabled: { ...state.peerEnabled, [peer.hostname]: v } })}
                    label={enabled ? "停止同步" : "启用同步"}
                  />
                  <button type="button" className="icon-button" title="撤销配对" aria-label="撤销配对">
                    <Trash2 size={16} strokeWidth={1.7} aria-hidden="true" />
                  </button>
                </>
              ) : (
                <button type="button" className="pair-device-action">配对</button>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
