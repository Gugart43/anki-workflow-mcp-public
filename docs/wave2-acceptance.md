# Aceitação da Integração Wave 2 — Matriz Go/No-Go

Documento de conformidade técnica da integração do **pipeline de criação clínica (Wave 2)** e da **camada visual AnkiMBR Theme v2** no workspace Rust/Python deste repositório.

---

## 1. Escopo e Limitações (não clínico)

1. **Natureza estritamente de software.** Os módulos `qa_clinical.rs`, `hints.rs`, `vocab.rs`, `evidence_matcher.rs`, `interference.rs` e `auto_repair.rs` são validadores determinísticos de forma textual e estrutura HTML/CSV. Não emitem diagnóstico, prescrição nem conduta.
2. **Ausência de certificação clínica.** Nenhum resultado `PASS` deste repositório certifica correção médica, qualidade pedagógica ou aderência a diretrizes. A conferência final é humana e deve consultar as fontes primárias (SBC, SBPT, FEBRASGO, SBP, MS, ADA, OMS).
3. **Ausência de dados de pacientes.** As fixtures (`tests/golden/wave2_clinical_batch.json`, testes sintéticos) contêm enunciados conceituais de literatura pública. Nenhum prontuário, PII ou export pessoal é processado.
4. **Limites de heurística.** As regras de dica Cloze, âncora clínica, par farmacológico e interferência operam por léxico/morfologia PT-BR. Sinonímia sem sobreposição morfológica (ex.: "cefaleia" / "dor de cabeça") permanece `PASS` por desenho.

---

## 2. Gates executados (resultado real desta integração)

Ambiente: Windows 11, Rust stable, Python 3.14.7, Node 22, `uv`, `git-bash`.

| # | Gate | Comando | Resultado |
|---|---|---|---|
| 1 | Formatação Rust | `cargo fmt --all -- --check` | **PASS** |
| 2 | Lint Rust | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | **PASS** (rc=0) |
| 3 | Testes Rust | `cargo test --workspace --all-features` | **PASS** — 221 testes em 11 alvos |
| 4 | Build dos binários | `cargo build --locked -p anki-workflow-mcp -p anki-theme-audit-mcp` | **PASS** — 2 executáveis gerados |
| 5 | Regressão do tema | `python tests/theme_regression_checks.py` | **PASS** — 5 checagens, 62 variáveis em simetria |
| 6 | Suíte Python | `pytest -q --ignore=tests/test_baseline_real.py --ignore=tests/differential` | **PASS** — 633 testes |
| 7 | Paridade Python↔Rust | `pytest tests/differential` | **PASS** — 142 testes |
| 8 | Lint/format Python | `ruff check src tests` + `ruff format --check src tests` | **PASS** — 97 arquivos |
| 9 | MCP de preview (TS) | `npm run build && npm run typecheck && npm test` | **PASS** — 17 testes Vitest |
| 10 | Sonda stdio `anki-workflow-mcp serve` | initialize → tools/list → tools/call → EOF | **PASS** — 3 tools, todas `readOnlyHint: true`, stdout 100% JSON-RPC |
| 11 | Sonda stdio `anki-theme-audit-mcp serve` | initialize → tools/list → tools/call → resource | **PASS** — 2 tools `readOnly`, 1 prompt, 1 resource |
| 12 | Sonda stdio `anki-theme-preview-mcp` | initialize → tools/list → tools/call → resource | **PASS** — 2 tools, contraste WCAG e harness CSP `script-src 'none'` |

`tests/test_baseline_real.py` é excluído do gate padrão por depender de CSVs reais do usuário (`~/Documents/Flashcards`); suas falhas refletem regras retroativas, não a suíte.

---

## 3. Superfície MCP exposta

### 3.1. `anki-workflow-mcp serve` (Rust, `apps/anki-workflow-mcp`)

| Tool | Operação | Read-only |
|---|---|---|
| `review_batch_information` | Revisão por cartão/índice Cloze + telemetria (`batch_size_limit: null`) | sim |
| `cards_compile_drafts` | Compila drafts em memória → CSVs QA/Cloze + `stats` | sim |
| `check_csv` | Estrutura, HTML, entidades, MathJax (CSV externo) | sim |

Limites pós-desserialização: 2 MiB por campo CSV, 200 drafts, 2 MiB de payload de drafts. Parâmetros inválidos retornam `isError: true` sem derrubar a sessão. O transporte ainda desserializa o JSON antes do limite — a proteção não é pré-parser.

### 3.2. `anki-theme-audit-mcp serve` (Rust, `tools/anki-theme-audit-mcp`)

| Tool | Operação | Read-only |
|---|---|---|
| `theme_audit_css` | `prefers-color-scheme` sem opt-in, `color-scheme: only light`, `!important` fora de `@media print`, simetria de tokens, contraste WCAG | sim |
| `theme_audit_card_html` | Marcação incompatível com Light/Dark (`bgcolor=`, `<font>`, `<svg>` solto, `style="color|background"`) | sim |

