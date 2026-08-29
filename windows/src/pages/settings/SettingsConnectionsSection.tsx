import { Activity, Grid2X2, RefreshCw, Trash2, Wifi } from "lucide-react";
import type { PeerRoute } from "../../tailsyncClient";
import { pairingAddressForPeer } from "../../utils/pairingAddress";
import { routeSupportsLatencyTest } from "../../utils/peerRoute";
import {
  peerCanSync,
  routeInterfaceLabel,
} from "./SettingsFormatters";
import type { SettingsConnectionsSectionProps } from "./SettingsSectionTypes";

export function SettingsConnectionsSection({
  settings,
  t,
  devices,
  devicesLoading,
  devicesError,
  pairingStatus,
  pairingBusy,
  connectionTests,
  refreshDevices,
  handleConnectionMode,
  closePairing,
  handleEnablePairing,
  handleTestConnection,
  handlePeerToggle,
  handleForget,
  openPairing,
}: SettingsConnectionsSectionProps) {
  return (
    <section className="setting-group connection-group">
      <div className="setting-group-header section-header-with-action">
        <div>
          <h3>{t("settings.connectionsTitle")}</h3>
          <p>{t("settings.connectionsDescription")}</p>
        </div>
        <button
          type="button"
          className="icon-button"
          onClick={() => void refreshDevices()}
          disabled={devicesLoading}
          title={t("settings.refreshDevices")}
          aria-label={t("settings.refreshDevices")}
        >
          <RefreshCw className={devicesLoading ? "spin" : ""} size={16} strokeWidth={1.7} aria-hidden="true" />
        </button>
      </div>

      <div className="connection-mode" role="radiogroup" aria-label={t("settings.connectionMode")}>
        <button
          type="button"
          className={settings.connection_mode === "auto" ? "active" : ""}
          onClick={() => void handleConnectionMode("auto")}
          role="radio"
          aria-checked={settings.connection_mode === "auto"}
        >
          {t("settings.modeAuto")}
        </button>
        <button
          type="button"
          className={settings.connection_mode === "lan_only" ? "active" : ""}
          onClick={() => void handleConnectionMode("lan_only")}
          role="radio"
          aria-checked={settings.connection_mode === "lan_only"}
        >
          <Wifi size={15} strokeWidth={1.7} aria-hidden="true" />
          {t("settings.modeLan")}
        </button>
        <button
          type="button"
          className={settings.connection_mode === "tailscale_only" ? "active" : ""}
          onClick={() => void handleConnectionMode("tailscale_only")}
          role="radio"
          aria-checked={settings.connection_mode === "tailscale_only"}
        >
          <Grid2X2 size={15} strokeWidth={1.7} aria-hidden="true" />
          Tailscale
        </button>
      </div>

      <div className="pairing-window-row">
        <div>
          <strong>{t("settings.devicePairing")}</strong>
          <span>
            {pairingStatus?.pairing_enabled
              ? `${t("settings.waiting")} · ${pairingStatus.remaining_seconds}s · ${pairingStatus.failed_attempts}/${pairingStatus.max_failures}`
              : t("settings.pairingClosed")}
          </span>
        </div>
        <button
          type="button"
          className={pairingStatus?.pairing_enabled ? "pairing-window-close" : "pair-device-action"}
          disabled={pairingBusy}
          onClick={() => pairingStatus?.pairing_enabled ? void closePairing() : void handleEnablePairing()}
        >
          {t(pairingStatus?.pairing_enabled
            ? "settings.closePairing"
            : "settings.allowPairing")}
        </button>
      </div>

      <div className="device-list" aria-live="polite">
        {devices && (
          <div className="device-row local-device">
            <div className="device-avatar self">{devices.self.hostname.slice(0, 1).toUpperCase()}</div>
            <div className="device-info">
              <div className="device-name">
                <span className="device-name-text">{devices.self.hostname}</span>
                <span>{t("settings.thisDevice")}</span>
              </div>
              <div className="device-fingerprint">{devices.self.fingerprint}</div>
              {devices.self.iroh_endpoint_id && (
                <div className="device-fingerprint" title={devices.self.iroh_endpoint_id}>
                  iroh: {devices.self.iroh_endpoint_id}
                </div>
              )}
              <div className="peer-route-list local-route-list">
                {(devices.self.routes?.length
                  ? devices.self.routes
                  : devices.self.tailscale_ip
                    ? [{
                      interface: devices.self.connection_mode === "tailscale_only" ? "tailscale" : "lan",
                      address: devices.self.tailscale_ip,
                      status: "connected",
                      online: true,
                      connected: true,
                      latency_ms: null,
                    } satisfies PeerRoute]
                    : []).map((route) => (
                      <div className="peer-route" key={`${route.interface}-${route.address}`}>
                        <span className="peer-route-address">{route.address}</span>
                        <span className={`peer-route-interface ${route.interface}`}>
                          {routeInterfaceLabel(route.interface)}
                        </span>
                        <span className="peer-route-status positive">
                          {t("settings.online")}
                        </span>
                      </div>
                    ))}
              </div>
            </div>
          </div>
        )}

        {devices?.peers.map((peer) => {
          const routes = peer.routes?.length
            ? peer.routes
            : (peer.address || peer.tailscale_ip)
              ? [{
                interface: peer.current_interface ?? (peer.connection_mode === "tailscale" ? "tailscale" : "lan"),
                address: peer.address || peer.tailscale_ip,
                status: peer.current_interface ? "connected" : peer.online ? "online" : "offline",
                online: peer.online,
                connected: Boolean(peer.current_interface),
                latency_ms: null,
                rtt_capable: peer.current_interface !== "iroh",
              } satisfies PeerRoute]
              : [];
          const pairingAddress = pairingAddressForPeer(peer);
          return (
            <div className="device-row peer-device-row" key={peer.hostname}>
              <div className="device-avatar">{peer.hostname.slice(0, 1).toUpperCase()}</div>
              <div className="device-info">
                <div className="device-name">
                  <span className="device-name-text">{peer.hostname}</span>
                  <span className={peer.trusted ? "peer-badge paired" : "peer-badge unpaired"}>
                    {t(peer.trusted ? "settings.paired" : "settings.notPaired")}
                  </span>
                </div>
                <div className="device-fingerprint">
                  {peer.trusted ? peer.fingerprint : t("settings.waitingSecurePairing")}
                </div>
                <div className={`peer-sync-state ${peerCanSync(peer) ? "ready" : "blocked"}`}>
                  {peerCanSync(peer)
                    ? t("settings.syncReady")
                    : !peer.trusted
                      ? t("settings.syncNeedsPairing")
                      : !peer.enabled
                        ? t("settings.syncPeerPaused")
                        : t("settings.syncNoRoute")}
                </div>
                {peer.required_protocol_version != null && (
                  <div className="peer-protocol-warning" role="status">
                    {t("settings.protocolUpgradeRequired").replace(
                      "{version}",
                      String(peer.required_protocol_version),
                    )}
                  </div>
                )}
                {routes.length > 0 ? (
                  <div className="peer-route-list">
                    {routes.map((route) => {
                      const testKey = `${peer.hostname}|${route.interface}|${route.address}`;
                      const test = connectionTests[testKey];
                      const reachabilityStatus = route.status === "connected" ? "online" : route.status;
                      return (
                        <div className="peer-route" key={`${route.interface}-${route.address}`}>
                          <span className="peer-route-address" title={route.address}>{route.address}</span>
                          <span className={`peer-route-interface ${route.interface}`}>
                            {routeInterfaceLabel(route.interface)}
                          </span>
                          <span className={`peer-route-status health-${reachabilityStatus}`}>
                            {reachabilityStatus === "online"
                              ? `${t("settings.online")}${route.latency_ms != null ? ` · ${route.latency_ms} ms` : ""}`
                              : reachabilityStatus === "confirming"
                                ? t("settings.confirming")
                                : reachabilityStatus === "discovered"
                                  ? t("settings.discovered")
                                  : t("settings.offline")}
                          </span>
                          <span className={`peer-route-connection ${route.connected ? "connected" : "idle"}`}>
                            {t(route.connected ? "settings.connected" : "settings.notConnected")}
                          </span>
                          <button
                            type="button"
                            className="connection-test-button"
                            disabled={test?.status === "testing" || !routeSupportsLatencyTest(route)}
                            onClick={() => void handleTestConnection(peer, route)}
                            title={!routeSupportsLatencyTest(route)
                              ? t("settings.testRouteRediscover")
                              : t(route.interface === "iroh" ? "settings.testRoute" : "settings.testTcpPort")}
                            aria-label={`${t("settings.testAddress")}: ${route.address}`}
                          >
                            {test?.status === "testing"
                              ? <RefreshCw className="spin" size={16} strokeWidth={1.7} aria-hidden="true" />
                              : <Activity size={16} strokeWidth={1.7} aria-hidden="true" />}
                          </button>
                          {test?.status === "success" && (
                            <span className="connection-test-result success">
                              {test.latency_ms} ms
                              {test.path === "relay" && ` · ${t("settings.relayPath")}`}
                            </span>
                          )}
                          {test?.status === "error" && (
                            <span className="connection-test-result error">
                              {t("settings.failed")}
                            </span>
                          )}
                        </div>
                      );
                    })}
                  </div>
                ) : (
                  <div className="device-address">
                    {t("settings.pairedWaiting")}
                  </div>
                )}
              </div>
              <div className="device-actions">
                {peer.trusted ? (
                  <>
                    <label className="toggle" title={t(peer.enabled ? "settings.disableSync" : "settings.enableSync")}>
                      <input
                        type="checkbox"
                        checked={peer.enabled}
                        onChange={(event) => void handlePeerToggle(peer, event.target.checked)}
                      />
                      <div className="toggle-track" />
                    </label>
                    <button
                      type="button"
                      className="icon-button"
                      onClick={() => void handleForget(peer)}
                      title={t("settings.forgetPairing")}
                      aria-label={t("settings.forgetPairing")}
                    >
                      <Trash2 size={16} strokeWidth={1.7} aria-hidden="true" />
                    </button>
                  </>
                ) : (
                  <button
                    type="button"
                    className="pair-device-action"
                    onClick={() => void openPairing(peer)}
                    disabled={!pairingAddress}
                    title={pairingAddress ? undefined : t("settings.pairUnavailable")}
                  >
                    {t("settings.pair")}
                  </button>
                )}
              </div>
            </div>
          );
        })}

        {devicesLoading && !devices && (
          <div className="device-list-state">{t("settings.discoveringDevices")}</div>
        )}
        {!devicesLoading && devices && devices.peers.length === 0 && (
          <div className="device-list-state">
            {t("settings.noDevices")}
          </div>
        )}
        {!devicesLoading && devicesError && (
          <div className="device-list-state error">
            {t(settings.connection_mode === "tailscale_only"
              ? "settings.tailscaleUnavailable"
              : "settings.lanUnavailable")}
          </div>
        )}
      </div>
    </section>
  );
}
