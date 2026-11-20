# Workflow de geração com evidência e revisão por recuperação

## Objetivo e limites

O LLM redige, o motor Rust verifica contratos e consistência, e um revisor avalia conteúdo e pedagogia. Nenhum resultado determinístico demonstra correção clínica ou superioridade sobre um baralho. Não aumentar notas, cortar contexto ou multiplicar Clozes para melhorar uma métrica.

Rust é a autoridade de parsing, compilação e `quality_v1`. A fronteira MCP expõe 7 ferramentas: seis operações em memória e `export_reviewed_files` (`apps/anki-workflow-mcp/src/export.rs`) para escrita transacional no disco local; operações de coleção (`sync-media`, `sync-decks`) ficam restritas à CLI nativa. Consulte [arquitetura](architecture.md) para contratos, segurança e compatibilidade. O hash de conteúdo não é uma atestação autenticada. `export_authorized` e `clinical_correctness_certified` continuam `false`, inclusive em `PASS`.

## Sequência operacional

1. **Fontes antes de cartões.** Selecionar inicialmente o `DomainProfile` (`med_br` ou `general`) e material autorizado, apropriado e atualizado. Guardar título, versão/edição, localização e trecho efetivamente consultado. Uma string bibliográfica ou URL não prova que a fonte foi consultada. Não usar os baralhos de comparação como autoridade clínica.
2. **Mapa de obrigações.** Enumerar decisões/conceitos essenciais, população, contexto, discriminações e condições de aplicação. Justificar exclusões na curadoria, sem preencher cotas ou gerar um cartão para cada frase. Cada obrigação tem ID estável e fontes identificadas.
3. **Desenho da recuperação.** Escolher QA ou Cloze pela tarefa, não para contornar validação. Uma unidade principal por recuperação. Contexto suficiente para resposta única; mecanismo, explicação e exceções opcionais vão às notas quando não forem a cobrança. Cada índice Cloze é revisado como cartão separado, com as demais lacunas reveladas.
4. **Rascunho e compilação.** Compilar os rascunhos estruturados chamando a ferramenta `cards_compile_drafts`. Na fronteira MCP, a ferramenta impõe validação estrita com `StrictRawCardDraft` (configurada com `#[serde(deny_unknown_fields)]`) e validação canônica de `NoteDocument`/AST, rejeitando terminantemente campos desconhecidos ou espúrios no payload (fora da fronteira MCP, `RawCardDraft` mantém deserialização tolerante para interoperabilidade legada). O compilador Rust normaliza campos, purga IDs internos, renumera Clozes sequencialmente e gera os CSVs canônicos em memória (`qa_csv` e `cloze_csv`), preservando a ordem dos campos AnkiMBR e integrando extensões seguras de `tabelas_notas` e `figuras_notas`. Não criar CSS/containers redundantes. Reutilizar templates locais sem copiá-los para o repositório. Evitar pistas, referências a cartão anterior, V/F isolado, opções de múltipla escolha e listas de fatos independentes ocultas numa lacuna. Opcionalmente, inspecione contagens e tokens com `review_batch_information`. Consultas às regras podem ser feitas via `generation_contract_get`.
5. **Enriquecimento seletivo.** Adicionar explicação apenas se ajuda a entender, discriminar ou evitar erro e tem suporte de fonte. Notas vazias podem ser corretas; notas extensas não são automaticamente boas nem ruins e não causam falha crítica de atomicidade por si sós. Dose, mecanismo e ressalva nas notas também precisam de verificação. Nunca fabricar citações para “completar” o cartão.
6. **Mídia e tabelas.** Usar quando ajudam o objetivo. Imagens com arquivo namespaced, `<figure>`, `<figcaption>` descritivo e alt apropriado; tabela limpa e semanticamente útil. O gate examina markup, não existência dos arquivos nem correspondência clínica dos pixels. Mídia remota/AnkiConnect permanece fora desta extensão pura.
7. **Preparar revisão.** Compilar CSVs e chamar `quality_prepare` com fontes, obrigações e vínculos de evidência para cada recuperação. Obter o snapshot determinístico (`snapshot_hash`). Trecho contido na fonte comprova apenas correspondência textual; não garante que implica a resposta.
8. **Revisão editorial independente.** Por QA e índice Cloze: fidelidade; unidade recuperada; contexto; ausência de pistas; relevância; notas; mídia. Registrar revisor, justificativa, dimensões e decisão para o snapshot exato. Não preencher todas as dimensões com `true` automaticamente. Conferir inclusive versões, populações, unidades e contradições entre fontes.
9. **Avaliar e reparar.** Chamar `quality_evaluate` para agregar evidência, cobertura, estrutura e revisões contra o snapshot. `FAIL`: corrigir ou colocar em quarentena; `INCOMPLETE`: completar evidência/revisão, nunca tratar como aprovado. Reparar drafts afetados via `batch_patch_draft` (patch puramente em memória recebendo `{draft, updates}` $\to$ `{card}`, sem tocar em arquivos de staging em disco) e recompilar com `cards_compile_drafts`. Uma alteração no lote, fontes, obrigações ou vínculos altera o `snapshot_hash` e invalida revisões anteriores.
10. **Exportação local transacional.** Um `PASS` não escreve por si só. Chamar explicitamente a ferramenta `export_reviewed_files` (`apps/anki-workflow-mcp/src/export.rs`) informando diretório de destino, batch ID, CSVs exatos, recibo `receipt_hash` e veredito de avaliação. A ferramenta executa exportação transacional no disco local com salvaguardas rigorosas:
    - **Validação de veredito:** Exige compulsoriamente a declaração `verdict == "PASS"`; avaliações com `FAIL` ou `INCOMPLETE` são terminantemente rejeitadas.
    - **Integridade via `receipt_hash`:** Valida o hash canônico dos CSVs (`sha256(b"qa\0" + qa_csv + b"\0cloze\0" + cloze_csv)`) contra o payload fornecido, impedindo escrita caso o conteúdo tenha sido alterado pós-auditoria.
    - **Escrita atômica e rollback:** Prepara a gravação em diretório temporário de staging no mesmo sistema de arquivos e promove os arquivos atomicamente; falhas de I/O acionam rollback completo sem corromper o lote.
    - **Manifesto transacional:** Registra o manifesto `.anki_export_manifest.json` com histórico de exportação, batch ID, hashes dos CSVs e metadados no diretório de destino.
    - O servidor revalida estrutura CSV e HTML antes da escrita. Não importa no Anki nem autentica o revisor. `sync-media` e `sync-decks` são utilitários da CLI nativa Rust. O runtime Python legado em `src/anki_workflow_mcp/` é deprecated, sem interoperabilidade de recibos entre diferentes protocolos.

