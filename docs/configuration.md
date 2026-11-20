# Configuração e Privacidade

Este documento detalha a configuração do servidor nativo Rust e do orquestrador Python legado, orientações de integração com clientes MCP e diretrizes de privacidade.

---

## 1. Configuração do Servidor Nativo Rust (Primário)

O servidor Rust `anki-workflow-mcp` opera via transporte stdio e expõe 7 ferramentas: 6 ferramentas determinísticas puramente em memória (`generation_contract_get`, `cards_compile_drafts`, `review_batch_information`, `quality_prepare`, `quality_evaluate` e `batch_patch_draft`), que não dependem de banco de dados nem de staging em disco, além de `export_reviewed_files` para persistência transacional local dos CSVs revisados e do ledger de estado (`.anki-workflow-export-state.json`) no diretório `exports/` do baralho.

### Invocação via Cliente MCP

Para integrar com clientes compatíveis com o Model Context Protocol (como Claude Desktop, Cursor, Hermes ou extensões do VS Code), registre o servidor indicando o caminho do binário e compulsoriamente o argumento `serve`:

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

> **Aviso Importante — Subcomando `serve` Obrigatório:** O executável Rust é um utilitário CLI multifuncional (`serve`, `--version`, `build-info`, `health`, `oracle`). Se o argumento `serve` for omitido na configuração do cliente MCP, o processo encerrará com código de erro 2 e imprimirá a sintaxe de uso em `stderr`, impedindo a conexão JSON-RPC.

### Desambiguação de Executáveis no PATH

Para eliminar colisões de executáveis no `PATH` entre os ecossistemas Rust e Python, o comando Python foi renomeado para `anki-workflow-mcp-legacy`, reservando `anki-workflow-mcp` ao servidor nativo primário:

| Servidor | Executável no `PATH` | Como iniciar no cliente MCP | Escopo |
| :--- | :--- | :--- | :--- |
| **Primário (Rust)** | `anki-workflow-mcp` | `"command": "anki-workflow-mcp", "args": ["serve"]` | 7 ferramentas: 6 determinísticas em memória (`quality_v1`) + `export_reviewed_files` (exportação CSV transacional local). |
| **Legado (Python)** | `anki-workflow-mcp-legacy` | `"command": "anki-workflow-mcp-legacy"` | 49 ferramentas legadas, staging em disco e `export_final`. |

O núcleo Rust requer MSRV mínimo 1.89 (exigido por dependências como `uuid 1.29.2`, `rmcp 3.10.0` e sintaxe let-chains). O toolchain padrão é fixado em `1.99.1` via `rust-toolchain.toml` (desvio de 1.99.0 alinhado ao canal estável disponível).

---

## 2. Configuração do Orquestrador Python (Legado / Persistência)

Para fluxos de trabalho que necessitam de persistência transacional em disco, manifesto, catálogos e sincronização de mídia local ou exportação final (`export_final`):

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

### Variáveis de Ambiente de Persistência e Mídia

O servidor Python pode ser parametrizado pelas seguintes variáveis de ambiente:

- `ANKI_COLLECTION_MEDIA`: Caminho do diretório de mídia da coleção local do Anki Desktop. Utilizado pelas ferramentas de catalogação e sincronização de imagens (`image_sync`).
- `ANKI_WORKFLOW_HIERARCHY_FILE`: Caminho opcional para o arquivo de definição de hierarquia de baralhos e sub-baralhos.

---

## 3. Julgamento Externo Opcional

As ferramentas de orçamento e triagem podem consultar provedores externos quando variáveis de ambiente com credenciais estiverem configuradas no ambiente de execução do processo MCP. **Essas integrações são estritamente opcionais; as ferramentas determinísticas e o núcleo Rust funcionam 100% offline.**

Caso utilize julgamento externo, injete as variáveis via gerenciador de segredos do seu lançador, nunca em arquivos versionados:

- `TYPESAFE_API_KEY` (opcionalmente `TYPESAFE_MODEL` / `TYPESAFE_SYSTEMONE_URL`).
- `OPENROUTER_API_KEY` (opcionalmente `OPENROUTER_JEV_MODEL` / `OPENROUTER_DECISIONS_URL`).
- `KNOWLINE_API_KEY`, `KNOWLINE_MODEL` e `KNOWLINE_SYSTEMONE_URL` **explícita** (não existe endpoint embutido; o identificador do modelo depende do serviço configurado).

---

## 4. Diretrizes Estritas de Privacidade e Segurança

1. **Sem Leitura de Ambientes Pessoais:** O código do repositório público não lê arquivos `.env` de aplicações pessoais, perfis locais ou chaves privadas.
2. **Dados de Pacientes e Obras Protegidas:** É terminantemente proibido processar, enviar a provedores externos ou versionar dados protegidos de pacientes (PHI), coleções completas de decks comerciais ou obras intelectuais sob direitos autorais restritos.
3. **Isolamento de Stdio:** O transporte stdio reserva `stdout` exclusivamente às mensagens JSON-RPC. Mensagens informativas ou de diagnóstico devem ser direcionadas a `stderr`.
4. **Sem Chaves Versionadas:** Nenhuma credencial ou URL privada deve constar em commits, issues, PRs ou artefatos do repositório.
