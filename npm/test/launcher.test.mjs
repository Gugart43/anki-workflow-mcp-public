import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import crypto from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const LAUNCHER_PATH = path.resolve(__dirname, "..", "anki-workflow-mcp", "bin", "anki-workflow-mcp.cjs");

test("launcher exports helper functions when imported or required", async () => {
  const launcher = await import(`file://${LAUNCHER_PATH}`);
  assert.ok(launcher);
  assert.equal(typeof launcher.detectLinuxLibc, "function");
  assert.equal(typeof launcher.resolveOverrideBinary, "function");
  assert.equal(typeof launcher.resolvePlatformTarget, "function");
  assert.equal(typeof launcher.verifyBinaryChecksum, "function");
  assert.equal(typeof launcher.TARGETS, "object");
});

test("detectLinuxLibc handles glibc, musl, and fails closed with unknown", async () => {
  const launcher = await import(`file://${LAUNCHER_PATH}`);
  const { detectLinuxLibc } = launcher;

  // Custom mock probe
  const libcAlpine = detectLinuxLibc({
    getReport: () => ({ header: {} }),
    existsSync: (p) => p === "/etc/alpine-release",
    readdirSync: () => [],
  });
  assert.equal(libcAlpine, "musl");

  const libcMuslDir = detectLinuxLibc({
    getReport: () => ({ header: {} }),
    existsSync: (p) => p === "/lib",
    readdirSync: (p) => (p === "/lib" ? ["ld-musl-x86_64.so.1"] : []),
  });
  assert.equal(libcMuslDir, "musl");

  const libcGlibcReport = detectLinuxLibc({
    getReport: () => ({ header: { glibcVersionRuntime: "2.35" } }),
    existsSync: () => false,
    readdirSync: () => [],
  });
  assert.equal(libcGlibcReport, "glibc");

  const libcGlibcFile = detectLinuxLibc({
    getReport: () => ({ header: {} }),
    existsSync: (p) => p === "/lib/x86_64-linux-gnu/libc.so.6",
    readdirSync: () => [],
  });
  assert.equal(libcGlibcFile, "glibc");

  // If nothing identifies glibc or musl, it must return 'unknown' and fail closed
  const libcUnknown = detectLinuxLibc({
    getReport: () => null,
    existsSync: () => false,
    readdirSync: () => [],
  });
  assert.equal(libcUnknown, "unknown");
});

test("resolvePlatformTarget fails closed for musl or unknown linux libc", async () => {
  const launcher = await import(`file://${LAUNCHER_PATH}`);
  const { resolvePlatformTarget } = launcher;

  // Linux x64 glibc succeeds
  const linuxGlibc = resolvePlatformTarget({ platform: "linux", arch: "x64", libc: "glibc" });
  assert.equal(linuxGlibc.pkg, "@gugart43/anki-workflow-mcp-linux-x64-gnu");

  // Linux x64 musl fails closed
  assert.throws(
    () => resolvePlatformTarget({ platform: "linux", arch: "x64", libc: "musl" }),
    /Unsupported Linux libc \(musl\)/
  );

  // Linux x64 unknown libc fails closed
  assert.throws(
    () => resolvePlatformTarget({ platform: "linux", arch: "x64", libc: "unknown" }),
    /Unsupported Linux libc \(unknown\)/
  );

  // win32 x64 succeeds
  const win = resolvePlatformTarget({ platform: "win32", arch: "x64", libc: "" });
  assert.equal(win.pkg, "@gugart43/anki-workflow-mcp-win32-x64");

  // darwin arm64 succeeds
  const darwinArm = resolvePlatformTarget({ platform: "darwin", arch: "arm64", libc: "" });
  assert.equal(darwinArm.pkg, "@gugart43/anki-workflow-mcp-darwin-arm64");

  // darwin x64 succeeds
  const darwinX64 = resolvePlatformTarget({ platform: "darwin", arch: "x64", libc: "" });
  assert.equal(darwinX64.pkg, "@gugart43/anki-workflow-mcp-darwin-x64");

  // unsupported arch/os fails
  assert.throws(
    () => resolvePlatformTarget({ platform: "linux", arch: "arm64", libc: "glibc" }),
    /Unsupported platform linux\/arm64/
  );
  assert.throws(
    () => resolvePlatformTarget({ platform: "freebsd", arch: "x64", libc: "" }),
    /Unsupported platform freebsd\/x64/
  );
});

test("resolveOverrideBinary handles environment override safely", async () => {
  const launcher = await import(`file://${LAUNCHER_PATH}`);
  const { resolveOverrideBinary } = launcher;

  // Unset or whitespace
  assert.equal(resolveOverrideBinary({ ANKI_WORKFLOW_MCP_BIN: "" }), null);
  assert.equal(resolveOverrideBinary({ ANKI_WORKFLOW_MCP_BIN: "   " }), null);
  assert.equal(resolveOverrideBinary({}), null);

  // Non-existent path fails closed
  const fakeMissing = path.join(os.tmpdir(), "missing-binary-xyz-" + Date.now());
  assert.throws(
    () => resolveOverrideBinary({ ANKI_WORKFLOW_MCP_BIN: fakeMissing }),
    /ANKI_WORKFLOW_MCP_BIN points to a missing path/
  );

  // Directory fails closed
  assert.throws(
    () => resolveOverrideBinary({ ANKI_WORKFLOW_MCP_BIN: os.tmpdir() }),
    /ANKI_WORKFLOW_MCP_BIN is not a regular file/
  );

  // Valid regular file returns resolved path
  const tmpFile = path.join(os.tmpdir(), "valid-override-" + Date.now());
  fs.writeFileSync(tmpFile, "dummy");
  try {
    const resolved = resolveOverrideBinary({ ANKI_WORKFLOW_MCP_BIN: tmpFile });
    assert.equal(resolved, path.resolve(tmpFile));
  } finally {
    fs.rmSync(tmpFile, { force: true });
  }
});

