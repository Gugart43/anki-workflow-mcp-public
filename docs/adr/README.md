# Architecture Decision Records (ADRs)

| ADR | Título | Status na `main` |
| :--- | :--- | :--- |
| [ADR-001](ADR-001-rust-migration.md) | Migração para Rust | Accepted (Partially Implemented) |
| ADR-002 | *(Reservada / não publicada)* | Incorporada ao desenho inicial antes da publicação do repositório público; numeração preservada para evitar quebra de referências. |
| [ADR-003](ADR-003-compatibility-profiles.md) | Perfis de Compatibilidade | Accepted (Partially Implemented) |
| [ADR-004](ADR-004-receipt-and-attestation.md) | Recibo e Atestação | Proposed / Target Architecture (Not Implemented) |
| [ADR-005](ADR-005-sqlite-writer-and-durability.md) | SQLite Writer e Durabilidade | Superseded by [ADR-007](ADR-007-local-transactional-export-ledger.md) |
| [ADR-006](ADR-006-immutable-publication-and-recovery.md) | Publicação Imutável e Recuperação | Superseded by [ADR-007](ADR-007-local-transactional-export-ledger.md) |
| [ADR-007](ADR-007-local-transactional-export-ledger.md) | Exportação Local Transacional com Ledger JSON | Accepted / Implemented |

ADR-005 e ADR-006 permanecem no repositório como registro histórico do desenho-alvo relacional. O que foi efetivamente implementado — ledger JSON `.anki-workflow-export-state.json`, lock cruzado de processos, swap atômico com `sync_all`/`fs::rename`, rollback reverso e adoção pós-crash — está em ADR-007. SQLite existe no workspace apenas como espelho de leitura da coleção Anki em `sync-decks`.

> **Nota sobre `PLANO_MESTRE_ANKI_WORKFLOW_MCP_RUST_V2.md`:** as ADRs 001, 003, 005 e 006 citam esse plano-mestre em suas listas de referência. Ele **não faz parte deste repositório público**; as citações são históricas e os caminhos apontados por elas não são verificáveis aqui.
