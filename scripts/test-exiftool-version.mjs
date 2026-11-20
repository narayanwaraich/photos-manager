import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
  checkExifToolVersion,
  PINNED_EXIFTOOL_SHA256_MAC,
  PINNED_EXIFTOOL_SHA256_WIN,
  PINNED_EXIFTOOL_VERSION,
} from "./exiftool-version.mjs";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

test("ci.yml and docs/native-dependencies.md must match PINNED_EXIFTOOL_VERSION and SHA-256 constants", () => {
  const ciPath = path.join(rootDir, ".github", "workflows", "ci.yml");
  const ciContent = fs.readFileSync(ciPath, "utf-8");

  // 1. Extract every ExifTool version reference in ci.yml
  const ciVersions = [
    ...[...ciContent.matchAll(/exiftool-(\d+\.\d+(?:\.\d+)?)_64\.zip/g)].map((m) => ({
      context: m[0],
      version: m[1],
    })),
    ...[...ciContent.matchAll(/Image-ExifTool-(\d+\.\d+(?:\.\d+)?)\.tar\.gz/g)].map((m) => ({
      context: m[0],
      version: m[1],
    })),
    ...[...ciContent.matchAll(/ExifTool (\d+\.\d+(?:\.\d+)?)\b/g)].map((m) => ({
      context: m[0],
      version: m[1],
    })),
  ];

  assert.ok(
    ciVersions.some((v) => v.context.endsWith("_64.zip")),
    "ci.yml must contain Windows exiftool-<ver>_64.zip URL"
  );
  assert.ok(
    ciVersions.some((v) => v.context.endsWith(".tar.gz")),
    "ci.yml must contain macOS Image-ExifTool-<ver>.tar.gz URL"
  );

  for (const { context, version } of ciVersions) {
    assert.equal(
      version,
      PINNED_EXIFTOOL_VERSION,
      `ci.yml ExifTool version mismatch at "${context}": expected ${PINNED_EXIFTOOL_VERSION}, got ${version}`
    );
  }

  // 2. Extract both ExifTool SHA-256 values from ci.yml
  const ciWinHashMatches = [...ciContent.matchAll(/\$exifHash\s+-ne\s+"([A-Fa-f0-9]{64})"/g)];
  assert.ok(ciWinHashMatches.length >= 1, "ci.yml must check Windows ExifTool SHA-256");
  for (const m of ciWinHashMatches) {
    assert.equal(
      m[1].toLowerCase(),
      PINNED_EXIFTOOL_SHA256_WIN.toLowerCase(),
      `ci.yml Windows ExifTool SHA-256 mismatch: expected ${PINNED_EXIFTOOL_SHA256_WIN}, got ${m[1]}`
    );
  }

  const ciMacHashMatches = [...ciContent.matchAll(/echo\s+"([A-Fa-f0-9]{64})\s+exiftool\.tar\.gz"/g)];
  assert.ok(ciMacHashMatches.length >= 1, "ci.yml must check macOS ExifTool SHA-256");
  for (const m of ciMacHashMatches) {
    assert.equal(
      m[1].toLowerCase(),
      PINNED_EXIFTOOL_SHA256_MAC.toLowerCase(),
      `ci.yml macOS ExifTool SHA-256 mismatch: expected ${PINNED_EXIFTOOL_SHA256_MAC}, got ${m[1]}`
    );
  }

  // 3. Extract every ExifTool version reference in docs/native-dependencies.md
  const docsPath = path.join(rootDir, "docs", "native-dependencies.md");
  const docsContent = fs.readFileSync(docsPath, "utf-8");

  const docVersions = [
    ...[...docsContent.matchAll(/\*\*ExifTool\*\*\s*\|\s*`(\d+\.\d+(?:\.\d+)?)`/g)].map((m) => ({
      context: m[0],
      version: m[1],
    })),
    ...[...docsContent.matchAll(/exiftool-(\d+\.\d+(?:\.\d+)?)_64\.zip/g)].map((m) => ({
      context: m[0],
      version: m[1],
    })),
    ...[...docsContent.matchAll(/Image-ExifTool-(\d+\.\d+(?:\.\d+)?)\.tar\.gz/g)].map((m) => ({
      context: m[0],
      version: m[1],
    })),
    ...[...docsContent.matchAll(/ExifTool `(\d+\.\d+(?:\.\d+)?)`/g)].map((m) => ({
      context: m[0],
      version: m[1],
    })),
    ...[...docsContent.matchAll(/pinned to identical version (\d+\.\d+(?:\.\d+)?)/g)].map((m) => ({
      context: m[0],
      version: m[1],
    })),
  ];

  assert.ok(docVersions.length >= 5, `Expected at least 5 ExifTool version references in docs/native-dependencies.md, found ${docVersions.length}`);
  for (const { context, version } of docVersions) {
    assert.equal(
      version,
      PINNED_EXIFTOOL_VERSION,
      `docs/native-dependencies.md ExifTool version mismatch at "${context}": expected ${PINNED_EXIFTOOL_VERSION}, got ${version}`
    );
  }

  // 4. Extract both ExifTool SHA-256 values from docs/native-dependencies.md (table + §5.3)
  const docWinTableHash = docsContent.match(
    /\|\s*`exiftool-[^`]+_64\.zip`\s*\|\s*Windows x64\s*\|\s*(\d+\.\d+(?:\.\d+)?)\s*\|\s*`[^`]+`\s*\|\s*`([A-Fa-f0-9]{64})`/
  );
  assert.ok(docWinTableHash, "docs/native-dependencies.md §4.1 must list Windows ExifTool version and SHA-256");
  assert.equal(docWinTableHash[1], PINNED_EXIFTOOL_VERSION);
  assert.equal(docWinTableHash[2].toLowerCase(), PINNED_EXIFTOOL_SHA256_WIN.toLowerCase());

  const docMacTableHash = docsContent.match(
    /\|\s*`Image-ExifTool-[^`]+\.tar\.gz`\s*\|\s*macOS arm64\s*\|\s*(\d+\.\d+(?:\.\d+)?)\s*\|\s*`[^`]+`\s*\|\s*`([A-Fa-f0-9]{64})`/
  );
  assert.ok(docMacTableHash, "docs/native-dependencies.md §4.1 must list macOS ExifTool version and SHA-256");
  assert.equal(docMacTableHash[1], PINNED_EXIFTOOL_VERSION);
  assert.equal(docMacTableHash[2].toLowerCase(), PINNED_EXIFTOOL_SHA256_MAC.toLowerCase());

  const docSec53MacHash = docsContent.match(/echo\s+"([A-Fa-f0-9]{64})\s+\/tmp\/exiftool\.tar\.gz"/);
  assert.ok(docSec53MacHash, "docs/native-dependencies.md §5.3 must include macOS SHA-256 check");
  assert.equal(docSec53MacHash[1].toLowerCase(), PINNED_EXIFTOOL_SHA256_MAC.toLowerCase());

  const docSec53WinHash = docsContent.match(/\$exifHash\s+-ne\s+"([A-Fa-f0-9]{64})"/);
  assert.ok(docSec53WinHash, "docs/native-dependencies.md §5.3 must include Windows SHA-256 check");
  assert.equal(docSec53WinHash[1].toLowerCase(), PINNED_EXIFTOOL_SHA256_WIN.toLowerCase());
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
