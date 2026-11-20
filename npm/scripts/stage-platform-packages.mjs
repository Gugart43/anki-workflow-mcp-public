#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const REPO_ROOT = path.resolve(__dirname, "..", "..");
const MAIN_PKG_SRC = path.join(REPO_ROOT, "npm", "anki-workflow-mcp");

const REPO_URL = "git+https://github.com/Gugart43/anki-workflow-mcp-public.git";
const HOMEPAGE_URL = "https://github.com/Gugart43/anki-workflow-mcp-public#readme";
const BUGS_URL = "https://github.com/Gugart43/anki-workflow-mcp-public/issues";

export const PLATFORMS = Object.freeze([
  {
    slug: "linux-x64-gnu",
    pkgName: "@gugart43/anki-workflow-mcp-linux-x64-gnu",
    rustTarget: "x86_64-unknown-linux-gnu",
    archiveExt: "tar.gz",
    binaryName: "anki-workflow-mcp",
    os: ["linux"],
    cpu: ["x64"],
    libc: ["glibc"],
    description: "Linux x64 (glibc) native binary for @gugart43/anki-workflow-mcp.",
  },
  {
    slug: "linux-arm64-gnu",
    pkgName: "@gugart43/anki-workflow-mcp-linux-arm64-gnu",
    rustTarget: "aarch64-unknown-linux-gnu",
    archiveExt: "tar.gz",
    binaryName: "anki-workflow-mcp",
    os: ["linux"],
    cpu: ["arm64"],
    libc: ["glibc"],
    description: "Linux arm64 (glibc) native binary for @gugart43/anki-workflow-mcp.",
  },
  {
    slug: "win32-x64",
    pkgName: "@gugart43/anki-workflow-mcp-win32-x64",
    rustTarget: "x86_64-pc-windows-msvc",
    archiveExt: "zip",
    binaryName: "anki-workflow-mcp.exe",
    os: ["win32"],
    cpu: ["x64"],
    libc: null,
    description: "Windows x64 native binary for @gugart43/anki-workflow-mcp.",
  },
  {
    slug: "darwin-arm64",
    pkgName: "@gugart43/anki-workflow-mcp-darwin-arm64",
    rustTarget: "aarch64-apple-darwin",
    archiveExt: "tar.gz",
    binaryName: "anki-workflow-mcp",
    os: ["darwin"],
    cpu: ["arm64"],
    libc: null,
    description: "macOS Apple Silicon (arm64) native binary for @gugart43/anki-workflow-mcp.",
  },
  {
    slug: "darwin-x64",
    pkgName: "@gugart43/anki-workflow-mcp-darwin-x64",
    rustTarget: "x86_64-apple-darwin",
    archiveExt: "tar.gz",
    binaryName: "anki-workflow-mcp",
    os: ["darwin"],
    cpu: ["x64"],
    libc: null,
    description: "macOS Intel (x64) native binary for @gugart43/anki-workflow-mcp.",
  },
]);

const SEMVER_REGEX =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;

export function validateSemver(rawVersion) {
  if (typeof rawVersion !== "string") {
    throw new Error("Invalid semver: version must be a string");
  }
  const stripped = rawVersion.replace(/^v/, "").trim();
  if (!SEMVER_REGEX.test(stripped)) {
    throw new Error(`Invalid semver version format: "${rawVersion}"`);
  }
  return stripped;
}

export function validateOutDir(outDir, repoRoot = REPO_ROOT) {
  if (!outDir || typeof outDir !== "string") {
    throw new Error("Dangerous out-dir: empty or invalid path");
  }

  const resolved = path.resolve(outDir);
  const resolvedRepo = path.resolve(repoRoot);
  const root = path.parse(resolved).root;

  if (resolved === root) {
    throw new Error(`Dangerous out-dir: root filesystem deletion forbidden (${resolved})`);
  }
  if (resolved === resolvedRepo) {
    throw new Error(`Dangerous out-dir: repo root deletion forbidden (${resolved})`);
  }
  if (resolved === process.cwd()) {
    throw new Error(`Dangerous out-dir: current working directory deletion forbidden (${resolved})`);
  }
  const gitDir = path.join(resolvedRepo, ".git");
  if (resolved === gitDir || resolved.startsWith(gitDir + path.sep)) {
    throw new Error(`Dangerous out-dir: git internal directory deletion forbidden (${resolved})`);
  }

  return resolved;
}

