# ADR-005: Arquitetura de Escrita em SQLite, Concorrência, Idempotência e Durabilidade

- **Status:** Proposed / Target Architecture (Not Implemented)
- **Status de Implementação:** Workspace Rust não possui dependência SQLite; persistência atual roda no Python via `filelock`.
- **Data:** 2026-11-14
- **Relacionado:** ADR-001, ADR-003, ADR-004, ADR-006, Seções 1.3, 3.1 e 4.2 do `PLANO_MESTRE_ANKI_WORKFLOW_MCP_RUST_V2.md`
- **Ressalvas do Parecer Técnico Incorporadas:** P-02, P-03, P-17, P-23

---

## Contexto

A auditoria da versão legada em Python v0.3.1 identificou fragilidades estruturais decorrentes da persistência direta em arquivos esparsos no disco (`drafts.jsonl`, `manifesto.json`, CSVs soltos) mediada por `filelock`:

1. **Ausência de Transações ACID e Risco de Corrupção (P-17):** Falhas de processo, travamento do sistema operacional ou cortes súbitos de energia durante escritas parciais deixavam diretórios de staging corrompidos e manifestos dessincronizados do conteúdo dos lotes.
2. **Falta de Idempotência Criptográfica e Conflitos de Replay (P-02):** A ausência de uma tabela formal de operações registrando o digest da requisição impedia que uma chamada retransmitida retornasse a resposta original determinística, e permitia que chamadas conflitantes sobrescrevessem dados silenciosamente.
3. **Condições de Corrida e Vulnerabilidade no Reconciliador de Boot (P-03):** Se um segundo processo MCP fosse inicializado enquanto outro estivesse executando uma operação transacional (`PREPARED`), não havia mecanismo de lease ou identificação de processo para impedir que o segundo processo assumisse ou abortasse prematuramente transações ativas do primeiro.
4. **Backup e Manutenção Inexistentes (P-23):** Sem transações transacionais nem checkpoints estruturados, a cópia de segurança a quente dependia da cópia crua de arquivos, com risco iminente de capturar estados inconsistentes.

---

## Alternativas consideradas

1. **Manter Concorrência com File Locks e JSONs (Legado):**
   - *Prós:* Sem introdução de banco de dados relacional.
   - *Contras:* Não atende garantias ACID; vulnerável a corrupção em falhas de energia; sem garantias transacionais de idempotência.
2. **SQLite com Pool Multi-Escrita Concorrente e `PRAGMA synchronous = NORMAL`:**
   - *Prós:* Implementação simples com pools de conexão genéricos.
   - *Contras:* SQLite em modo WAL suporta apenas um único escritor ativo por vez. Múltiplos escritores concorrentes geram erros frequentes de `SQLITE_BUSY` e deadlocks decorrentes de upgrade de locks (`SHARED` $\to$ `EXCLUSIVE`). Além disso, `synchronous = NORMAL` no modo WAL não garante durabilidade contra falhas de energia catastróficas antes de um checkpoint de fsync.
3. **Single-Writer Dedicado em Rust + `BEGIN IMMEDIATE` + Durabilidade Diferenciada + Tabela de Operações com Lease/CAS + SQLite Online Backup API (Adotada):**
   - *Prós:* Elimina deadlocks de upgrade; garante linearidade de escrita; provê idempotência rigorosa em nível de protocolo; protege contra split-brain de processos; assegura integridade total contra perda de energia nos commits críticos.
   - *Contras:* Requer arquitetura de canal/ator dedicada para operações de escrita e gestão explícita do ciclo de vida das conexões.

---

## Decisão

Adotar a **Alternativa 3** como o motor transacional padrão em Rust para o `anki-workflow-mcp` v2:

### 1. Modelo de Concorrência SQLite e `BEGIN IMMEDIATE` (P-17)

- **Modo WAL e Configuração de Locks:**
  Ao abrir o banco de dados (`anki_workflow.db`), toda conexão aplica compulsoriamente:
  ```sql
  PRAGMA journal_mode = WAL;
  PRAGMA busy_timeout = 5000;
  PRAGMA foreign_keys = ON;
  ```
- **Topologia de Conexões:**
  - **Pool de Leitores:** Conexões somente-leitura concorrentes (`SQLITE_OPEN_READ_ONLY`) para consultas de catálogo, validação de IDs, leitura de histórico e auditoria de estado.
  - **Escritor Único Dedicado (`SingleWriter`):** Uma única conexão com permissão de escrita, encapsulada em um ator de escrita ou gerenciada via canal thread-safe (MPSC / `Mutex<Connection>`), garantindo que apenas uma transação de mutação execute por vez no processo.
