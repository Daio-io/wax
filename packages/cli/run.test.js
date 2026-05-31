const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { spawnSync } = require("node:child_process");

const packageRoot = __dirname;
const binDir = path.join(packageRoot, "bin");
const waxPath = path.join(binDir, "wax");
const runPath = path.join(packageRoot, "run.js");

function removeWaxBin() {
  fs.rmSync(binDir, { recursive: true, force: true });
}

test("run.js reports reinstall guidance when wax binary is missing", () => {
  removeWaxBin();

  const result = spawnSync(process.execPath, [runPath, "--help"], {
    encoding: "utf8",
  });

  assert.equal(result.status, 1);
  assert.match(result.stderr, /wax binary is missing/);
  assert.match(result.stderr, /npm install -g @wax\/cli/);
  assert.match(result.stderr, /WAX_CLI_SKIP_DOWNLOAD=1/);
});

test("run.js forwards arguments and exit code to installed wax binary", () => {
  removeWaxBin();
  fs.mkdirSync(binDir, { recursive: true });
  fs.writeFileSync(
    waxPath,
    "#!/usr/bin/env sh\nprintf 'args:%s:%s\\n' \"$1\" \"$2\"\nexit 7\n"
  );
  fs.chmodSync(waxPath, 0o755);

  try {
    const result = spawnSync(process.execPath, [runPath, "scan", "--no-auto-install"], {
      encoding: "utf8",
    });

    assert.equal(result.status, 7);
    assert.equal(result.stdout, "args:scan:--no-auto-install\n");
  } finally {
    removeWaxBin();
  }
});
