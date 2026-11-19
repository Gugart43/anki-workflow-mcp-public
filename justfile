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
