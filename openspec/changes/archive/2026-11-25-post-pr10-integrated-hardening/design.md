# Design
## Context
Ver proposal.md e plano .superpowers/post-pr10/plan.md. Base d9feaa4, check Rust confirmado. Há worktrees antigas sujas intocadas.
## Goals / Non-Goals
**Goals:** fechar lacunas verificáveis dos anexos e preservar Rust como SSOT.
**Non-Goals:** UI adiada, deploy, aprovação clínica e publicação.
## Decisions
1. Macro compartilhada gera RawCardDraft tolerante e StrictRawCardDraft da fronteira MCP; evita quebrar oráculo legado.
2. Sete ferramentas finais: seis puras e export explícito. Tool desconhecida -32602. Escritas Anki reais somente CLI explícita.
3. Export usa ledger e lock de diretório, staging único no mesmo filesystem, fsync e rollback RAII; não promete transação multi-arquivo crash-safe. Recibo hash não é assinatura autenticada.
4. Sync-media preflight completo e rollback; sync-decks snapshot consistente SQLite com fixtures sintéticas.
5. QualityMode default Strict; Quick nunca equivale a certificação editorial. Hash canônico preserva multiplicidade.
6. Node24/pnpm fixo em subprojetos isolados; versões e SHAs conferidos no fornecedor. Nenhum cargo/build.rs requer Node.
7. Dezoito implementadores em arquivos disjuntos, dois revisores finais; controlador integra superfícies compartilhadas. Sem commits ou push.
## Implemented vs Planned
| Capacidade | Base | Alvo verificável |
|---|---|---|
| MCP | 6 tools | 7 com export |
| Sync | Python | CLI Rust |
| TS | ausente | harness/codegen/launcher |
| Python sunset | executável legado | aviso e caminho opcional; exclusão condicionada |
## Risks / Trade-offs
- Crash entre renames → não alegar atomicidade global; preservar backups e erros de recuperação.
- SQLite WAL concorrente → snapshot consistente ou rejeição explícita, nunca cópia silenciosamente incoerente.
- Drift intencional de validadores → testes de migração separados, não afrouxar paridade indiscriminadamente.
- Publicação/OIDC exige setup externo → runbook, sem side effects remotos.
## Migration Plan
Implementar e verificar em worktree isolada. Publicação e remoção final do legado dependem de autorização e CI remoto. Rollback local por patches revisados, sem alteração de outras worktrees.
