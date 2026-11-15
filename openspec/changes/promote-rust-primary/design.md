# Design

## Context

Ver proposal.md para motivação. A base Python pública é ancestral dos sete commits Rust; o candidato incorpora cópia byte a byte das alterações locais ainda não commitadas. Os ADRs 001/003–006 descrevem destino transacional ainda não implementado. Não existe anki-state nem autorização persistida em quality_v1. Há seis ferramentas stdio puras, com patch em memória diferente do staging Python.

## Goals / Non-Goals

**Goals:** promover a entrada principal Rust com limites explícitos, preservar melhorias existentes, eliminar blockers concretos de auditoria com testes sintéticos, identificar builds e manter regressão multiplataforma.

**Non-Goals:** implementar SQLite/exports/media físicos, autenticar revisores, certificar medicina, migrar estado, distribuir baralhos ou fazer deploy de produção. Não declarar equivalência universal pelos testes diferenciais.

## Decisions

1. **Candidato isolado e preservação.** Partir do HEAD Rust cujo ancestral é main, copiar allowlist de arquivos modificados/untracked com hashes privados. Alternativa de checkout/stash no original rejeitada: risco de perda de trabalho.
2. **Rust principal, legado separado.** README e instalação iniciam por Cargo/binário e `serve`; Python continua acessível por módulo/interpreter explícito. Documentar colisão do nome executável entre instalações. Não remover código Python nem fingir equivalência das APIs.
3. **Identidade limitada e verificável.** Comando `build-info` retorna JSON com implementação, versão do pacote, revisão e estado Git opcionais. `--version` informa versão legível; MCP usa a mesma versão de pacote. Capturar metadados no build com fallback desconhecido, sem timestamps não determinísticos nem paths pessoais. Metadados são declarativos, não assinatura. Testar CLI/initialize e fallback, sem exigir Git no arquivo-fonte distribuído.
4. **Qualidade fail-closed.** Cobertura por recuperação e snapshot incluem conteúdo/contexto; revisões não autenticadas nunca autorizam export. Preservar flags falsas. Corrigir apenas bugs comprovados, sem inventar regras clínicas ou limitar notas pela extensão.
5. **Gates e revisão.** Seis auditores independentes, três implementadores com arquivos disjuntos e um revisor final (dez subagentes). Cargo apenas no controlador para evitar contenção. CI mantém nomes protegidos Test (...) e adiciona/verifica Rust & Parity em Linux/Windows. Sem alteração de proteção ou modelo global.
6. **Publicação.** Inspecionar índice exato e todos os commits do PR, novos arquivos incluídos. Publicar apenas fontes e fixtures sintéticas. PR revisado, CI no head e merge linear via squash permitido; conferir CI de main após merge. Histórico original permanece preservado nas branches/worktrees existentes.

## Risks / Trade-offs

- [Rust parcial confundido com v2 completa] → tabela implementado/planejado e aviso nos ADRs existentes.
- [Metadados Git incompletos/cache] → fallback explícito e gatilhos de rebuild para arquivos/ref; SHA não representa conteúdo sujo.
- [Falsos positivos heurísticos] → testes de fronteira e avisos separados de bloqueios; julgamento editorial permanece externo.
- [Dados privados no material herdado] → revisão de diff completo e arquivos novos; scratch/deploy fora da publicação.
- [CI posterior ao squash diverge do head revisado] → ler main e checks novamente; não declarar concluído antes.

## Migration Plan

Nenhuma migração de estado. Instalar binário com destino explícito e selecionar `serve` no cliente. Preservar configuração Python e rollback manual. A implantação atual não é tocada por este change; qualquer atualização posterior exige candidato, backup e smoke pré/pós-troca próprios.
