set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

default:
    @just --list

# Instala dependências Python travadas no uv.lock
setup:
    uv sync --extra dev --frozen

# Formatação Rust + Python
fmt:
    cargo fmt --all
    uv run --frozen ruff format src tests scripts

# Gates completos de Rust (espelha o job `rust` do CI)
rust-check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    cargo test --workspace --locked

# Gates completos de Python (espelha o job `python` do CI)
py-check:
    uv run --frozen ruff check src tests scripts
    uv run --frozen mypy src
    uv run --frozen pytest --ignore=tests/differential

# Suíte de paridade diferencial Python vs. Rust (espelha o job `parity` do CI)
parity:
    cargo build --locked --features oracle -p anki-workflow-mcp
    uv run --frozen pytest tests/differential/

# Gate completo pré-PR (100% equivalente ao CI)
check: rust-check py-check parity

# Build release do binário primário Rust
build-release:
    cargo build --release --locked -p anki-workflow-mcp

# Detecta drift nos contratos legados sem falso positivo de versão Python em server_meta.json
contracts-check:
    uv run --frozen python scripts/legacy-oracle/export_contracts.py
    git diff --exit-code -- contracts/legacy/tools.json contracts/legacy/resources.json contracts/legacy/prompts.json

# Frente 1: Harness E2E de contrato MCP em TypeScript
[unix]
mcp-contract-ts:
    cargo build --locked -p anki-workflow-mcp
    pnpm --dir tests/mcp-contract-ts install --frozen-lockfile --ignore-scripts
    ANKI_MCP_BIN="$(pwd)/target/debug/anki-workflow-mcp" pnpm --dir tests/mcp-contract-ts run check

[windows]
mcp-contract-ts:
    cargo build --locked -p anki-workflow-mcp
    pnpm --dir tests/mcp-contract-ts install --frozen-lockfile --ignore-scripts
    $env:ANKI_MCP_BIN = (Join-Path (Get-Location) 'target\debug\anki-workflow-mcp.exe'); pnpm --dir tests/mcp-contract-ts run check

# Frente 2: Codegen determinístico schemars -> JSON Schema -> TS (.d.ts)
codegen-contracts:
    cargo run --locked -p anki-workflow-mcp --bin export-schemas
    pnpm --dir tools/codegen-ts install --frozen-lockfile --ignore-scripts
    pnpm --dir tools/codegen-ts run check

[unix]
check-contracts: codegen-contracts
    git diff --exit-code -- contracts/v2/
    test -z "$(git status --porcelain -- contracts/v2/)"

[windows]
check-contracts: codegen-contracts
    git diff --exit-code -- contracts/v2/
    if (git status --porcelain -- contracts/v2/) { exit 1 }

# Exporta JSON Schemas do binário Rust
export-schemas:
    cargo run --locked -p anki-workflow-mcp --bin export-schemas

# Executa testes, geração e typecheck do codegen TypeScript
codegen-ts:
    pnpm --dir tools/codegen-ts test && pnpm --dir tools/codegen-ts run generate && pnpm --dir tools/codegen-ts run typecheck && node tools/codegen-ts/generate.mjs --check

# Executa typecheck e testes do harness de contrato MCP TS
test-mcp-ts:
    pnpm --dir tests/mcp-contract-ts run typecheck && pnpm --dir tests/mcp-contract-ts test

# Executa testes do launcher npm
test-npm:
    node --test npm/test/*.test.mjs

