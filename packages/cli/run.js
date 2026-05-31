#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const binaryPath = path.join(__dirname, "bin", process.platform === "win32" ? "wax.exe" : "wax");

if (!fs.existsSync(binaryPath)) {
  console.error("wax binary is missing from this @wax/cli installation.");
  console.error("");
  console.error("Reinstall the package to download the host binary:");
  console.error("  npm install -g @wax/cli");
  console.error("");
  console.error("If npm lifecycle scripts were disabled, reinstall without --ignore-scripts.");
  console.error("If WAX_CLI_SKIP_DOWNLOAD=1 was intentional, provide packages/cli/bin/wax before running.");
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
});

if (result.error) {
  console.error(`failed to run wax: ${result.error.message}`);
  process.exit(1);
}

if (result.signal) {
  process.kill(process.pid, result.signal);
} else {
  process.exit(result.status === null ? 1 : result.status);
}
