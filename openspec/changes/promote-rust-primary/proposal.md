# Proposal

## Why

A linha Rust já oferece compilação e revisão em memória com transporte MCP, mas a entrada pública continua apresentando Python como único caminho e não distingue arquitetura futura de recursos executáveis. A promoção à `main` precisa preservar o legado e tornar instalação, identificação e gates verificáveis, sem alegar paridade transacional.

## What Changes

- Integrar os commits Rust e as melhorias locais preservadas, incluindo quality_v1, múltiplas tabelas/figuras nas notas e testes stdio.
- Apresentar Rust como caminho principal para geração/validação em memória e Python como legado explícito para manifesto, staging em disco, mídia e exportação.
- Expor identidade de versão/build verificável sem confundir versão de pacote com commit de origem ou conferir confiança a uma revisão declarada.
- Executar check, fmt, Clippy, testes, build e paridade amostral em CI Linux/Windows, preservando nomes dos checks protegidos.
- Revisar segurança, privacidade, contratos e documentação antes da integração remota; manter intactos worktrees originais e implantação existente.

## Capabilities

### New Capabilities

- `quality-review`: revisão determinística com grafo de evidência, snapshot e cobertura por recuperação, sem autorização de exportação ou certificação clínica.
- `runtime-identity`: identificação coerente da implementação Rust, versão e origem do build via CLI/MCP, com origem desconhecida explícita.

### Modified Capabilities

Nenhuma: não existia inventário OpenSpec neste repositório.

## Impact

Workspace Cargo, servidor MCP stdio, compilação/normalização/validação de drafts, fixtures públicas sintéticas, README, documentação/ADRs e CI. Python permanece disponível, sem migração automática de estado nem substituição de APIs transacionais. A integração usa PR e CI, sem force-push, mudança de visibilidade, proteção ou configuração global de modelos. Deploy de produção está fora deste change.
