const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { spawnSync } = require("node:child_process");

const { download, expectedSha256, validateArchive } = require("./postinstall");

const digest = "a".repeat(64);

test("expectedSha256 accepts checksum line for requested archive", () => {
  assert.equal(expectedSha256(`${digest}  wax-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz\n`, "wax-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz"), digest);
});

test("expectedSha256 accepts checksum line with absolute archive path", () => {
  assert.equal(expectedSha256(`${digest}  /tmp/release/wax-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz\n`, "wax-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz"), digest);
});

test("expectedSha256 rejects checksum line for another archive", () => {
  assert.throws(
    () => expectedSha256(`${digest}  wax-0.1.0-alpha.1-x86_64-apple-darwin.tar.gz\n`, "wax-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz"),
    /checksum file did not contain a sha256 for wax-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz/
  );
});

test("download rejects plaintext http URLs", async () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "wax-cli-test-"));
  try {
    await assert.rejects(
      download("http://example.invalid/wax.tar.gz", path.join(tmpDir, "wax.tar.gz")),
      /unsupported download URL protocol: http:/
    );
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("validateArchive rejects symlink wax entries before extraction", () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "wax-cli-test-"));
  try {
    const version = "0.1.0-alpha.1";
    const target = "aarch64-apple-darwin";
    const expectedDir = `wax-${version}-${target}`;
    const expectedMember = `${expectedDir}/wax`;
    const stageDir = path.join(tmpDir, expectedDir);
    const archivePath = path.join(tmpDir, `${expectedDir}.tar.gz`);

    fs.mkdirSync(stageDir);
    fs.symlinkSync("/tmp/not-wax", path.join(stageDir, "wax"));
    const result = spawnSync("tar", ["-czf", archivePath, "-C", tmpDir, expectedDir], {
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);

    assert.throws(
      () => validateArchive(archivePath, tmpDir, expectedDir, expectedMember),
      /archive entry is not a regular file: wax-0.1.0-alpha.1-aarch64-apple-darwin\/wax/
    );
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
