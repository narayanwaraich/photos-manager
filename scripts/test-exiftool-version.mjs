import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { checkExifToolVersion, PINNED_EXIFTOOL_VERSION } from "./exiftool-version.mjs";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

test("ci.yml and docs/native-dependencies.md must match PINNED_EXIFTOOL_VERSION", () => {
  const ciPath = path.join(rootDir, ".github", "workflows", "ci.yml");
  const ciContent = fs.readFileSync(ciPath, "utf-8");
  assert.ok(
    ciContent.includes(`exiftool-${PINNED_EXIFTOOL_VERSION}`),
    `ci.yml must reference exiftool-${PINNED_EXIFTOOL_VERSION}`
  );

  const docsPath = path.join(rootDir, "docs", "native-dependencies.md");
  const docsContent = fs.readFileSync(docsPath, "utf-8");
  assert.ok(
    docsContent.includes(`ExifTool** | \`${PINNED_EXIFTOOL_VERSION}\``) ||
    docsContent.includes(PINNED_EXIFTOOL_VERSION),
    `docs/native-dependencies.md must reference ${PINNED_EXIFTOOL_VERSION}`
  );
});

test("checkExifToolVersion: exact match succeeds without failure", () => {
  const resLocal = checkExifToolVersion("13.61\n", false);
  assert.equal(resLocal.ok, true);
  assert.equal(resLocal.version, "13.61");
  assert.equal(resLocal.shouldFail, false);

  const resCi = checkExifToolVersion("13.61", true);
  assert.equal(resCi.ok, true);
  assert.equal(resCi.version, "13.61");
  assert.equal(resCi.shouldFail, false);
});

test("checkExifToolVersion: mismatch produces warning locally (shouldFail=false)", () => {
  const res = checkExifToolVersion("13.55\n", false);
  assert.equal(res.ok, false);
  assert.equal(res.version, "13.55");
  assert.equal(res.shouldFail, false);
  assert.ok(res.message.includes("⚠️ EXIFTOOL VERSION MISMATCH"));
});

test("checkExifToolVersion: mismatch in CI produces error (shouldFail=true)", () => {
  const res = checkExifToolVersion("13.55", true);
  assert.equal(res.ok, false);
  assert.equal(res.version, "13.55");
  assert.equal(res.shouldFail, true);
  assert.ok(res.message.includes("⚠️ EXIFTOOL VERSION MISMATCH"));
});

test("checkExifToolVersion: unparsable output produces failure in CI and warning locally", () => {
  const resLocal = checkExifToolVersion("command not found", false);
  assert.equal(resLocal.ok, false);
  assert.equal(resLocal.shouldFail, false);

  const resCi = checkExifToolVersion("", true);
  assert.equal(resCi.ok, false);
  assert.equal(resCi.shouldFail, true);
});
