# Tasks

## 1. Preservação e contratos

- [x] 1.1 Preservar alterações tracked/untracked em candidato isolado, conferindo hashes de todos os arquivos copiados e ancestralidade de main.
- [x] 1.2 Criar proposta, design e specs quality-review/runtime-identity; conferir caminhos por status/instructions OpenSpec.
- [x] 1.3 Validar o change com OpenSpec estrito e registrar seis auditorias independentes com achados verificáveis.

## 2. Qualidade e conteúdo

- [x] 2.1 Revisar e integrar gate por recuperação, snapshots e evidência; verificar testes negativos/positivos de quality_gate, flags falsas e transporte stdio.
- [x] 2.2 Revisar HTML e extensões tabelas_notas/figuras_notas; corrigir blockers comprovados com regressões sintéticas e manter suíte diferencial verde.

## 3. Identidade de execução

- [x] 3.1 Implementar --version e build-info com origem/estado opcionais sem paths pessoais, com testes de CLI e fallback sem Git.
- [x] 3.2 Unificar versão de pacote com initialize MCP, verificar teste stdio e documentar limites declarativos da identidade.

## 4. Caminho principal e CI

- [x] 4.1 Atualizar README, instalação, contribuição, configuração e status dos ADRs para Rust-first sem retirar legado; conferir comandos contra CLI real.
- [x] 4.2 Atualizar CI para check/fmt/Clippy/test/build/paridade Linux/Windows com lockfile congelado; preservar nomes Test exigidos pela proteção e validar workflow.

## 5. Integração e publicação

- [x] 5.1 Executar cargo check, fmt, Clippy, testes completos e build, diferenciais e gates Python aplicáveis; registrar resultados reais.
- [x] 5.2 Obter revisão independente final do candidato e inspecionar conteúdo exato do índice/histórico contra main, incluindo arquivos novos e privacidade.
- [ ] 5.3 Criar PR e conferir leitura remota do head revisado; integrar apenas após CI e revisão aprovados, sem contornar proteção.
- [ ] 5.4 Ler estado remoto, merge SHA e CI de main; confirmar preservação do worktree original e registrar relatório final com limitações.

## Workflow follow-up

- Sincronizar e arquivar o change somente após tarefas e revisão concluídas; verificar specs e arquivo gerados.
- A VPS permanece na implantação já existente, sem alteração neste change.
