const assert = require("node:assert/strict");
const test = require("node:test");

const { expectedSha256 } = require("./postinstall");

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
