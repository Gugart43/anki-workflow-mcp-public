# ADR-007: Exportação Local Transacional com Ledger JSON (Substitui ADR-005 e ADR-006)

- **Status:** Accepted / Implemented
- **Status de Implementação:** Implementado em `apps/anki-workflow-mcp/src/export.rs` (1.611 linhas) e exposto como a ferramenta MCP `export_reviewed_files`.
- **Data:** 2026-11-15
- **Substitui:** ADR-005 (SQLite Writer e Durabilidade) e ADR-006 (Publicação Imutável e Recuperação)
- **Relacionado:** ADR-001, ADR-003, ADR-004

---

## Contexto

ADR-005 e ADR-006 especificaram um motor de persistência relacional (SQLite com `SingleWriter`, `BEGIN IMMEDIATE`, tabela `operations` com lease/CAS, `artifact_versions` versionado, GC em duas fases e rollback determinístico) para a versão 2. Ambas foram redigidas como *target architecture* declarando explicitamente que o núcleo Rust era puramente stateless.

Esse estado mudou. A promoção do Rust como autoridade de produção entregou, sem backend relacional, o conjunto de garantias que motivou aquelas duas ADRs: durabilidade, idempotência, exclusão mútua entre processos, replay e recuperação após crash. O veículo é um **ledger JSON** + **swap atômico de arquivos**, implementado em `apps/anki-workflow-mcp/src/export.rs`.

Manter ADR-005 e ADR-006 com status "Proposed / Not Implemented" passou a ser factualmente incorreto em dois pontos verificáveis:

1. **Dependência SQLite existe.** `apps/anki-workflow-mcp/Cargo.toml:29` declara `rusqlite = { version = "0.42", features = ["bundled", "collation", "backup"] }`, contradizendo "Cargo.toml não inclui `rusqlite`". O uso real é outro: `sync-decks` abre a coleção Anki em modo leitura, faz snapshot online via SQLite Backup API e ordena com colação PT-BR `unicase` (`apps/anki-workflow-mcp/src/cli/sync_decks.rs:14-15,285-391`).
2. **A exportação Rust grava em disco.** ADR-006 afirma que as ferramentas Rust "são puramente em memória e não realizam escritas em disco". `export_reviewed_files` faz escrita transacional com lock, staging, `sync_all`, rename e rollback.