## Critérios editoriais de aceitação

| Dimensão | Pergunta verificável na revisão | Não aceitar como substituto |
|---|---|---|
| Fidelidade | O trecho e a fonte realmente sustentam resposta e explicações? | Nome de diretriz sem seção/trecho |
| Recuperação | Qual resposta o aluno deve produzir neste cartão? | Contagem de caracteres ou omissões |
| Contexto | População/situação tornam a resposta inequívoca? | Frente curta e ambígua |
| Pistas | A resposta aparece no enunciado/hint/estrutura? | Lacuna sintaticamente válida |
| Relevância | Qual decisão/conceito essencial do mapa é coberto? | Quantidade produzida |
| Notas | Explicam por quê, discriminam ou alertam com suporte? | Texto longo, ornamentação ou repetição |
| Mídia | A imagem é necessária, correta e descrita? | Existência de uma tag img |

Redundância exata pode ser detectada deterministicamente. Duplicação semântica, interferência, relevância e fidelidade continuam exigindo julgamento. Não apagar automaticamente cartões com o mesmo alvo e contextos diferentes.

## Verificações determinísticas de conteúdo (`quality_content`)

Além da estrutura CSV/Cloze e do grafo `Source -> Obligation -> Binding -> Evidence`, o gate `quality_v1` aplica verificadores determinísticos de conteúdo:

| Código | Severidade | O que detecta |
|---|---|---|
| `QUALITY.UNSAFE_HTML` | `Critical` | HTML inseguro: atributos/eventos ativos (`onclick`, `onload`, `onerror`), estilos com `url(...)`/`expression(...)`, ou URIs remotas/inline data (`src="http..."`, `src="data:..."`). |
| `QUALITY.DUPLICATE_RETRIEVAL` | `Critical` | Mesmo par `(prompt_visível, alvo_recuperado)` normalizado em cartões distintos, mesmo com markup HTML diferente. |
| `QUALITY.BINARY_CHOICE_CLOZE` | `Critical` | Escolhas binárias fechadas entre colchetes com barra (ex.: `[A / B]`, `[sim/não]`), preservando unidades (`[mg/dL]`) e posologias (`[12/12h]`). |
| `QUALITY.ALLINONE_MC_LEAK` | `Critical` | Comandos de múltipla escolha (`é incorreto afirmar`, `assinale a alternativa`, `exceto:`) ou vetores de gabarito (`1 0 0 0 0`). |
| `QUALITY.MISSING_MEDIA_TAG` | `Critical` | Tag `mídia-ausente` / `midia-ausente` indicando placeholder inacabado. |
| `IMG.BARE_IMG` | `Critical` | `<img>` sem `<figure>` e `<figcaption>` descritiva nas imagens relacionadas. |
| `QUALITY.MISSING_REFERENCE` / `QUALITY.GENERIC_REFERENCE` | `Warn` | `Referências` vazia ou reduzida a rótulo genérico (`TdC`, `Resumo`, `UpToDate`, `Apostila`, etc.) sem ano, edição, seção ou identificador. |
| `QUALITY.AMBIGUOUS_PROMPT` | `Warn` | Mesmo enunciado visível exigindo respostas distintas. |
| `QUALITY.DUAL_TARGET_QA` | `Warn` | Enunciado QA combinando duas cobranças interrogativas ligadas por `e` ou `versus`. |
| `QUALITY.ANSWER_EXPLANATION_MIX` | `Warn` | Resposta QA extensa (`> 220` chars ou `>= 3` frases) enquanto `Notas Adicionais` permanece vazio. |
| `QUALITY.LIST_SCAFFOLD_WARN` | `Warn` | Lista `<ul>` com `>= 3` `<li>` e apenas 1 índice Cloze (risco de exclusão por eliminação). |
| `QUALITY.GENERIC_IMG_ALT` | `Warn` | `<img>` sem `alt` ou com `alt` genérico (`img`, `imagem`, `foto`, `figura`). |
| `QUALITY.NEAR_DUPLICATE_WARN` | `Warn` | Sobreposição lexical (`Jaccard >= 0.70`) no mesmo `Campo de Estudo` entre cartões distintos. |

