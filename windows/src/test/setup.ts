import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

const storedValues = new Map<string, string>();
const testStorage: Storage = {
  get length() {
    return storedValues.size;
  },
  clear() {
    storedValues.clear();
  },
  getItem(key) {
    return storedValues.get(key) ?? null;
  },
  key(index) {
    return [...storedValues.keys()][index] ?? null;
  },
  removeItem(key) {
    storedValues.delete(key);
  },
  setItem(key, value) {
    storedValues.set(key, String(value));
  },
};

// Node 26 exposes an incomplete experimental global when no storage file is
// configured. Bind a deterministic jsdom-compatible implementation for tests.
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: testStorage,
});
Object.defineProperty(window, "localStorage", {
  configurable: true,
  value: testStorage,
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  document.documentElement.lang = "";
  document.documentElement.removeAttribute("style");
  document.body.removeAttribute("style");
});
