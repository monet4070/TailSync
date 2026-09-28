import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SettingsData } from "../../types/settings.generated";
import { SettingsConnectionsSection } from "./SettingsConnectionsSection";

const mockDevices = {
  self: {
    hostname: "DESKTOP-TEST",
    fingerprint: "SHA256:abcd1234local",
    connection_mode: "auto",
    routes: [
      {
        interface: "lan",
        address: "192.168.1.100:52520",
        status: "connected",
        online: true,
        connected: true,
        latency_ms: null,
      },
    ],
  },
  peers: [
    {
      hostname: "MACBOOK-PRO",
      fingerprint: "SHA256:ef5678peer",
      trusted: true,
      enabled: true,
      online: true,
      routes: [
        {
          interface: "lan",
          address: "192.168.1.101:52520",
          status: "connected",
          online: true,
          connected: true,
          latency_ms: 12,
        },
      ],
    },
  ],
};

const labels: Record<string, string> = {
  "settings.connectionsTitle": "Connections & devices",
  "settings.connectionsDescription": "Choose how TailSync finds devices and manage secure pairing",
  "settings.refreshDevices": "Refresh devices",
  "settings.connectionMode": "Connection mode",
  "settings.connectionModeDescription": "Choose how TailSync discovers devices and manage secure pairing",
  "settings.modeAuto": "Automatic",
  "settings.modeLan": "Local network",
  "settings.modeIroh": "Iroh",
  "settings.modeTailscale": "Tailscale",
  "settings.devicePairing": "Device pairing",
  "settings.pairingClosed": "Currently closed",
  "settings.allowPairing": "Allow pairing",
  "settings.thisDevice": "This device",
  "settings.remotePairing": "Remote pairing",
  "settings.remotePairingDescription": "Pair devices over the internet via Iroh without port forwarding",
  "settings.createRemoteInvite": "Create invite link",
  "settings.createRemoteInviteDescription": "Create a one-time link on this device",
  "settings.useRemoteInvite": "Use invite link",
  "settings.useRemoteInviteDescription": "Paste the link from the other device",
  "settings.paired": "Paired",
  "settings.online": "Online",
  "settings.syncReady": "Sync ready",
};

function renderConnections(mode: "auto" | "lan_only" | "iroh_only" | "tailscale_only" = "auto") {
  const settings = {
    connection_mode: mode,
  } as SettingsData;

  const remotePairing = {
    invite: null,
    linkDraft: "",
    linkPreview: null,
    remotePairingBusy: false,
    remotePairingError: "",
    copied: false,
    handleCreateInvite: vi.fn(),
    handleLinkChange: vi.fn(),
    handleInspectLink: vi.fn(),
    handleStartRemotePairing: vi.fn(),
    handleCancelInvite: vi.fn(),
    handleCopyInvite: vi.fn(),
  };

  return render(
    <SettingsConnectionsSection
      settings={settings}
      t={(key) => labels[key] ?? key}
      devices={mockDevices as any}
      devicesLoading={false}
      devicesError=""
      pairingStatus={null}
      pairingBusy={false}
      connectionTests={{}}
      refreshDevices={vi.fn()}
      handleConnectionMode={vi.fn()}
      closePairing={vi.fn()}
      handleEnablePairing={vi.fn()}
      handleTestConnection={vi.fn()}
      handlePeerToggle={vi.fn()}
      handleForget={vi.fn()}
      openPairing={vi.fn()}
      remotePairing={remotePairing}
    />,
  );
}

describe("SettingsConnectionsSection", () => {
  it("renders connection mode caption and device list above remote pairing drawer", () => {
    const { container } = renderConnections("auto");

    // Check connection mode description
    expect(
      screen.getByText("Choose how TailSync discovers devices and manage secure pairing"),
    ).toBeInTheDocument();

    // Check that device-list comes before remote-pairing-panel in the DOM
    const deviceList = container.querySelector(".device-list");
    const remotePairing = container.querySelector(".remote-pairing-panel");
    expect(deviceList).toBeInTheDocument();
    expect(remotePairing).toBeInTheDocument();
    expect(
      deviceList!.compareDocumentPosition(remotePairing!) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("remote pairing drawer is collapsed by default and expands on click in auto mode", () => {
    renderConnections("auto");

    // Drawer header summary is visible
    expect(screen.getByText("Remote pairing")).toBeInTheDocument();
    expect(
      screen.getByText("Pair devices over the internet via Iroh without port forwarding"),
    ).toBeInTheDocument();

    // The inner create button should NOT be in the document while collapsed
    expect(screen.queryByText("Create a one-time link on this device")).not.toBeInTheDocument();

    // Click to expand drawer
    const drawerHeader = screen.getByRole("button", { name: /Remote pairing/i });
    fireEvent.click(drawerHeader);

    // Inner invite cards are now rendered
    expect(screen.getByText("Create a one-time link on this device")).toBeInTheDocument();
    expect(screen.getByText("Paste the link from the other device")).toBeInTheDocument();
  });

  it("hides the remote pairing drawer in lan_only and tailscale_only modes", () => {
    const { container: lanContainer } = renderConnections("lan_only");
    expect(lanContainer.querySelector(".remote-pairing-panel")).toBeNull();

    const { container: tailscaleContainer } = renderConnections("tailscale_only");
    expect(tailscaleContainer.querySelector(".remote-pairing-panel")).toBeNull();
  });

  it("shows the remote pairing drawer in iroh_only mode", () => {
    const { container } = renderConnections("iroh_only");
    expect(container.querySelector(".remote-pairing-panel")).not.toBeNull();
  });
});
