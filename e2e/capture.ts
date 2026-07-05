// Regenerates the docs screenshots of the web remote.
//
// Usage, from e2e/:
//
//     npm install
//     npx playwright install chromium
//     npm run screenshots
//
// The script starts the remote's dev server (the fake backend, no MIDI
// needed) with `cargo run -p miditool-remote --example dev -- 8321`,
// captures the phone, tablet, and desktop viewports into
// docs/src/assets/remote/, and exits. A dev server already running on
// port 8321 is reused. The first run may compile the Rust tree, so give
// it a few minutes.

import { spawn, type ChildProcess } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
import { chromium } from "@playwright/test";

const PORT = 8321;
const BASE = `http://localhost:${PORT}`;
const REPO = path.join(import.meta.dirname, "..");
const OUT_DIR = path.join(REPO, "docs", "src", "assets", "remote");

const VIEWPORTS = [
  { width: 375, height: 812 }, // phone
  { width: 820, height: 1180 }, // tablet
  { width: 1280, height: 800 }, // desktop
];

async function healthy(): Promise<boolean> {
  try {
    const res = await fetch(`${BASE}/health`);
    return res.ok;
  } catch {
    return false;
  }
}

async function waitForServer(timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await healthy()) return;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(`dev server did not answer at ${BASE}/health`);
}

async function main(): Promise<void> {
  let server: ChildProcess | undefined;
  if (await healthy()) {
    console.log(`reusing the dev server at ${BASE}`);
  } else {
    console.log("starting the dev server (first run may compile)...");
    server = spawn(
      "cargo",
      ["run", "-p", "miditool-remote", "--example", "dev", "--", String(PORT)],
      { cwd: REPO, stdio: "inherit" },
    );
    // Compiling the whole tree can take a while on a cold target dir.
    await waitForServer(300_000);
  }

  fs.mkdirSync(OUT_DIR, { recursive: true });
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.goto(BASE);
    // Wait for real content: the scene keys and a few monitor rows.
    await page.getByTestId("scene").first().waitFor();
    await page.getByTestId("event-row").first().waitFor({ timeout: 15_000 });
    await page.waitForTimeout(2_000);

    for (const viewport of VIEWPORTS) {
      const name = `remote-${viewport.width}x${viewport.height}.png`;
      await page.setViewportSize(viewport);
      await page.screenshot({ path: path.join(OUT_DIR, name), fullPage: true });
      console.log(`wrote ${path.join(OUT_DIR, name)}`);
    }
  } finally {
    await browser.close();
    server?.kill();
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
