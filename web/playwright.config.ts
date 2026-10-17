import { defineConfig, devices } from "@playwright/test";

// Off 7311 and 7312 so a running `triage serve` or `just dev` does not collide with the test daemon.
const port = 7313;

export default defineConfig({
  testDir: "e2e",
  forbidOnly: true,
  reporter: "list",
  use: { baseURL: `http://127.0.0.1:${port}`, trace: "retain-on-failure" },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    // A fresh data dir per run, so the list starts empty. `exec` lets Playwright signal the daemon itself.
    command: 'exec ../target/release/triage serve --data-dir "$(mktemp -d)"',
    url: `http://127.0.0.1:${port}/health`,
    env: { TRIAGE_PORT: String(port) },
    reuseExistingServer: false,
  },
});
