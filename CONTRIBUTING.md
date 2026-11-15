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

## Gates Obrigatórios Pré-Commit

Todas as verificações abaixo são executadas na Integração Contínua (CI) em ambientes Linux e Windows e devem passar sem falhas localmente:

### 1. Gates de Rust

```bash
# 1. Formatação de código
cargo fmt --all -- --check

# 2. Análise estática com Clippy (sem warnings tolerados)
cargo clippy --all-targets --all-features -- -D warnings

# 3. Testes unitários e de integração em todo o workspace
cargo test --workspace --all-features

# 4. Build de release com lockfile congelado
cargo build --release --locked -p anki-workflow-mcp
```

### 2. Gates de Python e Paridade Diferencial

```bash
# 1. Sincronização do ambiente isolado
uv sync --extra dev --frozen

# 2. Verificação de estilo e linting
uv run ruff check src tests
uv run ruff format --check src tests

# 3. Testes unitários com cobertura mínima exigida de 70%
uv run pytest --cov=anki_mcp --cov-fail-under=70

# 4. Checagem estática de tipos
uv run --with mypy mypy src --ignore-missing-imports

# 5. Suíte diferencial de equivalência matemática/comportamental
uv run pytest tests/differential/ -v
```

---

## Diretrizes de Qualidade e Contratos

- **Fronteira Verídica de Capacidades:** O servidor Rust stdio expõe exatamente **6 ferramentas puramente em memória** (`cards_compile_drafts`, `review_batch_information`, `quality_prepare`, `quality_evaluate`, `batch_patch_draft` e `generation_contract_get`), introduzindo capacidades exclusivas de compilação e avaliação em memória. O servidor Python disponibiliza seu catálogo de 49 ferramentas e retém com exclusividade as operações com estado em disco (manifesto, staging, ledger de IDs, sincronização de mídia e exportação final). As ferramentas Rust não são um subconjunto do catálogo Python. Não afirme paridade total para funcionalidades com estado não migradas.
- **Preservação de Compatibilidade:** O cálculo canônico de recibo `compute_receipt_v1`, os delimitadores de linha (`split_row`), os campos AnkiMBR e as convenções de CSV devem manter paridade absoluta bit a bit com os fixtures golden. Mudanças de contrato exigem revisão explícita.
- **Limites Epistemológicos:** O gate `quality_v1` valida consistência de fontes, cobertura de obrigações e estrutura em memória. Em todas as avaliações, `export_authorized` e `clinical_correctness_certified` permanecem estritamente `false`. Não afirme validação médica, semântica ou eficácia pedagógica a partir de métricas automatizadas.
- **Atomicidade e HTML:** O comprimento das notas adicionais isoladamente não deve ser classificado como falha crítica de atomicidade quando a cobrança for unívoca. Preserve tabelas e figuras nas notas (`tabelas_notas`, `figuras_notas`) com HTML seguro (`<figure>`, `<figcaption>`).
- **Segurança Fail-Closed de HTML vs. Formatadores:** O compilador Rust (`compile_drafts`) e o gate de qualidade rejeitam ativamente atributos/eventos ativos (`onclick`, `onload`, `onerror`), estilos com `url(...)`/`expression(...)` e URIs remotas ou inline data (`src="http..."`, `src="data:..."`), emitindo `QUALITY.UNSAFE_HTML` (`Critical`). Os helpers legados `sanitize_field` e `render_media_figure` são apenas formatadores/escapadores de compatibilidade de strings, **NÃO** sanitizadores de segurança.

---

## Segurança e Privacidade

- **Dados Sintéticos Exclusivos:** Testes e fixtures devem utilizar dados puramente fictícios ou sintéticos. É terminantemente proibido incluir dados reais de pacientes, coleções de estudo pessoais, notas privadas ou trechos de obras sob direitos restritos.
- **Ausência de Credenciais:** Nunca versione chaves de API, tokens, senhas ou URLs privadas. Testes devem ser estritamente offline e não devem depender de conectividade com serviços externos.
- **Isolamento de Ambiente:** O código não deve ler arquivos de configuração fora do diretório do projeto nem assumir caminhos absolutos atrelados a um usuário ou máquina específica.
