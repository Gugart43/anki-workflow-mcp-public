# F3-00: Inventário de Expressões Regulares, Parsing HTML, Normalização Unicode e Egress de Rede

- **Status:** Concluído / Aprovado para Migração Rust v2
- **Data:** 2026-11-14
- **Baseline de Código Python:** `d1d0098b367448c471303f7d0912996d50ac2f67` (`anki-workflow-mcp` v0.3.1)
- **Escopo Primário:** `src/anki_mcp/checks/*.py` e `src/anki_mcp/core/*.py`
- **Escopo Secundário Mapeado:** `src/anki_mcp/tools/*.py` (Unicode e CamelCase regexes)
- **Ressalva de Auditoria Relacionada:** P-07 (`PARECER_TECNICO_PLANO_MESTRE_ANKI_RUST_V2.md`)

---

## 1. Sumário Executivo e Diretrizes para o Engine Rust

A migração do runtime Python para Rust (ADR-001) impõe restrições fundamentais no tratamento de strings, parsing e rede:

1. **Semântica do Engine de Regex:** O crate padrão `regex` de Rust compila autômatos finitos determinísticos e não-determinísticos (DFA/NFA) com garantia de tempo de execução linear $O(mn)$, eliminando vulnerabilidades de ReDoS. Por construção matemática, o crate `regex` **não suporta** look-around (lookahead/lookbehind) nem retrovisores (backreferences).
2. **Estratégia para Look-Arounds e Backreferences:** Em vez de adotar dependências pesadas de backtracking como `fancy-regex` em todo o workspace, a auditoria comprovou que a grande maioria dos look-arounds legados funciona como verificadores de fronteira (delimitadores de pontuação, dígitos, tags HTML ou comentários). A estratégia padrão adotada é a **decomposição procedural** (inspeção de bytes/chars adjacentes ao casamento), já validada com 100% de paridade em `crates/anki-validate/src/csv_structure.rs` e `crates/anki-validate/src/sanitize.rs`.
3. **Parsing de HTML:** O stdlib `html.parser.HTMLParser` do Python é utilizado em 2 módulos com propósitos distintos: extração estrutural de texto visível com contagem de quebras de linha/listas (`checks/information.py`) e validação estrita de máquina de estados para conformidade de tabelas médicas (`checks/tables.py`).
4. **Normalização Unicode:** Mapeamento exato dos 6 locais onde ocorre a decomposição canônica `unicodedata.normalize("NFD", ...)` seguida da eliminação de marcas não-espaçadas (`category == 'Mn'`). Em Rust, a equivalência canônica é provida pelo crate `unicode-normalization` via decomposição NFD iterativa.
5. **Egress de Rede (HTTP):** Identificado exclusivamente em `checks/jev_triage.py`. O módulo conecta-se a backends remotos (KnowLine-31B / TypeSafe SystemOne / OpenRouter) com timeout estrito de 6,0s e cascata determinística de fallback local. Em Rust v2, esse egress deve ser estritamente isolado da camada de validação offline.

---

## 2. Inventário de Expressões Regulares com Look-Around (14 Padrões / 17 Cláusulas)

O inventário do repositório identificou exatamente **14 padrões de regex contendo look-arounds** (12 em `checks`/`core` e 2 em `tools/corpus.py`), totalizando 17 cláusulas de asserção de largura zero.

