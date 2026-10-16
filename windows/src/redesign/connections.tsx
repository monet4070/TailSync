import { RefreshCw, Wifi, Grid2X2 } from "lucide-react";
import { SectionShell, type Chrome } from "./primitives";
import { DeviceList } from "./devices";
import { AddDevicesBlock } from "./addDevices";
import {
  MOCK_PEERS,
  ZH,
  type ConnectionMode,
  type MockState,
} from "./mock";

type Patch = (p: Partial<MockState>) => void;

const MODES: Array<{ id: ConnectionMode; label: string; icon?: "wifi" | "grid" }> = [
  { id: "auto", label: ZH.modeAuto },
  { id: "lan_only", label: ZH.modeLan, icon: "wifi" },
  { id: "iroh_only", label: ZH.modeIroh },
  { id: "tailscale_only", label: ZH.modeTailscale, icon: "grid" },
];

export function ConnectionsSection({
  chrome,
  state,
  patch,
}: {
  chrome: Chrome;
  state: MockState;
  patch: Patch;
}) {
  const syncable = MOCK_PEERS.filter((p) => p.trusted && state.peerEnabled[p.hostname] !== false);
  const statusLine = `发现 ${MOCK_PEERS.length} 台设备 · ${syncable.length} 台可同步`;

  return (
    <SectionShell
      chrome={chrome}
      title={ZH.connectionsTitle}
      description={ZH.connectionsDescription}
      headerAction={
        <button type="button" className="icon-button rd-refresh" title="刷新设备" aria-label="刷新设备">
          <RefreshCw size={16} strokeWidth={1.7} aria-hidden="true" />
        </button>
      }
    >
      {/* 1. 连接方式 */}
      <div className="connection-mode" role="radiogroup" aria-label="连接方式">
        {MODES.map((m) => (
          <button
            key={m.id}
            type="button"
            role="radio"
            aria-checked={state.connectionMode === m.id}
            className={state.connectionMode === m.id ? "active" : ""}
            onClick={() => patch({ connectionMode: m.id })}
          >
            {m.icon === "wifi" && <Wifi size={15} strokeWidth={1.7} aria-hidden="true" />}
            {m.icon === "grid" && <Grid2X2 size={15} strokeWidth={1.7} aria-hidden="true" />}
            {m.label}
          </button>
        ))}
      </div>
      <p className="rd-mode-status">{statusLine}</p>

      {/* 2. 设备列表上移为本区主角 */}
      <DeviceList state={state} patch={patch} />

      {/* 3. 添加设备：配对窗口 + 折叠的远程配对 */}
      <AddDevicesBlock state={state} patch={patch} />
    </SectionShell>
  );
}
