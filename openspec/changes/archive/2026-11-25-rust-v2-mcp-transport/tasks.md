# Tasks

## 1. Transporte MCP Rust

- [x] 1.1 Selecionar dependência SDK compatível, adicionar comando `serve` e implementar inicialização/encerramento MCP stdio; verificar `cargo check --workspace` e handshake com cliente JSON-RPC real.
- [x] 1.2 Implementar descoberta e chamadas tipadas de `review_batch_information`, `cards_compile_drafts` e `check_csv`, reutilizando `anki-validate`; verificar respostas comparadas ao oráculo e ausência de I/O em decks.
- [x] 1.3 Adicionar testes de integração para catálogo, entradas inválidas, múltiplas chamadas, EOF e stdout só de protocolo; verificar `cargo test --workspace --all-features`.
- [x] 1.4 Documentar em README/arquitetura o comando experimental e a permanência do Python para estado/exportação/mídia; verificar comandos documentados com o binário local.

## 2. Verificação transversal

- [x] 2.1 Rodar `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, build e testes diferenciais Python↔Rust; corrigir regressões observadas e registrar resultados reais.
- [x] 2.2 Atualizar CI se necessário para executar os novos testes sem dependências externas nem dados privados; verificar workflow e status `openspec validate rust-v2-mcp-transport --strict`. Workflow existente já executa `cargo test --workspace --all-features` e suíte diferencial em Linux/Windows; nenhuma alteração necessária.

## Workflow follow-up

- Arquivar a mudança somente após revisão e decisão explícita de incorporar o contrato como spec de base.
