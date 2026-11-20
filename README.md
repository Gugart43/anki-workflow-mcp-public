# anki-workflow-mcp

[![CI](https://github.com/Gugart43/anki-workflow-mcp-public/actions/workflows/ci.yml/badge.svg)](https://github.com/Gugart43/anki-workflow-mcp-public/actions/workflows/ci.yml)

Camada de validação, compilação e controle de qualidade para geração de flashcards Anki: **o LLM escreve; o servidor verifica estrutura, avalia fidelidade e registra**. Compatível com notas QA e Cloze e as convenções de apresentação estrutural AnkiMBR. Templates visuais, arquivos de mídia e coleções pessoais não são distribuídos neste repositório.

O projeto opera sob uma arquitetura de transição com Rust como única autoridade de produção:
1. **Núcleo primário Rust:** o servidor MCP expõe 7 ferramentas MCP (`generation_contract_get`, `quality_prepare`, `quality_evaluate`, `review_batch_information`, `cards_compile_drafts`, `batch_patch_draft` e `export_reviewed_files`): seis operações puras em memória e `export_reviewed_files` para escrita local explícita. Além disso, disponibiliza subcomandos CLI nativos (`serve`, `validate`, `sync-decks` e `sync-media`) e o binário auxiliar `export-schemas`. Rust é a única autoridade de domínio e segurança HTML5.
2. **Runtime Python legado depreciado (`src/anki_workflow_mcp/` / `src/anki_mcp/`):** o catálogo histórico permanece disponível para manifesto, staging e fluxos antigos, emitindo `DeprecationWarning` e aviso em stderr. Rust é a única autoridade de produção; a depreciação não significa paridade integral das 49 ferramentas nem autoriza sua remoção antes da verificação de migração e CI Linux/Windows.

---

## Núcleo Rust Primário

O binário nativo `anki-workflow-mcp` disponibiliza o transporte MCP via stdio e expõe 7 ferramentas MCP (`generation_contract_get`, `quality_prepare`, `quality_evaluate`, `review_batch_information`, `cards_compile_drafts`, `batch_patch_draft` e `export_reviewed_files`):

| Ferramenta MCP | Descrição e Comportamento |
|---|---|
| `generation_contract_get` | Retorna o contrato normativo do workflow `quality_v1` (regras de compilação, severidade de diagnósticos, mídia e critérios de aceitação). |
| `quality_prepare` | Prepara snapshot determinístico imutável (`snapshot_hash`) e inventário por recuperação a partir dos CSVs compilados, fontes, obrigações e vínculos de evidência. |
| `quality_evaluate` | Avalia fidelidade à fonte literal, cobertura de obrigações, consistência de evidências e revisões editoriais independentes por QA e por índice Cloze contra o snapshot. |
| `review_batch_information` | Inspeciona `qa_csv` e `cloze_csv` em memória e retorna perfis de contagem de tokens, caracteres e recuperações individuais, sem teto fixo de 20 cartões. |
| `cards_compile_drafts` | Normaliza e compila rascunhos estruturados QA/Cloze em memória para `qa_csv` e `cloze_csv` canônicos, com diagnóstico estrutural e preservação de HTML seguro em `tabelas_notas` e `figuras_notas`. |
| `batch_patch_draft` | Aplica patch pontual em rascunho de cartão puramente em memória (`{draft, updates}` $\to$ `{card}`). Não lê nem grava arquivos de staging em disco. |
| `export_reviewed_files` | Revalida integridade do conteúdo, CSV e HTML; grava exportação local com lock, idempotência e rollback. O literal `PASS` e o hash não são assinatura de aprovação editorial. |

### Limites Epistemológicos e de Governança
- **Avaliação por recuperação:** Cada par QA e cada índice Cloze ($c_1, c_2, \dots$) constitui uma recuperação atômica inspecionada individualmente.
- **Evidência literal vs. Verdade clínica:** A correspondência textual com um trecho de fonte demonstra apenas fidelidade à fonte informada. Atestações de revisores editoriais são declarações declarativas não autenticadas.
- **Autorização restrita:** Em qualquer veredicto (inclusive `PASS`), `export_authorized` e `clinical_correctness_certified` permanecem sempre `false`. O motor valida consistência técnica e conformidade a contratos, sem substituir a responsabilidade editorial e clínica humana.
- **Atomicidade e notas:** O comprimento de `Notas Adicionais` isoladamente não acarreta falha crítica de atomicidade, desde que a cobrança focal permaneça unívoca e as notas preservem HTML seguro.
- **Segurança de HTML vs. Formatadores Legados:** O compilador Rust (`compile_drafts`) rejeita ativamente tags com atributos/eventos ativos (`onclick`, `onload`, `onerror`), estilos com `url(...)` ou `expression(...)`, e URIs remotas ou inline data (`src="http..."`, `src="data:..."`). O gate de qualidade classifica violações com o código `QUALITY.UNSAFE_HTML` (severidade `Critical`). Os helpers legados `sanitize_field` e `render_media_figure` são apenas formatadores/escapadores de compatibilidade, **NÃO** sanitizadores de segurança; o bloqueio fail-closed de conteúdo ativo é garantido pelo compilador e pelo gate `quality_v1`.

---

## Compilação e Instalação

### 1. Binário Nativo Rust (Recomendado)

Requer toolchain Rust estável (MSRV mínimo 1.89 exigido por `uuid 1.29.2`, `rmcp 3.10.0` e let-chains; toolchain fixado em `1.99.1` em `rust-toolchain.toml`, documentando desvio de 1.99.0 alinhado ao canal estável disponível):

```bash
# Compilar binário de release otimizado
cargo build --release --locked -p anki-workflow-mcp
```

O binário resultante estará em:
- **Linux/macOS:** `target/release/anki-workflow-mcp`
- **Windows:** `target/release/anki-workflow-mcp.exe`

#### Desambiguação de Executáveis no PATH

| Servidor | Executável no `PATH` | Como iniciar no cliente MCP | Escopo |
| :--- | :--- | :--- | :--- |
| **Primário (Rust)** | `anki-workflow-mcp` | `"command": "anki-workflow-mcp", "args": ["serve"]` | 7 ferramentas: 6 em memória e exportação local explícita. |
| **Legado (Python)** | `anki-workflow-mcp-legacy` | `"command": "anki-workflow-mcp-legacy"` | 49 ferramentas legadas, staging em disco e `export_final`. |

#### Configuração do Cliente MCP (Rust)

O binário Rust exige o subcomando `serve` para iniciar o transporte stdio. Configure seu cliente MCP (Claude Desktop, Cursor, Hermes, VS Code) com o comando e argumento explícitos:

```json
{
  "mcpServers": {
    "anki-workflow": {
      "command": "C:/caminho/para/target/release/anki-workflow-mcp.exe",
      "args": ["serve"]
    }
  }
}
```

*Nota para sistemas POSIX:* Substitua pelo caminho nativo correspondente (ex.: `/usr/local/bin/anki-workflow-mcp` e `args: ["serve"]`). Stdout é estritamente reservado às mensagens JSON-RPC.

#### Subcomandos CLI Nativos do Binário `anki-workflow-mcp`

Além de operar como servidor MCP stdio padrão, o executável nativo Rust disponibiliza subcomandos para operação e automação em linha de comando:

- **`serve`**: Inicia o servidor MCP através do transporte padrão stdio (JSON-RPC 2.0). É o comando padrão utilizado pelos clientes MCP.
- **`validate`**: Valida a estrutura e conformidade de arquivos de rascunho sem iniciar o loop MCP:
  - `--qa-file <path>`: caminho do arquivo CSV ou rascunho QA a ser inspecionado.
  - `--cloze-file <path>`: caminho do arquivo CSV ou rascunho Cloze a ser inspecionado.
  - `--mode <quick|full>`: perfil de validação (`quick` para checagem rápida ou `full` para avaliação completa).
  - `--format <human|json>`: formato da saída (`human` para leitura no terminal ou `json` estruturado para CI e scripts).
- **`sync-decks`**: Sincroniza baralhos compilados diretamente contra uma coleção Anki local (`collection.anki2`).
- **`sync-media`**: Sincroniza arquivos de mídia locais para o diretório de mídia do Anki (opera com dry-run por padrão).
- **Comandos de inspeção e integridade**:
  - `version` (ou `--version`): exibe o nome e a versão do pacote (atualmente `0.1.0`, correspondente à versão do pacote Cargo, não uma identidade de release).
  - `build-info` (ou `--build-info`): emite JSON estruturado com implementação, versão do pacote, `target`, revisão de origem e estado da árvore Git; caso compilado fora de um repositório Git, reporta revisão e estado desconhecidos. Os metadados não são uma assinatura de procedência.
  - `health`: emite JSON com status de integridade do runtime (`{"status": "ok", ...}`).
  - `help` (ou `--help`): documenta sintaxe, argumentos e flags aceitas.

#### Binário Auxiliar `export-schemas`

O repositório disponibiliza o binário auxiliar Rust `export-schemas` (executável via `cargo run --locked -p anki-workflow-mcp --bin export-schemas` ou tarefa `just export-schemas`), encarregado de exportar deterministicamente os JSON Schemas canônicos (Draft 2020-12) de todos os tipos e contratos v2 a partir do Rust via `schemars` para `contracts/v2/schemas/` (com suporte a `--out-dir <path>`).

### Frentes TypeScript/Node Desacopladas (Sem `package.json` na Raiz)

Para manter o núcleo Rust purista e livre de dependências Node na raiz do repositório (não há `package.json` na raiz e `just rust-check` não exige Node), o ferramental TypeScript é estritamente particionado em 3 frentes desacopladas:

1. **`tests/mcp-contract-ts` (Harness E2E Vitest):**
   - Suíte E2E construída com Vitest e `@modelcontextprotocol/sdk` conectada ao binário nativo compilado via `StdioClientTransport`.
   - Valida o ciclo de vida do servidor, a interoperabilidade stdio e a conformidade dos schemas das 7 ferramentas MCP (`just mcp-contract-ts`).
2. **`tools/codegen-ts` + `contracts/v2` (Geração de Tipos TypeScript):**
   - Pipeline de codegen que consome os JSON Schemas exportados do Rust via `schemars` em `contracts/v2/schemas/`.
   - Gera definições de tipos TypeScript fortemente tipadas (`contracts/v2/ts/index.d.ts`) usando `json-schema-to-typescript`, com validação estrita via `tsc --noEmit` (`just codegen-contracts`).
3. **`npm/anki-workflow-mcp` (Launcher npx Fino Multiplataforma):**
   - Launcher `npx` fino multiplataforma (`@gugart43/anki-workflow-mcp`) baseado em `optionalDependencies` por plataforma/arquitetura (Linux x64, Linux ARM64, Windows x64, macOS arm64/x64), sem scripts de ciclo de vida (`postinstall`).
   - Distribui 5 pacotes de plataforma nativa: `@gugart43/anki-workflow-mcp-linux-x64-gnu` (`x86_64-unknown-linux-gnu`), `@gugart43/anki-workflow-mcp-linux-arm64-gnu` (`aarch64-unknown-linux-gnu`), `@gugart43/anki-workflow-mcp-win32-x64` (`x86_64-pc-windows-msvc`), `@gugart43/anki-workflow-mcp-darwin-arm64` (`aarch64-apple-darwin`) e `@gugart43/anki-workflow-mcp-darwin-x64` (`x86_64-apple-darwin`).
   - Verifica integridade SHA-256 dos binários, não realiza download em runtime e aceita `ANKI_WORKFLOW_MCP_BIN` como override local explícito (detalhes em [npm/README.md](npm/README.md)).

---

### 2. Runtime Python Legado em `src/anki_workflow_mcp/` (Depreciado)

> **Status de Depreciação (`DeprecationWarning`):** O runtime Python legado localizado em `src/anki_workflow_mcp/` (empacotado como `src/anki_mcp/`) está formalmente depreciado. A execução do módulo emite aviso de depreciação (`DeprecationWarning`) em stderr. **O Rust é a única autoridade de produção.**

O runtime Python permanece disponível exclusivamente para compatibilidade histórica transitória em fluxos de trabalho legados que ainda dependem de persistência transacional em disco, manifesto (`manifesto_init`, `manifesto_commit`), staging em arquivo, catálogos e suítes diferenciais de paridade (`just parity`). A depreciação não significa paridade integral das 49 ferramentas nem autoriza sua remoção antes da verificação completa em CI Linux e Windows.

Requer Python 3.12–3.14 (CI em Python 3.13) e [uv](https://docs.astral.sh/uv/):

```bash
# Execução direta via módulo Python (sem ambiguidade com o binário Rust)
uv run python -m anki_mcp.server
```

#### Configuração do Cliente MCP (Python Legado - Compatibilidade)

```json
{
  "mcpServers": {
    "anki-workflow-legacy": {
      "command": "uv",
      "args": ["run", "python", "-m", "anki_mcp.server"],
      "cwd": "C:/caminho/para/repositorio"
    }
  }
}
```

---

## Desenvolvimento e Qualidade

O repositório disponibiliza um `justfile` unificado para execução local multiplataforma dos gates de verificação que espelham o CI:

```bash
# Configuração inicial do ambiente Python com dependências congeladas
just setup

# Gate completo pré-PR (Rust check, Python check e Parity)
just check
```

Consulte [CONTRIBUTING.md](CONTRIBUTING.md) para a lista completa de comandos e diretrizes de desenvolvimento.

---

## Estrutura do Repositório

```text
apps/
  anki-workflow-mcp/    Binário executável do servidor MCP Rust (stdio), subcomandos CLI e oráculo
crates/
  anki-domain/          Tipos de domínio fundamentais, diagnósticos e recibo v1
  anki-parser/          Parsing de CSV (split_row) e contagem de cartões
  anki-validate/        Validações de regras, compilador de drafts e motor quality_v1
src/anki_mcp/           Runtime Python legado depreciado (`src/anki_workflow_mcp/`; DeprecationWarning)
contracts/
  v2/                   JSON Schemas canônicos (Draft 2020-12) e tipos TypeScript derivados (.d.ts)
  legacy/               Contratos JSON congelados da especificação MCP v1
tests/
  differential/         Suíte de testes de paridade 1:1 entre Rust e Python
  golden/               Fixtures canônicos de teste para verificação determinística
  mcp-contract-ts/      Harness E2E Vitest com @modelcontextprotocol/sdk via StdioClientTransport
tools/
  codegen-ts/           Codegen de tipos TypeScript a partir de JSON Schemas do Rust (schemars + json2ts)
npm/
  anki-workflow-mcp/    Launcher npx fino multiplataforma via optionalDependencies (sem postinstall)
docs/                   Documentação arquitetural, guias de qualidade e ADRs
.github/workflows/      Workflows de CI multiplataforma (Linux e Windows)
```

- [Arquitetura](docs/architecture.md)
- [Configuração e privacidade](docs/configuration.md)
- [Workflow de geração com evidência (`quality_v1`)](docs/generation-workflow.md)
- [Quantidade de informação](docs/information-quality.md)
- [Decisões Arquiteturais (ADRs)](docs/adr/ADR-001-rust-migration.md)
- [Contribuição](CONTRIBUTING.md) · [Segurança](SECURITY.md) · [Licença MIT](LICENSE)

---

## Escopo Público e Privacidade

Esta distribuição contém exclusivamente código-fonte, suítes de teste e fixtures sintéticos. Não contém coleções Anki pessoais, estudos clínicos proprietários, chaves de API, templates extraídos ou credenciais de infraestrutura. Todas as demonstrações são de natureza técnica e não representam recomendações médicas.
