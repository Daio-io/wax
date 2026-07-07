import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  outputDir: ".wax/out/playwright-results",
  snapshotPathTemplate: "{testDir}/goldens/{arg}{ext}",
  reporter: [
    ["list"],
    ["html", { outputFolder: ".wax/out/playwright-report", open: "never" }],
  ],
  fullyParallel: false,
  use: {
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
      },
    },
  ],
});
