# ADR-006: Publicação Imutável de Artefatos, Projeções de Sistema de Arquivos, Retenção e Recuperação

- **Status:** Aceito
- **Data:** 2026-11-14
- **Relacionado:** ADR-001, ADR-003, ADR-004, ADR-005, Seções 3.2, 4.3 e 5.1 do `PLANO_MESTRE_ANKI_WORKFLOW_MCP_RUST_V2.md`
- **Ressalvas do Parecer Técnico Incorporadas:** P-04, P-15, P-16

---

## Contexto

Na arquitetura legada em Python v0.3.1, a exportação de cartões (`export_final`) sobrescrevia diretamente os arquivos CSV no disco (`01_QA.csv`, `02_CLOZE.csv`), atualizava o arquivo `manifesto.json` e tentava manter o registro em `id_ledger.json`. Essa abordagem apresentava graves fragilidades estruturais:

1. **Inexistência de Integridade Referencial e Histórico de Publicação (P-04):** Não havia controle formal de versões anteriores nem garantias relacionais entre o lote e seus artefatos exportados. Sobrescritas acidentais ou quedas durante a gravação deixavam o sistema sem capacidade de auditoria ou rollback determinístico.
2. **Dessincronização entre Resposta MCP e Arquivos no Disco (P-15):** A resposta da ferramenta `export_final` reportava o estado da memória sem garantir que os arquivos físicos no sistema de arquivos estivessem atomicamente confirmados e sincronizados, criando discrepâncias (TOCTOU) com ferramentas legadas de inspeção e sincronizadores de desktop (como AnkiConnect).
3. **Ausência de Política de Descarte Seguro e Risco de Acúmulo Ilimitado (P-16):** A introdução de um histórico imutável no SQLite exige regras rígidas de Garbage Collection (GC). Sem uma política clara, há risco de esgotamento de quota de disco por versões históricas ou, pior, purga acidental da versão ativa em produção ou de versões em preparação.

---

## Alternativas consideradas

1. **Manter o Sistema de Arquivos como Fonte Única da Verdade (Legado):**
   - *Prós:* Simplicidade inicial; compatibilidade direta com visualizadores de arquivo.
   - *Contras:* Impossibilidade de aplicar transações atômicas multiplataforma (especialmente em Windows com Defender/indexadores bloqueando arquivos); ausência de histórico imutável e atestação criptográfica vinculada.
2. **SQLite como Repositório Exclusivo, Abolindo Projeções em Disco:**
   - *Prós:* Centralização absoluta em uma única fonte relacional ACID.
   - *Contras:* Quebra completa da interoperabilidade com scripts externos, rotinas de backup de diretório do usuário e a importação tradicional do Anki Desktop.
3. **SQLite como Fonte Primária Imutável + Projeções Atômicas em Disco com Status Estruturado + GC em Duas Fases (Adotada):**
   - *Prós:* O SQLite atua como fonte da verdade canônica e imutável para auditoria e histórico; projeções físicas de arquivos CSV/JSON são geradas de forma atômica; a resposta MCP reporta com exatidão o status da projeção (`projection_status`); integridade referencial estrita impede referências órfãs; GC em duas fases previne vazamento de armazenamento sem jamais coletar a versão ativa.
   - *Contras:* Necessidade de coordenar a gravação no banco de dados relacional com a projeção atômica de arquivos no sistema operacional.

---

## Decisão

Adotar a **Alternativa 3**, estruturando o ciclo de vida de artefatos segundo os pilares abaixo:

### 1. Modelo Relacional de Publicação Imutável e Integridade Referencial (P-04)

O schema do banco de dados estabelece a rastreabilidade imutável entre lotes, versões publicadas e atestações de validação:

```sql
CREATE TABLE batches (
    deck_id TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    phase TEXT NOT NULL,
    active_artifact_version INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (deck_id, batch_id)
);

CREATE TABLE artifact_versions (
    artifact_id TEXT PRIMARY KEY,
    deck_id TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    version_num INTEGER NOT NULL,
    content_digest TEXT NOT NULL,
    receipt_token TEXT NOT NULL,
    attestation_id TEXT NOT NULL,
    qa_payload TEXT NOT NULL,
    cloze_payload TEXT NOT NULL,
    manifest_snapshot TEXT NOT NULL,
    ledger_snapshot TEXT NOT NULL,
    publication_state TEXT NOT NULL CHECK(publication_state IN ('DRAFT', 'PREPARED', 'COMMITTED', 'TOMBSTONED', 'PURGED')),
    created_at INTEGER NOT NULL,
    tombstoned_at INTEGER,
    FOREIGN KEY (deck_id, batch_id) 
        REFERENCES batches(deck_id, batch_id) 
        ON DELETE RESTRICT,
    FOREIGN KEY (attestation_id) 
        REFERENCES validation_attestations(attestation_id) 
        ON DELETE RESTRICT,
    UNIQUE (deck_id, batch_id, version_num)
);

CREATE INDEX idx_artifact_versions_lookup 
ON artifact_versions(deck_id, batch_id, publication_state);
```