- **Transações com `BEGIN IMMEDIATE`:**
  Toda transação que envolva mutação de estado (compilação de draft, validação com gravação de atestação, commit de manifesto, exportação final) deve obrigatoriamente iniciar com `BEGIN IMMEDIATE`. Isso reserva o lock de escrita imediatamente no início da transação, eliminando completamente disputas e deadlocks de upgrade de lock (`SQLITE_BUSY`) causados por transações deferred (`BEGIN DEFERRED`).

### 2. Política Diferenciada de Durabilidade e Resiliência a Queda de Energia (P-17)

- **Operações Transitórias / Intermediárias:**
  Operações de rascunho de alta frequência e gravações transitórias operam sob `PRAGMA synchronous = NORMAL;`, assegurando excelente performance com proteção contra corrupção do banco em falhas normais do processo.
- **Commits Críticos:**
  Nas operações que consolidam alterações irrevogáveis — especificamente `manifesto_commit`, `export_final` e a gravação de `ValidationAttestation` (ADR-004) — o servidor eleva temporariamente a sincronização para:
  ```sql
  PRAGMA synchronous = FULL;
  ```
  O commit garante que os dados e o log de transação do WAL sejam confirmados via chamada síncrona ao disco (`fsync`/`FlushFileBuffers` no Windows) antes que o resultado com o recibo (`receipt_token`) ou atestação seja transmitido ao cliente MCP.
- **Testes de Power-Loss:**
  A infraestrutura de testes em Rust deve incluir cenários de desligamento abrupto (process kill forçado durante transações em voo) seguidos de verificação imediata de integridade estrutural via `PRAGMA integrity_check` e `PRAGMA quick_check`.

### 3. Tabela `operations` e Idempotência Rigorosa (P-02)

Para cumprir o contrato de idempotência MCP sem risco de inconsistências, o schema do banco inclui a tabela `operations`:

```sql
CREATE TABLE operations (
    operation_id TEXT PRIMARY KEY,
    client_request_id TEXT,
    operation_type TEXT NOT NULL,
    request_payload_digest TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('PREPARED', 'COMMITTED', 'ABORTED', 'FAILED')),
    owner_id TEXT NOT NULL,
    lease_expires_at INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    result_ref TEXT,
    result_digest TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_operations_lookup 
ON operations(operation_type, request_payload_digest);
```

#### Regras de Processamento de Idempotência:
1. **Replay Idêntico:** Quando uma requisição chega com um `operation_id` já existente:
   - Se `status = 'COMMITTED'` e o `request_payload_digest` for rigorosamente idêntico ao registrado: o servidor **não reexecuta** a operação. Retorna instantaneamente a resposta original armazenada referenciada por `result_ref` e validada por `result_digest`.
2. **Conflito de Payload (`STATE.CONFLICT`):**
   - Se `operation_id` já existir no banco, mas com um `request_payload_digest` divergente da requisição atual: a operação é rejeitada imediatamente com código de erro estruturado `STATE.CONFLICT`, impedindo mutações sobrepostas sob o mesmo identificador de operação.
3. **Operação em Execução (`PREPARED`):**
   - Se `operation_id` constar como `PREPARED` e o lease estiver ativo, retorna erro de contenção transitória orientando o cliente a aguardar a conclusão.

### 4. Reconciliador de Boot, `owner_id`, Lease e CAS (P-03)

Para permitir a recuperação após falhas catastróficas sem criar riscos de conflito em ambientes multi-processo (por exemplo, instâncias concorrentes disparadas inadvertidamente):

1. **Identidade do Processo (`owner_id`):** Ao inicializar, cada instância do processo MCP gera um UUID v4 volátil próprio (`process_uuid`), utilizado como `owner_id`.
2. **Heartbeat de Lease:** Toda operação transacional em estado `PREPARED` obtém um lease com expiração estrita (`lease_expires_at = now() + 30_000` ms). O processo emissor mantém uma tarefa periódica em background de heartbeat renovando o lease a cada 5 segundos enquanto a operação estiver em curso.
3. **Compare-And-Swap (CAS) Atômico no Boot:**
   Durante o processo de reconciliação na inicialização, o servidor busca operações em estado `PREPARED`:
   - Um segundo processo MCP **nunca adota** nem altera uma operação `PREPARED` cujo lease ainda esteja válido (`lease_expires_at >= CURRENT_TIMESTAMP_MS`).
   - Se e somente se o lease estiver comprovadamente expirado (`lease_expires_at < CURRENT_TIMESTAMP_MS`), a operação é considerada órfã decorrente de queda do processo anterior e é cancelada via CAS na revisão:
     ```sql
     UPDATE operations 
     SET status = 'ABORTED',
         revision = revision + 1,
         updated_at = ?
     WHERE operation_id = ?
       AND revision = ?
       AND status = 'PREPARED'
       AND lease_expires_at < ?;
     ```
   - O sucesso da atualização atesta que nenhuma outra instância alterou a operação concorrentemente.

