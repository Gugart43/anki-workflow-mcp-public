# Guia de Contribuição

Agradecemos o interesse em contribuir com o `anki-workflow-mcp`. Para manter a integridade, reprodutibilidade e segurança do projeto, todas as contribuições devem seguir as diretrizes abaixo.

---

## Fluxo de Trabalho

1. Crie uma branch específica para sua alteração a partir da branch principal (`main`).
2. Mantenha as alterações pequenas, modulares e focadas em um único objetivo.
3. Toda modificação funcional ou correção de bug deve incluir testes de regressão reproduzíveis com dados sintéticos.
4. Antes de abrir um Pull Request, execute obrigatoriamente a totalidade dos gates de verificação de Rust e Python.
5. Não faça force-push na branch principal e nunca envie artefatos gerados ou diretórios de runtime (`target/`, `.venv/`, `.mypy_cache/`, etc.).

---

## Setup local

Pré-requisitos com as versões efetivamente usadas pelo CI (`.github/workflows/ci.yml`):

| Ferramenta | Versão | Fonte da verdade |
| --- | --- | --- |
| Rust (toolchain) | `1.99.1` + componentes `rustfmt` e `clippy` | `rust-toolchain.toml`; o CI lê o `channel` do próprio arquivo antes de instalar |
| MSRV declarada | `1.89` (mínimo real, não o toolchain de desenvolvimento) | `[workspace.package] rust-version` em `Cargo.toml` |
| Python | `>=3.12,<3.15` | `requires-python` em `pyproject.toml` |
| uv | última release (instalada por `astral-sh/setup-uv`); `uv.lock` validado por `uv lock --check` e `--locked` | `.github/workflows/ci.yml` |
| Node.js | `24` | `.nvmrc` e `node-version: "24"` no CI |
| pnpm | `10.26.0` | `pnpm/action-setup` no CI |
| just | opcional (apenas atalhos locais) | `justfile` |

O `.npmrc` na raiz define `engine-strict=true`: instalações npm falham quando `engines.node` do pacote não é satisfeito (use Node 24, conforme `.nvmrc`).

```bash
just setup
```

## Validação pré-PR

Antes de abrir um PR, execute o gate unificado (espelha exatamente `.github/workflows/ci.yml`):

```bash
just check
```

Receitas individuais disponíveis no `justfile`:
- `just rust-check` — `cargo fmt --check`, `clippy -D warnings` e `cargo test --workspace --locked`
- `just py-check` — `ruff check`, `mypy src` e `pytest --ignore=tests/differential`
- `just parity` — compila o binário Rust (`--locked --features oracle`) e executa `tests/differential/`
- `just contracts-check` — detecta drift nos contratos legados sem falsos positivos em `server_meta.json`
- `just build-release` — gera o binário otimizado `target/release/anki-workflow-mcp`
- `just fmt` — formata tanto Rust quanto Python

---

## Diretrizes de Qualidade e Contratos

- **Fronteira Verídica de Capacidades:** O servidor Rust stdio expõe exatamente **7 ferramentas** (`cards_compile_drafts`, `review_batch_information`, `quality_prepare`, `quality_evaluate`, `batch_patch_draft`, `generation_contract_get` e `export_reviewed_files`): seis operações puramente em memória mais `export_reviewed_files` para persistência local explícita, introduzindo capacidades exclusivas de compilação e avaliação em memória. O servidor Python disponibiliza seu catálogo de 49 ferramentas e retém com exclusividade as demais operações com estado em disco (manifesto, staging, ledger de IDs e sincronização de mídia). As ferramentas Rust não são um subconjunto do catálogo Python. Não afirme paridade total para funcionalidades com estado não migradas.
- **Preservação de Compatibilidade:** O cálculo canônico de recibo `compute_receipt_v1`, os delimitadores de linha (`split_row`), os campos AnkiMBR e as convenções de CSV devem manter paridade absoluta bit a bit com os fixtures golden. Mudanças de contrato exigem revisão explícita.
- **Limites Epistemológicos:** O gate `quality_v1` valida consistência de fontes, cobertura de obrigações e estrutura em memória. Em todas as avaliações, `export_authorized` e `clinical_correctness_certified` permanecem estritamente `false`. Não afirme validação médica, semântica ou eficácia pedagógica a partir de métricas automatizadas.
- **Atomicidade e HTML:** O comprimento das notas adicionais isoladamente não deve ser classificado como falha crítica de atomicidade quando a cobrança for unívoca. Preserve tabelas e figuras nas notas (`tabelas_notas`, `figuras_notas`) com HTML seguro (`<figure>`, `<figcaption>`).
- **Segurança Fail-Closed de HTML vs. Formatadores:** O compilador Rust (`compile_drafts`) e o gate de qualidade rejeitam ativamente atributos/eventos ativos (`onclick`, `onload`, `onerror`), estilos com `url(...)`/`expression(...)` e URIs remotas ou inline data (`src="http..."`, `src="data:..."`), emitindo `QUALITY.UNSAFE_HTML` (`Critical`). Os helpers legados `sanitize_field` e `render_media_figure` são apenas formatadores/escapadores de compatibilidade de strings, **NÃO** sanitizadores de segurança.

---

## Segurança e Privacidade

- **Dados Sintéticos Exclusivos:** Testes e fixtures devem utilizar dados puramente fictícios ou sintéticos. É terminantemente proibido incluir dados reais de pacientes, coleções de estudo pessoais, notas privadas ou trechos de obras sob direitos restritos.
- **Ausência de Credenciais:** Nunca versione chaves de API, tokens, senhas ou URLs privadas. Testes devem ser estritamente offline e não devem depender de conectividade com serviços externos.
- **Isolamento de Ambiente:** O código não deve ler arquivos de configuração fora do diretório do projeto nem assumir caminhos absolutos atrelados a um usuário ou máquina específica.
