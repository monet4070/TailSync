import { useCallback, useEffect, useRef, useState } from "react";
import {
  disable as disableSystemAutostart,
  enable as enableSystemAutostart,
  isEnabled as isSystemAutostartEnabled,
} from "@tauri-apps/plugin-autostart";

export interface LaunchAtLoginService {
  isEnabled: () => Promise<boolean>;
  enable: () => Promise<void>;
  disable: () => Promise<void>;
}

const systemLaunchAtLoginService: LaunchAtLoginService = {
  isEnabled: isSystemAutostartEnabled,
  enable: enableSystemAutostart,
  disable: disableSystemAutostart,
};

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  return "Unknown system error";
}

export function useLaunchAtLogin(
  service: LaunchAtLoginService = systemLaunchAtLoginService,
) {
  const [enabled, setEnabled] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const requestGeneration = useRef(0);
  const busyRef = useRef(false);

  const refresh = useCallback(async () => {
    if (busyRef.current) return;
    const generation = ++requestGeneration.current;
    setLoading(true);
    try {
      const current = await service.isEnabled();
      if (requestGeneration.current !== generation) return;
      setEnabled(current);
      setError("");
    } catch (cause) {
      if (requestGeneration.current !== generation) return;
      setError(errorMessage(cause));
    } finally {
      if (requestGeneration.current === generation) setLoading(false);
    }
  }, [service]);

  useEffect(() => {
    const initialRefresh = window.setTimeout(() => void refresh(), 0);
    const refreshOnFocus = () => void refresh();
    window.addEventListener("focus", refreshOnFocus);
    return () => {
      requestGeneration.current += 1;
      window.clearTimeout(initialRefresh);
      window.removeEventListener("focus", refreshOnFocus);
    };
  }, [refresh]);

  const setLaunchAtLogin = useCallback(async (nextEnabled: boolean) => {
    if (busyRef.current) return false;
    busyRef.current = true;
    const generation = ++requestGeneration.current;
    setBusy(true);
    setError("");
    try {
      if (nextEnabled) await service.enable();
      else await service.disable();

      const canonical = await service.isEnabled();
      if (canonical !== nextEnabled) {
        throw new Error("Windows reported that the startup registration did not change");
      }
      if (requestGeneration.current === generation) setEnabled(canonical);
      return true;
    } catch (cause) {
      try {
        const canonical = await service.isEnabled();
        if (requestGeneration.current === generation) setEnabled(canonical);
      } catch {
        // Keep the last known state when Windows cannot be queried either.
      }
      if (requestGeneration.current === generation) setError(errorMessage(cause));
      return false;
    } finally {
      busyRef.current = false;
      if (requestGeneration.current === generation) {
        setBusy(false);
        setLoading(false);
      }
    }
  }, [service]);

  return {
    enabled,
    loading,
    busy,
    error,
    refresh,
    setEnabled: setLaunchAtLogin,
  };
}