| ID | Arquivo:Linha | Variável / Contexto | Padrão Regex Legado | Cláusulas de Look-Around | Semântica e Função | Estratégia de Migração para Rust |
|---|---|---|---|---|---|---|
| **LA-01** | `checks/csv_structure.py:42` | `_CLASS_ATTR_RE` | `(?<![-\w])class\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+))` | `(?<![-\w])` (Lookbehind negativo) | Evita casar atributos prefixados como `data-class="..."` ou `myclass="..."`. | **Procedural (implementado):** regex linear `(?i)class\s*=\s*(...)` + verificação se o caractere imediatamente anterior ao casamento não é `_`, `-` ou alfanumérico. |
| **LA-02** | `checks/csv_structure.py:46` | `_DOLLAR_MATH` | `(?<![a-zA-Z])\$(?!\$)` | `(?<![a-zA-Z])` (Lookbehind neg.), `(?!\$)` (Lookahead neg.) | Identifica uso de cifrão simples `$` para MathJax inline (proibido), sem falso-positivo com `$$` ou moedas/variáveis `var$`. | **Varredura Linear de Bytes (implementado):** função `has_dollar_math(&str)` inspeciona bytes adjacentes ao caractere `$`. |
| **LA-03** | `checks/information.py:263` | `_NUMBERED_INLINE_RE` | `(?:^\|\s)(?:\(\d{1,2}\)\|(?<!\d)\d{1,2}\)\|\d{1,2}\.(?=\s+[A-Za-zÀ-ÖØ-öø-ÿ]))\s+` | `(?<!\d)` (Lookbehind neg.), `(?=\s+[...])` (Lookahead pos.) | Detecta enumerações inline `(1)`, `1)` ou `1. Texto` para alertar sobre múltiplos fatos em cartão único. | Regex linear nos tokens numéricos seguido de slice boundary check no prefixo e sufixo (`starts_with`). |
| **LA-04** | `checks/notes.py:30` | `_STYLE_ITEM_RE` | `[•\-\*]\s+(?!<b>)` | `(?!<b>)` (Lookahead neg.) | Exige que listas em notas adicionais sigam o padrão estilístico `• <b>Tópico:</b>`. | **Procedural (implementado em notes.rs):** split de linhas por marcador de bullet e validação direta `!after_ws.starts_with("<b>")`. |
| **LA-05** | `checks/qa.py:23` | `_COMMA_SPLIT_RE` | `,\s*(?:e\s+\|and\s+)?(?!(?:\d\|\s*\d))` | `(?!(?:\d\|\s*\d))` (Lookahead neg.) | Divide itens separados por vírgula na resposta, impedindo quebra de números decimais (ex: `1,5 mg`). | Split top-level com parser procedural que verifica se o caractere pós-vírgula é numérico antes de realizar o particionamento. |
| **LA-06** | `checks/qa.py:58` | `_BARE_NUMBER_RE` | `\b\d+(?:[.,]\d+)?\b(?!\s*(?:mg\|mcg\|g\|kg\|mL\|L\|mmHg\|%\|UI\|U\|°C\|anos\|dias\|horas\|h\b\|m\b))` | `(?!\s*(?:...))` (Lookahead neg.) | Alerta sobre números isolados sem unidade clínica de medida na resposta (`QA.NUMBER_UNIT`). | **Procedural (implementado em qa.rs):** casamento de tokens numéricos com `NUMBER_TOKEN_RE` seguido de validação se o sufixo casa com `ALLOWED_UNIT_PREFIX_RE`. |
| **LA-07** | `checks/triage.py:77` | `_FUSED_ITEM_RE` | `\.\s*-\s\|\s[-\u2013\u2014]\s+(?=\S)\|\s\|\s\|[\u2502\u25bc\u2514\u250c\u2534\u251c\u2524]` | `(?=\S)` (Lookahead pos.) | Detecta conectores que fundem itens de lista ou diagramas em uma única frase. | O lookahead `(?=\S)` simplesmente garante que há caractere não-espaço após o traço; em Rust utiliza-se casamento linear de hífen cercado de espaços com corte não vazio. |
| **LA-08** | `checks/triage.py:169` | `_SENTENCE_SPLIT_RE` | `(?<=\.)\s+\|(?<=;)\s+\|(?<=\?)\s+\|\.\s*-\s+\|\n` | `(?<=\.)`, `(?<=;)`, `(?<=\?)` (3 Lookbehinds pos.) | Divide parágrafos em sentenças clínicas preservando os sinais de pontuação no final da sentença anterior. | Parser de sentenças iterativo (scanner de caracteres) avançando até encontrar `.`, `;` ou `?` seguido de whitespace. |
| **LA-09** | `checks/triage.py:259` | `_YEAR_RE` | `\b(?:19\|20)\d{2}\b(?!\s*(?:mg\|mcg\|g\|kg\|ml\|l\|ui\|kcal\|%\|kg))` | `(?!\s*(?:...))` (Lookahead neg.) | Isola anos cronológicos (1900–2099) diferenciando de valores numéricos com unidades coincidentes. | Casamento de `\b(?:19|20)\d{2}\b` seguido de inspeção da fatia subsequente via `RegexSet` de unidades. |
| **LA-10** | `core/sanitize.py:94` | `_ITALIC_RE` | `(?<!\*)\*([^\*\s][^\*]*?[^\*\s]\|[^\*\s])\*(?!\*)` | `(?<!\*)` (Lookbehind neg.), `(?!\*)` (Lookahead neg.) | Converte sintaxe de itálico Markdown `*texto*` em `<i>texto</i>` sem colidir com negrito `**texto**`. | **Scanner de Bytes Linear (implementado em sanitize.rs):** função `convert_markdown_italic` itera bytes rastreando asteriscos adjacentes. |
| **LA-11** | `core/sanitize.py:172` | Escapamento `&` | `&(?!(#\d+\|#x[0-9a-fA-F]+\|[a-zA-Z]+);)` | `(?!(...);)` (Lookahead neg.) | Escapa ampersand isolado (`&` $\to$ `&amp;`) sem corromper entidades HTML válidas já existentes. | Scanner que localiza `&` e valida se o sufixo imediato corresponde a uma entidade via `ENTITY_RE`. |
| **LA-12** | `core/tables.py:51` | Escapamento `&` em Tbl | `&(?!(?:amp\|lt\|gt\|le\|ge\|#124\|nbsp);)` | `(?!(...);)` (Lookahead neg.) | Escapa ampersands soltos em células de tabelas sem duplicar entidades permitidas em AnkiMBR. | Substituição condicionada via lista estática `[&str]` das 7 entidades canônicas conhecidas. |
| **LA-13** | `tools/corpus.py:31` | `_CAMEL1` | `(?<=[a-z0-9])(?=[A-Z])` | `(?<=[a-z0-9])` (Lookbehind pos.), `(?=[A-Z])` (Lookahead pos.) | Separação de identificadores camelCase para títulos legíveis em mineração de tópicos. | Varredura de caracteres inserindo espaço onde `prev.is_alphanumeric() && next.is_uppercase()`. |
| **LA-14** | `tools/corpus.py:33` | `_CAMEL2` | `(?<=[A-Z])(?=[A-Z][a-z])` | `(?<=[A-Z])` (Lookbehind pos.), `(?=[A-Z][a-z])` (Lookahead pos.) | Separação de sequências de siglas em camelCase (ex: `HTMLParser` $\to$ `HTML Parser`). | Varredura de janelas de 3 caracteres (`prev`, `curr`, `next`) em Rust com slices determinísticos. |

