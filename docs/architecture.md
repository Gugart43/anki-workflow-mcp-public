# Arquitetura do Sistema

Rust é a fonte única de verdade de domínio, parsing, segurança HTML5 e servidor MCP. O Python permanece como compatibilidade depreciada, não como dependência do fluxo novo. TypeScript limita-se a testes de interoperabilidade, contratos derivados e launcher fino.

```text
┌────────────────────────────────────────────────────────────────────────┐
│                          Clientes MCP                                  │
│               (Claude Desktop, Cursor, Hermes, CLI)                    │
└──────────────────┬─────────────────────────────────┬───────────────────┘
                   │ stdio (JSON-RPC)                │ stdio (FastMCP)
                   ▼                                 ▼
┌──────────────────────────────────────┐  ┌──────────────────────────────┐
│       Núcleo Rust Primário           │  │   Orquestrador Python        │
│     (apps/anki-workflow-mcp)         │  │ (src/anki_workflow_mcp/ - dep)│
│                                      │  │                              │
│ • cards_compile_drafts               │  │ • 49 ferramentas (catálogo   │
│ • review_batch_information           │  │   legado completo)           │
│ • quality_prepare                    │  │ • Manifesto e staging        │
│ • quality_evaluate                   │  │ • id_ledger e catálogos      │
│ • batch_patch_draft                  │  │ • Sincronização de mídia     │
│ • generation_contract_get            │  │ • export_final legado        │
│ • export_reviewed_files (disco local)│  │                              │
│                                      │  │ (DEPRECATED: compatibilidade │
│ (7 ferramentas MCP;                  │  │  explícita; não dependência  │
│  sync-media/decks somente CLI)       │  │  do fluxo novo)              │
└──────────────────┬───────────────────┘  └──────────────┬───────────────┘
                   │ Crates internas                     │
                   ▼                                     ▼
┌──────────────────────────────────────┐  ┌──────────────────────────────┐
│  • anki-domain (tipos/recibo v1)     │  │  • models/                   │
│  • anki-parser (split_row/contagem)  │  │  • checks/                   │
│  • anki-validate (quality_v1/drafts) │  │  • core/ & tools/            │
└──────────────────────────────────────┘  └──────────────────────────────┘
```

---

## 1. Núcleo Primário Nativo (Rust Workspace)

O workspace Cargo organiza o processamento de regras, compilação e avaliação em crates modulares:

- **`crates/anki-domain`:**
  - Define tipos fundamentais do domínio: `NoteType` (`Qa`, `Cloze`), `Diagnostic`, `Severity` (`Critical`, `Warn`, `Info`), e `CompatibilityProfile` (`LegacyCompat`, `StrictV2`).
  - Implementa o cálculo canônico e imutável `compute_receipt_v1`, idêntico byte a byte ao SHA-256 do legado.

- **`crates/anki-parser`:**
  - Parsing estrito e determinístico de linhas CSV pipe-delimited (`split_row`) respeitando células escapadas, CRLF/LF e campos em branco sem trim indevido.
  - Funções de contagem e auditoria de lotes (`count_batch_cards_with_profile`), discriminando QA de índices Cloze distintos.

- **`crates/anki-validate`:**
  - Validações de sintaxe de Cloze, hints, balanceamento de campos QA, sanitização de HTML seguro e validação de referências/tags.
  - Normalizador e compilador de rascunhos em memória (`compile_drafts`), gerando `qa_csv` e `cloze_csv` canônicos com preservação de extensões de tabelas (`tabelas_notas`) e figuras (`figuras_notas`).
  - **Segurança Fail-Closed de HTML vs. Formatadores Legados:** O compilador Rust rejeita compulsoriamente tags com atributos/eventos ativos (`onclick`, `onload`, `onerror`), estilos inline com `url(...)` ou `expression(...)`, e URIs remotas ou inline data (`src="http..."`, `src="data:..."`). O gate `quality_v1` emite o diagnóstico `QUALITY.UNSAFE_HTML` (`Critical`). Por contraste, os helpers legados `sanitize_field` e `render_media_figure` são apenas formatadores/escapadores de compatibilidade de strings, **NÃO** sanitizadores de segurança.
  - Patch cirúrgico em memória (`patch_draft`).
  - Motor de qualidade `quality_v1` (`quality.rs` e `quality_content.rs`): preparação de snapshots imutáveis (`snapshot_hash`), inventário por unidade de recuperação e verificação de fidelidade à fonte, cobertura de obrigações e atestações de revisão editorial.

