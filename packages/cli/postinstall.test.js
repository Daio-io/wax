const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const { spawnSync } = require("node:child_process");

const { download, expectedSha256, refreshInstalledLanguages, validateArchive } = require("./postinstall");

const digest = "a".repeat(64);
const version = "0.1.0-alpha.1";
const target = "aarch64-apple-darwin";
const expectedDir = `wax-${version}-${target}`;
const expectedMember = `${expectedDir}/wax`;

function createArchive(tmpDir, entries) {
  for (const entry of entries) {
    const entryPath = path.join(tmpDir, entry.path);
    fs.mkdirSync(path.dirname(entryPath), { recursive: true });
    if (entry.type === "symlink") {
      fs.symlinkSync(entry.target, entryPath);
    } else {
      fs.writeFileSync(entryPath, entry.content || "");
      if (entry.executable) {
        fs.chmodSync(entryPath, 0o755);
      }
    }
  }

  const archivePath = path.join(tmpDir, `${expectedDir}.tar.gz`);
  const result = spawnSync("tar", ["-czf", archivePath, "-C", tmpDir, expectedDir], {
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
  return archivePath;
}

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

test("download copies file URLs", async () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "wax-cli-test-"));
  try {
    const source = path.join(tmpDir, "source.txt");
    const destination = path.join(tmpDir, "destination.txt");
    fs.writeFileSync(source, "wax");

    await download(new URL(`file://${source}`).toString(), destination);

    assert.equal(fs.readFileSync(destination, "utf8"), "wax");
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("refreshInstalledLanguages runs `wax language update --all` and warns on failure", () => {
  const warnings = [];
  const calls = [];
  const createdDirs = [];
  const removedDirs = [];

  refreshInstalledLanguages("/tmp/wax", {
    log: (message) => warnings.push(message),
    makeTempDir() {
      const dir = "/tmp/wax-neutral-repo-root";
      createdDirs.push(dir);
      return dir;
    },
    removeTempDir(dir) {
      removedDirs.push(dir);
    },
    spawnSync(command, args, options) {
      calls.push({ command, args, options });
      return {
        status: 23,
        stderr: "network unavailable\n",
      };
    },
  });

  assert.deepEqual(calls, [
    {
      command: "/tmp/wax",
      args: ["language", "update", "--all", "--repo-root", "/tmp/wax-neutral-repo-root"],
      options: {
        cwd: "/tmp/wax-neutral-repo-root",
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
      },
    },
  ]);
  assert.deepEqual(createdDirs, ["/tmp/wax-neutral-repo-root"]);
  assert.deepEqual(removedDirs, ["/tmp/wax-neutral-repo-root"]);
  assert.match(warnings[0], /Warning: unable to refresh installed wax language packs/);
  assert.match(warnings[0], /network unavailable/);
});

test("postinstall skip download path exits before attempting refresh", () => {
  const packageDir = __dirname;

  const result = spawnSync(process.execPath, ["postinstall.js"], {
    cwd: packageDir,
    encoding: "utf8",
    env: {
      ...process.env,
      WAX_CLI_SKIP_DOWNLOAD: "1",
      WAX_CLI_RELEASE_BASE_URL: "http://127.0.0.1:9/should-not-be-used",
    },
  });

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Skipping wax binary download because WAX_CLI_SKIP_DOWNLOAD=1/);
  assert.equal(result.stderr, "");
});

test("validateArchive accepts expected release archive shape", () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "wax-cli-test-"));
  try {
    const archivePath = createArchive(tmpDir, [
      { path: expectedMember, content: "#!/bin/sh\n", executable: true },
    ]);

    assert.doesNotThrow(() => validateArchive(archivePath, tmpDir, expectedDir, expectedMember));
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("validateArchive rejects archives missing wax binary", () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "wax-cli-test-"));
  try {
    const archivePath = createArchive(tmpDir, [
      { path: `${expectedDir}/README.txt`, content: "not wax" },
    ]);

    assert.throws(
      () => validateArchive(archivePath, tmpDir, expectedDir, expectedMember),
      /archive is missing expected entry: wax-0.1.0-alpha.1-aarch64-apple-darwin\/wax/
    );
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("validateArchive rejects archives with unexpected entries", () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "wax-cli-test-"));
  try {
    const archivePath = createArchive(tmpDir, [
      { path: expectedMember, content: "#!/bin/sh\n", executable: true },
      { path: `${expectedDir}/extra`, content: "nope" },
    ]);

    assert.throws(
      () => validateArchive(archivePath, tmpDir, expectedDir, expectedMember),
      /archive contains unexpected entries: wax-0.1.0-alpha.1-aarch64-apple-darwin\/extra/
    );
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("validateArchive rejects symlink wax entries before extraction", () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "wax-cli-test-"));
  try {
    const archivePath = createArchive(tmpDir, [
      { path: expectedMember, type: "symlink", target: "/tmp/not-wax" },
    ]);

    assert.throws(
      () => validateArchive(archivePath, tmpDir, expectedDir, expectedMember),
      /archive entry is not a regular file: wax-0.1.0-alpha.1-aarch64-apple-darwin\/wax/
    );
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