test("verifyBinaryChecksum verifies SHA-256 with timingSafeEqual and fails closed on tampering", async () => {
  const launcher = await import(`file://${LAUNCHER_PATH}`);
  const { verifyBinaryChecksum } = launcher;

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "test-launcher-chk-"));
  try {
    const binFile = path.join(tmpDir, "fake-binary");
    fs.writeFileSync(binFile, "Hello World MCP Binary");
    const realHash = crypto.createHash("sha256").update("Hello World MCP Binary").digest("hex");

    const checksums = {
      version: "0.1.0",
      packages: {
        "@test/pkg": {
          sha256: realHash,
        },
      },
    };
    const checksumsPath = path.join(tmpDir, "checksums.json");
    fs.writeFileSync(checksumsPath, JSON.stringify(checksums, null, 2));

    // Matching checksum succeeds without throwing
    assert.doesNotThrow(() => {
      verifyBinaryChecksum("@test/pkg", binFile, checksumsPath);
    });

    // Corrupted checksum fails closed with timing-safe check
    const badChecksums = {
      version: "0.1.0",
      packages: {
        "@test/pkg": {
          sha256: "0".repeat(64),
        },
      },
    };
    fs.writeFileSync(checksumsPath, JSON.stringify(badChecksums, null, 2));
    assert.throws(
      () => verifyBinaryChecksum("@test/pkg", binFile, checksumsPath),
      /SHA-256 integrity mismatch/
    );

    // Missing package entry
    const missingPkg = { version: "0.1.0", packages: {} };
    fs.writeFileSync(checksumsPath, JSON.stringify(missingPkg, null, 2));
    assert.throws(
      () => verifyBinaryChecksum("@test/pkg", binFile, checksumsPath),
      /Invalid or missing SHA-256 entry/
    );

    // Invalid sha256 format (non-hex or wrong length)
    const malformedPkg = {
      version: "0.1.0",
      packages: { "@test/pkg": { sha256: "not-a-valid-sha" } },
    };
    fs.writeFileSync(checksumsPath, JSON.stringify(malformedPkg, null, 2));
    assert.throws(
      () => verifyBinaryChecksum("@test/pkg", binFile, checksumsPath),
      /Invalid or missing SHA-256 entry/
    );
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("launcher end-to-end forwards stdout/stderr cleanly with no spurious stdout logs", () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "test-launcher-e2e-"));
  try {
    const isWin = process.platform === "win32";
    const binName = isWin ? "mock-bin.exe" : "mock-bin";
    const mockBin = path.join(tmpDir, binName);

    if (isWin) {
      const cSource = path.join(tmpDir, "mock.c");
      fs.writeFileSync(
        cSource,
        `#include <stdio.h>\n#include <string.h>\nint main(int argc, char **argv) {\n  if (argc > 1 && strcmp(argv[1], "mcp-test") == 0) {\n    printf("{\\"jsonrpc\\":\\"2.0\\",\\"id\\":1,\\"result\\":\\"ok\\"}\\n");\n    fprintf(stderr, "log to stderr\\n");\n    return 0;\n  }\n  return 2;\n}\n`
      );
      execFileSync("gcc", ["-O2", "-o", mockBin, cSource]);
    } else {
      fs.writeFileSync(
        mockBin,
        `#!/bin/sh\nif [ "$1" = "mcp-test" ]; then\n  printf '{"jsonrpc":"2.0","id":1,"result":"ok"}\\n'\n  echo "log to stderr" >&2\n  exit 0\nfi\nexit 2\n`,
        { mode: 0o755 }
      );
    }

    const nodePath = process.execPath;
    const res = spawnSync(nodePath, [LAUNCHER_PATH, "mcp-test"], {
      env: {
        ...process.env,
        ANKI_WORKFLOW_MCP_BIN: mockBin,
      },
      encoding: "utf8",
    });

    assert.equal(res.status, 0);
    // STDOUT must be STRICTLY the single line JSON without launcher debug banner or clutter
    const stdoutLines = res.stdout.trim().split(/\r?\n/);
    assert.equal(stdoutLines.length, 1);
    const jsonMsg = JSON.parse(stdoutLines[0]);
    assert.equal(jsonMsg.jsonrpc, "2.0");
    assert.equal(jsonMsg.id, 1);
    assert.equal(jsonMsg.result, "ok");

    // STDERR should receive child stderr
    assert.ok(res.stderr.includes("log to stderr"));
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