#### Invariante Estrita de `batches.active_artifact_version`:
- O ponteiro `batches.active_artifact_version` identifica a versão em produção do lote.
- **Invariante Relacional:** Uma versão só pode se tornar ativa se existir em `artifact_versions` com o mesmo `(deck_id, batch_id, version_num)` e seu `publication_state` for rigorosamente `'COMMITTED'`.
- Essa invariante é aplicada transacionalmente no SQLite via trigger de validação e restrições na camada de aplicação:
  ```sql
  CREATE TRIGGER trg_validate_active_artifact_version
  BEFORE UPDATE OF active_artifact_version ON batches
  FOR EACH ROW
  WHEN NEW.active_artifact_version IS NOT NULL
  BEGIN
      SELECT RAISE(ABORT, 'STATE.INVALID_POINTER: active_artifact_version must point to a COMMITTED artifact_version')
      WHERE NOT EXISTS (
          SELECT 1 FROM artifact_versions
          WHERE deck_id = NEW.deck_id
            AND batch_id = NEW.batch_id
            AND version_num = NEW.active_artifact_version
            AND publication_state = 'COMMITTED'
      );
  END;
  ```

### 2. Protocolo de Publicação e Resposta `EXPORTED` vs Projeções Legadas (P-15)

Para garantir integridade atômica e evitar que falhas transitórias de lock de arquivo no Windows impeçam o commit no banco:

#### Protocolo em Duas Etapas (Two-Stage Publication):
1. **Etapa 1: Consolidação Transacional SQLite (Fonte da Verdade):**
   - Sob transação `BEGIN IMMEDIATE` e durabilidade `synchronous = FULL` (ADR-005):
     - Validação de que a `ValidationAttestation` (ADR-004) do lote é válida e não-revogada.
     - Inserção da nova versão em `artifact_versions` com `publication_state = 'COMMITTED'`.
     - Atualização do ponteiro `batches.active_artifact_version`.
     - Registro da operação como `COMMITTED` na tabela `operations`.
     - Commit no SQLite. A partir deste instante, a publicação é definitiva, recuperável e canônica.
2. **Etapa 2: Projeção Físico-Atômica no Sistema de Arquivos:**
   - O servidor projeta os artefatos nos arquivos legados (`01_QA.csv`, `02_CLOZE.csv`, `manifesto.json`, `id_ledger.json`) utilizando gravação atômica: escrita em arquivo temporário `.tmp` no mesmo sistema de arquivos, chamada explícita de `flush` + `sync_all`, seguida de substituição atômica via renomeação (`std::fs::rename` com retry em Windows).
3. **Contrato de Resposta MCP:**
   - A resposta de `export_final` retorna explicitamente o estado consolidado no banco e o status da projeção no disco:
     ```json
     {
       "verdict": "EXPORTED",
       "deck_id": "02_MEDICINA_AnkiMBR",
       "batch_id": "B01",
       "artifact_version": 4,
       "content_digest": "sha256:7f83b1...",
       "receipt_token": "sha256:...",
       "attestation_id": "att_01...",
       "projection_status": "IN_SYNC",
       "projected_files": [
         "01_QA.csv",
         "02_CLOZE.csv",
         "manifesto.json",
         "id_ledger.json"
       ]
     }
     ```
   - **Tratamento de Contenção de Arquivo no Disco:** Se um lock transitório de arquivo externo (antivírus, indexador do Windows ou Anki Desktop aberto) impedir a renomeação imediata no disco:
     - O banco de dados **não sofre rollback**, pois o commit do SQLite já é soberano.
     - A resposta MCP retorna `projection_status: "DIRTY"` acompanhada de um aviso detalhado.
     - A reconciliação de boot ou uma chamada idempotente de `export_final` reexecuta a projeção atômica em disco sem revalidar nem recriar a versão já comprometida.

### 3. Política de Retenção e Garbage Collection em Duas Fases (P-16)

Para prevenir o consumo desenfreado de armazenamento sem comprometer a rastreabilidade:

#### Regras de Retenção Configuráveis:
- **Cota de Versões:** Preservar por padrão as últimas $N$ versões (padrão: $N = 5$) por lote.
- **Cota de Bytes:** Limite configurável de armazenamento total por deck (ex: máximo de 100 MB).

