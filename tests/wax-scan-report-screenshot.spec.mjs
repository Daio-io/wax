import { expect, test } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const artifactDir = path.resolve(
  process.env.WAX_SCAN_REPORT_ARTIFACT_DIR || path.join(root, ".wax/out/report-screenshots"),
);
const reportHtml = path.join(artifactDir, "index.html");

test.beforeAll(() => {
  rmSync(artifactDir, { recursive: true, force: true });
  mkdirSync(artifactDir, { recursive: true });
  execFileSync(
    path.join(root, "scripts/render-wax-scan-fixture-report.sh"),
    ["--repo-name", "wax-screenshot-test", reportHtml],
    { cwd: root, stdio: "inherit" },
  );
});

async function openReport(page, viewport) {
  await page.setViewportSize(viewport);
  await page.goto(pathToFileURL(reportHtml).href);
  await expect(page.getByRole("heading", { name: "Design System Adoption" })).toBeVisible();
  await expect(page.locator(".report-logo svg")).toBeVisible();
}

async function expectBumblebeeTheme(page) {
  const theme = await page.evaluate(() => {
    const rootStyle = getComputedStyle(document.documentElement);
    return {
      bg: rootStyle.getPropertyValue("--bg").trim(),
      accent: rootStyle.getPropertyValue("--accent").trim(),
    };
  });

  expect(theme.bg).toBe("#000000");
  expect(theme.accent).toBe("#FCC457");
}

async function verifyScreenshot(page, testInfo, name) {
  const artifactPath = path.join(artifactDir, name);
  await page.screenshot({
    path: artifactPath,
    animations: "disabled",
  });

  await testInfo.attach(name, {
    path: artifactPath,
    contentType: "image/png",
  });

  await expect(page).toHaveScreenshot(name, {
    animations: "disabled",
    maxDiffPixelRatio: 0.01,
  });
}

test("desktop screenshot keeps the current report layout with the black and bumblebee theme", async ({ page }, testInfo) => {
  await openReport(page, { width: 1440, height: 1100 });
  await expectBumblebeeTheme(page);

  const logoBox = await page.locator(".report-logo").boundingBox();
  expect(logoBox).not.toBeNull();
  expect(logoBox.x).toBeGreaterThan(1200);

  await expect(page.locator("h2")).toContainText([
    "Design system component usage",
    "Unused design system components",
    "Adoption by area",
    "Adoption gaps",
    "Candidates to bring into the design system",
    "Key findings",
  ]);
  await verifyScreenshot(page, testInfo, "wax-scan-report-desktop.png");
});