export function validatePackageIntegrity(version, mainPkg, cargoTomlContent, platforms = PLATFORMS) {
  if (mainPkg.version !== version) {
    throw new Error(
      `Version mismatch between --version (${version}) and package.json (${mainPkg.version})`
    );
  }

  // Extract version from Cargo.toml [workspace.package] version = "..."
  const cargoMatch = cargoTomlContent.match(/version\s*=\s*"([^"]+)"/);
  const cargoVersion = cargoMatch ? cargoMatch[1].trim() : null;
  if (!cargoVersion) {
    throw new Error("Failed to find version in Cargo.toml [workspace.package]");
  }
  if (cargoVersion !== version) {
    throw new Error(
      `Version mismatch between package.json (${version}) and Cargo.toml (${cargoVersion})`
    );
  }

  if (mainPkg.dependencies && Object.keys(mainPkg.dependencies).length > 0) {
    throw new Error("Main package must have zero dependencies.");
  }

  if (mainPkg.scripts && mainPkg.scripts.postinstall) {
    throw new Error("Main package must not define postinstall script.");
  }

  const optionalDeps = mainPkg.optionalDependencies || {};
  for (const platform of platforms) {
    const depVer = optionalDeps[platform.pkgName];
    if (depVer !== version) {
      throw new Error(
        `optionalDependencies["${platform.pkgName}"] must be exact "${version}", got "${depVer}"`
      );
    }
  }

  const expectedPkgNames = new Set(platforms.map((p) => p.pkgName));
  for (const depName of Object.keys(optionalDeps)) {
    if (!expectedPkgNames.has(depName)) {
      throw new Error(`Unexpected optional dependency: ${depName}`);
    }
  }
}