O que **permanece** não implementado de ADR-005/ADR-006: `anki_workflow.db` (nenhuma ocorrência em `apps/`, `crates/` ou `src/`), tabela `operations`, lease/heartbeat/`owner_id`, `artifact_versions`, ponteiro `active_artifact_version`, trigger de validade, GC *tombstone*/*purge*, retenção por cota e a ferramenta administrativa `batch_rollback`.

---

## Decisão

Adotar como contrato definitivo — e não como arquitetura-alvo — o modelo **ledger JSON local + swap atômico por lote**, descrito abaixo. SQLite permanece apenas como espelho de leitura da coleção Anki em `sync-decks`, nunca como fonte de verdade de staging, operações ou publicação.

### 1. Transação de publicação por lote (`apps/anki-workflow-mcp/src/export.rs`)

Ordem efetiva de `execute_export_reviewed_files` (a partir de `export.rs:522`):

1. Validação de `batch_id`/`topic_slug` seguros e de nomes reservados Windows/POSIX (`is_valid_slug`, `is_reserved_name` em `export.rs:169-206`).
2. Exigência do literal `verdict == "PASS"` (`EXPORT.VALIDATION_FAIL`) e do `receipt_hash` exato do conteúdo (`EXPORT.NO_RECEIPT`).
3. Revalidação fail-closed de estrutura CSV (`anki_validate::check_csv`) e do gate de HTML/qualidade (`EXPORT.INVALID_CONTENT`).
4. Rejeição de *path traversal*, de symlink no destino e de colisão de `topic_slug` entre lotes distintos (`EXPORT.PATH_TRAVERSAL`, `EXPORT.SYMLINK_TARGET`, `EXPORT.TOPIC_COLLISION`).
5. **Lock exclusivo cruzado de processos** sobre `.anki_workflow_export.lock` (`EXPORT_LOCK_FILE`, `export.rs:32`) com `fs2::FileExt::try_lock_exclusive`, timeout de 10 s (`EXPORT_LOCK_TIMEOUT`, `export.rs:33`) e poll de 25 ms (`export.rs:34`). O lock cobre leitura do ledger, checagem de idempotência e escrita/swap — resolvendo P-03 (duas instâncias MCP concorrentes) sem depender de `owner_id`/lease.
6. Leitura do ledger `.anki-workflow-export-state.json` (`EXPORT_STATE_FILE`, `export.rs:31`), com `EXPORT.STATE_READ_ERROR` / `EXPORT.STATE_CORRUPT`.
7. Checagem de replay idempotente. Um `batch_id` já registrado com hash, `topic_slug`, `write_to_deck_root` ou `state` divergentes é rejeitado com `EXPORT.BATCH_CONFLICT` / `EXPORT.STATE_INVALID`, em vez de sobrescrever silenciosamente — o papel que ADR-005 atribuía ao conflito de `request_payload_digest`.
8. Staging e swap: arquivos `.tmp_export_{batch_id}_{pid}_{nonce}_{seq}` no mesmo filesystem do destino (`export.rs:372-410`), `File::sync_all()` antes do swap (`export.rs:403`) e `fs::rename` para promoção (`replace_file`, `export.rs:299`), com fallback de cópia para troca entre dispositivos (`cross_device_replace`, `export.rs:265-297`).
9. Rollback RAII em ordem inversa (`StagedExport::rollback`, `export.rs:458`) preservando backups de recuperação; falha de restauração não é engolida: acumula e retorna `EXPORT.ROLLBACK_FAILED`.
10. Commit e gravação do ledger com `state = "exported"`, `receipt_hash`, nomes de arquivo, `exported_at_unix`, `root_mirror_written` e `topic_slug`.

### 2. Modelo de dados do ledger

`ExportStateLedger { batches: BTreeMap<String, ExportBatchRecord> }` e `ExportBatchRecord { state, receipt_hash, qa_file, cloze_file, exported_at_unix, root_mirror_written, topic_slug }` (`export.rs:106-124`). É a fonte de verdade de "este lote já foi publicado, com este conteúdo, neste lugar".

### 3. Recuperação após crash e adoção

Se um crash ocorrer entre o swap e a gravação do ledger, o replay reencontra os arquivos em disco com os hashes esperados e **adota** o lote marcando `recovered_from_disk: true` (`export.rs:71,1071`), em vez de reescrever ou falhar. Divergência de conteúdo nos arquivos existentes é erro explícito (`EXPORT.CONTENT_ALTERED` / `EXPORT.FILE_MISSING`), nunca mascarada por idempotência.

### 4. Limite explícito de atomicidade (desvio aceito de ADR-006)

O cabeçalho do módulo (`export.rs:14-17`) declara a limitação: cada swap individual de arquivo é atômico em POSIX/NTFS, mas a **atomicidade entre arquivos de um mesmo lote sob queda de energia** não é garantida por APIs de filesystem sem suporte transacional. A coordenação multi-arquivo é feita por rollback reverso em processo. ADR-006 propunha resolver isso com commit SQLite soberano + `projection_status: DIRTY`; esse contrato não existe e não deve ser citado como implementado.

### 5. Códigos de diagnóstico canônicos

| ADR-005/006 citava | Código real hoje |
| :--- | :--- |
| código do namespace `STATE.*` para conflito de `operation_id` | `EXPORT.BATCH_CONFLICT` |
| código do namespace `STATE.*` para o trigger de `active_artifact_version` | não aplicável: não há ponteiro de versão ativa. Equivalente mais próximo é `EXPORT.STATE_INVALID` |

Nenhum código do namespace `STATE.*` é emitido pela exportação. Os únicos `STATE.*` registrados são `STATE.NO_RECEIPT` e `STATE.OUT_OF_ORDER` (namespace do pipeline legado Python). Nenhum `HTML.*` existe: a fronteira de segurança HTML é reportada como `QUALITY.UNSAFE_HTML`.

---

## Consequências

- **Ganho:** durabilidade, idempotência, exclusão mútua e recuperação são reais e testados, sem servidor de banco, sem migração de schema e sem estado compartilhado além do diretório `exports/` do baralho.
- **Perda:** não há histórico imutável de versões por lote, nem GC, nem `batch_rollback` para versão anterior. Republicar um `batch_id` com conteúdo diferente é uma operação proibida (`EXPORT.BATCH_CONFLICT`), não uma nova versão.
- **Risco residual:** queda de energia no meio de um swap multi-arquivo pode deixar o lote parcialmente projetado; a recuperação depende de replay/adoção e do backup de recuperação preservado pelo rollback.
- **Escopo de SQLite:** permanece restrito a leitura da coleção Anki. Qualquer proposta futura de reintroduzir um backend relacional para staging/operações exige nova ADR, não a reativação de ADR-005.

---

## Teste que valida esta decisão

- Testes unitários de `export.rs` (`#[cfg(test)]`, a partir de `export.rs:1075`) cobrem replay idempotente (`test_idempotent_replay_and_mirror_verification`), conflito de hash (`test_conflict_different_hash_same_batch`), adoção após crash (`test_crash_between_swap_and_ledger_is_adopted`), falha de rollback acumulada (`test_rollback_failures_accumulate_into_rollback_failed`), timeout de lock (`test_lock_timeout_returns_export_lock_timeout`), symlink no lock (`test_symlink_lock_file_rejected`), `topic_slug`/`root_mirror` divergentes (`test_replay_divergent_topic_slug_or_root_mirror_returns_batch_conflict`) e path traversal/nomes reservados (`test_slug_traversal_and_reserved_names`).
- Verificação por execução real desta decisão: `cargo test --workspace --locked` (inclui os testes acima) e `.venv/Scripts/python.exe -m pytest tests -q` para os gates Python de contrato.
