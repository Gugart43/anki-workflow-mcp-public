# Proposta: Validação de Qualidade Incremental (digest por recuperação)

- **Status:** Proposed (design only — nenhum código de produto neste change)
- **Frente:** B28
- **Data:** 2026-11-25
- **Escopo de implementação:** zero. Nada em `crates/`, `apps/` ou `src/` é alterado por este change.

## Por que

Hoje o gate `quality_v1` amarra **todas** as revisões editoriais a um único hash do lote inteiro. `compute_snapshot_hash` (`crates/anki-validate/src/quality.rs:432`) serializa `SnapshotPayload` (`quality.rs:210-218`) — `schema_version`, `qa_csv`, `cloze_csv`, `sources`, `obligations`, `bindings` — e devolve `compute_receipt_v1("anki-quality-v1", canonical_json)`. A avaliação compara o hash global campo a campo:

```rust
let snapshot_ok = rev.snapshot_hash == snapshot_hash;   // quality.rs:1069
if !snapshot_ok {
    diagnostics.push(diag_critical("QUALITY.STALE_REVIEW_SNAPSHOT", ...));  // quality.rs:1073
}
```

O efeito é um acoplamento total: **um** patch em **um** cartão invalida as revisões de **todas** as recuperações do lote, mesmo as que nada têm a ver com a mudança. Cada revisão carrega 7 dimensões booleanas (`QualityDimensions`, `quality.rs:130-138`: `source_fidelity`, `atomic_retrieval`, `context_sufficient`, `no_answer_leak`, `clinically_relevant`, `notes_appropriate`, `media_appropriate`), então o retrabalho escala como `N × 7`.

### Medição real (executada com `./target/debug/anki-workflow-mcp.exe serve`, fixture `tests/golden/wave2_clinical_batch.json`)

Lote com 2 linhas QA + 1 linha Cloze (2 índices) = **4 recuperações**. Foram redigidas 4 revisões (4 × 7 = **28 booleanos**). Depois alterou-se **uma palavra** na resposta de **um** cartão QA (`"Suspender/adiar"` → `"Adiar"`), recompilando e re-preparando:

```
QA data rows: 2  Cloze data rows: 1
snapshot_hash#1: sha256:f424a208a568f406cd6eb20eef9172a58f00d025d5f6733bc961538bd23f4e4a  retrieval_count: 4
reviews authored: 4 x 7 dimensions = 28 booleans
evaluate#1 verdict: FAIL reviewed_count: 4 stale: 0
snapshot_hash#2: sha256:ec9b4c0d2a49f32b4707883c7961dd564a7a2363e52679c05f33380e12ae6132  changed: True
evaluate#2 verdict: FAIL reviewed_count: 0
QUALITY.STALE_REVIEW_SNAPSHOT emitted: 4 (retrievals reviewed: 4 )
payload bytes hashed by compute_snapshot_hash: 3191
```

Leitura dos números:

- `reviewed_count` cai de **4 → 0**.
- `QUALITY.STALE_REVIEW_SNAPSHOT` é emitido **4 vezes** — uma por revisão, inclusive nas 3 recuperações intocadas.
- **100%** das revisões (4/4) e **100%** dos booleanos (28/28) precisam ser refeitos por causa de 1 palavra em 1 cartão.
- O payload hasheado tem **3.191 bytes**; o custo de re-revisão não é o hash, é o julgamento humano/LLM descartado.

Projeção linear: um lote no teto legado de 20 recuperações que sofra 3 patches sequenciais exige 3 × 20 = **60 revisões** × 7 = **420 booleanos**, contra 20 × 7 = 140 no cenário de revisão única. Nenhum desses números mede correção clínica; medem apenas trabalho descartado.

## O que muda (proposta)

1. **Digest por recuperação.** Cada recuperação (linha QA ou índice Cloze) ganha um `retrieval_digest` próprio: `compute_receipt_v1("anki-quality-retrieval-v1", canonical_json)` sobre uma projeção restrita aos dados *daquela* recuperação — `note_type`, `source_row`, `cloze_index`, `retrieved_target`, `visible_context`, notas visíveis, mídia referenciada, o `QualityBinding` da própria recuperação e a evidência citada.
2. **A revisão referencia o digest da própria recuperação** (`QualityReview.retrieval_digest`), não o hash do lote.
3. **Hash global passa a ser informativo.** `snapshot_hash` continua no relatório, com papel declarado de "sinal de lote" (detecta mutação do lote, agrega auditoria), mas deixa de invalidar revisões.
4. **Diagnóstico novo e estável** para staleness local: `QUALITY.STALE_RETRIEVAL_DIGEST` (Critical), emitido só para a recuperação cujo digest mudou. `QUALITY.STALE_REVIEW_SNAPSHOT` permanece, restrito ao caminho legado (revisão sem `retrieval_digest`).

