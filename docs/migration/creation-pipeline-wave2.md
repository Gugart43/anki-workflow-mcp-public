# Pipeline de Criação Clínica de Flashcards — Wave 2

> **Documento histórico.** Este arquivo descreve o estado da integração Wave 2 no momento em que foi
> escrito e menciona ferramentas, subcomandos e códigos de diagnóstico que não existem mais na
> superfície atual. Ele é mantido apenas como registro de migração e **não deve ser usado como
> referência de comportamento**. Superfície atual: [docs/architecture.md](../architecture.md).

Este documento estabelece as especificações arquiteturais, APIs determinísticas, catálogo de diagnósticos e diretrizes operacionais do pipeline de criação clínica de flashcards (**Wave 2**) no projeto `anki-workflow-mcp`.

---

## 1. Escopo e Limitações Não Clínicas

> ### AVISO LEGAL E DIRETRIZ DE ESCOPO (NÃO CLÍNICO)
> 1. **Natureza Estritamente de Software:** Os módulos, validadores, heurísticas de vocabulário, pares de interferência e reparos automáticos descritos neste documento constituem algoritmos determinísticos de garantia de qualidade estrutural e pedagógica de texto para flashcards (Anki).
> 2. **Ausência de Certificação Médica:** Nenhuma funcionalidade deste sistema fornece diagnóstico, prescrição, aconselhamento terapêutico ou certificação clínica. O software **não substitui** o raciocínio clínico de profissionais de saúde, nem dispensa a consulta direta às diretrizes médicas oficiais atualizadas (SBC, SBPT, FEBRASGO, SBP, CFM, ADA, OMS, entre outras).
> 3. **Privacidade e Proteção de Dados:** Nenhuma informação pessoal identificável (PII), dado sensível de pacientes ou prontuário real é processado ou distribuído no repositório. Todos os dados de teste (`tests/golden/wave2_clinical_batch.json`) consistem em enunciados conceituais baseados em literatura pública e diretrizes de domínio público.

---

## 2. Visão Geral da Arquitetura Wave 2

A Wave 2 estende o motor Rust `anki-validate` e as ferramentas MCP expostas em `apps/anki-workflow-mcp`, introduzindo consistência taxonômica, segurança de vocabulário, análise de interferência em algoritmos de repetição espaçada (FSRS), validação de figuras tema-seguras e auto-reparo em memória:

```
[Entrada de Rascunhos / CSV]
             │
             ├──► tags.rs (Normalização PT-BR e consistência de ramos no lote)
             ├──► study_field.rs (Formatação canônica 'Especialidade > Subárea')
             ├──► vocab.rs (Normalização de unidades e barreiras ISMP/OMS)
             ├──► hints.rs (Anti-vazamento de radical e domínios canônicos Cloze)
             ├──► qa_clinical.rs (Âncoras contextuais e par farmacológico completo)
             ├──► media_figure.rs (Comportamento tema-seguro: Preserve/FrameLight/InvertDark)
             ├──► evidence_matcher.rs (Verificação de citações tolerante a quebras de PDF)
             ├──► interference.rs (Duplicatas semânticas cross-type e pares FSRS)
             └──► auto_repair.rs (Correções seguras em memória para batch_patch_draft)
             │
             ▼
[review_batch_information / batch_information.rs]
(Telemetria cognitiva expandida, perfil FSRS e advisories sem teto arbitrário)
```

---

## 3. Catálogo de APIs e Módulos Rust (`crates/anki-validate/src/`)

### 3.1. Precisão Clínica em QA (`qa_clinical.rs`)
Valida a solidez formal de cartões de Pergunta e Resposta voltados ao ensino médico, garantindo que condutas e posologias estejam devidamente contextualizadas.

* **Assinatura:**
  ```rust
  pub fn check_qa_clinical(enunciado: &str, resposta: &str, row: Option<usize>) -> Vec<Diagnostic>
  ```
