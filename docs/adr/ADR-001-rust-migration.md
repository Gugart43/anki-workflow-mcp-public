# ADR-001: Migração para 100% Rust em produção e política de compatibilidade

- **Status:** Aceito (Arquitetura-alvo / Parcialmente implementado)
- **Status de Implementação:** Parcialmente implementado. O motor Rust foi promovido como componente primário em memória (6 ferramentas stdio puras), enquanto a persistência transacional em disco, manifesto, catálogos e sincronização de mídia permanecem executados pelo servidor Python legado.
- **Data:** 2026-11-14
- **Baseline Python congelada:** `d1d0098b367448c471303f7d0912996d50ac2f67` (`anki-workflow-mcp` v0.3.1)
- **Plano de referência:** `PLANO_MESTRE_ANKI_WORKFLOW_MCP_RUST_V2.md`

## Contexto

O servidor `anki-workflow-mcp` v0.3.1 em Python (FastMCP + Pydantic + filelock) entrega 49 ferramentas MCP, 4 recursos e 1 prompt. Embora funcional, apresenta fragilidades estruturais identificadas na auditoria estática:

1. Estado persistido em arquivos JSON/CSV soltos e locks de arquivo, sem transações ACID nem reconciliador idempotente após queda.
2. `manifesto_commit` valida o digest computável do CSV sem verificar uma atestação interna de gate vinculada ao snapshot.
3. Risco de divergência silenciosa entre políticas declaradas por fase e execução integral.
4. Distribuição dependente de runtime Python e gerenciador `uv`.

## Alternativas consideradas

1. **Manter 100% Python** e adicionar SQLite + hardening incremental.
   - *Prós:* menor custo inicial.
   - *Contras:* mantém dependência de runtime Python/uv na distribuição desktop, sem tipagem estática nativa em tempo de compilação nem binário único.
2. **Híbrido Rust (PyO3) + Python**.
   - *Prós:* reuso imediato do FastMCP.
   - *Contras:* complexidade de build FFI multiplataforma e manutenção de dois runtimes em produção.
3. **100% Rust em produção** (`rmcp` + SQLite + binário nativo), usando Python **apenas** como oráculo diferencial durante a migração.
   - *Prós:* binário único sem runtime Python em produção, tipagem estática forte, controle explícito de memória, I/O, paths e transações.
   - *Contras:* esforço maior de portabilidade inicial e necessidade de paridade rigorosa de regex/HTML/Unicode.

## Decisão

Adotar a **Alternativa 3 (100% Rust em produção)**:

- Nenhum interpretador Python, sidecar Python ou extensão Python será necessário para executar o produto final (`anki-workflow-mcp`).
- Python permanece **exclusivamente** em `scripts/legacy-oracle/` e na suíte diferencial de desenvolvimento/CI até que todos os gates de equivalência (G0–G7) sejam atingidos.
- Dois perfis de compatibilidade serão mantidos de forma explícita:
  - `legacy_compat`: preserva exatamente contratos, recibos `sha256:` (v1), contagem de cartões (QA = 1, Cloze = índices distintos, limite 20), códigos de diagnóstico e semântica de `split_row` (sem `trim()` automático).
  - `strict_v2`: introduz atestação transacional em SQLite, ledger completo obrigatório no export, roots allowlisted, publicação versionada imutável e outbox para AnkiConnect opcional.
- `unsafe` é proibido nas crates próprias do workspace, salvo ADR excepcional revisado.

## Trade-offs e Riscos

- **Divergência Unicode/Regex/HTML:** mitigada com fixtures golden determinísticas e suíte diferencial Python vs Rust rodando em CI (Linux e Windows).
- **Alteração de recibos legados:** mitigada congelando `compute_receipt_v1` byte a byte com fixtures UTF-8, CRLF/LF e casos patológicos.
- **Escopo excessivo:** GUI desktop, busca semântica por embeddings e sincronização cloud ficam fora do caminho crítico da v2.0.0.

## Teste que valida esta decisão

- Discovery de contratos legados capturado em `contracts/legacy/tools.json`, `resources.json` e `prompts.json`.
- Suíte diferencial (`tests/differential/` + `scripts/legacy-oracle/`) comparando `compute_receipt_v1`, `split_row` e `count_batch_cards` entre Python e Rust com 100% de equivalência nos fixtures golden.
- CI multiplataforma (Linux + Windows) executando `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace` e `pytest`.

---

## Status de Implementação (Promoção Rust Primário)

- **Implementado no estado atual:** O núcleo nativo Rust (`apps/anki-workflow-mcp` + crates `anki-domain`, `anki-parser`, `anki-validate`) foi promovido como componente primário de execução em memória, expondo exatamente 6 ferramentas puras via stdio (`cards_compile_drafts`, `review_batch_information`, `quality_prepare`, `quality_evaluate`, `batch_patch_draft` e `generation_contract_get`).
- **Arquitetura futura / Persistência legada:** A visão de "100% Rust sem runtime Python" e a eliminação completa de dependência de Python em produção permanecem como arquitetura-alvo de longo prazo. No estágio atual, o ecossistema Python (`src/anki_mcp/server.py`) não atua apenas como oráculo diferencial: mantém o catálogo legado e as operações persistentes (manifesto, staging em disco, ledger de IDs, gerenciamento físico de mídia e exportação final). As seis ferramentas Rust não são um subconjunto estrito do catálogo Python.
