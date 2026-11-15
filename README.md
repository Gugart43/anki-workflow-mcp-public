# anki-workflow-mcp

[![CI](https://github.com/Gugart43/anki-workflow-mcp-public/actions/workflows/ci.yml/badge.svg)](https://github.com/Gugart43/anki-workflow-mcp-public/actions/workflows/ci.yml)

Camada de validação, compilação e controle de qualidade para geração de flashcards Anki: **o LLM escreve; o servidor verifica estrutura, avalia fidelidade e registra**. Compatível com notas QA e Cloze e as convenções de apresentação estrutural AnkiMBR. Templates visuais, arquivos de mídia e coleções pessoais não são distribuídos neste repositório.

O projeto opera sob uma arquitetura dual:
1. **Núcleo Primário Nativo (Rust):** Servidor MCP stdio leve, determinístico e de alta performance, executando estritamente **6 ferramentas puras em memória** para compilação de rascunhos, perfil informativo de lote, avaliação de qualidade com evidência textual (`quality_v1`) e consulta de contratos. As ferramentas Rust não são um subconjunto das ferramentas Python: introduzem capacidades novas e exclusivas de controle de qualidade e compilação em memória.
2. **Orquestrador de Persistência e Mídia (Python Legado):** Servidor FastMCP que disponibiliza 49 ferramentas (validações, mecânicas, hierarquia, corpus, além das ferramentas com estado em disco: manifesto transacional, staging em arquivos, ledger de IDs, gerenciamento de mídia física e exportação final). O núcleo Rust não é um substituto completo para os fluxos que requerem escrita persistente em disco.

---

## Núcleo Rust Primário

O binário nativo `anki-workflow-mcp` disponibiliza o transporte MCP via stdio e expõe exatamente 6 ferramentas estritamente em memória:

| Ferramenta MCP | Descrição e Comportamento |
|---|---|
| `cards_compile_drafts` | Normaliza e compila rascunhos estruturados QA/Cloze em memória para `qa_csv` e `cloze_csv` canônicos, com diagnóstico estrutural e preservação de HTML seguro em `tabelas_notas` e `figuras_notas`. |
| `review_batch_information` | Inspeciona `qa_csv` e `cloze_csv` em memória e retorna perfis de contagem de tokens, caracteres e recuperações individuais, sem teto fixo de 20 cartões. |
| `quality_prepare` | Prepara snapshot determinístico imutável (`snapshot_hash`) e inventário por recuperação a partir dos CSVs compilados, fontes, obrigações e vínculos de evidência. |
| `quality_evaluate` | Avalia fidelidade à fonte literal, cobertura de obrigações, consistência de evidências e revisões editoriais independentes por QA e por índice Cloze contra o snapshot. |
| `batch_patch_draft` | Aplica patch pontual em rascunho de cartão puramente em memória (`{draft, updates}` $\to$ `{card}`). Não lê nem grava arquivos de staging em disco. |
| `generation_contract_get` | Retorna o contrato normativo do workflow `quality_v1` (regras de compilação, severidade de diagnósticos, mídia e critérios de aceitação). |

### Limites Epistemológicos e de Governança
- **Avaliação por recuperação:** Cada par QA e cada índice Cloze ($c_1, c_2, \dots$) constitui uma recuperação atômica inspecionada individualmente.
- **Evidência literal vs. Verdade clínica:** A correspondência textual com um trecho de fonte demonstra apenas fidelidade à fonte informada. Atestações de revisores editoriais são declarações declarativas não autenticadas.
- **Autorização restrita:** Em qualquer veredicto (inclusive `PASS`), `export_authorized` e `clinical_correctness_certified` permanecem sempre `false`. O motor valida consistência técnica e conformidade a contratos, sem substituir a responsabilidade editorial e clínica humana.
- **Atomicidade e notas:** O comprimento de `Notas Adicionais` isoladamente não acarreta falha crítica de atomicidade, desde que a cobrança focal permaneça unívoca e as notas preservem HTML seguro.
- **Segurança de HTML vs. Formatadores Legados:** O compilador Rust (`compile_drafts`) rejeita ativamente tags com atributos/eventos ativos (`onclick`, `onload`, `onerror`), estilos com `url(...)` ou `expression(...)`, e URIs remotas ou inline data (`src="http..."`, `src="data:..."`). O gate de qualidade classifica violações com o código `QUALITY.UNSAFE_HTML` (severidade `Critical`). Os helpers legados `sanitize_field` e `render_media_figure` são apenas formatadores/escapadores de compatibilidade, **NÃO** sanitizadores de segurança; o bloqueio fail-closed de conteúdo ativo é garantido pelo compilador e pelo gate `quality_v1`.

---

## Compilação e Instalação

### 1. Binário Nativo Rust (Recomendado)

Requer toolchain Rust estável atual (validado localmente com Rust 1.99; CI usa stable):

```bash
# Compilar binário de release otimizado
cargo build --release --locked -p anki-workflow-mcp
```

O binário resultante estará em:
- **Linux/macOS:** `target/release/anki-workflow-mcp`
- **Windows:** `target/release/anki-workflow-mcp.exe`

#### Instalação no Sistema e Advertência de Colisão

Você pode instalar o binário localmente usando o Cargo:

```bash
cargo install --path apps/anki-workflow-mcp --locked --root <destino>
```

> **Atenção — Risco de Colisão de Executáveis:** O pacote Python legado também define um executável denominado `anki-workflow-mcp` via `[project.scripts]`. Se ambos forem instalados no mesmo diretório ou se diretórios do Python e do Cargo coexistirem na variável `PATH`, haverá colisão de nomes (*shadowing*). Recomenda-se apontar o caminho absoluto do binário na configuração do cliente MCP ou utilizar `--root` dedicado.

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

#### Comandos de Inspeção CLI

O executável oferece comandos diretos de linha de comando:
- `anki-workflow-mcp --version`: exibe o nome e a versão do pacote (atualmente `0.1.0`, correspondente à versão do pacote Cargo, não uma identidade de release).
- `anki-workflow-mcp build-info`: emite JSON estruturado com implementação, versão do pacote, `target`, revisão de origem e estado da árvore Git; caso compilado fora de um repositório Git, reporta revisão e estado desconhecidos. Os metadados não são uma assinatura de procedência.
- `anki-workflow-mcp health`: emite JSON com status de integridade do runtime.

---

### 2. Orquestrador Python Legado (Operações em Disco)

Para fluxos de trabalho que necessitam de persistência transacional em disco, manifesto (`manifesto_init`, `manifesto_commit`), staging em arquivo, catálogos e sincronização de mídia local ou exportação final (`export_final`), execute o servidor Python:

Requer Python 3.12–3.14 (CI em Python 3.13) e [uv](https://docs.astral.sh/uv/):

```bash
# Execução direta via módulo Python (sem ambiguidade com o binário Rust)
uv run python -m anki_mcp.server
```

#### Configuração do Cliente MCP (Python Legado)

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

O repositório impõe verificações estritas e automatizadas para os dois ecossistemas em Linux e Windows.

### Gates do Rust

```bash
# Formatação
cargo fmt --all -- --check

# Linter com avisos como erro
cargo clippy --all-targets --all-features -- -D warnings

# Suíte completa de testes unitários e de integração
cargo test --workspace --all-features

# Verificação de build com lockfile estrito
cargo build --locked -p anki-workflow-mcp
```

### Gates do Python e Paridade Diferencial

```bash
# Sincronizar ambiente de desenvolvimento
uv sync --extra dev --frozen

# Linters e formatação Python
uv run ruff check src tests
uv run ruff format --check src tests

# Testes com cobertura mínima de 70%
uv run pytest --cov=anki_mcp --cov-fail-under=70

# Verificação estática de tipos
uv run --with mypy mypy src --ignore-missing-imports

# Suíte diferencial de paridade bit a bit (Python vs Rust)
uv run pytest tests/differential/ -v
```

---

## Estrutura do Repositório

```text
apps/
  anki-workflow-mcp/    Binário executável do servidor MCP Rust (stdio) e oráculo
crates/
  anki-domain/          Tipos de domínio fundamentais, diagnósticos e recibo v1
  anki-parser/          Parsing de CSV (split_row) e contagem de cartões
  anki-validate/        Validações de regras, compilador de drafts e motor quality_v1
src/anki_mcp/           Servidor Python legado (FastMCP), persistência, manifesto e mídia
tests/
  differential/         Suíte de testes de paridade 1:1 entre Rust e Python
  golden/               Fixtures canônicos de teste para verificação determinística
contracts/legacy/       Contratos JSON congelados da especificação MCP v1
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