* **Diagnósticos Emitidos:**
  * `QA.CLINICAL_ANCHOR_MISSING_WARN`: Disparado quando perguntas sobre doses, fármacos de escolha ou condutas imediatas omitem a população (ex.: *adulto, gestante, pediatria, idoso*), gravidade (*grave, estável, UTI*) ou diretriz de referência (*SBC, ADA, MS*).
  * `QA.DRUG_PAIR_INCOMPLETE_WARN`: Disparado quando enunciados posológicos recebem respostas sem a quádrupla essencial: nome do fármaco, dose numérica, unidade clínica e via/frequência (ex.: *VO, EV, 12/12h*).
  * `QA.REVERSE_DIRECTION_WARN`: Alerta contra a formulação invertida "fármaco $\to$ indicação" (*qual doença é tratada por X?*), recomendando o raciocínio realista "quadro/síndrome $\to$ conduta".
  * `QA.BARE_CLINICAL_NUMBER_WARN`: Identifica números isolados sem unidade na resposta, poupando anos cronológicos e escores padronizados (TIMI, CURB-65).

### 3.2. Salvaguardas Anti-Vazamento e Domínios de Dica (`hints.rs`)
Governa a elaboração de dicas em omissões de palavras (`{{cN::alvo::dica}}`), impedindo que a dica revele a morfologia da palavra omitida.

* **Assinaturas:**
  ```rust
  pub fn check_cloze_hints(sentence: &str, row: Option<usize>) -> Vec<Diagnostic>;
  pub fn suggest_hint_domain(cloze_answer: &str, sentence_context: &str) -> Option<&'static str>;
  pub fn has_root_or_affix_leak(answer: &str, hint: &str) -> bool;
  pub fn normalize_ptbr(s: &str) -> String;
  ```
* **Diagnósticos Emitidos:**
  * `HINT.ROOT_LEAK` (Severidade `Error`): Bloqueante. Detecta quando a dica compartilha radicais ou afixos morfológicos com a resposta omitida (ex.: resposta *hipercalemia* e dica *distúrbio de potássio com hiper-*).
  * `HINT.GENERIC_WARN`: Detecta termos vazios como *"palavra"*, *"conceito"*, *"resposta"*.
  * `HINT.TOO_LONG_WARN`: Alerta quando a dica ultrapassa 48 caracteres (`MAX_HINT_CHARS`).
  * `HINT.LEAK_COUNT_WARN`: Alerta quando a dica expõe quantidade de letras ou palavras (ex.: *duas palavras*, *6 letras*).
* **Domínios Clínicos Canônicos (`CANONICAL_CLINICAL_HINT_DOMAINS`):**
  * *fármaco*, *classe farmacológica*, *dose e via*, *alvo terapêutico*, *achado semiológico*, *sinal ou sintoma*, *exame diagnóstico*, *exame padrão-ouro*, *agente etiológico*, *distúrbio eletrolítico*, *distúrbio ácido-base*, *mecanismo fisiopatológico*, *complicação*, *contraindicação*, *conduta imediata*, *estrutura anatômica*, *critério diagnóstico*.

### 3.3. Vocabulário Médico e Segurança de Prescrição (`vocab.rs`)
Implementa barreiras de segurança farmacológica com base nas convenções internacionais do ISMP (*Institute for Safe Medication Practices*) e normalização de unidades em português.

* **Assinaturas:**
  ```rust
  pub fn normalize_clinical_units(input: &str) -> String;
  pub fn check_clinical_vocab(text: &str, notes_context: &str, row: Option<usize>, field: &str) -> Vec<Diagnostic>;
  ```
* **Regras Determinísticas:**
  * Padronização de unidades: `mg/dl` $\to$ `mg/dL`, `meq/l` $\to$ `mEq/L`, `mmol/l` $\to$ `mmol/L`, `mmhg` $\to$ `mmHg`, `ug/` $\to$ `µg/`.
  * `VOCAB.UNSAFE_ABBREV_WARN`:
    - Abreviação `"U"` após dígito: exige conversão para `"UI"` para evitar que seja confundida com zero.
    - Abreviações latinas de frequência: proíbe `"QD"`, `"QOD"`, `"q.d."`, exigindo *"1x/dia"* ou *"em dias alternados"*.
    - Zeros decimais perigosos: condena zero terminal desnecessário (*"5,0 mg"* $\to$ risco de leitura como 50 mg) e ausência de zero à esquerda (*",5 mg"* $\to$ risco de leitura como 5 mg).
  * `VOCAB.UNEXPANDED_ACRONYM_WARN`: Exige que siglas técnicas (3 a 6 letras) não contidas na lista de siglas universais (*ECG, UTI, PA, IAM, AVC, etc.*) possuam expansão explícita entre parênteses no texto ou nas Notas Adicionais.

