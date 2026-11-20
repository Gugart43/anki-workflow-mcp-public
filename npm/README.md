# npm Distribution Packages

This directory contains the npm distribution assets for `anki-workflow-mcp`:

- `anki-workflow-mcp/`: Root package definition and thin CJS launcher (`bin/anki-workflow-mcp.cjs`).
- `scripts/`: Staging script (`stage-platform-packages.mjs`) used during release to extract native archives and generate per-platform npm packages.
- `test/`: Test suites for launcher, staging, and installation verification.

See [anki-workflow-mcp/README.md](./anki-workflow-mcp/README.md) for full configuration and deployment documentation.
