#!/usr/bin/env node

const crypto = require("node:crypto");
const fs = require("node:fs");
const http = require("node:http");
const https = require("node:https");
const os = require("node:os");
const path = require("node:path");
const { pipeline } = require("node:stream");
const { spawnSync } = require("node:child_process");

const PACKAGE_ROOT = __dirname;
const PACKAGE_JSON = require("./package.json");
const DEFAULT_REPO = "Daio-io/wax";
const MAX_REDIRECTS = 5;
const DOWNLOAD_TIMEOUT_MS = 60_000;

function fail(message) {
  console.error(`wax postinstall error: ${message}`);
  console.error("");
  console.error("Try reinstalling with:");
  console.error("  npm install -g @wax/cli");
  console.error("");
  console.error("Or install via curl:");
  console.error("  curl -fsSL https://raw.githubusercontent.com/Daio-io/wax/main/scripts/install.sh | bash");
  process.exit(1);
}

function targetTriple() {
  const osPart = {
    darwin: "apple-darwin",
    linux: "unknown-linux-gnu",
  }[process.platform];

  const archPart = {
    arm64: "aarch64",
    x64: "x86_64",
  }[process.arch];

  if (!osPart || !archPart) {
    fail(
      `unsupported host ${process.platform}/${process.arch}. Supported hosts: darwin/linux on x64 or arm64.`
    );
  }

  return `${archPart}-${osPart}`;
}

function versionTag() {
  const version = process.env.WAX_CLI_VERSION || PACKAGE_JSON.version;
  return `v${version.replace(/^v/, "")}`;
}

function releaseBaseUrl(repo, tag) {
  if (process.env.WAX_CLI_RELEASE_BASE_URL) {
    return process.env.WAX_CLI_RELEASE_BASE_URL.replace(/\/+$/, "");
  }
  return `https://github.com/${repo}/releases/download/${tag}`;
}

function download(url, destination, redirectsLeft = MAX_REDIRECTS) {
  return new Promise((resolve, reject) => {
    const parsedUrl = new URL(url);

    if (parsedUrl.protocol === "file:") {
      fs.copyFile(parsedUrl, destination, (error) => {
        if (error) {
          reject(error);
        } else {
          resolve();
        }
      });
      return;
    }

    if (parsedUrl.protocol !== "http:" && parsedUrl.protocol !== "https:") {
      reject(new Error(`unsupported download URL protocol: ${parsedUrl.protocol}`));
      return;
    }

    const client = parsedUrl.protocol === "http:" ? http : https;
    const request = client.get(
      url,
      {
        headers: {
          "user-agent": `@wax/cli/${PACKAGE_JSON.version}`,
        },
      },
      (response) => {
        const status = response.statusCode || 0;
        const location = response.headers.location;

        if (status >= 300 && status < 400 && location) {
          response.resume();
          if (redirectsLeft <= 0) {
            reject(new Error(`too many redirects while downloading ${url}`));
            return;
          }
          const redirectedUrl = new URL(location, url).toString();
          download(redirectedUrl, destination, redirectsLeft - 1).then(resolve, reject);
          return;
        }

        if (status < 200 || status >= 300) {
          response.resume();
          reject(new Error(`download failed (${status}) for ${url}`));
          return;
        }

        const file = fs.createWriteStream(destination, { mode: 0o600 });
        pipeline(response, file, (error) => {
          if (error) {
            reject(new Error(`download stream failed for ${url}: ${error.message}`));
          } else {
            resolve();
          }
        });
      }
    );

    request.setTimeout(DOWNLOAD_TIMEOUT_MS, () => {
      request.destroy(new Error(`download timed out after ${DOWNLOAD_TIMEOUT_MS / 1000}s for ${url}`));
    });
    request.on("error", reject);
  });
}

function sha256(filePath) {
  const hash = crypto.createHash("sha256");
  const file = fs.readFileSync(filePath);
  hash.update(file);
  return hash.digest("hex");
}

function expectedSha256(checksumText) {
  const match = checksumText.match(/[a-fA-F0-9]{64}/);
  if (!match) {
    fail("checksum file did not contain a 64-character sha256 digest");
  }
  return match[0].toLowerCase();
}

function runTar(args, cwd) {
  const result = spawnSync("tar", args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });

  if (result.error && result.error.code === "ENOENT") {
    fail("required command not found: tar");
  }
  if (result.status !== 0) {
    const detail = result.stderr.trim() || (result.error && result.error.message) || `exit status ${result.status}`;
    fail(`tar ${args.join(" ")} failed: ${detail}`);
  }

  return result.stdout;
}

async function install() {
  if (process.env.WAX_CLI_SKIP_DOWNLOAD === "1") {
    console.log("Skipping wax binary download because WAX_CLI_SKIP_DOWNLOAD=1");
    return;
  }

  const repo = process.env.WAX_CLI_REPO || DEFAULT_REPO;
  const tag = versionTag();
  const version = tag.replace(/^v/, "");
  const target = targetTriple();
  const archiveName = `wax-${version}-${target}.tar.gz`;
  const archiveUrl = `${releaseBaseUrl(repo, tag)}/${archiveName}`;
  const checksumUrl = `${archiveUrl}.sha256`;
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "wax-cli-"));
  const archivePath = path.join(tmpDir, archiveName);
  const checksumPath = `${archivePath}.sha256`;
  const extractDir = path.join(tmpDir, "extract");
  const expectedDir = `wax-${version}-${target}`;
  const expectedMember = `${expectedDir}/wax`;
  const installDir = path.join(PACKAGE_ROOT, "bin");
  const installPath = path.join(installDir, "wax");

  try {
    console.log(`Installing wax ${version} for ${target}`);
    console.log(`Download: ${archiveUrl}`);

    await download(archiveUrl, archivePath);
    await download(checksumUrl, checksumPath);

    const expected = expectedSha256(fs.readFileSync(checksumPath, "utf8"));
    const actual = sha256(archivePath);
    if (actual !== expected) {
      fail(`checksum mismatch for ${archiveName}; expected ${expected}, got ${actual}`);
    }

    const entries = runTar(["-tzf", archivePath], tmpDir)
      .trim()
      .split("\n")
      .filter(Boolean);
    if (!entries.includes(expectedMember)) {
      fail(`archive is missing expected entry: ${expectedMember}`);
    }

    const unexpected = entries.filter((entry) => entry !== `${expectedDir}/` && entry !== expectedMember);
    if (unexpected.length > 0) {
      fail(`archive contains unexpected entries: ${unexpected.join(", ")}`);
    }

    fs.rmSync(installDir, { recursive: true, force: true });
    fs.mkdirSync(extractDir, { recursive: true });
    fs.mkdirSync(installDir, { recursive: true });
    runTar(["-xzf", archivePath, "-C", extractDir, expectedMember], tmpDir);
    const extractedBinary = path.join(extractDir, expectedMember);
    const extractedStat = fs.lstatSync(extractedBinary);
    if (!extractedStat.isFile()) {
      fail(`archive entry is not a regular file: ${expectedMember}`);
    }
    fs.copyFileSync(extractedBinary, installPath);
    fs.chmodSync(installPath, 0o755);

    console.log(`Installed wax to ${installPath}`);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

install().catch((error) => fail(error.message));