### 3.4. Padronização do Campo de Estudo (`study_field.rs`)
Garante a consistência na classificação acadêmica exibida no cabeçalho dos cartões.

* **Assinaturas:**
  ```rust
  pub fn normalize_study_field(raw: &str) -> String;
  pub fn check_study_field(raw: &str, row: Option<usize>) -> Vec<Diagnostic>;
  ```
* **Diagnósticos Emitidos:**
  * `STUDY.EMPTY` (`Error`): Campo não preenchido.
  * `STUDY.TOO_GENERIC_WARN`: Rejeita termos sem granularidade como *"medicina"*, *"geral"*, *"estudo"*.
  * `STUDY.INCONSISTENT_SEP_WARN`: Converte separadores informais (`::`, `->`, `/`, `-`) para o formato canônico ` > `.
  * `STUDY.TOO_LONG_WARN`: Alerta para caminhos taxonômicos que excedam 80 caracteres.
  * Preservação de Siglas: Mantém acrônimos médicos (*UTI, ECG, SCA, TEP, AVC, CAD, IAM*) integralmente em maiúsculas durante a capitalização dos segmentos.

### 3.5. Taxonomia e Consistência de Lote (`tags.rs`)
Padroniza tags hierárquicas e avalia conflitos de taxonomia dentro do mesmo lote.

* **Assinaturas:**
  ```rust
  pub fn normalize_tag(raw: &str) -> String;
  pub fn check_tags(tags_field: &str, row: Option<usize>) -> Vec<Diagnostic>;
  pub fn check_batch_tag_consistency(all_tags: &[&str]) -> Vec<Diagnostic>;
  ```
* **Diagnósticos Emitidos:**
  * `TAG.EMPTY` (`Error`): Cartão sem etiquetas.
  * `TAG.INVALID_CHAR_WARN`: Converte caracteres acentuados e espaços em minúsculas com separadores `_` e hierarquia `::`. Preserva o prefixo `id::<uuid>`.
  * `TAG.DUPLICATE_WARN`: Alerta tags repetidas no mesmo cartão.
  * `TAG.TOO_DEEP_WARN`: Limita a profundidade a no máximo 4 níveis (`especialidade::area::subarea::tema`).
  * `TAG.MISSING_HIERARCHY_WARN`: Desaconselha tags de nível único sem hierarquia.
  * `TAG.BATCH_INCONSISTENT_WARN`: Disparado quando a mesma tag folha é categorizada em ramos taxonômicos divergentes dentro do mesmo lote (ex.: `pediatria::emergencias::cetoacidose` e `endocrinologia::diabetes::cetoacidose`).

### 3.6. Figuras Clínicas Tema-Seguras (`media_figure.rs`)
Gerencia a renderização semântica de mídia no padrão HTML do AnkiMBR com suporte ativo a Modo Escuro.

* **Modos de Apresentação (`MediaThemeBehavior`):**
  * `Preserve` (Padrão): Não inverte nem altera as cores da imagem no modo escuro. Obrigatório para fotografias dermatológicas, radiografias, tomografias, lâminas histológicas e traçados eletrocardiográficos com marcações coloridas. Gera a classe `.anki-figure--preserve`.
  * `FrameLight`: Adiciona uma moldura clara sutil ao redor da imagem em modo escuro, evitando o contraste excessivo de figuras que possuem fundo branco natural. Gera a classe `.anki-figure--frame-light`.
  * `InvertDark`: Aplica filtro de inversão cromática suave exclusivamente para diagramas e ilustrações esquemáticas em preto e branco. Gera a classe `.anki-figure--invert-dark`.
