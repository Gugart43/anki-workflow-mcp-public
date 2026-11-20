#!/usr/bin/env node
"use strict";

const { spawn } = require("node:child_process");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const RELEASES_URL =
  "https://github.com/Gugart43/anki-workflow-mcp-public/releases";

const TARGETS = Object.freeze({
  "linux:x64:glibc": {
    pkg: "@gugart43/anki-workflow-mcp-linux-x64-gnu",
    bin: "bin/anki-workflow-mcp",
  },
  "win32:x64:": {
    pkg: "@gugart43/anki-workflow-mcp-win32-x64",
    bin: "bin/anki-workflow-mcp.exe",
  },
  "darwin:arm64:": {
    pkg: "@gugart43/anki-workflow-mcp-darwin-arm64",
    bin: "bin/anki-workflow-mcp",
  },
  "darwin:x64:": {
    pkg: "@gugart43/anki-workflow-mcp-darwin-x64",
    bin: "bin/anki-workflow-mcp",
  },
});

function fail(message) {
  process.stderr.write(`[anki-workflow-mcp] ${message}\n`);
  if (process.env.NODE_ENV === "test" || process.env.NODE_TEST_CONTEXT) {
    throw new Error(message);
  }
  process.exit(1);
}

function detectLinuxLibc(probes = {}) {
  const getReport =
    probes.getReport !== undefined
      ? probes.getReport
      : process.report && typeof process.report.getReport === "function"
        ? () => process.report.getReport()
        : null;
  const existsSync = probes.existsSync || fs.existsSync;
  const readdirSync = probes.readdirSync || fs.readdirSync;

  try {
    if (getReport) {
      const report = getReport();
      const glibc = report && report.header && report.header.glibcVersionRuntime;
      if (typeof glibc === "string" && glibc.length > 0) {
        return "glibc";
      }
    }
  } catch {
    // Fallback below if process.report probe fails
  }

  try {
    if (existsSync("/etc/alpine-release")) {
      return "musl";
    }
    for (const dir of ["/lib", "/usr/lib"]) {
      if (
        existsSync(dir) &&
        readdirSync(dir).some((entry) => entry.startsWith("ld-musl-"))
      ) {
        return "musl";
      }
    }
  } catch {
    // Ignore filesystem probe errors and continue
  }

  try {
    for (const p of [
      "/lib/x86_64-linux-gnu/libc.so.6",
      "/lib64/ld-linux-x86-64.so.2",
      "/usr/lib/libc.so.6",
    ]) {
      if (existsSync(p)) {
        return "glibc";
      }
    }
  } catch {
    // Ignore filesystem probe errors
  }

  return "unknown";
}

function resolveOverrideBinary(env = process.env) {
  const raw = env.ANKI_WORKFLOW_MCP_BIN;
  if (!raw || raw.trim() === "") {
    return null;
  }

  const resolved = path.resolve(raw.trim());
  let stat;
  try {
    stat = fs.statSync(resolved);
  } catch (err) {
    fail(
      `ANKI_WORKFLOW_MCP_BIN points to a missing path (${resolved}): ${err.message}`
    );
  }

  if (!stat.isFile()) {
    fail(`ANKI_WORKFLOW_MCP_BIN is not a regular file: ${resolved}`);
  }

  return resolved;
}

function resolvePlatformTarget(options = {}) {
  const platform = options.platform || process.platform;
  const arch = options.arch || process.arch;
  const libc =
    options.libc !== undefined
      ? options.libc
      : platform === "linux"
        ? detectLinuxLibc()
        : "";

  if (platform === "linux" && libc !== "glibc") {
    fail(
      `Unsupported Linux libc (${libc}). Only glibc (x86_64-unknown-linux-gnu) is published. ` +
        `Build from source or set ANKI_WORKFLOW_MCP_BIN (${RELEASES_URL}).`
    );
  }

  const key = `${platform}:${arch}:${libc}`;
  const target = TARGETS[key];
  if (!target) {
    fail(
      `Unsupported platform ${platform}/${arch}${libc ? ` (${libc})` : ""}. ` +
        `Supported: linux/x64 (glibc), win32/x64, darwin/arm64, darwin/x64. ` +
        `You can override with ANKI_WORKFLOW_MCP_BIN (${RELEASES_URL}).`
    );
  }

  return target;
}

