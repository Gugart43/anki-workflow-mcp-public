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
const STAGING_SCRIPT = path.resolve(__dirname, "..", "scripts", "stage-platform-packages.mjs");

test("staging module exports helper functions in test mode", async () => {
  const staging = await import(`file://${STAGING_SCRIPT}`);
  assert.ok(staging);
  assert.equal(typeof staging.validateSemver, "function");
  assert.equal(typeof staging.validateOutDir, "function");
  assert.equal(typeof staging.validatePackageIntegrity, "function");
  assert.equal(typeof staging.inspectAndExtractArchive, "function");
  assert.equal(typeof staging.PLATFORMS, "object");
});

test("validateSemver enforces strict semver format", async () => {
  const staging = await import(`file://${STAGING_SCRIPT}`);
  const { validateSemver } = staging;

  assert.equal(validateSemver("0.1.0"), "0.1.0");
  assert.equal(validateSemver("1.2.3-alpha.1"), "1.2.3-alpha.1");
  assert.equal(validateSemver("v0.1.0"), "0.1.0");

  assert.throws(() => validateSemver(""), /Invalid semver/);
  assert.throws(() => validateSemver("0.1"), /Invalid semver/);
  assert.throws(() => validateSemver("1.0.0.0"), /Invalid semver/);
  assert.throws(() => validateSemver("v1"), /Invalid semver/);
  assert.throws(() => validateSemver("latest"), /Invalid semver/);
});

test("validateOutDir protects against dangerous directory deletions", async () => {
  const staging = await import(`file://${STAGING_SCRIPT}`);
  const { validateOutDir } = staging;

  const repoRoot = path.resolve(__dirname, "..", "..");
  const rootDir = path.parse(repoRoot).root;

  // Root directory is dangerous
  assert.throws(() => validateOutDir(rootDir, repoRoot), /Dangerous out-dir: root/);

  // REPO_ROOT is dangerous
  assert.throws(() => validateOutDir(repoRoot, repoRoot), /Dangerous out-dir: repo root/);

  // Inside .git is dangerous
  assert.throws(
    () => validateOutDir(path.join(repoRoot, ".git"), repoRoot),
    /Dangerous out-dir: git internal/
  );
  assert.throws(
    () => validateOutDir(path.join(repoRoot, ".git", "objects"), repoRoot),
    /Dangerous out-dir: git internal/
  );

  // Empty or relative root
  assert.throws(() => validateOutDir(".", repoRoot), /Dangerous out-dir/);

  // Safe staging folder inside repo or temp dir is allowed
  const safePath = path.join(repoRoot, "npm-stage-test");
  assert.equal(validateOutDir(safePath, repoRoot), safePath);
});

test("validatePackageIntegrity verifies Cargo.toml and package.json versions and dependencies", async () => {
  const staging = await import(`file://${STAGING_SCRIPT}`);
  const { validatePackageIntegrity, PLATFORMS } = staging;

  const validMainPkg = {
    name: "@gugart43/anki-workflow-mcp",
    version: "0.1.0",
    dependencies: {},
    optionalDependencies: {
      "@gugart43/anki-workflow-mcp-linux-x64-gnu": "0.1.0",
      "@gugart43/anki-workflow-mcp-linux-arm64-gnu": "0.1.0",
      "@gugart43/anki-workflow-mcp-win32-x64": "0.1.0",
      "@gugart43/anki-workflow-mcp-darwin-arm64": "0.1.0",
      "@gugart43/anki-workflow-mcp-darwin-x64": "0.1.0",
    },
  };
  const validCargoToml = '[workspace.package]\nversion = "0.1.0"\n';

  // Valid configuration passes
  assert.doesNotThrow(() => {
    validatePackageIntegrity("0.1.0", validMainPkg, validCargoToml, PLATFORMS);
  });

  // Version mismatch with --version
  assert.throws(
    () => validatePackageIntegrity("0.2.0", validMainPkg, validCargoToml, PLATFORMS),
    /Version mismatch between --version \(0\.2\.0\) and package\.json \(0\.1\.0\)/
  );

  // Version mismatch with Cargo.toml
  const diffCargo = '[workspace.package]\nversion = "0.2.0"\n';
  assert.throws(
    () => validatePackageIntegrity("0.1.0", validMainPkg, diffCargo, PLATFORMS),
    /Version mismatch between package\.json \(0\.1\.0\) and Cargo\.toml \(0\.2\.0\)/
  );

  // Non-empty dependencies forbidden
  const withDeps = { ...validMainPkg, dependencies: { foo: "1.0.0" } };
  assert.throws(
    () => validatePackageIntegrity("0.1.0", withDeps, validCargoToml, PLATFORMS),
    /Main package must have zero dependencies/
  );

  // postinstall forbidden
  const withPostinstall = { ...validMainPkg, scripts: { postinstall: "echo bad" } };
  assert.throws(
    () => validatePackageIntegrity("0.1.0", withPostinstall, validCargoToml, PLATFORMS),
    /Main package must not define postinstall/
  );

  // Mismatched optionalDependencies
  const badOpt = {
    ...validMainPkg,
    optionalDependencies: {
      ...validMainPkg.optionalDependencies,
      "@gugart43/anki-workflow-mcp-linux-x64-gnu": "0.2.0",
    },
  };
  assert.throws(
    () => validatePackageIntegrity("0.1.0", badOpt, validCargoToml, PLATFORMS),
    /optionalDependencies.*must be exact/
  );
});