- **`apps/anki-workflow-mcp`:**
  - Binário executável do servidor MCP que implementa o protocolo JSON-RPC via stdio assíncrono (Tokio).
  - Expõe 7 ferramentas MCP: seis operações em memória (`cards_compile_drafts`, `review_batch_information`, `quality_prepare`, `quality_evaluate`, `batch_patch_draft`, `generation_contract_get`) e a ferramenta de persistência `export_reviewed_files` (implementada em `apps/anki-workflow-mcp/src/export.rs`), com escrita local explícita e anotações de efeito colateral.
  - Subcomandos CLI nativos em Rust:
    - `serve`: inicia o transporte MCP stdio explícito obrigatório.
    - `validate`: executa validação offline e diagnósticos de qualidade via CLI.
    - `sync-decks`: sincroniza e publica hierarquia de baralhos no Anki.
    - `sync-media`: cataloga e sincroniza mídia física local com a coleção.
  - Binário auxiliar `export-schemas`: binário Rust dedicado para exportar esquemas JSON Schema 2020-12 a partir das structs Rust de domínio e MCP.
  - Modos e comandos auxiliares: subcomando `oracle` (sob feature flag `oracle` para execução diferencial), `build-info` para metadados declarativos de compilação, `--version` para identificação legível da versão e `health` para verificação de runtime.

---

## 2. Camada de Persistência e Mídia (Python Legado - Deprecated)

O runtime Python legado em `src/anki_workflow_mcp/` (baseado em FastMCP com catálogo legado de 49 ferramentas) está oficialmente marcado como **deprecated**. Seu entrypoint emite aviso de depreciação em stderr. Ele não é dependência do fluxo novo e permanece exclusivamente para fins de compatibilidade com o histórico de staging, manifesto e `id_ledger.json`. O núcleo Rust cobre nativamente validação, auditoria de qualidade, exportação local transacional e sincronizações via CLI.

A estrutura legada preservada inclui:
- **`src/anki_workflow_mcp/core/`:** Leitura e gravação atômica em disco, locks de arquivo (`filelock`), normalização e sanitização.
- **`src/anki_workflow_mcp/models/`:** Schemas Pydantic para manifesto, staging de drafts, catálogos e triagem.
- **`src/anki_workflow_mcp/tools/`:** Operações MCP transacionais legadas:
  - Ciclo de vida do manifesto: `manifesto_init`, `manifesto_state`, `manifesto_plan_set`, `manifesto_commit`, etc.
  - Gerenciamento de staging: gravação em arquivos `.jsonl` e controle de lotes.
  - Gerenciamento de IDs: persistência em `id_ledger.json` e verificação de unicidade.
  - Mídia física: inspeção em diretório, catalogação e sincronização com a pasta de coleção do Anki (`ANKI_COLLECTION_MEDIA`).
  - Exportação legada: `export_final`, responsável por gravar os arquivos históricos no sistema de arquivos.

---

## 3. Perfis de Compatibilidade

O sistema formaliza dois perfis operacionais:

1. **`legacy_compat`:**
   - Mantém APIs e recibos históricos; correções de bugs de segurança e parsing podem produzir divergências intencionais documentadas nos testes diferenciais.
   - Aplica teto máximo estrito de 20 cartões por lote (`MAX_BATCH = 20`), onde cartões Cloze são quantificados pela contagem de índices distintos ($c_N$).
   - Utiliza `compute_receipt_v1` (`sha256:` sobre os CSVs normalizados).

2. **`strict_v2`:**
   - Modelo operacional adotado pelo núcleo Rust.
   - **Sem teto arbitrário de 20 cartões:** a contagem de recuperações não é utilizada para rejeitar lotes válidos, mas sim para garantir que cada cobrança atômica (cada linha QA e cada índice Cloze) possua evidência rastreável e revisão correspondente.
   - Snapshot imutável de lote (`snapshot_hash`) vinculando CSVs, fontes, obrigações e vínculos textuais.
   - As flags `export_authorized` e `clinical_correctness_certified` permanecem sempre `false`, assegurando que o gate ateste apenas consistência técnica e conformidade estrutural.

---

## 4. Status de Implementação em Relação às ADRs

As Decisões de Arquitetura (ADRs 001 a 006) em `docs/adr/` documentam a visão de longo prazo para um sistema 100% nativo em Rust:
- **Implementado no estado atual:**
  - Núcleo de computação e validação 100% Rust em memória (ADR-001).
  - Paridade determinística de recibo `compute_receipt_v1` (ADR-004).
  - Separação dos perfis de compatibilidade `legacy_compat` e `strict_v2` (ADR-003).
  - Verificação de recuperações individuais sem teto fixo em `quality_v1` (ADR-003).
- **Arquitetura-alvo em roadmap (Não implementada no Rust atual):**
  - Armazenamento transacional em SQLite (`anki_workflow.db`) com modo WAL e `SingleWriter` dedicado (ADR-005).
  - Persistência estruturada de `ValidationAttestation` com âncora local no SQLite (ADR-004).
  - Publicação imutável com versionamento relacional (`artifact_versions`) e projeção em disco em duas etapas com Garbage Collection (ADR-006).
  - Manifesto e staging históricos permanecem no Python; a exportação Rust usa ledger local de arquivos, não implementa o backend relacional dessas ADRs.