#### Invariantes Absolutas de Proteção contra Coleta:
1. **Versão Ativa Intocável:** A versão referenciada por `batches.active_artifact_version` **JAMAIS** pode ser coletada, independentemente de sua idade ou cota.
2. **Versões em Preparação Protegidas:** Nenhuma versão em estado `PREPARED` associada a uma operação com lease ativo na tabela `operations` pode ser marcada para descarte.
3. **Imutabilidade do Ledger Histórico:** O registro histórico (metadados: `artifact_id`, `version_num`, `content_digest`, `receipt_token`, `attestation_id`, timestamps) permanece permanentemente no banco como trilha de auditoria; apenas os payloads de texto volumosos são purgados.

#### Ciclo de Vida em Duas Fases (Two-Phase Tombstone):
- **Fase 1: Marcação de Tombstone:**
  Quando uma versão excede a cota de retenção e não é a versão ativa:
  ```sql
  UPDATE artifact_versions 
  SET publication_state = 'TOMBSTONED',
      tombstoned_at = ?
  WHERE deck_id = ?
    AND batch_id = ?
    AND publication_state = 'COMMITTED'
    AND version_num NOT IN (
        SELECT active_artifact_version FROM batches WHERE deck_id = ? AND batch_id = ?
    )
    AND version_num <= (
        SELECT MAX(version_num) - ? FROM artifact_versions WHERE deck_id = ? AND batch_id = ?
    );
  ```
  Versões em estado `TOMBSTONED` ainda mantêm os payloads no banco e permitem rollback imediato em caso de emergência.
- **Fase 2: Purga Definitiva (Purge):**
  Após um período de quarentena configurável (ex: 48 horas) ou sob pressão crítica de cota de armazenamento:
  ```sql
  UPDATE artifact_versions 
  SET publication_state = 'PURGED',
      qa_payload = '',
      cloze_payload = '',
      manifest_snapshot = '',
      ledger_snapshot = ''
  WHERE publication_state = 'TOMBSTONED'
    AND tombstoned_at < ?;
  ```
  Isso libera o espaço das strings/blobs no banco de dados mantendo a integridade da árvore de versões e a validade histórica dos recibos emitidos.

### 4. Reconciliador de Boot, Crash Recovery e Rollback

1. **Reconciliação no Boot:**
   - Durante a inicialização do motor Rust, o reconciliador inspeciona `artifact_versions`:
     - Versões em estado `PREPARED` cujo lease na tabela `operations` tenha expirado (ADR-005) são marcadas como `TOMBSTONED`.
     - O sistema compara os hashes dos arquivos projetados no disco contra a versão ativa indicada em `batches.active_artifact_version`. Se houver discrepância ou se arquivos estiverem ausentes (`DIRTY`), executa-se a projeção atômica a partir do payload armazenado no SQLite.
2. **Operação de Rollback Determinístico:**
   - A ferramenta administrativa `batch_rollback(deck_id, batch_id, target_version)` permite reverter o lote para qualquer versão que possua `publication_state` igual a `'COMMITTED'` ou `'TOMBSTONED'`.
   - A operação atualiza transacionalmente `batches.active_artifact_version = target_version`, reativa a versão para `'COMMITTED'` e re-projeta atomicamente os arquivos no sistema de arquivos.

---

## Trade-offs e Riscos

- **Duplicação Transitória de Dados:** Os cartões residem simultaneamente como payloads de versão no SQLite e como arquivos projetados em CSV no disco. Dado o tamanho reduzido de lotes médicos (típico de 20 a 100 cartões por lote, ~50 KB por versão), esse custo é insignificante diante da confiabilidade contra travamentos e corrupção.
- **Latência no `export_final`:** A gravação transacional com fsync seguida de projeção atômica no disco adiciona aproximadamente 10–25ms ao tempo total da operação, valor imperceptível na interface do usuário e compatível com as garantias de durabilidade exigidas.

---

## Teste que valida esta decisão

1. **Integridade Referencial e Invariantes:**
   - Teste rejeitando a criação de `artifact_versions` com `(deck_id, batch_id)` inexistente na tabela `batches`.
   - Teste comprovando que a tentativa de apontar `active_artifact_version` para uma versão em estado `DRAFT`, `TOMBSTONED` ou inexistente é abortada pelo trigger do banco.
2. **Projeção Atômica e Reconciliação `DIRTY`:**
   - Teste simulando falha de escrita no disco após o commit do SQLite, verificando que o retorno contém `projection_status = 'DIRTY'`.
   - Teste validando que a reexecução ou o reconciliador de boot restaura com exatidão os arquivos CSV a partir do payload da versão ativa do SQLite.
3. **Ciclo de Vida do Garbage Collection (GC):**
   - Teste gerando 10 versões sucessivas de um lote com limite de retenção de 3 versões, confirmando que as versões antigas transicionam para `TOMBSTONED` e subsequentemente para `PURGED`, enquanto a versão ativa permanece intacta.
4. **Rollback Transacional:**
   - Teste aplicando `batch_rollback` para uma versão anterior e verificando que tanto o ponteiro no banco quanto os arquivos projetados no disco retornam ao conteúdo da versão desejada.
