import { ValidateEnv } from "@julr/vite-plugin-validate-env";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [ValidateEnv(), react()],
});
