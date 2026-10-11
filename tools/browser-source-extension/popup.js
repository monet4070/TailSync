import {
  collectInPage,
  SOURCE_CONTEXT_VERSION,
  SOURCE_PACKET_KIND,
  MAX_TOTAL_BYTES,
} from "./collect.js";
import { MAX_TICKET_BYTES, validateTicket } from "./ticket.js";

const fileInput = document.getElementById("ticket");
const scope = document.getElementById("scope");
const collectButton = document.getElementById("collect");
const status = document.getElementById("status");

let ticket = null;
let generation = 0;

function setStatus(message, ok = true) {
  status.textContent = message;
  status.dataset.ok = ok ? "true" : "false";
}

fileInput.addEventListener("change", async () => {
  const current = ++generation;
  ticket = null;
  collectButton.disabled = true;
  scope.textContent = "";
  const file = fileInput.files && fileInput.files[0];
  if (!file) return;
  try {
    if (file.size > MAX_TICKET_BYTES) throw new Error("source_ticket_too_large");
    const contents = await file.text();
    if (current !== generation) return;
    if (new TextEncoder().encode(contents).length > MAX_TICKET_BYTES) throw new Error("source_ticket_too_large");
    const parsed = validateTicket(JSON.parse(contents));
    ticket = parsed;
    scope.textContent = `${parsed.origin} · ${parsed.nodes.length} node(s) · expires ${new Date(
      Number(parsed.expires_at) * 1000,
    ).toISOString()}`;
    collectButton.disabled = false;
    setStatus("Ticket loaded. Open the source page, then click Collect.", true);
  } catch (error) {
    if (current !== generation) return;
    setStatus("Not a valid or unexpired TailSync source ticket.", false);
  }
});

collectButton.addEventListener("click", async () => {
  if (!ticket) return;
  const current = generation;
  const selectedTicket = ticket;
  collectButton.disabled = true;
  try {
    validateTicket(selectedTicket);
    const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
    const tab = tabs && tabs[0];
    if (!tab || tab.id === undefined || tab.id === null) throw new Error("no tab");
    const [originResult] = await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: () => location.origin,
    });
    const pageOrigin = originResult && originResult.result;
    if (current !== generation) return;
    validateTicket(selectedTicket);
    if (pageOrigin !== selectedTicket.origin) {
      setStatus("The active tab is not the authorized origin.", false);
      return;
    }
    const [collected] = await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: collectInPage,
      args: [selectedTicket.nodes, selectedTicket.origin, { maxTotalBytes: selectedTicket.budget_bytes }],
    });
    const outcome =
      collected && collected.result
        ? collected.result
        : { entries: [], failures: [{ index: 0, code: "source_collect_failed" }] };
    if (current !== generation) return;
    validateTicket(selectedTicket);
    const decodedTotal = outcome.entries.reduce((sum, entry) => sum + decodedLength(entry.png_b64), 0);
    if (decodedTotal > Math.min(MAX_TOTAL_BYTES, selectedTicket.budget_bytes)) {
      setStatus("Collected images exceed the packet budget.", false);
      return;
    }
    const packet = {
      kind: SOURCE_PACKET_KIND,
      version: SOURCE_CONTEXT_VERSION,
      grant_id: selectedTicket.grant_id,
      origin: selectedTicket.origin,
      entries: outcome.entries.map(({ index, src_sha256, png_b64 }) => ({ index, src_sha256, png_b64 })),
    };
    if (outcome.entries.length === 0) {
      setStatus("No authorized image could be collected; nothing was downloaded.", false);
      return;
    }
    downloadPacket(packet);
    if (outcome.failures.length > 0) {
      setStatus(
        `Packet downloaded with ${outcome.entries.length} image(s); ${outcome.failures.length} node(s) unavailable.`,
        true,
      );
    } else {
      setStatus(`Packet downloaded with ${outcome.entries.length} image(s).`, true);
    }
  } catch (error) {
    if (current !== generation) return;
    setStatus("Collection failed; nothing was downloaded.", false);
  } finally {
    if (current === generation) collectButton.disabled = false;
  }
});

function decodedLength(base64) {
  const padding = base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0;
  return Math.floor((base64.length * 3) / 4) - padding;
}

function downloadPacket(packet) {
  const blob = new Blob([JSON.stringify(packet)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = "tailsync-source-packet.json";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