## 5. Camadas de `anki-validate`

O layout físico plano de módulos foi mantido deliberadamente; as camadas são conceituais:

| Camada | Módulos representativos |
|---|---|
| AST e renderização | `note_model`, `validated_flow`, `note_flow`, `note_render`, `notes_render`, `legacy_adapter`, `table_render`, `media_figure` |
| Perfis e métricas | `domain_profile`, `html_text`, `note_metrics`, `batch_information` |
| Segurança e normalização | `html_security` (Ammonia/HTML5), `draft_normalize`, `sanitize`, `draft_compile`, `draft_patch` |
| Validação e qualidade | `cloze`, `hints`, `qa`, `tables`, `images`, `csv_structure`, `tags`, `crossrefs`, `quality`, `quality_content` |

`NoteDocument` contém `NoteBlock` tipados e proveniência (`NoteSource`). `CompileMode::StrictV2` rejeita combinações conflitantes e HTML legado não autorizado; `LegacyCompat` adapta entradas antigas. `RawCardDraft` continua tolerante para consumidores legados fora da fronteira MCP, enquanto `StrictRawCardDraft` (com `#[serde(deny_unknown_fields)]`) rejeita estritamente chaves desconhecidas na fronteira MCP e exige conformidade com `NoteDocument`. Aliases conhecidos são preservados. Tipos derivados não implementam domínio em JS.

`oracle` só é compilado com `--features oracle`; o binário padrão não o expõe. O toolchain de desenvolvimento permanece 1.99.1 e a MSRV declarada permanece 1.89, sem alegar que esta sessão exercitou a MSRV.

## 6. Exportação Transacional e CLI Local

A exportação é realizada pela ferramenta MCP `export_reviewed_files` (implementada em `apps/anki-workflow-mcp/src/export.rs`):
- Requer diretório explícito, slug seguro de batch, declaração obrigatória `verdict == "PASS"` e o `receipt_hash` exato dos CSVs.
- O recibo é `sha256(b"qa\0" + qa_csv + b"\0cloze\0" + cloze_csv)` hexadecimal sem prefixo; difere de `compute_receipt_v1` e de `snapshot_hash`. Ele detecta alteração de conteúdo pós-revisão, não autentica aprovação clínica.
- Estrutura dos CSVs e sanitização fail-closed de HTML são revalidadas compulsoriamente antes da escrita.
- Escrita transacional atômica: utiliza diretório de staging temporário no mesmo filesystem, locks de concorrência, geração de manifesto local `.anki_export_manifest.json` com histórico e hashes, e rollback integral em caso de falha de I/O ou conflito de destino. Idempotência não mascara arquivos alterados.

Os subcomandos nativos de coleção e manutenção no binário Rust operam exclusivamente via CLI:
- `sync-media`: recebe catálogo e caminhos locais explícitos, planeja colisões case-insensitive/SHA-256 e opera em dry-run por padrão.
- `sync-decks`: lê snapshot SQLite sintético nos testes, ordena com `unicase` e publica a hierarquia de baralhos atomicamente.
- `validate`: validação direta de lotes e regras estruturais e de qualidade via terminal sem necessidade do protocolo MCP.
Nenhum deles é exposto como ferramenta MCP nem descobre automaticamente o perfil real do usuário.

## 7. Contratos e Borda Node / TypeScript

Os contratos e a camada de interoperabilidade com TypeScript/Node são estritamente derivados da implementação Rust:
- **Binário `export-schemas`:** utilitário nativo Rust que gera os contratos e esquemas canônicos JSON Schema 2020-12 em `contracts/v2/schema/*.schema.json` a partir das structs Rust.
- **Contratos TypeScript:** o gerador `tools/codegen-ts/` produz os tipos estáticos em `contracts/v2/ts/index.d.ts` de forma determinística, sem lógica de domínio em JS.
- **Harness de testes E2E (`tests/mcp-contract-ts`):** suíte de integração ponta a ponta que valida o binário do servidor Rust via stdio utilizando o cliente oficial `@modelcontextprotocol/sdk` em TypeScript contra as 7 ferramentas MCP.
- **Launcher npm (`npm/`):** wrapper fino publicado no ecossistema Node/npm que despacha o binário Rust compilado para a plataforma hospedeira sem scripts de pós-instalação nem downloads arbitrários em runtime (pacotes opcionais por plataforma e verificação de integridade via manifesto SHA-256). Override local é uma escolha explícita de confiança do operador.

`quick` faz triagem estrutural/segurança, não revisão editorial. `strict` é padrão e vincula revisões ao snapshot canônico do grafo. Campos de certificação permanecem falsos. Ver [workflow](generation-workflow.md) e [perfis/métricas](information-quality.md) para as respectivas fontes de verdade.
