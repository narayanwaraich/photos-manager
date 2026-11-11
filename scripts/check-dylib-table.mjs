#!/usr/bin/env node

/**
 * check-dylib-table.mjs
 *
 * Verifies that every bundled *.dylib in Contents/Frameworks of the packaged
 * macOS .app has an exact corresponding row in section 4.3 of docs/native-dependencies.md,
 * and that every row in section 4.3 corresponds to an actual bundled file.
 *
 * Exits with code 1 on any discrepancy, printing both counts and diffs.
 */

import fs from 'node:fs';
import path from 'node:path';

const appPath = process.argv[2];
const docsPath = process.argv[3] || 'docs/native-dependencies.md';

if (!appPath) {
  console.error('Usage: node scripts/check-dylib-table.mjs <path-to-photos-manager.app> [docs/native-dependencies.md]');
  process.exit(1);
}

// 1. Locate Contents/Frameworks
let frameworksDir = appPath;
if (fs.existsSync(path.join(appPath, 'Contents', 'Frameworks'))) {
  frameworksDir = path.join(appPath, 'Contents', 'Frameworks');
}

if (!fs.existsSync(frameworksDir)) {
  console.error(`ERROR: Frameworks directory not found at: ${frameworksDir}`);
  process.exit(1);
}

// 2. Enumerate bundled .dylib files (rejecting accidental compile-time symlinks)
const allFrameworkDylibs = fs
  .readdirSync(frameworksDir)
  .filter((file) => file.endsWith('.dylib'))
  .sort();

const symlinkDylibs = allFrameworkDylibs.filter((file) =>
  fs.lstatSync(path.join(frameworksDir, file)).isSymbolicLink()
);
if (symlinkDylibs.length > 0) {
  console.error(`ERROR: Bundled Contents/Frameworks contains ${symlinkDylibs.length} symbolic link(s):`);
  for (const sym of symlinkDylibs) {
    console.error(`  - ${sym}`);
  }
  process.exit(1);
}

const onDiskDylibs = allFrameworkDylibs;

console.log(`[check-dylib-table] Bundled dylibs on disk in ${frameworksDir}: ${onDiskDylibs.length}`);

// 3. Read section 4.3 of docs/native-dependencies.md
if (!fs.existsSync(docsPath)) {
  console.error(`ERROR: Documentation file not found at: ${docsPath}`);
  process.exit(1);
}

const docsContent = fs.readFileSync(docsPath, 'utf8');

// Match section 4.3 up to section 4.4 or end of section
const sec43Match = docsContent.match(/### 4\.3 macOS Bundled Dynamic Libraries Audit[\s\S]*?(?=### 4\.4|$)/);
if (!sec43Match) {
  console.error('ERROR: Could not locate section 4.3 in ' + docsPath);
  process.exit(1);
}

const sec43Text = sec43Match[0];

// Extract dylib names from markdown table rows: | `libxyz.dylib` | ...
const tableDylibs = [];
const rowRegex = /\|\s*`([^`]+\.dylib)`\s*\|/g;
let match;
while ((match = rowRegex.exec(sec43Text)) !== null) {
  tableDylibs.push(match[1]);
}
tableDylibs.sort();

const duplicatesInTable = tableDylibs.filter((d, idx) => idx > 0 && tableDylibs[idx - 1] === d);
if (duplicatesInTable.length > 0) {
  console.error(`ERROR: Duplicate dylib row(s) in ${docsPath} §4.3: ${duplicatesInTable.join(', ')}`);
  process.exit(1);
}

console.log(`[check-dylib-table] Documented dylib rows in ${docsPath} §4.3: ${tableDylibs.length}`);

// 4. Compare sets
const missingInTable = onDiskDylibs.filter((dylib) => !tableDylibs.includes(dylib));
const missingOnDisk = tableDylibs.filter((dylib) => !onDiskDylibs.includes(dylib));

let hasError = false;

if (missingInTable.length > 0) {
  console.error(`ERROR: The following ${missingInTable.length} dylib(s) exist on disk but are MISSING from ${docsPath} §4.3:`);
  for (const file of missingInTable) {
    console.error(`  - ${file}`);
  }
  hasError = true;
}

if (missingOnDisk.length > 0) {
  console.error(`ERROR: The following ${missingOnDisk.length} dylib(s) are listed in ${docsPath} §4.3 but are MISSING from the bundled app:`);
  for (const file of missingOnDisk) {
    console.error(`  - ${file}`);
  }
  hasError = true;
}

if (hasError) {
  console.error(`\nFAILED: Dylib inventory mismatch (Disk: ${onDiskDylibs.length}, Docs: ${tableDylibs.length}).`);
  process.exit(1);
}

console.log(`SUCCESS: All ${onDiskDylibs.length} bundled dylibs exactly match the ${tableDylibs.length} entries in ${docsPath} §4.3.`);
process.exit(0);
