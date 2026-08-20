import type { PeerRoute } from "../tailsyncClient";

export const routeSupportsLatencyTest = (route: PeerRoute) => (
  route.interface !== "iroh" || route.rtt_capable === true
);
