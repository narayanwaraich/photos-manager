#!/usr/bin/env node
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");
const tauriResourcesDir = path.join(rootDir, "src-tauri", "resources");

console.log("Preparing resources for Tauri bundle packaging...");
fs.mkdirSync(tauriResourcesDir, { recursive: true });

// 1. Stage test fixtures into resources/fixtures
const fixturesTargetDir = path.join(tauriResourcesDir, "fixtures");
fs.mkdirSync(fixturesTargetDir, { recursive: true });

const srcFixture = path.join(rootDir, "tests", "fixtures", "generated", "image_orientation_6.jpg");
if (fs.existsSync(srcFixture)) {
  const dstFixture = path.join(fixturesTargetDir, "image_orientation_6.jpg");
  fs.copyFileSync(srcFixture, dstFixture);
  fs.chmodSync(dstFixture, 0o644);
  console.log(`✓ Staged fixture: ${dstFixture}`);
} else {
  console.warn(`⚠️ Source fixture not found at ${srcFixture}`);
}

// 2. Stage ExifTool into resources/exiftool
const exiftoolTargetDir = path.join(tauriResourcesDir, "exiftool");
fs.mkdirSync(exiftoolTargetDir, { recursive: true });

const isWindows = process.platform === "win32";

if (isWindows) {
  // On Windows, expect exiftool.exe and exiftool_files directory
  const customExifDir = process.env.EXIFTOOL_DIR || process.env.EXIFTOOL_SRC_DIR;
  const candidates = [
    customExifDir,
    path.join(rootDir, "deps", "exiftool"),
    "C:\\ProgramData\\chocolatey\\bin",
  ].filter(Boolean);

  let found = false;
  for (const cand of candidates) {
    const exePath = path.join(cand, "exiftool.exe");
    if (fs.existsSync(exePath)) {
      const dstExe = path.join(exiftoolTargetDir, "exiftool.exe");
      fs.copyFileSync(exePath, dstExe);
      fs.chmodSync(dstExe, 0o755);

      const filesDir = path.join(cand, "exiftool_files");
      if (fs.existsSync(filesDir)) {
        fs.cpSync(filesDir, path.join(exiftoolTargetDir, "exiftool_files"), { recursive: true });
      }
      console.log(`✓ Staged Windows ExifTool from ${cand}`);
      found = true;
      break;
    }
  }

  if (!found) {
    console.warn("⚠️ Windows ExifTool standalone distribution not found in candidate paths.");
  }
} else {
  // macOS / Unix
  const customExifDir = process.env.EXIFTOOL_DIR || process.env.EXIFTOOL_SRC_DIR;
  const candidates = [
    customExifDir,
    "/opt/homebrew/bin",
    "/usr/local/bin",
  ].filter(Boolean);

  let found = false;
  for (const cand of candidates) {
    const binPath = path.join(cand, "exiftool");
    if (fs.existsSync(binPath)) {
      const targetBin = path.join(exiftoolTargetDir, "exiftool");
      let scriptContent = fs.readFileSync(binPath, "utf-8");

      // Ensure shebang points to system perl and bundle lib is prioritized
      scriptContent = scriptContent.replace(/^#!.*perl.*/, "#!/usr/bin/perl");
      fs.writeFileSync(targetBin, scriptContent, { mode: 0o755 });

      // Stage the pure Perl modules (Image and File/RandomAccess)
      const targetLibDir = path.join(exiftoolTargetDir, "lib");
      fs.mkdirSync(targetLibDir, { recursive: true });

      let dynamicBrewLib = null;
      try {
        const prefix = execSync("brew --prefix exiftool", { stdio: ["ignore", "pipe", "ignore"] }).toString().trim();
        dynamicBrewLib = path.join(prefix, "libexec", "lib", "perl5");
      } catch {}

      const libSources = [
        dynamicBrewLib,
        "/opt/homebrew/opt/exiftool/libexec/lib/perl5",
        "/usr/local/opt/exiftool/libexec/lib/perl5",
        "/opt/homebrew/Cellar/exiftool/13.55_1/libexec/lib/perl5",
        "/usr/local/Cellar/exiftool",
      ].filter(Boolean);

      for (const libSrc of libSources) {
        if (fs.existsSync(path.join(libSrc, "Image"))) {
          fs.cpSync(path.join(libSrc, "Image"), path.join(targetLibDir, "Image"), { recursive: true });
          if (fs.existsSync(path.join(libSrc, "File", "RandomAccess.pm"))) {
            const fileDir = path.join(targetLibDir, "File");
            fs.mkdirSync(fileDir, { recursive: true });
            fs.copyFileSync(path.join(libSrc, "File", "RandomAccess.pm"), path.join(fileDir, "RandomAccess.pm"));
          }
          break;
        }
      }

      console.log(`✓ Staged macOS ExifTool from ${binPath}`);
      found = true;
      break;
    }
  }

  if (!found) {
    console.warn("⚠️ macOS ExifTool not found in candidate paths.");
  }
}

// Make all files in resources writable to prevent Tauri build permission errors
try {
  if (!isWindows) {
    execSync(`chmod -R u+rw "${tauriResourcesDir}"`);
  }
} catch {
  // ignore
}

console.log("Bundle resource preparation complete.\n");
