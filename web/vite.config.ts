import { ValidateEnv } from "@julr/vite-plugin-validate-env";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  // Validation runs first so a bad env fails before any other plugin does work.
  plugins: [ValidateEnv(), react()],
});
