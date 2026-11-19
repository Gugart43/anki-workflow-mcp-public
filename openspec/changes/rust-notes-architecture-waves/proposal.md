# Proposal
## Why
Os quatro pacotes anexados eliminam caminhos concorrentes de notas e melhoram a manutenção do núcleo Rust. A base confirmada é main em 0a7ae05; o clone original possui mudanças anteriores e não será alterado.
## What Changes
- Perfis declarativos, extração HTML e métricas de notas desacoplados.
- AST canônica com fluxo DAG validado, render semântico e adaptador explícito para legado.
- Domínio tipado, schemas MCP derivados, erros completos e oracle opt-in.
- CI paralelo, execução local unificada, documentação e release verificável.
- **BREAKING**: tipo de draft desconhecido passa a erro; quality_prepare rejeita reviews; executável Python passa a anki-workflow-mcp-legacy; oracle exige feature.
## Capabilities
### New Capabilities
- `notes-document`: compilação de notas estruturadas e políticas de domínio configuráveis.
- `mcp-input-contract`: schemas precisos e diagnóstico completo de parâmetros.
### Modified Capabilities
Nenhuma exigência existente é removida. As garantias quality-review e runtime-identity permanecem aplicáveis.
## Impact
Workspace Rust, perfis JSON, ferramentas de desenvolvimento, testes diferenciais e documentação. Acrescenta schemars 1 e ammonia 4.2.5. Não implanta serviços, publica releases, altera proteções nem exclui branches remotas.