function createTarGz(archivePath, cwdDir, files) {
  const normArchive = archivePath.replaceAll("\\", "/");
  const normCwd = cwdDir.replaceAll("\\", "/");
  try {
    execFileSync("tar", ["-czf", normArchive, "-C", normCwd, ...files], {
      stdio: ["ignore", "ignore", "ignore"],
    });
  } catch {
    execFileSync("tar", ["--force-local", "-czf", normArchive, "-C", normCwd, ...files]);
  }
}

test("inspectAndExtractArchive detects traversal and unexpected members", async () => {
  const staging = await import(`file://${STAGING_SCRIPT}`);
  const { inspectAndExtractArchive } = staging;

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "test-staging-archive-"));
  try {
    const srcDir = path.join(tmpDir, "src");
    fs.mkdirSync(srcDir, { recursive: true });
    fs.writeFileSync(path.join(srcDir, "anki-workflow-mcp"), "binary-content");
    fs.writeFileSync(path.join(srcDir, "evil.sh"), "evil");

    const badArchive = path.join(tmpDir, "bad.tar.gz");
    createTarGz(badArchive, srcDir, ["anki-workflow-mcp", "evil.sh"]);

    const destDir = path.join(tmpDir, "dest");
    fs.mkdirSync(destDir, { recursive: true });

    // Should reject because of unexpected member evil.sh
    assert.throws(
      () => inspectAndExtractArchive(badArchive, "tar.gz", "anki-workflow-mcp", destDir),
      /Unexpected member in archive: evil\.sh/
    );

    // Valid archive with single binary succeeds
    const goodArchive = path.join(tmpDir, "good.tar.gz");
    createTarGz(goodArchive, srcDir, ["anki-workflow-mcp"]);
    const goodDest = path.join(tmpDir, "dest-good");
    const extracted = inspectAndExtractArchive(goodArchive, "tar.gz", "anki-workflow-mcp", goodDest);
    assert.equal(extracted, path.join(goodDest, "anki-workflow-mcp"));
    assert.equal(fs.readFileSync(extracted, "utf8"), "binary-content");
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test("full staging run generates complete platform and main packages with checksums", () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "test-staging-full-"));
  try {
    const isWin = process.platform === "win32";
    const distDir = path.join(tmpDir, "dist");
    const outDir = path.join(tmpDir, "staged");
    fs.mkdirSync(distDir, { recursive: true });

    // Create 5 synthetic platform archives
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
        fs.writeFileSync(cSource, "int main() { return 0; }\n");
        execFileSync("gcc", ["-O2", "-o", binPath, cSource]);
      } else {
        fs.writeFileSync(binPath, `synthetic-binary-${t.target}`);
      }

      const archiveName = `anki-workflow-mcp-v0.1.0-${t.target}.${t.ext}`;
      const archivePath = path.join(distDir, archiveName);

      if (t.ext === "tar.gz") {
        createTarGz(archivePath, buildDir, [t.bin]);
      } else if (t.ext === "zip") {
        // Zip creation using python or zip
        const pyZip = `import zipfile, sys; z = zipfile.ZipFile(sys.argv[1], 'w'); z.write(sys.argv[2], sys.argv[3]); z.close()`;
        execFileSync(process.execPath ? "python" : "python", ["-c", pyZip, archivePath, binPath, t.bin]);
      }
    }

    const nodePath = process.execPath;
    const res = spawnSync(
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
      { encoding: "utf8" }
    );

    assert.equal(res.status, 0, `Staging script failed: ${res.stderr}`);
    assert.ok(res.stdout.includes("Staged 5 platform packages"));

    // Verify main package staged files
    const mainDir = path.join(outDir, "main");
    assert.ok(fs.existsSync(path.join(mainDir, "package.json")));
    assert.ok(fs.existsSync(path.join(mainDir, "bin", "anki-workflow-mcp.cjs")));
    assert.ok(fs.existsSync(path.join(mainDir, "checksums.json")));

    const checksums = JSON.parse(fs.readFileSync(path.join(mainDir, "checksums.json"), "utf8"));
    assert.equal(checksums.version, "0.1.0");
    assert.equal(Object.keys(checksums.packages).length, 5);

    // Verify platform packages
    const platformsDir = path.join(outDir, "platforms");
    for (const slug of ["linux-x64-gnu", "linux-arm64-gnu", "win32-x64", "darwin-arm64", "darwin-x64"]) {
      const pkgDir = path.join(platformsDir, slug);
      assert.ok(fs.existsSync(path.join(pkgDir, "package.json")));
      assert.ok(fs.existsSync(path.join(pkgDir, "README.md")));
      const pkgJson = JSON.parse(fs.readFileSync(path.join(pkgDir, "package.json"), "utf8"));
      assert.equal(pkgJson.version, "0.1.0");
      assert.ok(pkgJson.name.includes(slug));
    }
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

