import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const STAGING_SCRIPT = path.resolve(__dirname, "..", "scripts", "stage-platform-packages.mjs");

test("npm pack, install --ignore-scripts, and stdio JSON hygiene smoke test", () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "test-smoke-npm-"));
  try {
    const isWin = process.platform === "win32";
    const distDir = path.join(tmpDir, "dist");
    const outDir = path.join(tmpDir, "staged");
    fs.mkdirSync(distDir, { recursive: true });

    // Build synthetic native binary that acts like anki-workflow-mcp serve
    const targets = [
      { target: "x86_64-unknown-linux-gnu", bin: "anki-workflow-mcp", ext: "tar.gz" },
      { target: "aarch64-unknown-linux-gnu", bin: "anki-workflow-mcp", ext: "tar.gz" },
      { target: "x86_64-pc-windows-msvc", bin: "anki-workflow-mcp.exe", ext: "zip" },
      { target: "aarch64-apple-darwin", bin: "anki-workflow-mcp", ext: "tar.gz" },
      { target: "x86_64-apple-darwin", bin: "anki-workflow-mcp", ext: "tar.gz" },
    ];

    for (const t of targets) {
      const buildDir = path.join(tmpDir, `build-${t.target}`);
      fs.mkdirSync(buildDir, { recursive: true });
      const binPath = path.join(buildDir, t.bin);

      if (t.bin.endsWith(".exe") && isWin) {
        const cSource = path.join(buildDir, "main.c");
        fs.writeFileSync(
          cSource,
          `#include <stdio.h>\n#include <string.h>\nint main(int argc, char **argv) {\n  if (argc > 1 && strcmp(argv[1], "serve") == 0) {\n    char buf[1024];\n    if (fgets(buf, sizeof(buf), stdin)) {\n      printf("{\\"jsonrpc\\":\\"2.0\\",\\"id\\":1,\\"result\\":{\\"serverInfo\\":{\\"name\\":\\"anki-workflow-mcp\\",\\"version\\":\\"0.1.0\\"}}}\\\\n");\n      fflush(stdout);\n      return 0;\n    }\n  }\n  return 1;\n}\n`
            .replace("\\\\n", "\\n")
        );
        execFileSync("gcc", ["-O2", "-o", binPath, cSource]);
      } else {
        fs.writeFileSync(
          binPath,
          `#!/bin/sh\nif [ "$1" = "serve" ]; then\n  read line\n  printf '{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"anki-workflow-mcp","version":"0.1.0"}}}\\n'\n  exit 0\nfi\nexit 1\n`,
          { mode: 0o755 }
        );
      }

      const archiveName = `anki-workflow-mcp-v0.1.0-${t.target}.${t.ext}`;
      const archivePath = path.join(distDir, archiveName);

      if (t.ext === "tar.gz") {
        const normArchive = archivePath.replaceAll("\\", "/");
        const normBuild = buildDir.replaceAll("\\", "/");
        try {
          execFileSync("tar", ["-czf", normArchive, "-C", normBuild, t.bin], {
            stdio: ["ignore", "ignore", "ignore"],
          });
        } catch {
          execFileSync("tar", ["--force-local", "-czf", normArchive, "-C", normBuild, t.bin]);
        }
      } else if (t.ext === "zip") {
        const pyZip = `import zipfile, sys; z = zipfile.ZipFile(sys.argv[1], 'w'); z.write(sys.argv[2], sys.argv[3]); z.close()`;
        execFileSync("python", ["-c", pyZip, archivePath, binPath, t.bin]);
      }
    }

    const nodePath = process.execPath;
    execFileSync(
      nodePath,
      [
        STAGING_SCRIPT,
        "--version",
        "0.1.0",
        "--dist-dir",
        distDir,
        "--out-dir",
        outDir,
      ],
      { stdio: "inherit" }
    );

    // npm pack all packages
    const tarballsDir = path.join(tmpDir, "tarballs");
    fs.mkdirSync(tarballsDir, { recursive: true });

    const npmCmd = isWin ? "npm.cmd" : "npm";
    const platformDirs = [
      path.join(outDir, "platforms", "linux-x64-gnu"),
      path.join(outDir, "platforms", "linux-arm64-gnu"),
      path.join(outDir, "platforms", "win32-x64"),
      path.join(outDir, "platforms", "darwin-arm64"),
      path.join(outDir, "platforms", "darwin-x64"),
      path.join(outDir, "main"),
    ];

    for (const pDir of platformDirs) {
      execFileSync(npmCmd, ["pack", pDir, "--pack-destination", tarballsDir], {
        stdio: "inherit",
        shell: isWin,
      });
    }

    // Install into a clean smoke test project
    const smokeDir = path.join(tmpDir, "smoke-app");
    fs.mkdirSync(smokeDir, { recursive: true });
    execFileSync(npmCmd, ["init", "-y"], { cwd: smokeDir, stdio: "ignore", shell: isWin });

    // Identify current platform tarball + main tarball
    const currentPlatformSlug = isWin
      ? "win32-x64"
      : process.arch === "arm64"
        ? "linux-arm64-gnu"
        : "linux-x64-gnu";
    const currentPlatformTgz = path.join(
      tarballsDir,
      `gugart43-anki-workflow-mcp-${currentPlatformSlug}-0.1.0.tgz`
    );
    const mainTgz = path.join(tarballsDir, "gugart43-anki-workflow-mcp-0.1.0.tgz");

    execFileSync(
      npmCmd,
      ["install", "--ignore-scripts", "--no-audit", "--no-fund", currentPlatformTgz, mainTgz],
      { cwd: smokeDir, stdio: "inherit", shell: isWin }
    );

    const launcherBin = path.join(smokeDir, "node_modules", ".bin", isWin ? "anki-workflow-mcp.cmd" : "anki-workflow-mcp");
    const initReq = JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion: "2025-03-26",
        capabilities: {},
        clientInfo: { name: "smoke-test", version: "0.1.0" },
      },
    });

    // 1) Test MCP initialize over stdio + strict stdout JSON hygiene
    const runRes = spawnSync(launcherBin, ["serve"], {
      input: `${initReq}\n`,
      encoding: "utf8",
      shell: isWin,
    });

    assert.equal(runRes.status, 0, `Launcher failed: ${runRes.stderr}`);
    const stdoutRaw = runRes.stdout.trim();
    assert.ok(stdoutRaw, "Stdout should not be empty");
    const firstLine = stdoutRaw.split(/\r?\n/)[0];
    const msg = JSON.parse(firstLine);
    assert.equal(msg.jsonrpc, "2.0");
    assert.equal(msg.id, 1);
    assert.equal(msg.result.serverInfo.name, "anki-workflow-mcp");

    // 2) ANKI_WORKFLOW_MCP_BIN override
    const overrideBinPath = path.join(
      outDir,
      "platforms",
      currentPlatformSlug,
      "bin",
      isWin ? "anki-workflow-mcp.exe" : "anki-workflow-mcp"
    );
    const overrideRes = spawnSync(launcherBin, ["serve"], {
      input: `${initReq}\n`,
      encoding: "utf8",
      env: {
        ...process.env,
        ANKI_WORKFLOW_MCP_BIN: overrideBinPath,
      },
      shell: isWin,
    });
    assert.equal(overrideRes.status, 0);
    const overrideMsg = JSON.parse(overrideRes.stdout.trim().split(/\r?\n/)[0]);
    assert.equal(overrideMsg.jsonrpc, "2.0");
    assert.equal(overrideMsg.id, 1);

    // 3) Tampered checksums.json must fail closed with non-zero exit and empty stdout
    const checksumsFile = path.join(
      smokeDir,
      "node_modules",
      "@gugart43",
      "anki-workflow-mcp",
      "checksums.json"
    );
    const chkData = JSON.parse(fs.readFileSync(checksumsFile, "utf8"));
    const pkgKey = `@gugart43/anki-workflow-mcp-${currentPlatformSlug}`;
    chkData.packages[pkgKey].sha256 = "0".repeat(64);
    fs.writeFileSync(checksumsFile, JSON.stringify(chkData, null, 2), "utf8");

    const tamperRes = spawnSync(launcherBin, ["serve"], {
      input: `${initReq}\n`,
      encoding: "utf8",
      shell: isWin,
    });
    assert.notEqual(tamperRes.status, 0, "Tampered binary must exit with non-zero status");
    assert.equal(tamperRes.stdout.trim(), "", "Tampered binary must not output to stdout");
    assert.ok(
      tamperRes.stderr.includes("SHA-256 integrity mismatch"),
      `Expected stderr to report integrity mismatch, got: ${tamperRes.stderr}`
    );
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});
