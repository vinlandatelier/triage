/// <reference types="vitest/config" />
import { ValidateEnv } from "@julr/vite-plugin-validate-env";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  // The router plugin must precede react() so route files are split before JSX is transformed.
  plugins: [
    ValidateEnv(),
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    react(),
    tailwindcss(),
  ],
  // `just dev` passes --port from TRIAGE_DEV_PORT and runs the daemon on 7312.
  server: {
    port: 7311,
    strictPort: true,
    proxy: { "/api": "http://127.0.0.1:7312" },
  },
  resolve: {
    // Reads the `@/*` alias from tsconfig.json, so there is one definition.
    tsconfigPaths: true,
  },
  test: {
    environment: "jsdom",
    // Playwright owns e2e/*.spec.ts.
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["./src/test/setup.ts"],
    coverage: { exclude: ["src/api/**"] },
  },
});
