# ADR-004: Receipt SHA-256 v1 vs `ValidationAttestation` v2

- **Status:** Aceito
- **Data:** 2026-11-14
- **Relacionado:** ADR-001, ADR-003, Seções 3.3 e 4.1 do `PLANO_MESTRE_ANKI_WORKFLOW_MCP_RUST_V2.md`

## Contexto

No legado (`src/anki_mcp/core/receipt.py`), o recibo é apenas:

```text
sha256(canonical_csv_text(qa_csv) + "\n---\n" + canonical_csv_text(cloze_csv))
```

Qualquer cliente pode computar esse hash localmente sem ter executado `validate_batch` ou `pipeline_gate_check`, e o token não vincula `deck_id`, `batch_id`, fase, política ou versão da taxonomia.

## Decisão

1. **Preservar `compute_receipt_v1` bit a bit** em `anki_domain::compute_receipt_v1` para compatibilidade com `content_hash` do legado.
2. **Introduzir `ValidationAttestation` (v2)** persistida exclusivamente pelo servidor no SQLite do workflow (`validation_attestations`) após conclusão sem `CRITICAL` e sem `INCOMPLETE`:
   - `attestation_id`, `deck_id`, `batch_id`, `phase`
   - `content_digest` (v1/v2), `context_digest` (taxonomia/catálogo/IDs)
   - `policy_version`, `engine_version`, `verdict`, `valid`, `created_at`
3. No modo `strict_v2`, `manifesto_commit` e `export_final` autorizam a operação consultando a atestação transacionalmente no snapshot ativo (prevenindo TOCTOU). Alterações no conteúdo, taxonomia ou catálogo invalidam atestações anteriores.

## Teste que valida esta decisão

- Fixtures golden em `fixtures/golden_parity.json` validando equivalência bit a bit de `compute_receipt_v1` com `core/receipt.py`.
- Testes transacionais de invalidação e rejeição de atestação forjada/expirada na camada `anki-state`.
