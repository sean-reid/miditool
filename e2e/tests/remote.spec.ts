import * as path from "node:path";
import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";

const SCENES = ["scrambled", "echoes", "xenakis clouds"];

const scene = (page: Page, name: string) =>
  page.getByTestId("scene").filter({ hasText: name });

test("loads: scenes appear, first is active, connection goes green", async ({ page }) => {
  await page.goto("/");
  const buttons = page.getByTestId("scene");
  await expect(buttons).toHaveText(SCENES);
  // data-active is set only from the server's status push; aria-pressed
  // is the programmatic active state assistive tech sees.
  await expect(buttons.first()).toHaveAttribute("data-active", "1");
  await expect(buttons.first()).toHaveAttribute("aria-pressed", "true");
  await expect(buttons.nth(1)).toHaveAttribute("aria-pressed", "false");
  const dot = page.getByTestId("conn-dot");
  await expect(dot).toHaveAttribute("data-state", "open");
  await expect(dot).toHaveAttribute("aria-label", "connected");
  await expect(dot).toHaveText("connected");
});

test("switching scenes is confirmed by the server", async ({ page }) => {
  await page.goto("/");
  await expect(scene(page, "scrambled")).toHaveAttribute("data-active", "1");

  await scene(page, "echoes").click();

  // Wait for the server-driven marker, not the optimistic highlight.
  await expect(scene(page, "echoes")).toHaveAttribute("data-active", "1");
  await expect(scene(page, "echoes")).toHaveAttribute("aria-pressed", "true");
  await expect(scene(page, "scrambled")).not.toHaveAttribute("data-active", "1");
  await expect(scene(page, "scrambled")).toHaveAttribute("aria-pressed", "false");
  await expect(scene(page, "echoes")).not.toHaveClass(/optimistic/);

  // Restore scene 0 so the suite is idempotent against a reused server.
  await scene(page, "scrambled").click();
  await expect(scene(page, "scrambled")).toHaveAttribute("data-active", "1");
});

test("disconnect deadens the controls; reconnect revives them", async ({ page }) => {
  // Proxy the WebSocket so the test can cut the line without touching
  // the dev server other workers share.
  let refuse = false;
  let live: WebSocketRoute | undefined;
  await page.routeWebSocket(/\/ws$/, (ws) => {
    if (refuse) {
      ws.close();
      return;
    }
    live = ws;
    ws.connectToServer();
  });

  await page.goto("/");
  const dot = page.getByTestId("conn-dot");
  const scenes = page.getByTestId("scene");
  const panic = page.getByTestId("panic");
  await expect(dot).toHaveAttribute("data-state", "open");
  await expect(scenes.first()).toBeEnabled();
  await expect(panic).toBeEnabled();

  refuse = true;
  live!.close();

  await expect(dot).toHaveAttribute("data-state", "closed");
  await expect(dot).toHaveAttribute("aria-label", "reconnecting");
  await expect(dot).toHaveText("reconnecting");
  await expect(page.getByTestId("conn-label")).toBeVisible();
  for (const button of await scenes.all()) await expect(button).toBeDisabled();
  await expect(panic).toBeDisabled();
  // A tap while dead must not paint an optimistic press.
  await scenes.nth(1).click({ force: true });
  await expect(scenes.nth(1)).not.toHaveClass(/optimistic/);

  refuse = false;

  // Reconnect (backoff tops out at 5s) resyncs from the status frame.
  await expect(dot).toHaveAttribute("data-state", "open", { timeout: 15_000 });
  await expect(scenes.first()).toBeEnabled();
  await expect(panic).toBeEnabled();
  await expect(scenes.first()).toHaveAttribute("data-active", "1");
});

test("panic reaches the backend and shows up in the monitor", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("panic").click();
  await expect(page.getByTestId("monitor")).toContainText("panic");
  await expect(page.getByTestId("monitor")).toContainText("all notes off");
});

test("monitor accumulates events, newest first", async ({ page }) => {
  await page.goto("/");
  const rows = page.getByTestId("event-row");
  await expect(rows.first()).toBeVisible({ timeout: 10_000 });

  const before = await rows.count();
  await page.waitForTimeout(2500);
  const after = await rows.count();
  expect(after).toBeGreaterThan(before);

  // Rows carry the backend timestamp; the top row must be the newest.
  const newest = Number(await rows.first().getAttribute("data-t"));
  const oldest = Number(await rows.last().getAttribute("data-t"));
  expect(newest).toBeGreaterThan(oldest);
});

test("space holds the monitor, space again releases it", async ({ page }) => {
  await page.goto("/");
  const rows = page.getByTestId("event-row");
  await expect(rows.first()).toBeVisible({ timeout: 10_000 });

  await page.getByTestId("monitor").focus();
  await page.keyboard.press("Space");
  const heldTop = await rows.first().getAttribute("data-t");
  await page.waitForTimeout(2000);
  expect(await rows.first().getAttribute("data-t")).toBe(heldTop);

  await page.keyboard.press("Space");
  await expect
    .poll(() => rows.first().getAttribute("data-t"), { timeout: 10_000 })
    .not.toBe(heldTop);
});

test("screenshots at phone and desktop sizes", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("scene").first()).toBeVisible();
  // Let a few monitor rows land so the panel is not empty.
  await expect(page.getByTestId("event-row").first()).toBeVisible({ timeout: 10_000 });

  const shot = (name: string) => path.join(__dirname, "..", "screenshots", name);
  await page.setViewportSize({ width: 375, height: 812 });
  await page.screenshot({ path: shot("remote-375x812.png"), fullPage: true });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.screenshot({ path: shot("remote-1280x800.png"), fullPage: true });
});
