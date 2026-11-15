# Arquitetura do Sistema

O `anki-workflow-mcp` adota uma arquitetura em transição coordenada (dual-runtime), na qual o motor **Rust** atua como núcleo primário de processamento em memória e o ecossistema **Python** atua como orquestrador legado de persistência em disco, estado transacional e gerenciamento de mídia.

```text
┌────────────────────────────────────────────────────────────────────────┐
│                          Clientes MCP                                  │
│               (Claude Desktop, Cursor, Hermes, CLI)                    │
└──────────────────┬─────────────────────────────────┬───────────────────┘
                   │ stdio (JSON-RPC)                │ stdio (FastMCP)
                   ▼                                 ▼
┌──────────────────────────────────────┐  ┌──────────────────────────────┐
│       Núcleo Rust Primário           │  │   Orquestrador Python        │
│     (apps/anki-workflow-mcp)         │  │   (src/anki_mcp/server.py)   │
│                                      │  │                              │
│ • cards_compile_drafts               │  │ • 49 ferramentas (catálogo   │
│ • review_batch_information           │  │   legado completo)           │
│ • quality_prepare                    │  │ • Manifesto e staging        │
│ • quality_evaluate                   │  │ • id_ledger e catálogos      │
│ • batch_patch_draft                  │  │ • Sincronização de mídia     │
│ • generation_contract_get            │  │ • export_final em disco      │
│                                      │  │                              │
│ (6 ferramentas puras em memória;     │  │ (Persistência com locks de   │
│  não são subconjunto do Python)      │  │  arquivo e JSONs/CSVs)       │
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
  - Exponibiliza estritamente **6 ferramentas puras em memória** (`cards_compile_drafts`, `review_batch_information`, `quality_prepare`, `quality_evaluate`, `batch_patch_draft`, `generation_contract_get`).
  - Modos auxiliares via CLI: subcomando `serve` (transporte MCP stdio explícito obrigatório), subcomando `oracle` para execução diferencial, `build-info` para metadados declarativos de compilação, `--version` para identificação legível da versão do pacote e `health` para verificação de runtime.

---

## 2. Camada de Persistência e Mídia (Python Legado)

O servidor Python (`src/anki_mcp/server.py`), baseado em FastMCP, disponibiliza o catálogo legado de 49 ferramentas (cobrindo validações, mecânicas, hierarquia, corpus, tabelas, pipeline, estado, mídia e compilador) e continua responsável pelas operações que realizam leitura e escrita física em disco. As 6 ferramentas Rust não constituem um subconjunto estrito dessas 49 ferramentas: o Rust introduz capacidades exclusivas puramente em memória (`quality_prepare`, `quality_evaluate`, `generation_contract_get`, e patch em memória sem arquivos de staging).

A camada Python retém as operações com estado:
- **`src/anki_mcp/core/`:** Leitura e gravação atômica em disco, locks de arquivo (`filelock`), normalização e sanitização.
- **`src/anki_mcp/models/`:** Schemas Pydantic para manifesto, staging de drafts, catálogos e triagem.
- **`src/anki_mcp/tools/`:** Operações MCP transacionais:
  - Ciclo de vida do manifesto: `manifesto_init`, `manifesto_state`, `manifesto_plan_set`, `manifesto_commit`, etc.
  - Gerenciamento de staging: gravação em arquivos `.jsonl` e controle de lotes.
  - Gerenciamento de IDs: persistência em `id_ledger.json` e verificação de unicidade.
  - Mídia física: inspeção em diretório, catalogação e sincronização com a pasta de coleção do Anki (`ANKI_COLLECTION_MEDIA`).
  - Exportação definitiva: `export_final`, responsável por gravar os arquivos finais `01_QA.csv` e `02_CLOZE.csv` no sistema de arquivos.

---

## 3. Perfis de Compatibilidade

O sistema formaliza dois perfis operacionais:

1. **`legacy_compat`:**
   - Mantém paridade absoluta com as regras históricas do Python.
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
  - Todas as operações com disco e arquivos de staging continuam delegadas à camada Python até que o backend SQLite seja integrado ao workspace Rust.