---

## 3. Inventário de Retrovisores / Backreferences (1 Padrão)

A auditoria identificou **exatamente 1 padrão com backreference** em toda a base de código:

- **Local:** `src/anki_mcp/checks/triage.py:315`
- **Variável:** `_TAUTOLOGY_RE`
- **Padrão:**
  ```python
  _TAUTOLOGY_RE = re.compile(
      r"\b(\w{4,})\s+(?:e\s+um\s+tipo\s+de|e\s+uma\s+forma\s+de|e\s+um\s+caso\s+de)\s+\1\b",
      re.IGNORECASE,
  )
  ```
- **Finalidade Clínica:** Detecta pseudocartões tautológicos gerados por extração cega, nos quais a mesma entidade médica de 4 ou mais letras é definida por si mesma (ex: *"Cefaleia é um tipo de cefaleia"* ou *"Epilepsia é uma forma de epilepsia"*). O retrovisor `\1` garante que o primeiro grupo de captura coincida exatamente com o substantivo repetido no predicado.
- **Implementação em Rust (sem necessidade de fancy-regex):**
  Como a gramática é fixa, a detecção procedural em Rust é linear e trivial:
  ```rust
  pub fn is_tautology(text: &str) -> bool {
      static TAUTOLOGY_CONNECTORS: &[&str] = &[
          "e um tipo de", "e uma forma de", "e um caso de",
          "é um tipo de", "é uma forma de", "é um caso de"
      ];
      let lower = text.to_lowercase();
      for connector in TAUTOLOGY_CONNECTORS {
          if let Some(pos) = lower.find(connector) {
              let prefix_word = lower[..pos].split_whitespace().next_back();
              let suffix_word = lower[pos + connector.len()..].split_whitespace().next();
              if let (Some(w1), Some(w2)) = (prefix_word, suffix_word) {
                  let w1_clean = w1.trim_matches(|c: char| !c.is_alphanumeric());
                  let w2_clean = w2.trim_matches(|c: char| !c.is_alphanumeric());
                  if w1_clean.len() >= 4 && w1_clean == w2_clean {
                      return true;
                  }
              }
          }
      }
      false
  }
  ```

---

## 4. Inventário de Expressões Regulares Dinâmicas

Foram mapeados 4 grupos de compilação dinâmica de expressões regulares:

1. **Gatilhos da Taxonomia de Decisões (`checks/decisions.py:117-122`):**
   - **Mecanismo:** A classe `DecisionType` armazena alternâncias de strings brutas (`triggers`) para 12 categorias clínicas (`marco_diagnostico`, `monitorizacao`, `bioetica`, `cenario_especial`, `prognostico`, `diferencial`, `red_flag`, `seguranca_farmacologica`, `tratamento_1a_linha`, `confirmacao`, `mecanismo`, `reconhecimento`).
   - **Compilação:** Função `_compile(pattern: str)` usa um dicionário `_CACHE: dict[str, re.Pattern[str]]` com `re.IGNORECASE`.
   - **Migração Rust:** Em Rust, os 12 tipos são estáticos e devem ser compilados como `std::sync::LazyLock<Regex>` individuais ou indexados em um `regex::RegexSet` para permitir detecção em única passagem sobre o texto.
2. **Cópia Case-Insensitive Dinâmica (`checks/decisions.py:353-356`):**
   - **Mecanismo:** Função `_ci(rx: re.Pattern[str])` recompila padrões legados (`_META_RE`, `_CHATTER_RE`, etc.) aplicando `rx.flags | re.IGNORECASE`.
   - **Migração Rust:** Definir todos os padrões de descarte com flag `(?i)` diretamente na string de compilação estática.
3. **Interpolação de Rótulos de Provas de Residência (`checks/information.py:197-210`):**
   - **Mecanismo:** `_EXAM_BOILERPLATE_RE` monta um f-string injetando `_KNOWN_EXAM_LABELS` (siglas de bancas como ENARE, SURCE, HCFMUSP) e `_EXAM_QUALIFIER`, intercalando desativação de sensibilidade a maiúsculas/minúsculas `(?-i:(?:...))`.
   - **Migração Rust:** O crate `regex` suporta modificadores de flag `(?-i:...)`. O padrão completo pode ser consolidado em uma única constante estática em `LazyLock<Regex>`.
4. **Varredura Heurística em Jev Triage (`checks/jev_triage.py:315, 402, 624, 646, 654`):**
   - **Mecanismo:** Varreduras iterativas de tuplas `(slug, label, pattern)` em `UNIVERSAL_INVARIANTS`, `entity_regexes`, `acute_patterns` e `multisystem_patterns`.
   - **Migração Rust:** Pré-compilar coleções em vetores estáticos de `Regex` emparelhados com enums tipados.

---

## 5. Inventário Completo de Regexes por Módulo (`checks/*.py` e `core/*.py`)

O rastreamento via AST mapeou todas as 164 ocorrências de chamadas `re.*`:

### 5.1. Módulos em `src/anki_mcp/checks/`

- **`checks/batch.py`:**
  - L.67: `re.match(r"(B\d+)", batch_id)` — Validação do prefixo canônico do lote.
