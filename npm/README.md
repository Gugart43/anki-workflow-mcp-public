# npm Distribution Packages

This directory contains the npm distribution assets for `anki-workflow-mcp`:

- `anki-workflow-mcp/`: Root package definition and thin CJS launcher (`bin/anki-workflow-mcp.cjs`).
- `scripts/`: Staging script (`stage-platform-packages.mjs`) used during release to extract native archives and generate per-platform npm packages.
- `test/`: Test suites for launcher, staging, and installation verification.

## Supported Platform Packages

The thin launcher resolves one of the 5 platform packages based on OS and architecture:

| Platform | npm Package | Target Triple |
| :--- | :--- | :--- |
| Linux x64 (glibc) | `@gugart43/anki-workflow-mcp-linux-x64-gnu` | `x86_64-unknown-linux-gnu` |
| Linux ARM64 (glibc) | `@gugart43/anki-workflow-mcp-linux-arm64-gnu` | `aarch64-unknown-linux-gnu` |
| Windows x64 | `@gugart43/anki-workflow-mcp-win32-x64` | `x86_64-pc-windows-msvc` |
| macOS Apple Silicon | `@gugart43/anki-workflow-mcp-darwin-arm64` | `aarch64-apple-darwin` |
| macOS Intel | `@gugart43/anki-workflow-mcp-darwin-x64` | `x86_64-apple-darwin` |

See [anki-workflow-mcp/README.md](./anki-workflow-mcp/README.md) for full configuration and deployment documentation.
