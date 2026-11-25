# Tasks — incremental-quality-validation

> Este change é **design only** (frente B28). Nenhuma tarefa abaixo autoriza alteração em
> `crates/`, `apps/` ou `src/` enquanto o change não for aprovado. A implementação exige
> change nova por fase.

## 1. Design (este change)

- [x] 1.1 Medir o custo atual de staleness global com o binário real (fixture `wave2_clinical_batch`).
- [x] 1.2 Especificar o `retrieval_digest` por recuperação e a projeção canônica que ele cobre.
- [x] 1.3 Especificar o contrato JSON aditivo e a semântica de `snapshot_hash_role`.
- [x] 1.4 Definir critérios de aceitação executáveis e riscos com mitigação.

## 2. Implementação — Fase 1 (aditiva, sem quebra)

- [ ] 2.1 Registrar `QUALITY.STALE_RETRIEVAL_DIGEST` em `src/anki_mcp/core/codes.py` e no catálogo Rust.
- [ ] 2.2 Calcular `retrieval_digest` em `crates/anki-validate/src/quality.rs`, reutilizando `canonical_csv_for_snapshot` e `compute_receipt_v1`.
- [ ] 2.3 Expor `retrieval_digest` em `CardInformationReview` e aceitá-lo opcionalmente em `QualityReview`.
- [ ] 2.4 Expor `snapshot_hash_role` e `stale_retrieval_count` em `QualityReport`.
- [ ] 2.5 Testes de isolamento, permutação, sensibilidade, anti-herança e compatibilidade de payload antigo.
- [ ] 2.6 Regenerar `contracts/v2/schemas/*.schema.json` e `contracts/v2/ts/index.d.ts` (somente campos opcionais) pela tarefa do próprio repositório.

## 3. Implementação — Fases 2 e 3 (fora deste change)

- [ ] 3.1 Adicionar `review_binding` a `quality_prepare`/`quality_evaluate` com default `global`.
- [ ] 3.2 Espelhar `retrieval_digest` no oráculo Python e cobrir o diferencial.
- [ ] 3.3 Decidir o flip de default com evidência de suíte diferencial e harness TS verdes.

## 4. Aceitação

- [ ] 4.1 `openspec validate --changes --strict` e `--specs --strict` verdes.
- [ ] 4.2 Critérios 1–7 do `proposal.md` demonstrados por execução real, com saída colada.
