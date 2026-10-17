import { ValidateEnv } from "@julr/vite-plugin-validate-env";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  // The router plugin must precede react() so route files are split before JSX is transformed.
  plugins: [ValidateEnv(), tanstackRouter({ target: "react", autoCodeSplitting: true }), react()],
});