Invariante que precisa valer: o `retrieval_digest` SHALL ser **insensível** a mudanças em outras recuperações do lote (inclusive reordenação de `sources`/`obligations`/`bindings` alheios) e **sensível** a qualquer mudança semântica na própria recuperação (texto do enunciado, alvo, contexto visível, notas, mídia, evidência vinculada).

## Contrato JSON alternativo

### Campos novos (todos opcionais na primeira versão)

| Onde | Campo | Tipo | Obrigatório | Semântica |
| :--- | :--- | :--- | :--- | :--- |
| `quality_prepare` → `review.cards[]` (`CardInformationReview`) | `retrieval_digest` | `string` (`sha256:<64 hex>`) | não | Digest da projeção canônica daquela recuperação. Presente sempre que o motor souber calcular (builds novas); ausente em payloads antigos. |
| `quality_prepare`/`quality_evaluate` → `QualityReview` | `retrieval_digest` | `string` (`sha256:<64 hex>`) | não | Digest que o revisor declara estar revisando. Quando presente, é a única base de staleness. |
| `QualityReport` | `snapshot_hash_role` | `"authoritative" \| "informative"` | não | Declara o papel do hash global naquele relatório. Default `informative` em builds novas; `authoritative` quando o chamador pedir o modo legado. |
| `QualityReport` | `stale_retrieval_count` | `integer >= 0` | não | Quantas recuperações tiveram digest divergente. Informativo. |

Nada é removido. `snapshot_hash`, `QUALITY.STALE_REVIEW_SNAPSHOT`, `reviewed_count` e as 7 dimensões permanecem com o mesmo nome e o mesmo tipo.

### Regras de avaliação (fail-closed, sem exceções)

| Situação da revisão | Base de staleness | Resultado |
| :--- | :--- | :--- |
| `retrieval_digest` presente e igual ao da recuperação | digest local | revisão válida |
| `retrieval_digest` presente e divergente | digest local | `QUALITY.STALE_RETRIEVAL_DIGEST` (Critical) **só nessa recuperação**; as demais seguem válidas |
| `retrieval_digest` ausente | `snapshot_hash` global (comportamento atual) | `QUALITY.STALE_REVIEW_SNAPSHOT`, como hoje |
| `retrieval_digest` presente e `snapshot_hash` divergente | digest local | revisão **válida**; o relatório marca `snapshot_hash_role: "informative"` e a divergência global não bloqueia |
| `retrieval_digest` presente mas de outra recuperação (swap de endereço) | digest local | rejeitada como digest divergente — nunca herda validade |

Último caso é o risco óbvio de um digest por recuperação e precisa de cenário negativo próprio.

### Modo e migração compatível

- **Fase 1 (compatível, sem quebra):** o motor emite `retrieval_digest` nos resultados e aceita revisões com ou sem ele. Revisão com `snapshot_hash` e sem `retrieval_digest` = comportamento de hoje. Revisão com ambos = staleness local, `snapshot_hash` informativo. Nenhum cliente existente quebra; nenhum campo obrigatório muda.
- **Fase 2 (opt-in explícito):** argumento `review_binding: "global" | "retrieval"` em `quality_prepare`/`quality_evaluate`, default `global`, para o chamador escolher. `review_binding: "global"` reproduz byte a byte o comportamento atual.
- **Fase 3 (eventual flip de default):** só depois de a suíte diferencial e o harness TS passarem com `retrieval` em ambos os lados. Exige nova decisão registrada; este change **não** a toma.

Depreciação de `QUALITY.STALE_REVIEW_SNAPSHOT` fica fora de escopo: ele continua sendo o caminho de quem não migra.

## Critérios de aceitação (testáveis por execução real)

1. **Isolamento de staleness.** Lote de 4 recuperações, 4 revisões válidas; patch em 1 QA palavra; reavaliar com as revisões antigas → `reviewed_count == 3`, exatamente 1 `QUALITY.STALE_RETRIEVAL_DIGEST`, e a recuperação patchada é a única rejeitada. Hoje o mesmo experimento produz `reviewed_count == 0` e 4 `QUALITY.STALE_REVIEW_SNAPSHOT` (medido acima).
2. **Insensibilidade a permutação.** Reordenar `sources`, `obligations` e `bindings` sem mudança semântica → todos os `retrieval_digest` idênticos (mesma propriedade que `snapshot_hash` já garante).
3. **Sensibilidade local.** Mudar 1 caractere no enunciado, no alvo, no contexto visível, nas notas ou na mídia da recuperação R → `retrieval_digest(R)` muda e `retrieval_digest(R')` de todas as demais não muda.
4. **Anti-herança de endereço.** Revisão de R1 carregando `retrieval_digest` de R2 → rejeitada com `QUALITY.STALE_RETRIEVAL_DIGEST`, nunca contada como válida.
5. **Compatibilidade de contrato.** Todo payload antigo (só `snapshot_hash`) continua produzindo o mesmo `verdict`, os mesmos diagnósticos e o mesmo `reviewed_count` de hoje; `contracts/v2/schemas/*.schema.json` só ganha propriedades opcionais.
6. **Paridade.** Suíte diferencial (`tests/differential/`) verde com adição puramente aditiva em ambos os lados.
7. **Paridade de mensagem.** `quality_prepare` chamado duas vezes sobre o mesmo lote é byte-idêntico, incluindo os novos campos.