- **`checks/cloze.py`:**
  - L.22: `re.compile(r"\{\{c(\d+)::([^}]*?)(?:::([^}]*?))?\}\}")` — Parser canônico de omissões Cloze (índice, alvo, dica opcional).
  - L.25: `re.compile(r"^\s*(de|do|da|dos|das|...)\s+", re.IGNORECASE)` — Detecção de preposição vazada no início do alvo.
  - L.31: `re.compile(r"\s+(mg|mcg|µg|g|kg|mL|L|mmHg|mmol|mEq|UI|U|%|°C)\s*$", re.IGNORECASE)` — Detecção de unidade médica isolada no final do alvo.
  - L.35: `re.compile(r"[,;:\.]\s*$")` — Detecção de pontuação trailing incorreta no alvo.
  - L.103: `re.search(r"\{\{c\d+::[^}]*\{\{c\d+", sentence)` — Deteção de Cloze aninhado proibido.
  - L.114: `re.search(r"^\s*[\(\[]?(?:ANKI-\d+|B\d+-[A-Z]+-\d+)[\)\]]?", sentence)` — Detecção de ID técnico visível no enunciado.
  - L.125: `re.search(r"\{\{c\d+::[^}]+\}\}\s*\((?:ex|exemplo|ex\.|e\.g\.)\s*:[^)]+\)", ...)` — Dica com exemplo clínico externo.
  - L.139: `re.search(r"\{\{c\d+::[^}]*?(?:classe|grupo|categoria)[^}]*?\}\}\s*,\s*(?:como|a exemplo de|tais como)\s+.*?\bou\b", ...)` — Restrição de escopo de classe com conectivo `ou`.
  - L.173, L.193: `re.match(r"^\(([A-Z0-9]{2,8})\)")`, `re.match(r"^[A-Z0-9]{2,8}$")` — Siglas médicas e acrônimos entre parênteses.
  - L.176, L.197: `re.split(r"[\s\-]+", ...)` — Tokenização de termos para verificação de vazamento de raiz.
  - L.268: `re.sub(r"\{\{c\d+::.*?\}\}", "'BLANK'", sentence)` — Mascaramento para validação sintática do texto remanescente.
- **`checks/crossrefs.py`:**
  - L.15: `re.compile(r"\b(B\d{2}-(?:QA|CL)-\d{3}[a-z]?)\b")` — Rastreamento de referências cruzadas no formato AnkiMBR.
- **`checks/csv_structure.py`:**
  - L.36: `re.compile(r"&(#\d+|#x[\da-fA-F]+|[a-zA-Z]+);")` — Entidades HTML suportadas.
  - L.39: `re.compile(r"""<([a-zA-Z/?!](?:"[^"]*"|'[^']*'|[^>"'])*)>""")` — Detecção de tags HTML preservando operadores clínicos `< 135 mEq/L`.
  - L.42: `_CLASS_ATTR_RE` — *(LA-01)*.
  - L.46: `_DOLLAR_MATH` — *(LA-02)*.
  - L.47: `re.compile(r"<!--.*?-->", re.DOTALL)` — Comentários HTML.
  - L.77: `re.match(r"/?([a-zA-Z][a-zA-Z0-9]*)", tag)` — Extração de nome de tag HTML normalizada.
  - L.356, L.357, L.367, L.368: `re.findall(r"\\\(", ...)` e correlatos — Pareamento de delimitadores MathJax `\(` e `\[`.
- **`checks/decisions.py`:**
  - L.332: `_DISCARD_EPIDEMIOLOGY` — Descarte de dados puramente epidemiológicos.
  - L.340: `_DISCARD_HISTORY` — Descarte de história médica e homenagens.
  - L.346: `_DISCARD_BIOCHEM` — Descarte de detalhes bioquímicos e pKa irrelevantes para conduta.
  - L.377: `_CONTRAINDICATION_RE` (`contraindica\w*`) — Preservação de polaridade de contraindicação.
  - L.899: `re.compile(r"^[\s\u2022\u25cf\u25e6\u25aa\u00b7\-\*\u2013\u2014\u25a0\u2b1c]+")` — Stripping de marcadores de lista Unicode.
  - L.900: `re.compile(r"\s+")` — Colapso de espaços em branco.
  - L.912: `re.split(r"\n\s*\n", source_text)` — Particionamento de blocos de parágrafo.
- **`checks/hints.py`:**
  - L.16: `re.compile(r"\{\{c(\d+)::([^:}]+)::([^}]+)\}\}")` — Extração de omissões Cloze contendo dicas explícitas.
  - L.90: `re.compile(r"\b(ou|or|e\/ou|and\/or)\b.{0,20}\b(sim|não|yes|no|...)\b|...", re.IGNORECASE)` — Detecção de dicas que revelam alternativas binárias (`HINT.ELIMINATION`).
  - L.97: `re.compile(r"\b([A-Z]{2,6})\s*/\s*([A-Z]{2,6})\b")` — Detecção de alternativas por barra oblíqua em dicas.
  - L.25: `re.sub(r"[^a-z0-9 ]", " ", text)` — Sanitização alfanumérica pós-NFD.