function resolvePackageBinary(target) {
  let pkgJsonPath;
  try {
    pkgJsonPath = require.resolve(`${target.pkg}/package.json`);
  } catch {
    fail(
      `Optional platform package "${target.pkg}" is not installed. ` +
        `Reinstall without --omit=optional / --no-optional, or set ANKI_WORKFLOW_MCP_BIN (${RELEASES_URL}).`
    );
  }

  const binaryPath = path.join(path.dirname(pkgJsonPath), target.bin);
  if (!fs.existsSync(binaryPath)) {
    fail(`Expected binary not found inside ${target.pkg}: ${binaryPath}`);
  }

  return binaryPath;
}

function verifyBinaryChecksum(pkgName, binaryPath, customChecksumsPath) {
  const checksumsPath =
    customChecksumsPath || path.join(__dirname, "..", "checksums.json");
  let manifest;
  try {
    manifest = JSON.parse(fs.readFileSync(checksumsPath, "utf8"));
  } catch (err) {
    fail(`Failed to read integrity manifest (${checksumsPath}): ${err.message}`);
  }

  const entry =
    manifest && manifest.packages && manifest.packages[pkgName];
  const expected =
    entry && typeof entry.sha256 === "string"
      ? entry.sha256.trim().toLowerCase()
      : "";

  if (!/^[0-9a-f]{64}$/.test(expected)) {
    fail(
      `Invalid or missing SHA-256 entry for "${pkgName}" in checksums.json.`
    );
  }

  let actual;
  try {
    const bytes = fs.readFileSync(binaryPath);
    actual = crypto.createHash("sha256").update(bytes).digest("hex");
  } catch (err) {
    fail(`Failed to hash binary (${binaryPath}): ${err.message}`);
  }

  const expectedBuf = Buffer.from(expected, "hex");
  const actualBuf = Buffer.from(actual, "hex");
  if (
    expectedBuf.length !== actualBuf.length ||
    !crypto.timingSafeEqual(expectedBuf, actualBuf)
  ) {
    fail(
      `SHA-256 integrity mismatch for "${pkgName}" (${binaryPath}).\n` +
        `  expected: ${expected}\n` +
        `  actual:   ${actual}`
    );
  }
}

function runBinary(binaryPath) {
  const child = spawn(binaryPath, process.argv.slice(2), {
    stdio: "inherit",
    windowsHide: true,
  });

  const forwardedSignals =
    process.platform === "win32"
      ? ["SIGINT", "SIGTERM"]
      : ["SIGINT", "SIGTERM", "SIGHUP"];

  const signalHandlers = new Map();
  for (const sig of forwardedSignals) {
    const handler = () => {
      if (!child.killed && child.exitCode === null) {
        try {
          child.kill(sig);
        } catch {
          // Child may have already exited.
        }
      }
    };
    signalHandlers.set(sig, handler);
    process.on(sig, handler);
  }

  const cleanupHandlers = () => {
    for (const [sig, handler] of signalHandlers.entries()) {
      process.removeListener(sig, handler);
    }
  };

  child.on("error", (err) => {
    cleanupHandlers();
    fail(`Failed to execute ${binaryPath}: ${err.message}`);
  });

  child.on("exit", (code, signal) => {
    cleanupHandlers();
    if (signal) {
      const sigNum =
        os.constants && os.constants.signals && os.constants.signals[signal];
      if (process.platform !== "win32") {
        try {
          process.kill(process.pid, signal);
          return;
        } catch {
          // Fall back to POSIX 128 + signal exit code.
        }
      }
      process.exit(typeof sigNum === "number" ? 128 + sigNum : 1);
    }
    process.exit(typeof code === "number" ? code : 1);
  });
}

function main() {
  const overridePath = resolveOverrideBinary();
  if (overridePath) {
    runBinary(overridePath);
    return;
  }

  const target = resolvePlatformTarget();
  const binaryPath = resolvePackageBinary(target);
  verifyBinaryChecksum(target.pkg, binaryPath);
  runBinary(binaryPath);
}

if (require.main === module) {
  main();
} else {
  module.exports = {
    TARGETS,
    fail,
    detectLinuxLibc,
    resolveOverrideBinary,
    resolvePlatformTarget,
    resolvePackageBinary,
    verifyBinaryChecksum,
    runBinary,
    main,
  };
}
