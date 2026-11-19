# Design
## Context
Ver proposal.md. Cargo check e 165 testes Rust passaram na base remota. Required checks reais são Test (ubuntu-latest) e Test (windows-latest).
## Goals / Non-Goals
**Goals:** unir os pacotes com preservação de contratos fora das mudanças explícitas e fronteira HTML5 fail-closed.
**Non-Goals:** certificação clínica, persistência Rust, rollout de produção e mutações destrutivas no GitHub.
## Decisions
1. A Fase 2 rege o modelo canônico; tipos legados da Onda 2 continuam aceitos por adaptador. Não pode haver fallback de documento v2 inválido para legado vazio.
2. Os movimentos físicos vêm depois das implementações paralelas. A fachada pública mantém aliases dos módulos antigos; render é folha, draft e quality não se importam mutuamente.
3. Ammonia usa a união estrita de tags/classes semânticas dos pacotes, nunca wildcard. Não regredir normalizações benignas, conteúdo clínico ou formato AnkiMBR.
4. Diagnósticos usam DiagnosticCode validado; dados de CSV usam CsvRowNumber e conversão explícita nas fronteiras históricas. Digests não expõem tuple field público.
5. CI mantém os checks protegidos antigos como agregadores fail-closed dependentes de Rust/Python/parity; não muda branch protection. Actions já fixadas por SHA não serão rebaixadas.
6. O toolchain é fixado conforme versão realmente disponível, documentando desvio se 1.99.0 não estiver acessível. A MSRV real é 1.89 no lock vigente: uuid 1.29.2 declara 1.89; rmcp 3.10.0 e as let-chains exigem 1.88. Será testada, não presumida como 1.85.
7. Paridade estrita permanece para comportamentos não alterados. Mudanças intencionais de notas/tipo/fluxo terão testes de migração separados; não fingir equivalência universal.
8. Os subagentes editam conjuntos disjuntos, sem commits, pushes ou cargo concorrente; o controlador integra manifests, exports, mudanças de códigos e módulos e executa gates.
## Risks / Trade-offs
- Render semântico muda bytes do HTML legado → testes de conteúdo/segurança e documentação de migração.
- Perfis desserializados não podem perder regex derivada → testes do caminho serde e loader.
- Ammonia normaliza HTML benigno → comparar semântica segura sem aceitar remoção de atributos ativos.
- MSRV 1.85 versus sintaxe atual → testar toolchain e declarar mínimo real, sem alegação infundada.
## Migration Plan
Worktree isolado, sete frentes disjuntas, integração serial, revisões independentes e gates finais. Publicação depende de autorização explícita; runbook de arquivo é apenas documento.