- **`checks/images.py`:**
  - L.22, L.23: `re.compile(r"<figure[^>]*>(.*?)</figure>", re.DOTALL | re.IGNORECASE)` — Envelopamento de figuras clínicas.
  - L.24, L.25: `re.compile(r"<img\b[^>]*>")`, `re.compile(r"""<img\b[^>]*?\bsrc\s*=\s*["']([^"']+)["']""")` — Tags de imagem e extração do atributo `src`.
  - L.26: `re.compile(r"\balt\s*=", re.IGNORECASE)` — Exigência de acessibilidade (`alt`).
  - L.27: `re.compile(r"<(figure|img|figcaption)\b[^>]*?\bstyle\s*=", re.IGNORECASE)` — Proibição estrita de estilos inline que corrompem o tema escuro.
  - L.28: `re.compile(r"<figcaption[^>]*>(.*?)</figcaption>", re.DOTALL | re.IGNORECASE)` — Legenda clínica obrigatória.
  - L.30: `re.compile(r"^[a-z0-9][a-z0-9]*_[a-z0-9_.]+$")` — Validação do namespace do arquivo de mídia.
  - L.33: `re.compile(r"(?:ISBN|Copyright|©|\bReceived\s*:|ISSN|\bDOI\s*:|\bCatalog\s+No\b|Todos\s+os\s+direitos\s+reservados)", re.IGNORECASE)` — Rejeição de imagens de copyright/editora.
  - L.39: `re.compile(r"^(?:fig(?:ura)?|imagem?|photo|foto|quadro|tabela)\.?\s*...", re.IGNORECASE)` — Rejeição de legendas genéricas sem valor de ancoragem.
  - L.47: `re.compile(r"\.(?:png|jpe?g|gif|webp|svg)$", re.IGNORECASE)` — Extensões válidas.
  - L.229: `re.sub(r"<[^>]+>", "", caption)` — Extração de texto plano de legendas.
- **`checks/information.py`:**
  - L.165: `_RESTATEMENT_PREFIX_RE` (`^(?:a\s+resposta\s+(?:correta\s+)?(?:é|e)|resposta(?:...))\s*:?\s*`) — Detecção de prefixo tautológico em notas.
  - L.197: `_EXAM_BOILERPLATE_RE` — *(Dinâmico / Flag mod)*.
  - L.231: Detecção de orações relativas e explicativas longas.
  - L.247: Detecção de posologias e vias de administração (`VO|IV|IM|SC|EV|SL|...`).
  - L.262: `_COMPARISON_RE` (`\b(?:vs\.?|versus)\b`).
  - L.263: `_NUMBERED_INLINE_RE` — *(LA-03)*.
  - L.266: `_BULLET_INLINE_RE` (`(?:^|\s)[•]\s+`).
  - L.331: `re.match(r"^(.+?)\s+(?:e|and)\s+(.+)$", ...)` — Separação de conjunções copulativas duplas.
- **`checks/jev_triage.py`:**
  - L.225: `re.compile(r"^(#{1,4})\s+(.+)$")` — Parser de cabeçalhos Markdown para quebra em seções.
  - L.291, L.297, L.303, L.323: Heurísticas de detecção de metaconversa de LLM, epidemiologia pura e alvos de dosagem terapêutica.
  - L.493-498: Extração de densidade de listas, tabelas e unidades laboratoriais (`mg/dL`, `mmHg`, `cmH2O`).
  - L.660: Classificação de grandes tópicos clínicos (`neuropatias|cefaleias|glomerulopatias|artrites|...`).
- **`checks/notes.py`:**
  - L.25: `re.compile(r"^\s*(ID\s*:|Âncora\s*:|Anchor\s*:|Cross-ref\s*:|...)", re.IGNORECASE | re.MULTILINE)` — Detecção de metadados técnicos nas notas.
  - L.30: `_STYLE_ITEM_RE` — *(LA-04)*.
  - L.99: `re.sub(r"<table[\s\S]*?</table>", "", notes, flags=re.IGNORECASE)` — Remoção temporária de tabelas para auditoria de texto corrido.
- **`checks/qa.py`:**
  - L.21: `_LIST_SEPARATOR_RE` — Separadores de lista em respostas QA.
  - L.23: `_COMMA_SPLIT_RE` — *(LA-05)*.
  - L.25: `_ENTITY_BOUNDARY_RE` (`&\w+;`) — Proteção de entidades contra partição indevida.
  - L.28: Detecção de perguntas do tipo verdadeiro/falso (`QA.BINARY`).
  - L.37: Respostas binárias isoladas (`sim|não|verdadeiro|falso`).
  - L.43: Detecção de alternativas residuais de múltipla escolha (`(A) ... (B) ...`).
  - L.50: Verificação de contexto clínico obrigatório (`paciente|diretriz|critério|adulto|...`).
  - L.58: `_BARE_NUMBER_RE` — *(LA-06)*.
  - L.61: Operadores direcionais clínicos (`[><≥≤=]|&[gl]t;|&[gl]e;`).
  - L.64: Detecção de explicações prolixas na resposta (`QA.ANSWER_BURDEN`).
  - L.105: Detecção de ID visível no enunciado.
  - L.181, L.184: Remoção de designações de estágio/grau para isolamento de números puros.
  - L.200: Detecção de perguntas duplas no mesmo enunciado (`e/ou qual|quando|onde|como`).
- **`checks/tables.py`:**
  - L.30: `re.compile(r"^:?-+:?$")` — Linhas separadoras de tabela Markdown.
  - L.31: `re.compile(r"(?i)<br\s*/?>\s*\n?|\r?\n")` — Normalização de quebras de linha em células.
- **`checks/tags.py`:**
  - L.15: `re.compile(r"^[A-Za-zÀ-ÿ0-9_:]+$")` — Caracteres válidos para hierarquia de tags do Anki.
  - L.16: `re.compile(r"\s")` — Rejeição de espaços em branco em tags.
  - L.61: `re.match(r"^[A-Za-zÀ-ÿ0-9_]+$", tag)` — Segmento individual de tag sem dois-pontos.
