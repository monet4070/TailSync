export const SOURCE_TICKET_KIND = "tailsync-source-ticket";
export const SOURCE_PACKET_KIND = "tailsync-source-packet";
export const SOURCE_CONTEXT_VERSION = 1;
export const MAX_NODE_BYTES = 8 * 1024 * 1024;
export const MAX_TOTAL_BYTES = 16 * 1024 * 1024;

export async function collectInPage(nodes, pageOrigin, limits = {}) {
  const clamp = (value, maximum) => Number.isSafeInteger(value) && value > 0 ? Math.min(value, maximum) : maximum;
  const maxNodeBytes = 8 * 1024 * 1024;
  const maxTotalBytes = clamp(limits.maxTotalBytes, 16 * 1024 * 1024);
  const requestTimeoutMs = clamp(limits.requestTimeoutMs, 10_000);
  const deadline = Date.now() + clamp(limits.totalDeadlineMs, 30_000);
  const entries = [];
  const failures = [];
  const validNodes = Array.isArray(nodes) && nodes.length > 0 && nodes.length <= 8
    && nodes.every(node => Number.isSafeInteger(node.index) && node.index >= 0 && node.index <= 65535
      && /^[a-f0-9]{64}$/.test(node.src_sha256))
    && new Set(nodes.map(node => node.index)).size === nodes.length;
  if (!validNodes) return { entries, failures: [{ index: 0, code: "source_ticket_invalid" }] };
  const failAll = code => ({ entries: [], failures: nodes.map(node => ({ index: node.index, code })) });
  const canonicalOrigin = value => {
    try {
      const parsed = new URL(value);
      const effective = parsed.protocol === "blob:" ? new URL(parsed.pathname) : parsed;
      if (!["http:", "https:"].includes(effective.protocol) || effective.username || effective.password) return null;
      return effective.origin;
    } catch {
      return null;
    }
  };
  if (canonicalOrigin(pageOrigin) !== pageOrigin || location.origin !== pageOrigin) return failAll("source_origin_mismatch");
  const bounded = async (promise, until, controller) => {
    let timer;
    try {
      return await Promise.race([
        promise,
        new Promise((resolve, reject) => {
          timer = setTimeout(() => {
            controller?.abort();
            reject(new Error("source_deadline"));
          }, Math.max(0, until - Date.now()));
        }),
      ]);
    } finally {
      clearTimeout(timer);
    }
  };
  const sha256Hex = async value => {
    const bytes = new TextEncoder().encode(value);
    const digest = new Uint8Array(await bounded(crypto.subtle.digest("SHA-256", bytes), deadline));
    return Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join("");
  };
  const staticPng = bytes => {
    const signature = [137, 80, 78, 71, 13, 10, 26, 10];
    if (bytes.length < 45 || signature.some((byte, index) => bytes[index] !== byte)) return false;
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    let offset = 8;
    let chunks = 0;
    let hasData = false;
    while (offset + 12 <= bytes.length && chunks < 4096) {
      const length = view.getUint32(offset);
      if (length > bytes.length - offset - 12) return false;
      const type = String.fromCharCode(...bytes.subarray(offset + 4, offset + 8));
      if (chunks === 0 && (type !== "IHDR" || length !== 13 || view.getUint32(offset + 8) === 0 || view.getUint32(offset + 12) === 0)) return false;
      if (["acTL", "fcTL", "fdAT"].includes(type)) return false;
      if (type === "IDAT") hasData = true;
      offset += 12 + length;
      chunks += 1;
      if (type === "IEND") return length === 0 && offset === bytes.length && hasData;
    }
    return false;
  };
  const encodeBase64 = bytes => {
    const parts = [];
    for (let offset = 0; offset < bytes.length; offset += 48 * 1024) {
      parts.push(btoa(String.fromCharCode(...bytes.subarray(offset, offset + 48 * 1024))));
    }
    return parts.join("");
  };
  const elements = document.querySelectorAll("img");
  if (elements.length > 4096) return failAll("source_dom_limit");
  let readTotal = 0;
  let scannedBytes = 0;
  const candidates = [];
  try {
    for (const element of elements) {
      const raw = element.getAttribute("src") || "";
      const resolved = element.src || raw;
      scannedBytes += raw.length + resolved.length;
      if (raw.length > 16_384 || resolved.length > 16_384 || scannedBytes > 1024 * 1024) return failAll("source_dom_limit");
      candidates.push({ element, raw, resolved, rawHash: raw ? await sha256Hex(raw) : null, resolvedHash: resolved ? await sha256Hex(resolved) : null });
    }
    for (const node of nodes) {
      if (Date.now() >= deadline) {
        failures.push({ index: node.index, code: "source_deadline" });
        continue;
      }
      if (location.origin !== pageOrigin) return failAll("source_origin_mismatch");
      const matches = candidates.filter(candidate => candidate.rawHash === node.src_sha256 || candidate.resolvedHash === node.src_sha256);
      if (matches.length !== 1) {
        failures.push({ index: node.index, code: matches.length ? "source_ambiguous" : "source_node_missing" });
        continue;
      }
      const { element, raw, resolved } = matches[0];
      const selected = element.currentSrc || element.src || raw;
      if (element.getAttribute("src") !== raw || (element.src || raw) !== resolved || selected !== resolved) {
        failures.push({ index: node.index, code: "source_reference_changed" });
        continue;
      }
      if (canonicalOrigin(resolved) !== pageOrigin) {
        failures.push({ index: node.index, code: "source_origin_mismatch" });
        continue;
      }
      if (readTotal >= maxTotalBytes) {
        failures.push({ index: node.index, code: "source_total_budget" });
        continue;
      }
      const controller = new AbortController();
      const until = Math.min(deadline, Date.now() + requestTimeoutMs);
      let reader;
      try {
        const response = await bounded(fetch(resolved, { credentials: "same-origin", redirect: "error", signal: controller.signal }), until, controller);
        if (!response.ok || !response.body || (response.url && canonicalOrigin(response.url) !== pageOrigin)) throw new Error("source_fetch_failed");
        const mime = (response.headers.get("content-type") || "").split(";")[0].trim().toLowerCase();
        if (mime !== "image/png") throw new Error("source_mime");
        const maximum = Math.min(maxNodeBytes, maxTotalBytes - readTotal);
        reader = response.body.getReader();
        const chunks = [];
        let length = 0;
        for (;;) {
          const { done, value } = await bounded(reader.read(), until, controller);
          if (done) break;
          readTotal += value.length;
          length += value.length;
          if (length > maximum) throw new Error(maximum < maxNodeBytes ? "source_total_budget" : "source_resource_too_large");
          chunks.push(value);
        }
        const bytes = new Uint8Array(length);
        let offset = 0;
        for (const chunk of chunks) {
          bytes.set(chunk, offset);
          offset += chunk.length;
        }
        if (!staticPng(bytes)) throw new Error("source_decode");
        if (location.origin !== pageOrigin) return failAll("source_origin_mismatch");
        if (Date.now() >= deadline) throw new Error("source_deadline");
        entries.push({ index: node.index, src_sha256: node.src_sha256, png_b64: encodeBase64(bytes) });
      } catch (error) {
        const stable = ["source_deadline", "source_fetch_failed", "source_mime", "source_total_budget", "source_resource_too_large", "source_decode"];
        failures.push({ index: node.index, code: stable.includes(error.message) ? error.message : "source_fetch_failed" });
      } finally {
        controller.abort();
        if (reader) void reader.cancel().catch(() => {});
      }
    }
    return { entries, failures };
  } catch {
    return failAll("source_deadline");
  }
}
