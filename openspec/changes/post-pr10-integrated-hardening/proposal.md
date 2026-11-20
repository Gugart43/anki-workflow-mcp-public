# Proposal
## Why
A auditoria pós-PR10 identificou divergências entre contrato MCP, validação e ergonomia. Os anexos fornecem correções e três extensões de borda que devem ser integradas sem duplicar o domínio Rust.
## What Changes
- Drafts MCP estritos, ranges coerentes, CLI lazy e hardening UTF-8/HTML/regex/tabelas.
- Snapshots canônicos, modo quick explicitamente não editorial, testes consolidados.
- Sétima ferramenta de exportação local com recibo, bloqueios e rollback; sincronização de mídia/decks somente CLI.
- Harness TS, schemas derivados e launcher npm sem scripts de instalação.
- Depreciação formal do Python, mantido como oráculo opcional até paridade e CI multiplataforma comprovados.
- **BREAKING**: ferramenta desconhecida retorna -32602; schemas estritos rejeitam typos; snapshot canônico invalida hashes antigos quando mudam bytes do protocolo.
## Capabilities
### New Capabilities
- `atomic-export`: exportação local fail-closed, idempotência e recuperação de falhas de I/O.
- `cli-sync`: sincronização explícita e segura para Anki local com fixtures sintéticas.
- `contract-distribution`: contratos gerados, interoperabilidade e distribuição fina npm.
- `validator-boundaries`: parsing UTF-8 e validação determinística visível com perfis.
- `legacy-sunset`: separação explícita do caminho produtivo Rust e oráculo legado.
### Modified Capabilities
- `quality-review`: seis tools puras mais uma exportação explícita; modo quick não certifica revisão; snapshot canônico.
- `runtime-identity`: ajuda e erros de argumento previsíveis sem runtime assíncrono desnecessário.
## Impact
Workspace Rust, testes Python diferenciais, CI/just, docs/OpenSpec, subprojetos Node isolados. In-Scope: implementação local e verificação real. Out-of-Scope: push, publicação npm/GitHub, deploy, coleção real, UI/workbench, gateway TS e remoção destrutiva prematura do Python.