- **`checks/triage.py`:**
  - L.77: `_FUSED_ITEM_RE` — *(LA-07)*.
  - L.94: Detecção de casos clínicos fictícios do estilo prompt (`Paciente A`, `este primeiro`).
  - L.110: Detecção de pontuação incompleta ou preposições pendentes no final da frase.
  - L.161, L.162, L.165, L.166: Parsers de linhas estruturadas Markdown (títulos `#`, números `1.`, listas `*`).
  - L.169: `_SENTENCE_SPLIT_RE` — *(LA-08)*.
  - L.170, L.171, L.172, L.173: Remoção de sintaxe de links, negritos, citações de fonte (`Source:`) e caracteres de escape.
  - L.176, L.184, L.189: Identificação de dosagens, faixas terapêuticas (`10-20 mg/kg`) e porcentagens.
  - L.194: Enumerações por extenso (`duas grandes causas`, `três critérios`).
  - L.201: Dicionário extenso de fármacos clínicos (antiepilépticos, cardiovasculares, biológicos, corticoides, etc.).
  - L.225: Dicionário de escores e critérios validados (Glasgow, Wells, SOFA, NIHSS, etc.).
  - L.233: Vocabulário neuroanatômico e fisiopatológico.
  - L.252: Termos de volatilidade temporal e epidemiologia transitória.
  - L.259: `_YEAR_RE` — *(LA-09)*.
  - L.260, L.268: Marcadores de estabilidade do conhecimento e núcleos de decisão.
  - L.278: Ruído editorial (histórico, sumário, agradecimentos).
  - L.286: Recheio retórico e clichês de linguagem médica.
  - L.293: Metalinguagem de material didático (`neste capítulo`, `a figura mostra`, `resumindo`).
  - L.303: Alucinações de assistente conversacional (`posso formular um caso`, `espero ter ajudado`).
  - L.314: Definições excessivamente curtas.
  - L.315: `_TAUTOLOGY_RE` — *(Backreference Único)*.
  - L.609, L.618: Expressões de negação clínica estrita vs marcadores concessivos (`não só`).

### 5.2. Módulos em `src/anki_mcp/core/`

- **`core/compiler.py`:**
  - L.32: `re.compile(r"\b(?:[A-Z]{2,4}-)?B\d{2}-(?:QA|CL|CLOZE)-\d{2,3}\b|\b[A-Z]{2,4}-B\d{2}-\d{3}\b", re.IGNORECASE)` — Normalização e validação de identificadores de cartões compilados.
  - L.43: `re.sub(r" {2,}", " ", cleaned)` — Colapso de múltiplos espaços.
- **`core/receipt.py`:**
  - L.14: `re.compile(r"^sha256:[0-9a-f]{64}$")` — Validação estrita do token de recibo canônico v1.
- **`core/safe.py`:**
  - L.17: `re.compile(r"^([A-Z][A-Z_]*(?:\.[A-Z_]+)+):")` — Parser de prefixos de código de diagnóstico (`DOM.NOME:`).
- **`core/sanitize.py`:**
  - L.26: `re.compile(r"<(/?)([a-zA-Z][a-zA-Z0-9]*)((\s+[^>]*?)?)(/?)>", re.DOTALL)` — Tokenizador de tags HTML permitidas vs proibidas.
  - L.32: `re.compile(r"<!--.*?-->", re.DOTALL)` — Isolamento de comentários HTML.
  - L.35: `re.compile(r"&(#\d+|#x[0-9a-fA-F]+|[a-zA-Z]+);")` — Validador de entidades numéricas e nomeadas.
  - L.86, L.87: `re.sub(r"\*\*(.+?)\*\*", ...)`, `re.sub(r"__(.+?)__", ...)` — Conversão de negrito Markdown para `<b>...</b>`.
  - L.94: `_ITALIC_RE` — *(LA-10)*.
  - L.172: Escapamento de `&` solto — *(LA-11)*.
- **`core/tables.py`:**
  - L.34: `re.compile(r"&lt;(/?[a-zA-Z]+(?:\s*/?)?)&gt;")` — Restauração de tags seguras permitidas em tabelas.
  - L.35: `re.compile(r"^:?-+:?$")` — Detecção de linhas de alinhamento em Markdown.
  - L.36: `re.compile(r"(?i)<br\s*/?>\s*\n?|\r?\n")` — Normalização de quebras de linha em células.
  - L.51: Escapamento de `&` em tabelas — *(LA-12)*.
- **`core/vocab.py`:**
  - L.192: `re.compile(r"<[^>]+>")` — Remoção de tags HTML para cálculo de texto visível.
  - L.240, L.243: Remoção de `<br>` e colapso de espaços em normalização de vocabulário.

---

## 6. Inventário de `HTMLParser` (2 Módulos)

O parser nativo de HTML do Python (`html.parser.HTMLParser`) é empregado em exatamente dois módulos em `checks/`:

### 6.1. `checks/information.py`: `_BlockTextParser` / `parse_html_text`