Prompt `review-theme-contrast` e resource `ankimbr://theme/presentation-v2.css` também expostos.

### 3.3. `anki-theme-preview-mcp` (Node/TypeScript, `tools/anki-theme-preview-mcp`)

| Tool | Operação |
|---|---|
| `validate_contrast_matrix` | Razão WCAG 2.1 para `lightPairs` e `darkPairs` (hex #RGB/#RRGGBB, `minRatio` 1–21) |
| `generate_theme_preview` | Harness HTML autocontido com o mesmo cartão em `data-theme="light"` e `data-theme="dark" class="card nightMode"`, sob CSP `script-src 'none'` |

Resource `ankimbr://preview/harness`.

---

## 4. Divergências conscientes entre especificação e código

| Item | Especificação | Código | Justificativa |
|---|---|---|---|
| `QUALITY.DUPLICATE_RETRIEVAL` | `Error` | `Warn` (advisory) | Duplicata lexical não bloqueia export; o crivo é humano. |
| `QUALITY.GENERIC_IMG_ALT` | `Error` | `Critical` em `validate()`, sem bloquear `render_html()` | Nunca descartar mídia por qualidade de metadado; a falha aparece como diagnóstico e o arquivo continua renderizável/importável. |
| `recommend_bury_siblings` | `true` em pares de interferência | `true` apenas para irmãos Cloze da mesma nota | O Anki só enterra irmãos gerados pela mesma nota; pares QA×QA não têm essa semântica. |
| `export-schemas` | `cargo run --bin export-schemas` | inexistente; usar `scripts/legacy-oracle/export_contracts.py` | Não há binário com esse nome neste workspace; o exportador real é o script Python que grava `contracts/legacy/`. |
| `TAG.FORMAT` / `TAG.DEPTH` | códigos `TAG.INVALID_CHAR_WARN` / `TAG.TOO_DEEP_WARN` como `Warn` | mantidos `TAG.FORMAT` / `TAG.DEPTH` como `Critical` | É o contrato compartilhado com `src/anki_mcp/checks/tags.py` e com o oráculo diferencial; renomear exigiria mudar os dois lados e um código de diagnóstico já consumido. |
| `infer_campo_estudo` | separador canônico ` > ` | devolve `::` | Testado byte-a-byte contra `_infer_campo_estudo` em `tests/differential/test_rust_wave6_parity.py`; a normalização para ` > ` acontece depois, em `normalize_study_field`. |

---

## 5. Limitações conhecidas

1. **Parser HTML por regex.** A sanitização e as checagens usam tokenização própria, não o parser do WebView do Anki. Diante de sintaxe ambígua, a política é neutralizar ou rejeitar; não há garantia de equivalência universal com todos os clientes.
2. **`auto_repair_notes_html` colapsa espaços** (via `split_whitespace`). Isso normaliza HTML de Notas, mas remove espaçamento intencional em texto corrido; não preserve `pre` formatting nesse campo.
3. **Interferência é lexical.** O mascaramento de Cloze remove os spans antes de tokenizar, mas a similaridade continua baseada em conjuntos de tokens (Jaccard), sem embeddings.
4. **Não há certificação clínica** em nenhum diagnóstico, inclusive `QA.CLINICAL_ANCHOR_MISSING_WARN` e `HINT.ROOT_LEAK`, que avaliam forma, não conteúdo.
5. **Contrato de transporte.** Os limites de tamanho valem depois da desserialização do SDK; não substituem um transporte autenticado.
6. **`style=` em HTML externo.** O `check_csv` rejeita `style`/`bgcolor`/`on*` como `FMT.ATTR_FORBIDDEN` crítico e não reescreve a entrada; só a sanitização de drafts neutraliza os atributos.

---

## 6. Rollback

1. Os dois MCPs auxiliares são crates/pacotes aditivos: remover `tools/anki-theme-audit-mcp` da lista `members` de `Cargo.toml` e apagar `tools/` restaura o workspace anterior sem tocar no servidor principal.
2. As mudanças de comportamento dos validadores estão em módulos novos (`auto_repair.rs`, `evidence_matcher.rs`, `html_tag.rs`, `interference.rs`, `qa_clinical.rs`); revertê-las é remover os `pub mod` correspondentes em `crates/anki-validate/src/lib.rs` e as reexportações.
3. `docs/reference-theme/*.css` e `tests/theme_regression_checks.py` são documentação/teste; não afetam o runtime dos templates Anki instalados.
4. Nenhum gate desta lista escreve em coleção, baralho ou `collection.media`.

---

## 7. Decisão

**Go para integração técnica; No-Go para qualquer alegação de certificação clínica.**

Os 12 gates acima passaram com saída real nesta máquina. A integração está pronta para revisão por pares e CI; a validação de conteúdo médico permanece humana e fora do escopo do software.
