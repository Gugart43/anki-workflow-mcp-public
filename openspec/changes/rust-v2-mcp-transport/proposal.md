# Proposal

## Why

A branch Rust já implementa validadores, compilação e revisão determinística, mas o executável `anki-workflow-mcp` expõe apenas `version`, `health` e `oracle`. Clientes MCP não conseguem usar essas capacidades via stdio; a instalação descrita no README ainda executa o servidor Python. Uma superfície Rust pequena e verificável permite experimentar a migração sem confundir o oráculo de testes com uma implantação completa.

## What Changes

- Acrescentar o comando explícito `serve` com transporte MCP stdio, sem logs no stdout.
- Expor ferramentas puramente em memória apoiadas no código Rust existente: `review_batch_information`, `cards_compile_drafts` e `check_csv`.
- Documentar o escopo experimental da branch Rust, a diferença entre Python e Rust e o comando de execução correto.
- Adicionar testes de protocolo (initialize, list/call, entradas inválidas, saída e stdout) e gates locais/CI sem substituir nem modificar o servidor Python.
- Fora de escopo: manifesto transacional, exportação, sincronização de mídia, emissão de vereditos clínicos e migração automática de configurações/produção.

## Capabilities

### New Capabilities
- `rust-mcp-stdio`: sessão MCP stdio no binário Rust, descoberta e invocação segura de ferramentas determinísticas em memória.

### Modified Capabilities
Nenhuma; o projeto ainda não possui specs de base no OpenSpec.

## Impact

`apps/anki-workflow-mcp` (CLI, transporte, testes), `Cargo.toml`/`Cargo.lock` conforme dependências verificadas, documentação de execução e workflow de CI. Não altera API ou execução do servidor Python existente, nem toca dados reais de Anki.
