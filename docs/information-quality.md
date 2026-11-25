# Quantidade e qualidade de informação

Separe três camadas: **recuperação obrigatória**, **contexto necessário** e **explicação opcional**. Uma frente longa pode sustentar uma única decisão; uma lacuna curta pode esconder uma lista.

`information_profile`/`information_profiles` apresentam caracteres visíveis, candidatos de enumeração e sinais consultivos, sempre com revisão semântica necessária. Em Cloze, leia cada índice como um cartão e não some todas as respostas como se fossem cobradas simultaneamente. No modo compacto, orientações comuns aparecem uma vez em `information_guidance`.

Notas curtas ou vazias são permitidas. Repetição literal sem explicação e comentários exclusivamente sobre ocorrência em prova podem gerar avisos; mencionar resposta/prova e explicar algo não basta para reprovar. Não há volume mínimo nem orçamento derivado de estatísticas de baralhos particulares. No Rust `strict_v2`, tampouco há limite fixo de 20 cartões por lote: reporte a contagem e revise cada recuperação, inclusive cada índice Cloze. `legacy_compat` mantém o teto histórico enquanto o Python permanece intacto.

A regra operacional QA de 1–2 itens permanece, sem conversão automática para Cloze: revise a unidade cobrada e mova detalhes opcionais para notas. Comprimento isolado gera aviso, não prova de sobrecarga. Pontuação não demonstra independência semântica; enumeração misturada com apostos e conjunções pode super/subcontar. Prefira listas explícitas para itens independentes e revisão editorial em construções ambíguas.

## Perfis declarativos e métricas

`DomainProfile::by_name` seleciona `med_br` (padrão) ou `general`. Ambos declaram `default_max_note_chars=1200` e `table_max_note_chars=2500`; são limiares de aviso de notas, não cotas de aprendizado. Um `max_length` explícito prevalece sobre o limite do perfil. O motor Rust compila as regexes declarativas; não existe validador de perfil JS paralelo.

`InfoPolicy` define propósito das notas, fidelidade, restrição de metadados/repetição literal e formato de lista. `ExamBoilerplateConfig` contém `enabled`, rótulos e padrões: `med_br` ativa a detecção consultiva de notas exclusivamente sobre provas; `general` a desativa. `EditorialChecklistItem` orienta o revisor, não é uma atestação automática.

O parser HTML5 real (`ammonia` sobre `html5ever`, em `crates/anki-validate/src/html_security.rs`) atua como autoridade única da validação de fronteira HTML5, e a extração de texto é feita pelo mesmo parser em `crates/anki-validate/src/html_text.rs` (`parse_html_text`, `visible_char_count`), substituindo abordagens frágeis de strip por regex/chars. O parser suprime ativamente o conteúdo interno de tags proibidas (`<script>`, `<style>`, `<iframe>`, `<object>`, `<embed>`, `<svg>`, `<math>`, `<form>`), impedindo que código ou metadados inflem a contagem visível. `compute_notes_metrics` (`crates/anki-validate/src/note_metrics.rs`) pondera prosa, listas, legendas visuais e células de tabela separadamente (pesos 1.0, 0.75, 0.60 e 0.55); esses números descrevem apresentação e não medem diretamente carga cognitiva. `batch_information` reporta candidatos de lista em QA e Cloze, que ainda exigem julgamento.

## Segurança HTML e integridade estrutural

A fronteira de segurança rejeita conteúdo hostil ou malformado com o código `QUALITY.UNSAFE_HTML`, cobrindo:
- Tags proibidas: `<script>`, `<style>`, `<iframe>`, `<object>`, `<embed>`, `<svg>`, `<math>`, `<form>`.
- Atributos de evento inline: qualquer manipulador `on*` (como `onload`, `onclick`, `onerror`).
- Esquemas de URI perigosos: links e fontes utilizando `javascript:`, `vbscript:` ou `data:text/html`.

