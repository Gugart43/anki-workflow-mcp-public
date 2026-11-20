# @gugart43/anki-workflow-mcp

Thin, secure `npx` launcher for the `anki-workflow-mcp` Model Context Protocol (MCP) server.

- **Zero dependencies** in the main package.
- **No `postinstall` scripts** and no runtime downloads.
- **Timing-safe SHA-256 integrity verification** on the native platform binary.
- **Fail-closed Linux libc detection**: supports `glibc` (`x86_64-unknown-linux-gnu`), fails closed on `musl` or unknown libc.
- **Local binary override** via `ANKI_WORKFLOW_MCP_BIN`.

---

## Quick Start / Configuration

### Claude Desktop — Windows (`%APPDATA%\Claude\claude_desktop_config.json`)

```json
{
  "mcpServers": {
    "anki-workflow-mcp": {
      "command": "cmd",
      "args": [
        "/c",
        "npx",
        "-y",
        "-q",
        "@gugart43/anki-workflow-mcp@0.1.0",
        "serve"
      ]
    }
  }
}
```

### Claude Desktop / Cursor — macOS & Linux

```json
{
  "mcpServers": {
    "anki-workflow-mcp": {
      "command": "npx",
      "args": [
        "-y",
        "-q",
        "@gugart43/anki-workflow-mcp@0.1.0",
        "serve"
      ]
    }
  }
}
```

### Cursor — Windows (`.cursor/mcp.json`)

```json
{
  "mcpServers": {
    "anki-workflow-mcp": {
      "command": "cmd",
      "args": [
        "/c",
        "npx",
        "-y",
        "-q",
        "@gugart43/anki-workflow-mcp@0.1.0",
        "serve"
      ]
    }
  }
}
```

---

## Environment Override

To run a locally compiled binary instead of the platform package:

```bash
export ANKI_WORKFLOW_MCP_BIN="/path/to/target/release/anki-workflow-mcp"
npx @gugart43/anki-workflow-mcp serve
```

On Windows (PowerShell):
```powershell
$env:ANKI_WORKFLOW_MCP_BIN = "C:\path\to\target\release\anki-workflow-mcp.exe"
npx @gugart43/anki-workflow-mcp serve
```

---

## Supported Platforms

| Platform | npm Package | Target Triple |
| :--- | :--- | :--- |
| Linux x64 (glibc) | `@gugart43/anki-workflow-mcp-linux-x64-gnu` | `x86_64-unknown-linux-gnu` |
| Windows x64 | `@gugart43/anki-workflow-mcp-win32-x64` | `x86_64-pc-windows-msvc` |
| macOS Apple Silicon | `@gugart43/anki-workflow-mcp-darwin-arm64` | `aarch64-apple-darwin` |
| macOS Intel | `@gugart43/anki-workflow-mcp-darwin-x64` | `x86_64-apple-darwin` |

---

## npm Trusted Publishers & Release Setup

Releases are published via GitHub Actions using npm Trusted Publishing (OIDC) with `--provenance` and zero persistent `NPM_TOKEN`.

### 1. Initial Bootstrap (One-Time per Package)
If the 5 packages do not yet exist on npmjs.com, publish the initial version manually with local 2FA because npm does not allow OIDC to create entirely new packages on the first publish:
```bash
# Stage platform packages locally
node npm/scripts/stage-platform-packages.mjs --version 0.1.0 --dist-dir dist --out-dir npm-stage

# Publish 4 platform packages first
npm publish npm-stage/platforms/linux-x64-gnu --access public
npm publish npm-stage/platforms/win32-x64 --access public
npm publish npm-stage/platforms/darwin-arm64 --access public
npm publish npm-stage/platforms/darwin-x64 --access public

# Publish main launcher
npm publish npm-stage/main --access public
```

### 2. Configure Trusted Publishers on npmjs.com
For each of the 5 packages (`https://www.npmjs.com/package/<package>/access`):
1. Navigate to **Publishing Access** → **Add Trusted Publisher** → **GitHub Actions**.
2. Configure exact fields:
   - **Organization / User**: `Gugart43`
   - **Repository**: `anki-workflow-mcp-public`
   - **Workflow filename**: `release.yml` (without `.github/workflows/`)
   - **Allowed actions**: Select `npm publish`.
3. Harden package settings:
   - Select **Require two-factor authentication and disallow tokens**.
   - Verify that no `NPM_TOKEN` exists in GitHub repository secrets.

### 3. File Permissions
Ensure the launcher and staging script have executable bits set:
```bash
git update-index --chmod=+x npm/anki-workflow-mcp/bin/anki-workflow-mcp.cjs npm/scripts/stage-platform-packages.mjs
```
