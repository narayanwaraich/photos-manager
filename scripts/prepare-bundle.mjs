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

for (const fName of ["image_orientation_6.jpg", "image_orientation_1.jpg", "fixture_blocked.svg"]) {
  const srcFixture = path.join(rootDir, "tests", "fixtures", "generated", fName);
  if (fs.existsSync(srcFixture)) {
    const dstFixture = path.join(fixturesTargetDir, fName);
    fs.copyFileSync(srcFixture, dstFixture);
    fs.chmodSync(dstFixture, 0o644);
    console.log(`✓ Staged fixture: ${dstFixture}`);
  } else {
    console.warn(`⚠️ Source fixture not found at ${srcFixture}`);
  }
}

// 2. Stage ExifTool into resources/exiftool
const exiftoolTargetDir = path.join(tauriResourcesDir, "exiftool");
fs.mkdirSync(exiftoolTargetDir, { recursive: true });

const isWindows = process.platform === "win32";

if (isWindows) {
  // On Windows, expect exiftool.exe and exiftool_files directory
  const customExifDir = process.env.EXIFTOOL_DIR || process.env.EXIFTOOL_SRC_DIR;
  const candidates = [
    path.join(rootDir, "deps", "exiftool"),
    customExifDir,
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

  // 3. Stage Windows libvips DLLs into src-tauri/vips_dlls so Tauri bundles them into $INSTDIR adjacent to photos-manager.exe
  const vipsDllsTargetDir = path.join(rootDir, "src-tauri", "vips_dlls");
  fs.mkdirSync(vipsDllsTargetDir, { recursive: true });
  const releaseTargetDir = path.join(rootDir, "src-tauri", "target", "release");
  fs.mkdirSync(releaseTargetDir, { recursive: true });

  const customVipsDir = process.env.VIPS_DIR || process.env.VIPS_LIB_DIR;
  const vipsBinCandidates = [
    path.join(rootDir, "deps", "vips", "bin"),
    customVipsDir ? path.join(customVipsDir, "bin") : null,
    customVipsDir ? path.join(customVipsDir, "..", "bin") : null,
    customVipsDir,
  ].filter(Boolean);

  let dllCount = 0;
  for (const cand of vipsBinCandidates) {
    if (fs.existsSync(cand)) {
      const files = fs.readdirSync(cand);
      for (const f of files) {
        if (f.endsWith(".dll")) {
          fs.copyFileSync(path.join(cand, f), path.join(vipsDllsTargetDir, f));
          fs.copyFileSync(path.join(cand, f), path.join(releaseTargetDir, f));
          dllCount++;
        }
      }
      if (dllCount > 0) break;
    }
  }
  console.log(`✓ Staged ${dllCount} libvips DLLs for Windows Tauri installer bundle.`);
} else {
  // macOS / Unix
  // Ensure vips_dlls directory exists with .gitkeep so tauri.conf.json mapping doesn't fail on macOS
  const vipsDllsTargetDir = path.join(rootDir, "src-tauri", "vips_dlls");
  fs.mkdirSync(vipsDllsTargetDir, { recursive: true });
  fs.writeFileSync(path.join(vipsDllsTargetDir, ".gitkeep"), "");

  const customExifDir = process.env.EXIFTOOL_DIR || process.env.EXIFTOOL_SRC_DIR;
  const candidates = [
    path.join(rootDir, "deps", "exiftool"),
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

      // Check if candidate directory itself contains lib/
      if (fs.existsSync(path.join(cand, "lib", "Image"))) {
        fs.cpSync(path.join(cand, "lib", "Image"), path.join(targetLibDir, "Image"), { recursive: true });
        if (fs.existsSync(path.join(cand, "lib", "File", "RandomAccess.pm"))) {
          const fileDir = path.join(targetLibDir, "File");
          fs.mkdirSync(fileDir, { recursive: true });
          fs.copyFileSync(path.join(cand, "lib", "File", "RandomAccess.pm"), path.join(fileDir, "RandomAccess.pm"));
        }
      } else {
        // Fallback to Homebrew/system paths for local dev
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
