#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

// Ensure reports/logs directory exists
const logsDir = path.join(rootDir, "reports", "logs");
fs.mkdirSync(logsDir, { recursive: true });

const today = new Date().toISOString().slice(0, 10);
const logPath = path.join(logsDir, `verify-${today}.txt`);
const logStream = fs.createWriteStream(logPath, { flags: "w" });

function log(message) {
  process.stdout.write(message + "\n");
  logStream.write(message + "\n");
}

function runStep(name, command, args, cwd = rootDir) {
  log(`\n======================================================`);
  log(`STEP: ${name}`);
  log(`COMMAND: ${command} ${args.join(" ")}`);
  log(`CWD: ${cwd}`);
  log(`======================================================\n`);

  // Ensure cargo path and native dependencies are included in PATH and git global config avoids permission issues
  const isWindows = process.platform === "win32";
  const homeDir = process.env.HOME || process.env.USERPROFILE || "";
  const cargoBin = isWindows
    ? path.join(homeDir, ".cargo", "bin")
    : `${homeDir}/.cargo/bin`;
  const vipsBin = isWindows
    ? (process.env.VIPS_DIR ? path.join(process.env.VIPS_DIR, "bin") : path.join(rootDir, "deps", "vips", "bin"))
    : null;
  const extraPaths = isWindows
    ? [cargoBin, vipsBin].filter(Boolean)
    : [cargoBin, "/opt/homebrew/bin", "/usr/local/bin"];

  const env = {
    ...process.env,
    GIT_CONFIG_GLOBAL: isWindows ? "NUL" : "/dev/null",
    PATH: `${extraPaths.join(path.delimiter)}${path.delimiter}${process.env.PATH || ""}`,
  };

  const result = spawnSync(command, args, {
    cwd,
    env,
    encoding: "utf-8",
    stdio: ["ignore", "pipe", "pipe"],
    shell: isWindows,
  });

  if (result.stdout) {
    process.stdout.write(result.stdout);
    logStream.write(result.stdout);
  }
  if (result.stderr) {
    process.stderr.write(result.stderr);
    logStream.write(result.stderr);
  }
  if (result.error) {
    process.stderr.write(`Spawn error: ${result.error.message}\n`);
    logStream.write(`Spawn error: ${result.error.message}\n`);
  }

  if (result.status !== 0) {
    log(`\n❌ FAILED: ${name} (exit code ${result.status})`);
    logStream.end();
    process.exit(result.status ?? 1);
  }

  log(`\n✓ PASSED: ${name}`);
}

log(`Photos Manager — pnpm verify verification run`);
log(`Date: ${new Date().toISOString()}`);
log(`Log output file: ${logPath}`);

// 1. Cargo fmt check
runStep(
  "cargo fmt check",
  "cargo",
  ["fmt", "--manifest-path", "src-tauri/Cargo.toml", "--", "--check"]
);

// 2. Cargo clippy
runStep(
  "cargo clippy",
  "cargo",
  ["clippy", "--manifest-path", "src-tauri/Cargo.toml", "--", "-D", "warnings"]
);

// 3. Cargo test
runStep(
  "cargo test",
  "cargo",
  ["test", "--manifest-path", "src-tauri/Cargo.toml"]
);

// 4. Frontend typecheck
runStep("frontend typecheck", "pnpm", ["exec", "tsc", "--noEmit"]);

// 5. Frontend lint
runStep("frontend lint", "pnpm", ["exec", "eslint", "src/"]);

// 6. Production build
runStep("production build", "pnpm", ["exec", "vite", "build"]);

// 7. AGENTS.md length check (< 12000 chars)
log(`\n======================================================`);
log(`STEP: AGENTS.md length check (< 12,000 characters)`);
log(`======================================================\n`);
const agentsContent = fs.readFileSync(path.join(rootDir, "AGENTS.md"), "utf-8");
const agentsCharCount = agentsContent.length;
log(`AGENTS.md character count: ${agentsCharCount}`);
if (agentsCharCount >= 12000) {
  log(`❌ FAILED: AGENTS.md has ${agentsCharCount} characters (must be < 12000)`);
  process.exit(1);
} else {
  log(`✓ PASSED: AGENTS.md character count is under 12,000`);
}

log(`\n======================================================`);
log(`ALL VERIFICATION CHECKS PASSED SUCCESSFULLY!`);
log(`======================================================\n`);

logStream.end();