### 5. Rotina de Backup via SQLite Online Backup API e WAL Checkpoint (P-23)

- **Backup Online Não-Bloqueante:**
  O sistema realiza cópias de segurança a quente utilizando a SQLite Online Backup API (`rusqlite::backup::Backup` / `sqlite3_backup_init`). Esse mecanismo copia páginas do banco em blocos incrementais sem interromper ou bloquear conexões concorrentes de leitura e escrita.
- **Política de Checkpoint de WAL:**
  - **Rotina Regular:** Durante a operação normal, o checkpoint padrão do SQLite opera sob limite automático (`PRAGMA wal_autocheckpoint = 1000;`).
  - **Checkpoint Pré/Pós-Backup:** Antes de arquivar ou rotacionar o banco de backup, executa-se explicitamente `PRAGMA wal_checkpoint(PASSIVE);` para transferir páginas do WAL para o arquivo mestre sem bloquear escritores.
  - **Truncamento de Manutenção:** Durante tarefas de manutenção administrativa ou finalização controlada, executa-se `PRAGMA wal_checkpoint(TRUNCATE);` para zerar o log e economizar espaço em disco.

---

## Trade-offs e Riscos

- **Serialização de Escritas:** Todas as operações que alteram o estado são serializadas através da conexão única de escrita. Como o volume de lotes por usuário humano é tipicamente de 20 cartões por vez e transações de compilação duram menos de 20ms, a contenção é desprezível face à eliminação total de conflitos de concorrência.
- **Overhead de I/O no `synchronous = FULL`:** Adiciona uma latência de sincronização de hardware (~5–15ms) nos commits finais. Este custo é plenamente aceitável e necessário para garantir a durabilidade de acervos médicos e atestações de conformidade.
- **Gerenciamento de Leases:** Falhas de rede interna ou suspensão de threads poderiam teoricamente expirar um lease de uma transação longa. O intervalo de 30 segundos com heartbeat a cada 5 segundos fornece margem robusta de 6x para evitar falsos positivos de expiração.

---

## Teste que valida esta decisão

1. **Paridade e Idempotência:**
   - Teste automatizado validando replay idêntico com retorno do mesmo digest e sem mutação adicional no banco.
   - Teste de colisão injetando o mesmo `operation_id` com payload alterado, confirmando o lançamento do erro `STATE.CONFLICT`.
2. **Concorrência e Lease Multi-Processo:**
   - Teste simulando dois processos concorrentes (`owner_A` e `owner_B`), comprovando que `owner_B` não consegue adotar nem abortar uma transação ativa com lease válido pertencente a `owner_A`.
   - Teste de reconciliador com avanço temporal forçado, comprovando que transações órfãs com lease vencido são revertidas via CAS com sucesso.
3. **Resiliência a Queda de Energia (Crash Recovery):**
   - Teste com encerramento forçado do processo (`SIGKILL`/abort forçado) imediatamente antes do commit de `manifesto_commit`, verificando recuperação atômica limpa e integridade via `PRAGMA integrity_check`.
4. **Validação de Backup:**
   - Teste automatizado executando `rusqlite::backup` durante carga contínua de escrita e confirmando que o snapshot resultante é 100% íntegro e consistente com o digest do estado.

---

## Status de Implementação (Promoção Rust Primário)

- **Implementado no estado atual:** Nenhuma dependência relacional ou SQLite está presente no workspace Rust atual (`Cargo.toml` não inclui `rusqlite`). O núcleo nativo Rust opera de forma puramente stateless em memória.
- **Arquitetura futura / Persistência legada:** Esta ADR estabelece a especificação formal completa para quando a camada de persistência for portada para Rust na versão 2. Até que essa fase seja implementada, a durabilidade, concorrência e idempotência continuam sendo providas pelo ecossistema Python (`src/anki_mcp/`) por meio de arquivos JSON/CSV e travas de arquivo (`filelock`).