* **Diagnósticos Emitidos:**
  * `IMG.INVALID_FILENAME` (`Error`): Extensão não autorizada ou caracteres inseguros (`..`, caminhos absolutos). Extensões válidas: `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.avif`.
  * `IMG.NAMESPACE_WARN`: Exige nomenclatura estruturada `<slug>_<nome>.<ext>` em minúsculas.
  * `QUALITY.GENERIC_IMG_ALT` (`Error`): Exige texto alternativo acessível e descritivo com no mínimo 12 caracteres, bloqueando palavras genéricas (*"imagem"*, *"foto"*, *"ecg"*).
  * `IMG.MISSING_CAPTION_WARN`: Recomenda legenda clínica associada (`<figcaption>`).

### 3.7. Verificação de Evidências e Obrigações (`evidence_matcher.rs`)
Verifica a autenticidade das fontes e a rastreabilidade das afirmações textuais frente ao material de origem.

* **Assinaturas:**
  ```rust
  pub fn normalize_pdf_text(input: &str) -> String;
  pub fn verify_quote_in_source(quote: &str, source_text: &str) -> QuoteMatchResult;
  pub fn check_reference_specificity(reference: &str, row: Option<usize>) -> Option<Diagnostic>;
  pub fn check_obligation_coverage(obligations: &[ObligationSpec], bindings: &[BindingSpec]) -> ObligationCoverageReport;
  ```
* **Mecanismos:**
  * Normalização Tolerante a PDF: Reconstrói palavras quebradas por hifens de fim de linha em diagramação de PDFs (`hiper-\ntensão` $\to$ `hipertensão`), preservando hifens conceituais (`beta-bloqueador`).
  * `QuoteMatchResult`: Retorna `ExactMatch`, `NormalizedPdfMatch`, `TooShort` (< 15 caracteres visíveis) ou `NotFound`.
  * `QUALITY.GENERIC_REFERENCE`: Alerta contra citações vagas (ex.: apenas *"UpToDate"*, *"Harrison"* ou *"apostila"* sem indicação de página, capítulo ou ano).

### 3.8. Análise de Interferência e Algoritmos de Repetição (`interference.rs`)
Evita a confusão de memória durante as revisões no Anki (especialmente no algoritmo FSRS), detectando duplicatas semânticas entre perguntas e respostas, inclusive cruzando cartões QA e Cloze.

* **Assinatura:**
  ```rust
  pub fn analyze_batch_interference(units: &[RetrievalUnit]) -> InterferenceReport
  ```
* **Diagnósticos Emitidos:**
  * `QUALITY.DUPLICATE_RETRIEVAL` (`Error`): Similaridade combinada de enunciado e resposta $\ge 85\%$ ou alvos $\ge 90\%$ e prompts $\ge 70\%$.
  * `QUALITY.NEAR_DUPLICATE_WARN`: Similaridade entre 60% e 84%, recomendando unificação editorial.
  * `CARD.INTERFERENCE_PAIR_WARN`: Detecta prompts muito semelhantes ($\ge 65\%$) que cobram alvos clínicos distintos ($< 25\%$). Nesses casos, o sistema recomenda enterramento mútuo (`recommend_bury_siblings: true`) para que os cartões não apareçam na mesma sessão de estudos, além de exigir notas comparativas contrastivas.
* **Salvaguarda de Polaridade Clínica:** Stopwords como *"não"* e *"sem"* são intencionalmente preservadas no cálculo vetorial para não equalizar condutas indicadas e contraindicadas.

### 3.9. Motor de Auto-Reparo em Memória (`auto_repair.rs`)
Fornece transformações cirúrgicas idempotentes e auditáveis para rascunhos que falham em regras triviais de formatação.

* **Assinaturas:**
  ```rust
  pub fn auto_repair_retrieval_field(input: &str, is_cloze: bool) -> (String, Vec<RepairAction>);
  pub fn auto_repair_notes_html(input: &str) -> (String, Vec<RepairAction>);
  pub fn repair_mathjax_dollars(input: &str) -> String;
  ```
