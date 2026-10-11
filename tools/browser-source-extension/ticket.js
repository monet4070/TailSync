export const MAX_TICKET_BYTES = 64 * 1024;

export function validateTicket(ticket, now = Date.now()) {
  const keys = ["kind", "version", "grant_id", "origin", "expires_at", "budget_bytes", "nodes"];
  if (!ticket || typeof ticket !== "object" || Object.keys(ticket).length !== keys.length
    || keys.some(key => !Object.hasOwn(ticket, key))
    || ticket.kind !== "tailsync-source-ticket" || ticket.version !== 1
    || typeof ticket.grant_id !== "string" || !/^[a-f0-9]{64}$/.test(ticket.grant_id)
    || !Number.isSafeInteger(ticket.expires_at) || ticket.expires_at * 1000 <= now
    || ticket.expires_at * 1000 > now + 300_000
    || !Number.isSafeInteger(ticket.budget_bytes) || ticket.budget_bytes <= 0 || ticket.budget_bytes > 16 * 1024 * 1024
    || !Array.isArray(ticket.nodes) || !ticket.nodes.length || ticket.nodes.length > 8) throw new Error("source_ticket_invalid");
  const origin = new URL(ticket.origin);
  if (!["http:", "https:"].includes(origin.protocol) || origin.username || origin.password || origin.origin !== ticket.origin) throw new Error("source_ticket_invalid");
  const indices = new Set();
  for (const node of ticket.nodes) {
    if (!node || Object.keys(node).length !== 2 || !Object.hasOwn(node, "index") || !Object.hasOwn(node, "src_sha256")
      || !Number.isSafeInteger(node.index) || node.index < 0 || node.index > 65535
      || typeof node.src_sha256 !== "string" || !/^[a-f0-9]{64}$/.test(node.src_sha256)
      || indices.has(node.index)) throw new Error("source_ticket_invalid");
    indices.add(node.index);
  }
  return ticket;
}