export function sha256File(filePath) {
  const bytes = fs.readFileSync(filePath);
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

export function inspectAndExtractArchive(archivePath, archiveExt, expectedBinaryName, destBinDir) {
  fs.mkdirSync(destBinDir, { recursive: true });

  const isWin = process.platform === "win32";
  const normArchive = archivePath.replaceAll("\\", "/");
  const normDest = destBinDir.replaceAll("\\", "/");
  let entries = [];

  if (archiveExt === "tar.gz") {
    let output;
    try {
      output = execFileSync("tar", ["-tzf", normArchive], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
    } catch {
      output = execFileSync("tar", ["--force-local", "-tzf", normArchive], { encoding: "utf8" });
    }
    entries = output
      .split(/\r?\n/)
      .map((e) => e.trim())
      .filter(Boolean);
  } else if (archiveExt === "zip") {
    try {
      const output = execFileSync("unzip", ["-Z1", archivePath], { encoding: "utf8" });
      entries = output
        .split(/\r?\n/)
        .map((e) => e.trim())
        .filter(Boolean);
    } catch {
      // Fallback inspection via python zipfile if unzip -Z1 fails
      const pyScript = `import zipfile, sys; z = zipfile.ZipFile(sys.argv[1]); print('\\n'.join(z.namelist()))`;
      const output = execFileSync("python", ["-c", pyScript, archivePath], { encoding: "utf8" });
      entries = output
        .split(/\r?\n/)
        .map((e) => e.trim())
        .filter(Boolean);
    }
  } else {
    throw new Error(`Unsupported archive extension: ${archiveExt}`);
  }

  for (const rawEntry of entries) {
    const normalized = rawEntry.replace(/^(\.\/|\.\\)/, "");
    if (
      normalized.includes("..") ||
      path.isAbsolute(normalized) ||
      /^[a-zA-Z]:/.test(normalized)
    ) {
      throw new Error(`Path traversal attempt detected in archive member: ${rawEntry}`);
    }
    if (normalized !== expectedBinaryName) {
      throw new Error(
        `Unexpected member in archive: ${rawEntry}. Only "${expectedBinaryName}" is allowed.`
      );
    }
  }

  if (archiveExt === "tar.gz") {
    try {
      execFileSync("tar", ["-xzf", normArchive, "-C", normDest, expectedBinaryName], {
        stdio: ["ignore", "inherit", "ignore"],
      });
    } catch {
      execFileSync("tar", ["--force-local", "-xzf", normArchive, "-C", normDest, expectedBinaryName], {
        stdio: "inherit",
      });
    }
  } else if (archiveExt === "zip") {
    try {
      execFileSync("unzip", ["-o", "-q", archivePath, expectedBinaryName, "-d", destBinDir], {
        stdio: ["ignore", "inherit", "ignore"],
      });
    } catch {
      const pyExtract = `import zipfile, sys; zipfile.ZipFile(sys.argv[1]).extract(sys.argv[2], sys.argv[3])`;
      execFileSync("python", ["-c", pyExtract, archivePath, expectedBinaryName, destBinDir], {
        stdio: "inherit",
      });
    }
  }

  const binaryPath = path.join(destBinDir, expectedBinaryName);
  if (!fs.existsSync(binaryPath)) {
    throw new Error(
      `Expected binary "${expectedBinaryName}" not found after extracting ${archivePath}`
    );
  }

  const stat = fs.statSync(binaryPath);
  if (!stat.isFile() || stat.size === 0) {
    throw new Error(`Extracted binary is empty or not a regular file: ${binaryPath}`);
  }

  if (!expectedBinaryName.endsWith(".exe")) {
    fs.chmodSync(binaryPath, 0o755);
  }

  return binaryPath;
}

export function buildPlatformPackageJson(platform, version) {
  const pkg = {
    name: platform.pkgName,
    version,
    description: platform.description,
    license: "MIT",
    author: "anki-workflow-mcp contributors",
    homepage: HOMEPAGE_URL,
    bugs: {
      url: BUGS_URL,
    },
    repository: {
      type: "git",
      url: REPO_URL,
    },
    os: platform.os,
    cpu: platform.cpu,
    files: ["bin/"],
    preferUnplugged: true,
    publishConfig: {
      access: "public",
    },
  };

  if (platform.libc) {
    pkg.libc = platform.libc;
  }

  return pkg;
}

export function parseArgs(argv) {
  const args = {
    version: null,
    distDir: path.join(REPO_ROOT, "dist"),
    outDir: path.join(REPO_ROOT, "npm-stage"),
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--version") {
      args.version = argv[++i];
    } else if (arg === "--dist-dir") {
      args.distDir = path.resolve(argv[++i]);
    } else if (arg === "--out-dir") {
      args.outDir = path.resolve(argv[++i]);
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return args;
}

export function main() {
  const args = parseArgs(process.argv.slice(2));
  const mainPkgJsonPath = path.join(MAIN_PKG_SRC, "package.json");
  const mainPkg = JSON.parse(fs.readFileSync(mainPkgJsonPath, "utf8"));

  const version = validateSemver(args.version || mainPkg.version || "");
  const cargoTomlContent = fs.readFileSync(path.join(REPO_ROOT, "Cargo.toml"), "utf8");

  validatePackageIntegrity(version, mainPkg, cargoTomlContent, PLATFORMS);
  const outDir = validateOutDir(args.outDir, REPO_ROOT);

  fs.rmSync(outDir, { recursive: true, force: true });
  const platformsOutDir = path.join(outDir, "platforms");
  const mainOutDir = path.join(outDir, "main");
  fs.mkdirSync(platformsOutDir, { recursive: true });
  fs.mkdirSync(path.join(mainOutDir, "bin"), { recursive: true });

  const licenseSrc = path.join(REPO_ROOT, "LICENSE");
  const readmeSrc = path.join(MAIN_PKG_SRC, "README.md");

  const checksums = {
    version,
    packages: {},
  };

  for (const platform of PLATFORMS) {
    const archiveName = `anki-workflow-mcp-v${version}-${platform.rustTarget}.${platform.archiveExt}`;
    const archivePath = path.join(args.distDir, archiveName);
    if (!fs.existsSync(archivePath)) {
      throw new Error(`Missing build archive: ${archivePath}`);
    }

    const pkgDir = path.join(platformsOutDir, platform.slug);
    const binDir = path.join(pkgDir, "bin");
    const binaryPath = inspectAndExtractArchive(
      archivePath,
      platform.archiveExt,
      platform.binaryName,
      binDir
    );

    const digest = sha256File(binaryPath);
    checksums.packages[platform.pkgName] = {
      target: platform.rustTarget,
      binary: `bin/${platform.binaryName}`,
      sha256: digest,
    };

    const platformPkgJson = buildPlatformPackageJson(platform, version);
    fs.writeFileSync(
      path.join(pkgDir, "package.json"),
      `${JSON.stringify(platformPkgJson, null, 2)}\n`,
      "utf8"
    );

    if (fs.existsSync(licenseSrc)) {
      fs.copyFileSync(licenseSrc, path.join(pkgDir, "LICENSE"));
    }

    fs.writeFileSync(
      path.join(pkgDir, "README.md"),
      `# ${platform.pkgName}\n\n${platform.description}\n\nInstall via \`npx @gugart43/anki-workflow-mcp serve\`.\n`,
      "utf8"
    );
  }

  const launcherSrc = path.join(MAIN_PKG_SRC, "bin", "anki-workflow-mcp.cjs");
  const launcherDest = path.join(mainOutDir, "bin", "anki-workflow-mcp.cjs");
  fs.copyFileSync(mainPkgJsonPath, path.join(mainOutDir, "package.json"));
  fs.copyFileSync(launcherSrc, launcherDest);
  fs.chmodSync(launcherDest, 0o755);

  if (fs.existsSync(licenseSrc)) {
    fs.copyFileSync(licenseSrc, path.join(mainOutDir, "LICENSE"));
  }
  if (fs.existsSync(readmeSrc)) {
    fs.copyFileSync(readmeSrc, path.join(mainOutDir, "README.md"));
  }

  fs.writeFileSync(
    path.join(mainOutDir, "checksums.json"),
    `${JSON.stringify(checksums, null, 2)}\n`,
    "utf8"
  );

  process.stdout.write(
    `Staged ${PLATFORMS.length} platform packages and @gugart43/anki-workflow-mcp@${version} into ${outDir}\n`
  );
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main();
}
