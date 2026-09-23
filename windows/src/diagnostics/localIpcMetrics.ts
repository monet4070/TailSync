import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { normalizeIpcError } from "../utils/stableIpcError";

const MAX_DURATION_SAMPLES = 256;

interface CommandSamples {
  count: number;
  failures: number;
  durationsMs: number[];
}

function percentile(samples: number[], fraction: number): number | null {
  if (samples.length === 0) return null;
  const sorted = [...samples].sort((left, right) => left - right);
  return sorted[Math.ceil(sorted.length * fraction) - 1];
}

/** Diagnostic-only in-memory counters. Never retain IPC arguments or results. */
export class LocalIpcMetrics {
  private firstStartedMs: number | null = null;
  private readonly commands = new Map<string, CommandSamples>();

  record(command: string, startedMs: number, endedMs: number, succeeded: boolean): void {
    this.firstStartedMs = this.firstStartedMs === null
      ? startedMs
      : Math.min(this.firstStartedMs, startedMs);
    let samples = this.commands.get(command);
    if (!samples) {
      samples = { count: 0, failures: 0, durationsMs: [] };
      this.commands.set(command, samples);
    }
    samples.count += 1;
    if (!succeeded) samples.failures += 1;
    samples.durationsMs.push(Math.max(0, endedMs - startedMs));
    if (samples.durationsMs.length > MAX_DURATION_SAMPLES) samples.durationsMs.shift();
  }

  snapshot(nowMs: number): {
    schema: 1;
    elapsed_ms: number;
    commands: Array<{
      command: string;
      count: number;
      failures: number;
      calls_per_second: number;
      sampled_calls: number;
      p50_ms: number | null;
      p95_ms: number | null;
    }>;
  } {
    const elapsedMs = this.firstStartedMs === null ? 0 : Math.max(0, nowMs - this.firstStartedMs);
    return {
      schema: 1,
      elapsed_ms: elapsedMs,
      commands: [...this.commands.entries()]
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([command, samples]) => ({
          command,
          count: samples.count,
          failures: samples.failures,
          calls_per_second: elapsedMs > 0 ? samples.count * 1000 / elapsedMs : 0,
          sampled_calls: samples.durationsMs.length,
          p50_ms: percentile(samples.durationsMs, 0.50),
          p95_ms: percentile(samples.durationsMs, 0.95),
        })),
    };
  }
}

const metrics = new LocalIpcMetrics();

function invokeTauri<T>(command: string, args?: Parameters<typeof tauriInvoke>[1]): Promise<T> {
  return args === undefined ? tauriInvoke<T>(command) : tauriInvoke<T>(command, args);
}

/** Only an explicitly opted-in diagnostic build collects IPC timings. */
export function getLocalIpcMetrics(): ReturnType<LocalIpcMetrics["snapshot"]> | null {
  return import.meta.env.VITE_TAILSYNC_DIAGNOSTICS === "1"
    ? metrics.snapshot(performance.now())
    : null;
}

export function invoke<T>(
  command: string,
  args?: Parameters<typeof tauriInvoke>[1],
): Promise<T> {
  if (import.meta.env.VITE_TAILSYNC_DIAGNOSTICS !== "1") {
    return invokeTauri<T>(command, args).catch(error => { throw normalizeIpcError(error); });
  }
  const startedMs = performance.now();
  return invokeTauri<T>(command, args).then(
    value => {
      metrics.record(command, startedMs, performance.now(), true);
      return value;
    },
    error => {
      metrics.record(command, startedMs, performance.now(), false);
      throw normalizeIpcError(error);
    },
  );
}