### Segurança de HTML e Limites dos Formatadores Legados

O compilador Rust (`compile_drafts`) e o gate determinístico aplicam rejeição estrita (fail-closed) contra qualquer construção de HTML ativo que possa introduzir scripts, execução no webview do Anki ou carregamento remoto de recursos:
- **Atributos e eventos ativos:** Qualquer manipulador como `onclick`, `onload`, `onerror`, `onmouseover`, etc., é terminantemente recusado.
- **Estilos perigosos:** Propriedades `style` contendo `url(...)`, `expression(...)` ou comportamentos dinâmicos são rejeitadas.
- **URIs remotas e dados embutidos:** Atributos `src` apontando para protocolos remotos (`http://`, `https://`) ou esquemas de dados embutidos (`data:`) são bloqueados com o código `QUALITY.UNSAFE_HTML` (`Critical`). Imagens locais devem referenciar nomes simples de arquivo devidamente namespaced.

> **Distinção Fundamental de Segurança:** Os helpers legados `sanitize_field` e `render_media_figure` permanecem como formatadores de compatibilidade destinados à normalização e escaping estrutural de strings. Eles **NÃO** atuam como sanitizadores de segurança. O bloqueio contra HTML ativo reside exclusivamente na lógica fail-closed do compilador Rust e do validador `quality_v1` (`QUALITY.UNSAFE_HTML`).

## Modos e notas estruturadas

`strict` é o padrão para preparação/avaliação editorial. `quick` mantém verificações estruturais e de segurança, mas dispensa o grafo editorial: não certifica fidelidade nem substitui revisão e não autoriza exportação. Consulte [informação e perfis](information-quality.md).

Para notas novas, prefira `NoteDocument`/`NoteBlock` com `NoteSource` e compile no caminho AST; os campos `tabelas_notas` e `figuras_notas` pertencem à compatibilidade. Na fronteira MCP, `StrictRawCardDraft` impõe `#[serde(deny_unknown_fields)]` e validação canônica de `NoteDocument`, enquanto `RawCardDraft` mantém deserialização permissiva para compatibilidade legada fora da fronteira MCP. Na etapa 8, use os `EditorialChecklistItem` do perfil escolhido. O campo de notas nunca recebe JavaScript ou templates privados.

## Quantidade e Atomicidade

`strict_v2` não impõe teto de 20 cartões. Dimensionar pelo mapa de obrigações, depois conferir cobertura e cada recuperação. Não confundir nota Cloze com cartão: cada índice Cloze ($c_1, c_2, \dots$) é tratado como recuperação separada. Lotes operacionais menores podem facilitar revisão, mas não são critério de qualidade. O comprimento de notas adicionais explicativas isoladamente não acarreta violação crítica de atomicidade quando a cobrança focal for unívoca e notas ricas/seguras contextualizam o alvo. O caminho Python/`legacy_compat` ainda mantém o teto legado de 20 cartões e não deve receber lotes maiores sem suporte explícito.

## Evidência educacional

Prática de recuperação e distribuição temporal têm apoio na educação em saúde, mas isso não valida um layout, um limite de tamanho ou a superioridade deste workflow sobre um deck particular:

- Trumble E, Lodge J, Mandrusiak A, Forbes R. *Systematic review of distributed practice and retrieval practice in health professions education*. Adv Health Sci Educ Theory Pract. 2024;29:689–714. DOI: [10.1007/s10459-023-10274-3](https://doi.org/10.1007/s10459-023-10274-3). [PubMed](https://pubmed.ncbi.nlm.nih.gov/37615780/).
- Serra MJ, Kaminske AN, Nebel C, Coppola KM. *The Use of Retrieval Practice in the Health Professions: A State-of-the-Art Review*. Behav Sci. 2025;15(7):974. [PubMed](https://pubmed.ncbi.nlm.nih.gov/40723757/).

Superioridade editorial requer comparação cega com rubrica pré-definida e verificação de fontes. Superioridade de aprendizado requer estudo de retenção/transferência, não teste de software.
