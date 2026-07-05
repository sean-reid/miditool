import * as path from "node:path";
import { expect, test, type Page } from "@playwright/test";

const SCENES = ["scrambled", "echoes", "xenakis clouds"];

const scene = (page: Page, name: string) =>
  page.getByTestId("scene").filter({ hasText: name });

test("loads: scenes appear, first is active, connection goes green", async ({ page }) => {
  await page.goto("/");
  const buttons = page.getByTestId("scene");
  await expect(buttons).toHaveText(SCENES);
  // data-active is set only from the server's status push.
  await expect(buttons.first()).toHaveAttribute("data-active", "1");
  await expect(page.getByTestId("conn-dot")).toHaveAttribute("data-state", "open");
});

test("switching scenes is confirmed by the server", async ({ page }) => {
  await page.goto("/");
  await expect(scene(page, "scrambled")).toHaveAttribute("data-active", "1");

  await scene(page, "echoes").click();

  // Wait for the server-driven marker, not the optimistic highlight.
  await expect(scene(page, "echoes")).toHaveAttribute("data-active", "1");
  await expect(scene(page, "scrambled")).not.toHaveAttribute("data-active", "1");
  await expect(scene(page, "echoes")).not.toHaveClass(/optimistic/);

  // Restore scene 0 so the suite is idempotent against a reused server.
  await scene(page, "scrambled").click();
  await expect(scene(page, "scrambled")).toHaveAttribute("data-active", "1");
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
