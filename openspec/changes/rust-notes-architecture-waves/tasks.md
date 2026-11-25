# Tasks
## 1. Preparação
- [x] 1.1 Reconciliar os anexos, registrar conflitos e validar OpenSpec estritamente.
## 2. Notas
- [x] 2.1 Implementar Fase 1 com perfis, extração e métricas; verificar testes de compatibilidade.
- [x] 2.2 Implementar AST, DAG, render e adaptador da Fase 2; verificar invariantes e segurança.
- [x] 2.3 Integrar normalização tipada e compilador único; testar aliases, tipos inválidos e notas v2.
## 3. Engenharia
- [x] 3.1 Implementar CI, DX, documentação, release e runbook; verificar lock, receitas e contratos.
- [x] 3.2 Implementar domínio/parser tipados e preparação sem revisões; verificar serde e snapshots.
- [x] 3.3 Implementar schemas MCP, erros completos e oracle opt-in; verificar subprocessos reais.
- [x] 3.4 Integrar HTML5 seguro e whitelist semântica; verificar corpus seguro e malicioso.
- [x] 3.5 Modularizar draft/render/quality com fachada compatível; verificar grafo acíclico e cargo check.
## 4. Aceitação
- [x] 4.1 Revisar cada frente e o diff inteiro; resolver achados e repetir testes afetados.
- [x] 4.2 Executar fmt, clippy, Rust, Python, mypy, diferencial, contratos e smoke MCP; registrar resultados reais.
## 5. Follow-up P0-3 (`compile_mode` alcançável)
- [x] 5.1 Expor `compile_mode` em `cards_compile_drafts` (`legacy_compat` por padrão, `strict_v2` opt-in) e delegar o compilador único a `CompileMode::StrictV2` quando solicitado, mantendo os wrappers históricos em `legacy_compat`; verificar rejeição de campos legados, bordas de `notas_adicionais` e gating via stdio real.
- [x] 5.2 Regenerar `contracts/v2/schemas` e `contracts/v2/ts/index.d.ts` com o codegen real; verificar que `compile_mode` ganhou enum fechado e default e que não houve drift nos demais schemas.

## 6. Onda P0 (inconsistências do caminho de produção)
- [x] 6.1 P0-1: aceitar `<section class="anki-list|anki-steps|anki-flow">` como formato canônico em `NOTE.STYLE_WARN`, mantendo punidos bullets legados e itens sem negrito fora de seção canônica; alinhar `preferred_list_format` ao renderizador. Verificado por E2E (`compile_drafts_strict` → `review_batch_information` sem `NOTE.STYLE_WARN`) e por regressão dos casos legados.
- [x] 6.2 P0-2: expor `report.notes_diagnostics` (agregado consultivo, deduplicado por cartão) e preservar a identidade na promoção de `NOTE.*` Critical via `Diagnostic.cause_code` + `suggestion`, sem alterar o cálculo de `verdict` (Warn de nota permanece consultivo por decisão de produto). Coberto por `quality_gate/notes.rs` incluindo colisão QA×CL na mesma linha.
- [x] 6.3 P0-4: conferir `quote` de evidência `scope="notes"` contra o campo `Notas Adicionais` do cartão (HTML cru ou texto visível) com `QUALITY.NOTES_EVIDENCE_QUOTE_NOT_FOUND` (Critical); omitir o check quando o campo não é localizável. Documentado em `docs/information-quality.md`.
- [ ] 6.4 Follow-up conhecido: escopo `context` não é conferido contra `Enunciado`/`Sentença` (mesma assimetria do P0-4); decidir entre novo código por escopo ou exigência documentada de dupla conferência.
- [ ] 6.5 Follow-up conhecido: `</section>` dentro de comentário HTML fecha o intervalo canônico cedo (falso positivo teórico, nunca emitido pelo compilador); avaliar ignorar comentários no scanner de seções.
- [x] 6.6 Corrigir `has_unclosed_cloze_marker` (`quality.rs`): comparava índices de `char` com offsets de byte e marcava como `QUALITY.MALFORMED_CLOZE` (Critical) sentenças válidas com acento antes do marcador — em pt-BR praticamente todo Cloze real. Regressão coberta por teste unitário com multibyte.
- [x] 6.7 Alinhar a mensagem de `NOTE.STYLE_WARN` ao novo critério ("Legacy list formatting found — preferred: …") em Rust, no oráculo Python e no prefixo do teste de paridade.
- [ ] 6.8 Follow-up conhecido: `NOTE.METADATA` é evadível prefixando bullet (`"• ID: 4242 …"`) porque a regex de cabeçalhos exige início de linha; avaliar `(?:[•▪▸►]\s*)?` no padrão de Rust e de Python juntos (paridade).
- [ ] 6.9 Follow-up conhecido: mensagem de erro para `notas_adicionais` misturando `blocos`/`fontes` com chave legada não nomeia a chave nem o `CompileMode` (comportamento pré-existente, idêntico em ambos os modos); melhorar a UX sem mudar a semântica.
- [ ] 6.10 Follow-up conhecido: o `inputSchema` de `cards_compile_drafts` é a união dos dois modos (declara campos legados e a string de notas, recusados só em runtime por `compile_mode=strict_v2`); se o contrato precisar refletir a exclusividade, exige `if/then` sobre `compile_mode` no schema gerado.
- [ ] 6.11 Follow-up conhecido: o oráculo Python de `NOTE.STYLE_WARN` só detecta bullets (`[•\-\*]`) e não conhece a regra de `<li>`/`preferred_list_format`; a paridade do diferencial é apenas de prefixo de mensagem. Alinhar as duas implementações (ou declarar a divergência como aceita) ao tocar nesse check de novo.

## Workflow follow-up
- Publicar PR ou integrar na main somente após autorização explícita.
- Arquivar branch antiga somente após auditoria e autorização separadas.
