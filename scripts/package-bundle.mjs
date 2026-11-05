#!/usr/bin/env node
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");
const isWindows = process.platform === "win32";
const isMac = process.platform === "darwin";

console.log("Running post-build packaging and dependency relocation...");

function runCmd(cmd) {
  try {
    return execSync(cmd, { encoding: "utf-8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  } catch (err) {
    return null;
  }
}

if (isMac) {
  // 1. Locate macOS .app bundle
  const bundleSearchDirs = [
    path.join(rootDir, "src-tauri", "target", "release", "bundle", "macos"),
    path.join(rootDir, "src-tauri", "target", "release"),
  ];

  let appBundlePath = null;
  for (const bDir of bundleSearchDirs) {
    if (fs.existsSync(bDir)) {
      const entries = fs.readdirSync(bDir);
      for (const e of entries) {
        if (e.endsWith(".app")) {
          appBundlePath = path.join(bDir, e);
          break;
        }
      }
    }
    if (appBundlePath) break;
  }

  if (!appBundlePath) {
    console.log("No .app bundle found in target/release. Checking for standalone binary...");
    const rawExe = path.join(rootDir, "src-tauri", "target", "release", "photos-manager");
    if (!fs.existsSync(rawExe)) {
      console.warn("⚠️ Standalone release executable not found yet. Run after tauri build.");
      process.exit(0);
    }
  } else {
    console.log(`Found application bundle: ${appBundlePath}`);
    const frameworksDir = path.join(appBundlePath, "Contents", "Frameworks");
    fs.mkdirSync(frameworksDir, { recursive: true });

    const macosDir = path.join(appBundlePath, "Contents", "MacOS");
    const exeName = fs.readdirSync(macosDir).find((f) => !f.startsWith("."));
    const exePath = path.join(macosDir, exeName);

    // 2. Identify required dylibs from otool -L
    const otoolOut = runCmd(`otool -L "${exePath}"`) || "";
    console.log("Main binary linked libraries:");
    console.log(otoolOut);

    // Collect non-system dylibs from main binary and existing Frameworks
    const lines = otoolOut.split("\n");
    const dylibMap = new Map(); // filename -> original path

    for (const line of lines) {
      const match = line.trim().match(/^([@\/][^\s]+)\s+\(compatibility/);
      if (match) {
        let fullPath = match[1];
        if (
          !fullPath.startsWith("/System/") &&
          !fullPath.startsWith("/usr/lib/")
        ) {
          const filename = path.basename(fullPath);
          if (fullPath.startsWith("@rpath/")) {
            fullPath = path.join("/opt/homebrew/lib", filename);
          }
          dylibMap.set(filename, fullPath);
        }
      }
    }

    // Also include any dylib already copied into Frameworks
    if (fs.existsSync(frameworksDir)) {
      for (const f of fs.readdirSync(frameworksDir)) {
        if (!dylibMap.has(f)) {
          dylibMap.set(f, path.join("/opt/homebrew/lib", f));
        }
      }
    }

    // 3. Recursively copy dependent dylibs into Frameworks
    const visited = new Set();
    const queue = Array.from(dylibMap.values());

    while (queue.length > 0) {
      const libPath = queue.shift();
      const filename = path.basename(libPath);
      if (visited.has(filename)) continue;
      visited.add(filename);

      if (fs.existsSync(libPath)) {
        const targetPath = path.join(frameworksDir, filename);
        fs.copyFileSync(libPath, targetPath);
        fs.chmodSync(targetPath, 0o755);

        // Check dependencies of this dylib (handling both absolute paths and @rpath references)
        const depOut = runCmd(`otool -L "${libPath}"`) || "";
        for (const depLine of depOut.split("\n")) {
          const m = depLine.trim().match(/^([@\/][^\s]+)\s+\(compatibility/);
          if (m) {
            let depPath = m[1];
            if (
              !depPath.startsWith("/System/") &&
              !depPath.startsWith("/usr/lib/")
            ) {
              const depName = path.basename(depPath);
              if (depPath.startsWith("@rpath/")) {
                // Resolve against common library locations (e.g. Homebrew)
                const candidate = path.join("/opt/homebrew/lib", depName);
                if (fs.existsSync(candidate)) {
                  depPath = candidate;
                }
              }
              dylibMap.set(depName, depPath);
              if (!visited.has(depName)) {
                queue.push(depPath);
              }
            }
          }
        }
      }
    }

    console.log(`Bundled ${visited.size} dynamic libraries into Contents/Frameworks/`);

    // 4. Update rpath and install names using install_name_tool
    runCmd(`install_name_tool -add_rpath "@executable_path/../Frameworks" "${exePath}"`);

    // Patch executable dependencies
    for (const [name, origPath] of dylibMap.entries()) {
      runCmd(`install_name_tool -change "${origPath}" "@rpath/${name}" "${exePath}"`);
    }

    // Patch each bundled library ID and references
    for (const filename of fs.readdirSync(frameworksDir)) {
      if (filename.endsWith(".dylib") || filename.includes(".dylib.")) {
        const libTarget = path.join(frameworksDir, filename);
        runCmd(`install_name_tool -id "@rpath/${filename}" "${libTarget}"`);

        for (const [name, origPath] of dylibMap.entries()) {
          runCmd(`install_name_tool -change "${origPath}" "@rpath/${name}" "${libTarget}"`);
        }
      }
    }

    // 5. Re-sign the application bundle ad-hoc to satisfy macOS code integrity
    runCmd(`codesign --force --deep -s - "${appBundlePath}"`);
    console.log("✓ Relocatable @rpath patching and ad-hoc codesign complete for macOS bundle.");
  }
} else if (isWindows) {
  // On Windows, copy libvips DLLs next to photos-manager.exe
  const targetDir = path.join(rootDir, "src-tauri", "target", "release");
  const customVipsDir = process.env.VIPS_DIR || process.env.VIPS_LIB_DIR;
  const candidates = [
    customVipsDir ? path.join(customVipsDir, "..", "bin") : null,
    path.join(rootDir, "deps", "vips", "bin"),
  ].filter(Boolean);

  let copied = 0;
  for (const cand of candidates) {
    if (fs.existsSync(cand)) {
      const files = fs.readdirSync(cand);
      for (const f of files) {
        if (f.endsWith(".dll")) {
          fs.copyFileSync(path.join(cand, f), path.join(targetDir, f));
          copied++;
        }
      }
      break;
    }
  }
  console.log(`✓ Copied ${copied} Windows libvips DLLs to release target directory.`);
}

console.log("Post-build packaging complete.\n");