- **Objetivo:** Extração determinística do conteúdo textual visível de notas e campos HTML, decodificando entidades e preservando quebras de linha estruturais para contagem de carga cognitiva.
- **Comportamento Mapeado:**
  - Inicialização com `convert_charrefs=True`.
  - Tags de quebra de bloco (`_BLOCK_BREAK_TAGS`): `br`, `hr`, `p`, `div`, `li`, `tr`, `blockquote`, `h1` até `h6`, `ul`, `ol`. Ao encontrar essas tags, injeta `\n` no fluxo.
  - Tags de tabela (`td`, `th`): injetam um espaço em branco `' '`.
  - Tags ignoradas (`_SKIP_CONTENT_TAGS`): `script` e `style`. Mantém um contador `_skip_depth` para suprimir integralmente o conteúdo interno dessas tags.
  - Contadores: incrementa `li_count` ao processar `<li>` e `br_count` ao processar `<br>`.
  - Dados de texto (`handle_data`): substitui espaços não-quebráveis `\xa0` por espaços regulares e acumula os blocos em memória.
- **Funções Consumidoras:**
  - `parse_html_text(raw: str) -> tuple[str, int, int]`: retorna o texto limpo, `li_count` e `br_count`.
  - `html_to_visible_text(raw: str) -> str`: colapsa o texto visível em uma única linha.
  - `visible_char_count(raw: str) -> int`: contagem real de caracteres visíveis para controle de sobrecarga cognitiva.
  - `normalize_for_exact_match(raw: str) -> str`: normalização com `casefold()` e remoção de pontuação de extremidade para detectar repetições cegas da resposta dentro das notas.
- **Estratégia de Implementação em Rust:**
  - Implementar uma máquina de estados linear ou utilizar o crate de streaming `lol-html` / parser leve de tokens HTML (`tl` ou `html5ever`). Para o propósito de extração de texto e contagem de tags, um tokenizador streaming baseado em fatias de bytes é suficiente, dispensando alocação de árvore DOM e atingindo ordens de magnitude maior throughput.

### 6.2. `checks/tables.py`: `_TableHTMLParser`

- **Objetivo:** Validador estrutural e temático de tabelas HTML inseridas nos campos dos cartões (AnkiMBR).
- **Máquina de Estados e Regras Auditadas:**
  1. `TBL.INLINE_STYLE` (CRITICAL): Disparado se tags de tabela possuírem atributos proibidos (`style`, `width`, `height`, `bgcolor`, `align`, `valign`, `border`, `cellspacing`, `cellpadding`) que quebrem a responsividade e o tema escuro.
  2. `TBL.IN_CARD_FACE` (CRITICAL): Disparado se `<table>` for detectada na face do cartão (`Enunciado`, `Resposta` ou `Sentença`). Tabelas são permitidas exclusivamente em `Notas Adicionais`.
  3. `TBL.NESTED` (CRITICAL): Proíbe expressamente tabelas aninhadas (`table_depth > 1`).
  4. `TBL.MALFORMED` (CRITICAL): Valida a hierarquia estrita:
     - `thead`, `tbody`, `tfoot`, `caption` devem residir diretamente em `table`.
     - `tr` deve residir em `table`, `thead`, `tbody` ou `tfoot`.
     - `th` e `td` devem residir diretamente dentro de `tr`.
     - Validação de fechamento pareado de tags com pilha `tag_stack`.
  5. `TBL.CELL_TOO_LONG` (WARN): Verifica se o texto de qualquer célula excede `MAX_CELL_CHARS = 120` caracteres visíveis.
  6. `TBL.RAGGED` (WARN): Rastreia `row_cols` (respeitando atributos `colspan`) e sinaliza disparidade na contagem de colunas entre diferentes linhas da mesma tabela.
  7. `TBL.NO_HEADER` (WARN): Exige presença de cabeçalho (`thead` ou primeira linha composta por `th`).
  8. `TBL.EMPTY` (CRITICAL): Detecta tabelas vazias sem linhas ou sem células de dados.
  9. `TBL.TOO_WIDE` e `TBL.TOO_LONG` (WARN): Limita número de colunas ($\le 5$) e número de linhas ($\le 8$).
- **Estratégia de Implementação em Rust:**
  - Implementar o struct `TableValidator` com máquina de estados finita e pilha de tags (`Vec<TableTag>`), processando tokens emitidos por um scanner léxico HTML seguro.

---

## 7. Inventário de `unicodedata.normalize("NFD", ...)` (6 Locais)

O mapeamento da base de código confirmou exatamente os **6 locais** onde a decomposição NFD é executada para normalização fonética e remoção de acentos:

| ID | Arquivo:Linha | Função | Trecho de Código Python Original | Finalidade e Comportamento |
|---|---|---|---|---|
| **NFD-01** | `checks/hints.py:23` | `_normalize(text: str)` | `text = unicodedata.normalize("NFD", text.lower())`<br>`"".join(c for c in text if unicodedata.category(c) != "Mn")` | Limpeza de acentos na dica e no alvo Cloze para detectar vazamento de raiz médica (`HINT.ROOT_LEAK`). |
| **NFD-02** | `checks/triage.py:144` | `_fold_ascii(text: str)` | `decomposed = unicodedata.normalize("NFD", text.lower())`<br>`"".join(c for c in decomposed if unicodedata.category(c) != "Mn")` | Redução a ASCII do texto da fonte bibliográfica para casamento robusto contra termos clínicos invariantes. |
| **NFD-03** | `tools/corpus.py:40` | `_slugify(label: str)` | `normalized = unicodedata.normalize("NFD", label.lower())`<br>`"".join(c for c in normalized if unicodedata.category(c) != "Mn")` | Geração de identificadores alfanuméricos estáveis (`slugs`) a partir dos títulos das seções. |
| **NFD-04** | `tools/corpus.py:79` | `_normalize_header(s: str)` | `decomposed = unicodedata.normalize("NFD", spaced.lower())`<br>`"".join(c for c in decomposed if unicodedata.category(c) != "Mn")` | Normalização de títulos de cabeçalhos médicos para agrupamento semântico no corpus. |
| **NFD-05** | `tools/hierarchy.py:79` | `_normalize_name(s: str)` | `"".join(c for c in unicodedata.normalize("NFD", s.lower()) if unicodedata.category(c) != "Mn")` | Comparação e busca insensível a acentuação na hierarquia taxonômica de decks do Anki. |
| **NFD-06** | `tools/media.py:136` | `_normalize_stem(s: str)` | `"".join(c for c in unicodedata.normalize("NFD", s.lower()) if unicodedata.category(c) != "Mn")` | Normalização do nome base de figuras e diagramas para reconciliação com o catálogo universal de mídia. |