Além da sanitização HTML, a validação estrutural aplica verificações rigorosas:
- **Hardening de Cloze**:
  - `CLOZE.ACRONYM_LEAK`: emitido quando o alvo da lacuna é seguido de sigla parentética visível (`(IAM)`, `(AVE)`) ou é ele próprio uma sigla precedida do nome completo, vazando a resposta. O matching de iniciais aceita siglas de 2 a 8 caracteres (`POST_ACRONYM_RE`/`TARGET_ACRONYM_RE`).
  - `CLOZE.EXT_HINT_LEAK`: emitido quando um exemplo parentético imediatamente após a lacuna pode entregar a resposta; a correção esperada é a sintaxe canônica de hint `{{cN::alvo::hint}}`.
  - `CLOZE.NESTED`: disparado quando a lacuna contém um marcador de Cloze aninhado (`{{cN::`).
- **Restrição de tabelas**:
  - `TBL.CELL_TOO_LONG`: acusa células de tabela cujo conteúdo exceda 120 caracteres visíveis (`> 120`), prevenindo sobrecarga informacional em linhas tabulares.

### Limites determinísticos de campo HTML (16 KiB e 32 níveis)

Antes de qualquer parse, cada campo HTML passa por dois limites determinísticos (`crates/anki-validate/src/html_security.rs`). Estourá-los é rejeição **fail-closed** com diagnóstico próprio — e **não** `QUALITY.UNSAFE_HTML`: o conteúdo pode ser legítimo, apenas grande ou fundo demais.

| Limite | Valor | Código emitido |
| --- | --- | --- |
| Tamanho do campo | 16.384 bytes (16 KiB) | `HTML.FIELD_TOO_LARGE` |
| Profundidade de aninhamento | 32 níveis de elemento | `HTML.NESTING_TOO_DEEP` |

- **Onde se aplica**: todo campo HTML dos cartões avaliados pelo gate (`quality_prepare`/`quality_evaluate`, diagnóstico Critical com `row`) e todo campo de draft/coluna compilada em `cards_compile_drafts` (a mensagem de `COMPILE.INVALID_DRAFT` abre com o código específico). A profundidade é avaliada primeiro: um payload que estoura os dois limites reporta o aninhamento, que é a causa real do custo do parser.
- **Por que existe**: o parser HTML5 real (`ammonia` sobre `html5ever`) tem custo superlinear em profundidade. Medido antes do limite: 20.000 `<div>` aninhados levavam 72 s em debug (4,1 s em release) e ainda assim eram **aceitos**; hoje a mesma entrada é rejeitada em menos de 1 s. O teto de bytes é uma segunda barreira contra payloads de amplificação antes do parse; 16 KiB é ~44× o maior campo dos fixtures de paridade do repositório (371 bytes).
- **Mensagem**: cita o limite real e o valor medido (por exemplo `HTML field has 17007 bytes, above the 16384-byte (16 KiB) field limit`), para que "campo grande" nunca mais apareça como "conteúdo ativo".

Não confundir perfis de compatibilidade (`legacy_compat`/`strict_v2`), modo de compilação de notas (`CompileMode`) e modo editorial (`QualityMode`). O `CompileMode` é escolhido por `cards_compile_drafts.compile_mode` e seu padrão é `legacy_compat`: `strict_v2` recusa a string legada de notas (formato do modo legado) e recusa qualquer campo legado **populado** (`fisiopatologia`, `raciocinio_clinico`, `armadilhas_diferenciais`, `correlacao_visual`, `tabela`, `texto_livre`, `tabela_notas`, `tabelas_notas`, `figuras_notas`), sem alterar o HTML renderizado quando o documento é válido. Ausência, `null`, documento vazio e objetos legados com **todas** as chaves vazias (o formato que o próprio schema publica como default) são aceitos: a fronteira MCP poda chaves legadas vazias antes de compilar. Misturar campos canônicos (`blocos`/`fontes`) com qualquer chave legada é erro de normalização em **ambos** os modos (não é uma regra do `strict_v2`); um `NoteDocument` canônico inválido é recusado pela validação do modelo também em `legacy_compat`, e `notas_adicionais` que não seja objeto nem string (por exemplo um array) é recusado pela normalização em ambos os modos. O `inputSchema` da ferramenta é a união dos dois modos: campos legados e a string de notas continuam declarados e só são recusados em runtime quando `compile_mode=strict_v2`. `strict` é o padrão editorial; `quick` dispensa fontes/obrigações/revisões e por isso não deve ser usado como selo de revisão. Contratos de segurança e persistência estão em [arquitetura](architecture.md); as dez etapas e rubrica de sete dimensões estão no [workflow](generation-workflow.md).