* **Ações Executadas:**
  * Substituição de quebras físicas cruas por `<br>` e colapso de espaços repetidos.
  * Escape automático de barras verticais cruas (`|` $\to$ `&#124;`) para prevenir corrupção estrutural de tabelas e separadores de campo.
  * Conversão de notação matemática de cifrões `$..$` em `\(..\)` e `$$..$$` em `\[..\]`, com salvaguarda estrita para expressões monetárias (`R$`, `US$`).
  * Renumeração sequencial de índices Cloze sem lacunas (`{{c1::..}}`, `{{c2::..}}`).

---

## 4. Telemetria Cognitiva e Lote (`batch_information.rs`)

O subcomando e ferramenta MCP `review_batch_information` analisa o lote como um todo sem impor restrições arbitrárias de tamanho máximo (princípio `no_count_only_limit: true` do perfil `strict_v2`):

* **Métricas Apresentadas:**
  * Proporção de cartões QA vs Cloze e total de cartões efetivos no Anki (contando cada índice Cloze individualmente).
  * Distribuição percentual de nós taxonômicos (`topic_distribution`).
  * Identificação de notas com alta carga cognitiva (`sibling_heavy_notes`, notas com $\ge 3$ omissões).
  * Proporção de notas estruturadas com tabelas, fluxogramas de conduta e alertas (`notes_with_redflag`).
  * Estimativa realista do tempo de primeira passagem de estudo em minutos.
* **Advisories Globais:**
  * `BATCH.ALL_CLOZE`: Alerta lotes que utilizam 100% de Cloze, recomendando diversificação pedagógica com QA para fixação de condutas ativas.
  * `BATCH.LOW_CLINICAL_DEPTH`: Alerta lotes longos (> 10 cartões) que carecem de elementos enriquecidos (sem tabelas, alertas ou correlações clínicas).
  * `BATCH.HIGH_WALL_OF_TEXT`: Alerta quando um terço ou mais das notas do lote ultrapassa limites recomendados de texto contínuo sem formatação.

---

## 5. Ferramentas MCP e Executáveis

### 5.1. Servidor MCP Principal em Rust (`apps/anki-workflow-mcp`)
O binário suporta dois modos operacionais principais:
1. `anki-workflow-mcp oracle`: Interface por linha de comando em canal `stdin/stdout` consumindo e retornando envelopes JSON estruturados. Utilizado para oráculos diferenciais e testes de compatibilidade.
2. `anki-workflow-mcp serve`: Servidor nativo baseado no protocolo MCP (Model Context Protocol via `rmcp` e transporte `stdio`), disponibilizando:
   * `review_batch_information`: Avaliação de perfil cognitivo e advisories de lote.
   * `cards_compile_drafts`: Compilação de rascunhos para formato CSV final com sanitização e normalização.
   * `check_csv`: Validação estrutural de arquivos CSV de importação direta.

---

## 6. Procedimentos e Comandos de Verificação Técnica

Os comandos listados a seguir são documentados para fins de execução e auditoria por pipelines de integração contínua (CI) e mantenedores técnicos do repositório.

> **NOTA DE CONFORMIDADE:** Os gates abaixo foram executados no workspace local desta integração (Windows, Rust stable, Python 3.14, Node 22) e o resultado real de cada um está registrado em `docs/migration/wave2-acceptance.md`. Nenhum comando desta seção altera coleções, baralhos ou mídia do Anki.

### 6.1. Verificação da Suíte Rust
```bash
# Formatação do código do workspace
cargo fmt --all -- --check

# Análise estática contra advertências
cargo clippy --workspace --all-targets -- -D warnings

# Suíte de testes unitários e de integração
cargo test --workspace

# Teste de integração específico da Wave 2 (Golden Clínico)
cargo test --test creation_pipeline_wave2_integration
```

### 6.2. Verificação de Esquemas e Suíte Python
```bash
# Contratos do servidor Python legado (não existe binário `export-schemas` neste workspace)
PYTHONPATH=src python scripts/legacy-oracle/export_contracts.py

# Verificação diferencial Python dos modelos de rascunho e figuras
pytest tests/test_images_v2.py tests/differential/test_rust_wave7_compiler.py
```
