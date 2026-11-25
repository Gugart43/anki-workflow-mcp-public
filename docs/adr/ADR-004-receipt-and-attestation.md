# ADR-004: Receipt SHA-256 v1 vs `ValidationAttestation` v2

- **Status:** Proposed / Target Architecture (Not Implemented)
- **Status de Implementação:** Apenas `compute_receipt_v1` e `snapshot_hash` existem; `ValidationAttestation` v2 e HMAC via secret manager não foram implementados (em revisão para simplificação na Onda 3).
- **Data:** 2026-11-14
- **Relacionado:** [ADR-001](ADR-001-rust-migration.md), ADR-003

## Contexto

No legado (`src/anki_mcp/core/receipt.py`), o recibo é apenas:

```text
sha256(canonical_csv_text(qa_csv) + "\n---\n" + canonical_csv_text(cloze_csv))
```

Qualquer cliente pode computar esse hash localmente sem ter executado `validate_batch` ou `pipeline_gate_check`, e o token não vincula `deck_id`, `batch_id`, fase, política ou versão da taxonomia.

## Decisão

1. **Preservar `compute_receipt_v1` bit a bit** em `anki_domain::compute_receipt_v1` para compatibilidade com `content_hash` do legado.
2. **Introduzir `ValidationAttestation` (v2)** persistida exclusivamente pelo servidor no SQLite do workflow (`validation_attestations`) após conclusão sem `CRITICAL` e sem `INCOMPLETE`:
   - `attestation_id`, `deck_id`, `batch_id`, `phase`, `snapshot_id`, `operation_id`
   - `content_digest` (v1/v2), `context_digest` (taxonomia/catálogo/IDs), `mutation_payload_digest` (cobrindo `new_ids`, tópicos, tags, changelog e catálogo — ressalva P-06)
   - `gate_params_digest`, `schema_version`, `policy_version`, `engine_version` (ressalva P-01)
   - `verdict`, `valid` (`CHECK(valid IN (0,1))`), `revoked_at`, `created_at`, e identidade do emissor quando em modo remoto
   - Índice parcial por `(deck_id, batch_id, phase) WHERE valid = 1`
3. No modo `strict_v2`, `manifesto_commit` e `export_final` autorizam a operação consultando a atestação transacionalmente no snapshot ativo (prevenindo TOCTOU). Alterações no conteúdo, taxonomia, catálogo ou payload de mutação invalidam atestações anteriores.
4. **Âncora de confiança local (P-18):** no modelo local, o próprio `anki_workflow.db` é a âncora de confiança; opcionalmente, a atestação pode ser autenticada com HMAC-SHA256 usando chave armazenada no gerenciador de segredos do SO.

## Teste que valida esta decisão

- Fixtures golden em `fixtures/golden_parity.json` validando equivalência bit a bit de `compute_receipt_v1` com `core/receipt.py`.
- Testes transacionais de invalidação e rejeição de atestação forjada/expirada na camada `anki-state`.

---

## Status de Implementação (Promoção Rust Primário)

- **Implementado no estado atual:** `compute_receipt_v1` está plenamente funcional e verificado em `crates/anki-domain/src/lib.rs` (`compute_receipt_v1`, linha 747), mantendo identidade exata byte a byte com o cálculo legado. O motor `quality_v1` gera snapshots imutáveis em memória (`snapshot_hash`) vinculando os CSVs compilados às fontes, obrigações e evidências informadas. Em todas as avaliações técnicas (`PASS`, `FAIL`, `INCOMPLETE`), os campos `export_authorized` e `clinical_correctness_certified` permanecem sempre `false`.
- **Arquitetura futura / Persistência legada:** A tabela relacional `validation_attestations` em SQLite e o mecanismo transacional anti-TOCTOU para autorização de `manifesto_commit` e `export_final` no banco de dados constituem a especificação de destino da versão 2. Atualmente, a persistência de manifesto e exportação permanece mediada pelo servidor Python legado.
