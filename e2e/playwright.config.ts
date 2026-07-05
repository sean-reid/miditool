import * as path from "node:path";
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  timeout: 30_000,
  use: {
    baseURL: "http://localhost:8321",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "cargo run -p miditool-remote --example dev -- 8321",
    cwd: path.join(__dirname, ".."),
    url: "http://localhost:8321/health",
    reuseExistingServer: true,
    // The first run may compile the whole dependency tree.
    timeout: 300_000,
  },
});
