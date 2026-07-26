import { useState, useEffect, useCallback } from "react";
import en from "../i18n/en.json";
import zhCN from "../i18n/zh-CN.json";

const messages: Record<string, Record<string, string>> = { en, "zh-CN": zhCN };
const STORAGE_KEY = "tailsync-lang";

export function useI18n() {
  const [locale, setLocaleState] = useState<string>(() => {
    return (
      localStorage.getItem(STORAGE_KEY) ||
      (navigator.language.startsWith("zh") ? "zh-CN" : "en")
    );
  });

  const t = useCallback(
    (key: string): string => {
      return messages[locale]?.[key] || messages["en"]?.[key] || key;
    },
    [locale],
  );

  const setLocale = useCallback((lang: string) => {
    setLocaleState(lang);
    localStorage.setItem(STORAGE_KEY, lang);
  }, []);

  // Pick up language changes from other windows (e.g. Settings → History)
  useEffect(() => {
    const handler = (e: StorageEvent) => {
      if (e.key === STORAGE_KEY && e.newValue) {
        setLocaleState(e.newValue);
      }
    };
    window.addEventListener("storage", handler);
    return () => window.removeEventListener("storage", handler);
  }, []);

  return { t, locale, setLocale };
}