## Riscos

| Risco | Impacto | Mitigação proposta |
| :--- | :--- | :--- |
| Digest local não cobrir um insumo relevante (ex.: obrigação compartilhada por 2 recuperações) → revisão fica "válida" após mudança material | Falso PASS relativo a evidência | Incluir `obligation_ids` e a evidência citada na projeção local; teste 3 cobre cada insumo |
| Digest local cobrir demais (ex.: o CSV inteiro) → recai no acoplamento global | Regressão disfarçada | Critério 2 (permutação) e critério 1 (isolamento) são executados com contagem exata |
| Clientes antigos não enviam `retrieval_digest` e ganham staleness global por engano | Nenhum (é o comportamento atual) | Fase 1 é aditiva; fase 3 exige decisão explícita |
| Dois cálculos de digest (global e local) divergirem em normalização de CSV | Revisões válidas rejeitadas | Reutilizar `canonical_csv_for_snapshot` (`quality.rs:413`) e um único `compute_receipt_v1` |
| `snapshot_hash` "informativo" ser lido como atestação | Confusão epistêmica | `snapshot_hash_role` explícito no relatório e no `generation_contract_get` |

## In-Scope

- Especificação do digest por recuperação, do contrato JSON aditivo e do plano de migração em 3 fases.
- Regras de staleness por revisão, incluindo o caso anti-herança de endereço.
- Critérios de aceitação executáveis e riscos.

## Out-of-Scope

- Qualquer alteração em `crates/`, `apps/` ou `src/` — inclusive registrar `QUALITY.STALE_RETRIEVAL_DIGEST` em `src/anki_mcp/core/codes.py` e no Rust.
- Regeneração de `contracts/v2/schemas/*.schema.json` e `contracts/v2/ts/index.d.ts`.
- Flip do default de `review_binding`.
- Remoção de `QUALITY.STALE_REVIEW_SNAPSHOT`.
- Revisão automática/LLM de qualquer recuperação; o `retrieval_digest` mede staleness, não qualidade.

## Alterações de CLI/MCP

- **MCP:** `quality_prepare` e `quality_evaluate` ganham o argumento opcional `review_binding`; resultados ganham `retrieval_digest`, `snapshot_hash_role` e `stale_retrieval_count`. Nenhuma ferramenta nova, nenhuma ferramenta removida: o servidor Rust continua expondo as mesmas 7 ferramentas.
- **CLI:** nenhuma. `sync-media` e `sync-decks` não são tocados.

## Fronteira Rust vs. legado Python

O motor Rust (`crates/anki-validate/src/quality.rs`) é a autoridade. O cálculo de `retrieval_digest` deve nascer lá e, se o oráculo Python precisar responder ao diferencial, ser espelhado em `src/anki_mcp/core/evaluate.py` — nunca o contrário. Enquanto o espelho não existir, o diferencial compara apenas o caminho `review_binding: "global"`. `legacy_compat` e o teto de 20 cartões do Python não são afetados.

## Delta de contrato no momento da implementação (não agora)

Somente adições opcionais, o que mantém a regeneração backward-compatible:

- `contracts/v2/schemas/mcp_quality_prepare_args.schema.json`: +`review_binding`.
- `contracts/v2/schemas/mcp_quality_evaluate_args.schema.json`: +`review_binding`.
- `contracts/v2/schemas/quality_input.schema.json`: +`QualityReview.retrieval_digest` (opcional).
- `contracts/v2/schemas/batch_information_review.schema.json`: +`CardInformationReview.retrieval_digest` (opcional).
- `contracts/v2/schemas/quality_report.schema.json`: +`snapshot_hash_role`, +`stale_retrieval_count` (ambos opcionais).
- `contracts/v2/ts/index.d.ts`: tipos correspondentes, derivados.

Nenhum `required` é alterado, então `.d.ts` e schemas atuais continuam válidos para consumidores existentes.
