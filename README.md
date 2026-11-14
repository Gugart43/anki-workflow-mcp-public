# anki-workflow-mcp

[![CI](https://github.com/Gugart43/anki-workflow-mcp-public/actions/workflows/ci.yml/badge.svg)](https://github.com/Gugart43/anki-workflow-mcp-public/actions/workflows/ci.yml)

Servidor MCP para validar, compilar e registrar flashcards Anki: **o LLM escreve; o servidor verifica estrutura, conta e registra**. Compatível com os campos QA/Cloze e as convenções de apresentação AnkiMBR; templates, mídia e coleções não são distribuídos aqui.

## Recursos

- Validação CSV, Cloze, hints, QA, notas, imagens, tabelas, taxonomia e referências cruzadas.
- Manifesto transacional, registro de IDs, recibos de conteúdo e exportação com locks e recuperação de falhas.
- Catálogos de mídia, namespacing e sincronização para destino explicitamente configurado.
- Perfis descritivos que separam contexto, recuperação obrigatória e explicação opcional; não certificam correção clínica ou qualidade pedagógica.
- Triagem determinística e integração opcional com provedores externos, somente quando configurados.

## Instalação

Requer Python 3.12–3.14; CI em Python 3.13. Instale o [uv](https://docs.astral.sh/uv/) e execute:

```bash
uv tool install --python 3.13 "git+https://github.com/Gugart43/anki-workflow-mcp-public.git"
anki-workflow-mcp
```

O comando inicia o transporte stdio; configure seu cliente MCP para executar `anki-workflow-mcp`. Evite mensagens próprias em stdout, reservado ao protocolo. A branch pode evoluir; para instalação reproduzível, fixe um SHA revisado em vez da branch.

## Desenvolvimento

```bash
git clone https://github.com/Gugart43/anki-workflow-mcp-public.git
cd anki-workflow-mcp-public
uv sync --extra dev --frozen
uv run ruff check src tests
uv run ruff format --check src tests
uv run pytest --cov=anki_mcp --cov-fail-under=70
uv run --with mypy mypy src --ignore-missing-imports
```

## Organização

```text
.github/workflows/   CI Linux e Windows
src/anki_mcp/        servidor, modelos, checks, estado e ferramentas MCP
tests/               testes de regressão, sem datasets ou coleções pessoais
docs/                arquitetura, segurança e limites dos perfis
```

- [Arquitetura](docs/architecture.md)
- [Configuração e privacidade](docs/configuration.md)
- [Quantidade de informação](docs/information-quality.md)
- [Contribuição](CONTRIBUTING.md) · [Segurança](SECURITY.md) · [Licença MIT](LICENSE)

## Escopo público

Esta distribuição começa com histórico novo. Não inclui estudos privados, corpus, exports, logs, backups, templates extraídos, dados de Anki ou configurações de infraestrutura. Exemplos/testes são demonstrações técnicas, não recomendações médicas. Uma revisão de segurança reduz riscos, mas não oferece garantia absoluta. Não use dados de pacientes para testes ou envie conteúdo confidencial a provedores externos.
