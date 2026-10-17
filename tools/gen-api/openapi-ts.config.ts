import { defineConfig } from "@hey-api/openapi-ts";

export default defineConfig({
  input: "../../crates/triage/openapi.json",
  output: "../../web/src/api",
  plugins: [
    // Resolved from this file; the generated client imports it as `../api-config`.
    { name: "@hey-api/client-fetch", runtimeConfigPath: "../../web/src/api-config", throwOnError: true },
    "@hey-api/typescript",
    "@hey-api/sdk",
    "@tanstack/react-query",
  ],
});
