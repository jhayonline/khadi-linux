import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// jsdom, not the browser: these assert what the panels PUT ON SCREEN from a
// given set of numbers. Nothing here needs a GPU, a compositor or a webview,
// so the suite runs in CI and in under a second.
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["src/__tests__/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