### Padronização da Implementação Canônica em Rust:

Em Rust, o algoritmo equivalente exato deve ser encapsulado no crate `anki-parser` ou `anki-domain`:

```rust
use unicode_normalization::UnicodeNormalization;

/// Decompõe a string em NFD e remove diacríticos (categoria Unicode `Mn` - Nonspacing Mark).
/// Reproduz byte a byte a semântica de Python:
/// unicodedata.normalize("NFD", s.lower()) filtrando unicodedata.category(c) != "Mn"
pub fn fold_to_ascii_nfd(input: &str) -> String {
    input
        .to_lowercase()
        .nfd()
        .filter(|&c| !unicode_categories::UnicodeCategories::is_mark_nonspacing(c))
        .collect()
}
```

*Nota:* Atualmente, `crates/anki-validate/src/hints.rs` possui a função `fold_latin_char`, que cobre o alfabeto latino básico. Para assegurar paridade absoluta nos 6 pontos (especialmente ao lidar com símbolos farmacêuticos ou termos médicos importados), a adição de `unicode-normalization = "0.1"` ao workspace garantirá conformidade universal de acordo com os testes diferenciais.

---

## 8. Inventário de Egress HTTP (`checks/jev_triage.py`)

A auditoria confirmou que o subsistema de rede ativo reside exclusivamente em `src/anki_mcp/checks/jev_triage.py`.

### 8.1. Detalhes de Conectividade e Endpoints

O método `_post_systemone_with_fallback(state: str, questions: dict[str, Any], ...)` gerencia o envio de requisições JSON-RPC via `urllib.request`:

1. **Endpoint Primário (KnowLine):**
   - URL: `KNOWLINE_SYSTEMONE_URL` (variável de ambiente, padrão vazio).
   - Modelo: `DEFAULT_KNOWLINE_MODEL` (`"knowline/knowline-31b-gen1"`).
   - Cabeçalhos: `Authorization: Bearer <KNOWLINE_API_KEY>`, `ngrok-skip-browser-warning: true`.
2. **Endpoint Secundário / Fallback 1 (TypeSafe SystemOne):**
   - URL: `TYPESAFE_SYSTEMONE_URL` (padrão: `https://api.typesafe.ai/v1/systemone`).
   - Modelo: `DEFAULT_JEV_MODEL` (`"typesafe/systemone-v1"`).
   - Cabeçalhos: `Authorization: Bearer <TYPESAFE_API_KEY>`.
3. **Endpoint Terciário / Fallback 2 (OpenRouter Decisions):**
   - URL: `OPENROUTER_DECISIONS_URL` (padrão: `https://openrouter.ai/api/alpha/decisions`).
   - Modelo: `DEFAULT_OPENROUTER_JEV_MODEL` (`"anthropic/claude-3.5-haiku"`).
   - Cabeçalhos: `Authorization: Bearer <OPENROUTER_API_KEY>`.

### 8.2. Protocolo, Timeouts e Falhas

- **Payload JSON:**
  ```json
  {
    "model": "<model_name>",
    "state": "<contexto_clinico>",
    "questions": { ... }
  }
  ```
- **Timeout Estrito:** `timeout_s = 6.0` segundos por chamada. Se a resposta exceder esse limite ou retornar HTTP $\ge 400$, o sistema tenta o próximo endpoint da lista e, em última instância, realiza fallback gracioso.
- **Gestão de Segredos:** Segredos são lidos exclusivamente de variáveis de ambiente do processo (`_discover_env_secret`), rejeitando leituras de arquivos soltos.
- **Fallback Determinístico Local:** Se todos os backends remotos falharem ou se nenhuma chave for configurada, o sistema ativa imediatamente os classificadores heurísticos determinísticos (`classify_zone_heuristic`, `triage_topics_heuristic` e `rank_sections_heuristic`), garantindo que nenhuma falha de conectividade interrompa o fluxo de triagem.

### 8.3. Diretrizes para Rust v2

1. **Isolamento de Egress:** O pipeline local de validação e compilação de cartões (`validate_batch`, `pipeline_gate_check`, `export_final`) **jamais deve realizar chamadas de rede**. A execução é 100% determinística, local e offline.
2. **Cliente Opcional em Rust:** O cliente para KnowLine/TypeSafe deve residir em um crate de integração (`anki-remote` ou módulo de feature em `apps/anki-workflow-mcp`), utilizando `reqwest` com TLS rustls e timeout configurado em 6 segundos.
3. **Receipt e Attestation:** Nenhum receipt v1 (`compute_receipt_v1`) ou `ValidationAttestation` v2 depende de resposta remota ou conectividade externa, preservando a imutabilidade do workflow.
