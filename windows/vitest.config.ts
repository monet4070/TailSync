import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Native UI prototypes use node:test and have their own Node entry point.
    include: ["src/**/*.{test,spec}.{js,mjs,cjs,ts,tsx}"],
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    clearMocks: true,
    restoreMocks: true,
  },
});
