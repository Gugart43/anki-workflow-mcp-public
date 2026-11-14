# Configuração e privacidade

## Sem credenciais

Validações determinísticas, compilação, manifesto e operações de mídia não exigem conta de provedor. Defina destinos explicitamente e use apenas diretórios que o processo deve acessar.

- `ANKI_COLLECTION_MEDIA`: destino de mídia. Inspecione o dry-run antes de aplicar.
- `ANKI_WORKFLOW_HIERARCHY_FILE`: caminho opcional do arquivo de hierarquia.

## Julgamento externo opcional

As ferramentas de orçamento podem usar provedores externos quando há credenciais. Configure variáveis no ambiente do processo MCP usando seu gerenciador de segredos, nunca no repositório:

- `TYPESAFE_API_KEY` e opcionalmente `TYPESAFE_MODEL` / `TYPESAFE_SYSTEMONE_URL`.
- `OPENROUTER_API_KEY` e opcionalmente `OPENROUTER_JEV_MODEL` / `OPENROUTER_DECISIONS_URL`.
- `KNOWLINE_API_KEY`, `KNOWLINE_MODEL` e `KNOWLINE_SYSTEMONE_URL` **explícita**; não existe endpoint pessoal embutido. O identificador de modelo depende do serviço configurado; não é normalizado automaticamente.

O código público não lê `.env` de aplicativos, arquivos do perfil pessoal ou `.env_jev`. Se seu lançador carregar um arquivo de segredos, faça isso fora do repositório e forneça apenas as variáveis necessárias. Não envie dados pessoais, pacientes ou conteúdo sem permissão aos provedores. Os endpoints padrão dos provedores públicos são os serviços documentados no código; overrides são de responsabilidade do operador.
