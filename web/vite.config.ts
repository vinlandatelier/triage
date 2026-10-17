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
  resolve: {
    // Reads the `@/*` alias from tsconfig.json, so there is one definition.
    tsconfigPaths: true,
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
  },
});
