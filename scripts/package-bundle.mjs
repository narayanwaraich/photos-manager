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

function runRequiredCmd(cmd) {
  try {
    return execSync(cmd, { encoding: "utf-8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  } catch (err) {
    console.error(`❌ Required command failed: ${cmd}`);
    if (err.stderr) {
      console.error(err.stderr.toString().trim());
    }
    process.exit(1);
  }
}

if (isMac) {
  function isTruthyEnv(val) {
    return ["1", "true"].includes(String(val || "").trim().toLowerCase());
  }

  function getOtoolDeps(machoPath, isDylib) {
    const out = runRequiredCmd(`otool -L "${machoPath}"`);
    const lines = out.split("\n").slice(isDylib ? 2 : 1);
    const deps = [];
    for (const line of lines) {
      const m = line.trim().match(/^([@\/][^\s]+)\s+\(compatibility/);
      if (m) {
        deps.push(m[1]);
      }
    }
    return deps;
  }

  function getRpaths(machoPath) {
    const out = runRequiredCmd(`otool -l "${machoPath}"`);
    const lines = out.split("\n");
    const rpaths = [];
    for (let i = 0; i < lines.length; i++) {
      if (/^\s*cmd LC_RPATH\s*$/.test(lines[i])) {
        for (let j = i + 1; j < Math.min(i + 6, lines.length); j++) {
          const pm = lines[j].match(/^\s*path (.+?) \(offset/);
          if (pm) {
            rpaths.push(pm[1]);
            break;
          }
        }
      }
    }
    return rpaths;
  }

  // Resolve libvips prefix strictly from VIPS_LIB_DIR / VIPS_DIR when set (never fall back to Homebrew)
  const customVipsLibDir = (process.env.VIPS_LIB_DIR || "").trim();
  const customVipsDir = (process.env.VIPS_DIR || "").trim();
  const devHomebrew = isTruthyEnv(process.env.PHOTOS_DEV_HOMEBREW);
  const repoDepsLibDir = path.join(rootDir, "deps", "vips", "lib");

  let strictCustomPrefix = false;
  let searchLibDirs = [];

  if (customVipsLibDir) {
    if (!fs.existsSync(customVipsLibDir) || !fs.statSync(customVipsLibDir).isDirectory()) {
      console.error(`❌ VIPS_LIB_DIR is set to '${customVipsLibDir}', which does not exist.`);
      process.exit(1);
    }
    strictCustomPrefix = true;
    searchLibDirs = [path.resolve(customVipsLibDir)];
  } else if (customVipsDir) {
    const resolved = path.join(path.resolve(customVipsDir), "lib");
    if (!fs.existsSync(resolved) || !fs.statSync(resolved).isDirectory()) {
      console.error(`❌ VIPS_DIR is set to '${customVipsDir}', but '${resolved}' does not exist.`);
      process.exit(1);
    }
    strictCustomPrefix = true;
    searchLibDirs = [resolved];
  } else if (!devHomebrew && fs.existsSync(repoDepsLibDir)) {
    strictCustomPrefix = true;
    searchLibDirs = [repoDepsLibDir];
  } else {
    searchLibDirs = ["/opt/homebrew/lib", "/usr/local/lib"].filter((d) => fs.existsSync(d));
    if (devHomebrew && searchLibDirs.length === 0) {
      console.error("❌ PHOTOS_DEV_HOMEBREW is set, but neither /opt/homebrew/lib nor /usr/local/lib exists.");
      process.exit(1);
    }
  }

  console.log(`macOS dylib harvest mode: ${strictCustomPrefix ? "strict custom prefix" : "dev/homebrew"} (search dirs: ${searchLibDirs.join(", ") || "<none>"})`);

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

    // Remove any stale .dylib files left from an earlier incremental build
    for (const existing of fs.readdirSync(frameworksDir)) {
      if (existing.endsWith(".dylib") || existing.includes(".dylib.")) {
        fs.unlinkSync(path.join(frameworksDir, existing));
      }
    }

    const macosDir = path.join(appBundlePath, "Contents", "MacOS");
    const exeName = fs.readdirSync(macosDir).find((f) => !f.startsWith("."));
    if (!exeName) {
      console.error(`❌ Could not locate executable inside ${macosDir}`);
      process.exit(1);
    }
    const exePath = path.join(macosDir, exeName);

    // 2. Identify required dylibs from otool -L
    const otoolOut = runRequiredCmd(`otool -L "${exePath}"`);
    console.log("Main binary linked libraries:");
    console.log(otoolOut);

    function resolveDependencyPath(depRef, requesterPath) {
      if (depRef.startsWith("/System/") || depRef.startsWith("/usr/lib/")) {
        return null;
      }
      if (
        strictCustomPrefix &&
        (depRef.includes("/opt/homebrew") ||
          depRef.includes("/usr/local") ||
          depRef.includes("Cellar"))
      ) {
        console.error(
          `❌ Custom prefix mode forbids Homebrew/local dependency '${depRef}' referenced by '${requesterPath}'.`
        );
        process.exit(1);
      }

      const depName = path.basename(depRef);
      if (depRef.startsWith("@rpath/")) {
        const candDirs = strictCustomPrefix
          ? searchLibDirs
          : [...searchLibDirs, path.dirname(requesterPath)];
        for (const dir of candDirs) {
          const cand = path.join(dir, depName);
          if (fs.existsSync(cand)) {
            return cand;
          }
        }
      } else if (depRef.startsWith("@loader_path/")) {
        const rel = depRef.slice("@loader_path/".length);
        const direct = path.join(path.dirname(requesterPath), rel);
        if (fs.existsSync(direct)) {
          return direct;
        }
        for (const dir of searchLibDirs) {
          const cand = path.join(dir, depName);
          if (fs.existsSync(cand)) {
            return cand;
          }
        }
      } else if (depRef.startsWith("/")) {
        if (strictCustomPrefix) {
          console.error(
            `❌ Custom prefix mode forbids absolute non-system dependency '${depRef}' referenced by '${requesterPath}'.`
          );
          process.exit(1);
        }
        if (fs.existsSync(depRef)) {
          return depRef;
        }
        for (const dir of searchLibDirs) {
          const cand = path.join(dir, depName);
          if (fs.existsSync(cand)) {
            return cand;
          }
        }
      }

      console.error(
        `❌ Required dynamic library '${depRef}' (needed by '${requesterPath}') was not found in: ${searchLibDirs.join(", ") || "<none>"}`
      );
      process.exit(1);
    }

    // Collect non-system dylibs from main binary and resolve transitive closure strictly
    const queue = [];
    for (const depRef of getOtoolDeps(exePath, false)) {
      const resolved = resolveDependencyPath(depRef, exePath);
      if (resolved) {
        queue.push({ depName: path.basename(depRef), srcPath: resolved });
      }
    }

    // 3. Recursively copy dependent dylibs into Contents/Frameworks/
    const visited = new Set();
    while (queue.length > 0) {
      const { depName, srcPath } = queue.shift();
      if (visited.has(depName)) continue;
      if (!fs.existsSync(srcPath)) {
        console.error(`❌ Resolved library path does not exist: ${srcPath}`);
        process.exit(1);
      }
      visited.add(depName);

      const targetPath = path.join(frameworksDir, depName);
      // copyFileSync follows symlinks cleanly and writes a regular file under the exact versioned name
      fs.copyFileSync(srcPath, targetPath);
      fs.chmodSync(targetPath, 0o755);

      for (const subDep of getOtoolDeps(srcPath, true)) {
        const subResolved = resolveDependencyPath(subDep, srcPath);
        if (subResolved) {
          const subName = path.basename(subDep);
          if (!visited.has(subName)) {
            queue.push({ depName: subName, srcPath: subResolved });
          }
        }
      }
    }

    if (visited.size === 0) {
      console.error("❌ No libvips dynamic libraries were harvested into Contents/Frameworks/.");
      process.exit(1);
    }
    console.log(`Bundled ${visited.size} dynamic libraries into Contents/Frameworks/`);

    // 4. Normalize LC_RPATH, LC_ID_DYLIB, and LC_LOAD_DYLIB references using install_name_tool
    for (const rp of getRpaths(exePath)) {
      if (rp !== "@executable_path/../Frameworks") {
        runRequiredCmd(`install_name_tool -delete_rpath "${rp}" "${exePath}"`);
      }
    }
    if (!getRpaths(exePath).includes("@executable_path/../Frameworks")) {
      runRequiredCmd(`install_name_tool -add_rpath "@executable_path/../Frameworks" "${exePath}"`);
    }

    function fixLinkages(filePath, isLib, libFilename) {
      if (isLib) {
        runRequiredCmd(`install_name_tool -id "@rpath/${libFilename}" "${filePath}"`);
        for (const rp of getRpaths(filePath)) {
          if (rp !== "@loader_path") {
            runRequiredCmd(`install_name_tool -delete_rpath "${rp}" "${filePath}"`);
          }
        }
      }
      for (const dep of getOtoolDeps(filePath, isLib)) {
        if (
          !dep.startsWith("/System/") &&
          !dep.startsWith("/usr/lib/") &&
          !dep.startsWith("@rpath/")
        ) {
          const base = path.basename(dep);
          runRequiredCmd(`install_name_tool -change "${dep}" "@rpath/${base}" "${filePath}"`);
        }
      }
    }

    // Fix main binary
    fixLinkages(exePath, false);

    // Fix all bundled dylibs
    for (const filename of fs.readdirSync(frameworksDir)) {
      if (filename.endsWith(".dylib") || filename.includes(".dylib.")) {
        const libTarget = path.join(frameworksDir, filename);
        fixLinkages(libTarget, true, filename);
      }
    }

    // 5. Re-sign the application bundle ad-hoc and verify signature integrity
    runRequiredCmd(`codesign --force --deep -s - "${appBundlePath}"`);
    runRequiredCmd(`codesign --verify --deep --strict "${appBundlePath}"`);
    console.log("✓ Relocatable @rpath patching and ad-hoc codesign complete for macOS bundle.");

    // 6. Verify zero Homebrew / /usr/local / build-prefix leaks in both otool -L AND LC_RPATH
    console.log("Verifying clean otool -L dependency graph and LC_RPATH headers...");
    const bundledFiles = new Set(fs.readdirSync(frameworksDir));
    const checkTargets = [
      { filePath: exePath, isDylib: false },
      ...fs.readdirSync(frameworksDir).map((f) => ({
        filePath: path.join(frameworksDir, f),
        isDylib: true,
      })),
    ];

    const leaks = [];
    for (const { filePath, isDylib } of checkTargets) {
      const base = path.basename(filePath);
      if (isDylib && fs.lstatSync(filePath).isSymbolicLink()) {
        leaks.push(`${base} is a symbolic link (expected regular versioned file)`);
      }
      for (const dep of getOtoolDeps(filePath, isDylib)) {
        if (dep.startsWith("/System/") || dep.startsWith("/usr/lib/")) {
          continue;
        }
        if (dep.startsWith("@rpath/")) {
          const targetLib = dep.slice("@rpath/".length);
          if (!bundledFiles.has(targetLib)) {
            leaks.push(`${base} -> missing bundled @rpath target: ${dep}`);
          }
          continue;
        }
        leaks.push(`${base} -> non-system, non-@rpath dependency: ${dep}`);
      }
      for (const rp of getRpaths(filePath)) {
        const allowed = isDylib ? rp === "@loader_path" : rp === "@executable_path/../Frameworks";
        if (!allowed) {
          leaks.push(`${base} -> disallowed LC_RPATH: ${rp}`);
        }
      }
    }

    if (leaks.length > 0) {
      console.error("❌ Dependency / LC_RPATH leaks found in packaged macOS bundle:");
      for (const leak of leaks) {
        console.error("  " + leak);
      }
      process.exit(1);
    }
    console.log(
      `✓ Zero Homebrew, build-prefix, or unresolved @rpath linkages across all ${checkTargets.length} binaries/libraries.`
    );

    // 7. Produce a relocatable zip archive of photos-manager.app
    const macosBundleDir = path.dirname(appBundlePath);
    const zipPath = path.join(macosBundleDir, "photos-manager-macos-arm64.zip");
    if (fs.existsSync(zipPath)) fs.unlinkSync(zipPath);
    runRequiredCmd(
      `ditto --norsrc --noextattr --noqtn --noacl -c -k --keepParent "${appBundlePath}" "${zipPath}"`
    );
    console.log(`✓ Created portable macOS zip package: ${zipPath}`);
  }
} else if (isWindows) {
  // On Windows, ensure all libvips DLLs are present in target/release adjacent to photos-manager.exe
  const targetDir = path.join(rootDir, "src-tauri", "target", "release");
  const customVipsDir = process.env.VIPS_DIR || process.env.VIPS_LIB_DIR;
  const candidates = [
    path.join(rootDir, "deps", "vips", "bin"),
    customVipsDir ? path.join(customVipsDir, "bin") : null,
    customVipsDir ? path.join(customVipsDir, "..", "bin") : null,
    customVipsDir,
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
      if (copied > 0) break;
    }
  }
  console.log(`✓ Copied ${copied} Windows libvips DLLs to release target directory.`);

  // Also copy staged resources (exiftool and test fixtures) adjacent to target/release/photos-manager.exe
  const stagedResources = path.join(rootDir, "src-tauri", "resources");
  const targetResources = path.join(targetDir, "resources");
  if (fs.existsSync(stagedResources)) {
    fs.cpSync(stagedResources, targetResources, { recursive: true });
    console.log("✓ Copied staged resources to release target directory.");
  }
}

console.log("Post-build packaging complete.\n");
