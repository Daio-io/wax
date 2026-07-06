import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  outputDir: ".wax/out/playwright-results",
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