## Gate de qualidade: notas adicionais e evidência por escopo

O check de estilo de notas reserva `NOTE.STYLE_WARN` (Warn, consultivo) a **formatação legada**: bullets em texto puro (`•`, `-`, `*`) e itens de lista **sem lead-in em negrito** fora de `<section>` canônica (`<li><b>Tópico:</b> …</li>` fora de seção nunca é punido). Listas emitidas pelo próprio compilador (`<section class="anki-list">`, `<section class="anki-steps">`, `<section class="anki-flow">`, inclusive dentro de blocos colapsáveis) são isentas de exigência de negrito; um glifo de bullet literal no prefixo do item (`<li>• …</li>`, `<li>- • …</li>`) continua punido porque duplicaria o marcador nativo, exceto quando o item começa com negrito (`<li>• <b>Tópico:</b> …</li>`). Prefixos `-`/`*` digitados pelo autor dentro de `<li>` canônico são saída legítima do compilador e não são punidos; HTML legado preservado (por exemplo um `<ul><li>` sem `<section>`) continua sendo avaliado pela regra antiga. O valor de `preferred_list_format` nos perfis descreve esse formato canônico e mantém a alternativa legada `• <b>Tópico:</b> …`.

Diagnósticos de notas (`NOTE.*`) vivem por cartão em `report.review.cards[].diagnostics` e aparecem em dois canais:

- `report.notes_diagnostics` — agregado consultivo, deduplicado por identidade do cartão (`note_type` + `source_row`) e conteúdo, preservando severidade, `owner_skill` e `suggestion`. O agregado em si não entra no cálculo do `verdict`, e **avisos** de nota continuam consultivos (por exemplo `NOTE.LENGTH_WARN`). **Critical** de nota continua bloqueando como antes: ele é promovido ao canal de topo.
- `report.diagnostics` — um `NOTE.*` Critical por cartão é promovido como `QUALITY.CARD_CRITICAL` com `cause_code` apontando para o código original (por exemplo `NOTE.METADATA`) e a `suggestion` original preservada. O agregado carrega `row`/`field` do cartão, mas não o tipo de nota: um cartão QA e um Cloze de mesma linha geram duas entradas com o mesmo `row`, distinguíveis apenas por `review.cards[]`. Recuperações múltiplas da mesma nota Cloze compartilham uma única entrada.

A evidência de qualidade é conferida por escopo. Todo escopo exige que `quote` seja substring literal de `QualitySource.text`. Evidência com `scope="notes"` exige adicionalmente que o `quote` ocorra no campo `Notas Adicionais` do próprio cartão (HTML bruto com CRLF↔LF normalizado nos dois lados, embora o CSV não carregue quebra crua — **ou** texto visível, para não punir markup legítimo como `<b>`); a ausência gera `QUALITY.NOTES_EVIDENCE_QUOTE_NOT_FOUND` (Critical) no campo em questão. Três propriedades conhecidas desse matching: (i) no ramo de texto visível a comparação é *substring* do texto extraído, não continuidade visual — um `quote` que atravessa dois `<li>` distintos casa quando o separador aparece no próprio `quote` (`"Alfa Beta"` casa, `"AlfaBeta"` não); (ii) não há comprimento mínimo de `quote`; (iii) `mode="quick"` não executa a verificação de evidência (não valida fontes nem obrigações). Quando o campo não é localizável (linha estruturalmente inválida ou binding órfão), o check é omitido e apenas o diagnóstico estrutural próprio é emitido. Escopo `context` ainda não é conferido contra o enunciado/sentença: é follow-up conhecido, não coberto por este código.

PASS técnico não certifica originalidade, fidelidade às fontes, validade clínica ou eficácia pedagógica. Nenhum corpus ou material de estudo privado é distribuído com este documento.
