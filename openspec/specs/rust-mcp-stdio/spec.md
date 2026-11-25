# rust-mcp-stdio Specification

## Purpose

Permitir que clientes MCP acessem, pela entrada e saída padrão do binário Rust, operações determinísticas de Anki sem alterar os fluxos transacionais do servidor Python.

## Requirements

### Requirement: Transporte MCP explícito
O comando `serve` SHALL aceitar mensagens JSON-RPC do protocolo MCP por stdio e SHALL reservar stdout para mensagens de protocolo. O modo `oracle` SHALL permanecer disponível e separado.

#### Scenario: Inicialização
- **WHEN** um cliente inicia `serve` e envia `initialize` seguido da notificação `initialized`
- **THEN** o processo responde com informações de protocolo/capacidades válidas e continua apto a receber requisições.

#### Scenario: Sem ruído na saída
- **WHEN** o processo recebe uma sequência de requisições válida
- **THEN** cada linha de stdout é uma mensagem JSON-RPC de resposta, sem banner ou logs.

### Requirement: Ferramentas determinísticas em memória
O servidor SHALL anunciar e executar `review_batch_information`, `cards_compile_drafts` e `check_csv` com argumentos JSON explícitos. Os resultados SHALL derivar das funções Rust existentes, sem gravar arquivos ou acessar coleções Anki.

#### Scenario: Descoberta e chamada
- **WHEN** o cliente pede `tools/list` e chama uma das três ferramentas com argumentos válidos
- **THEN** a ferramenta consta na lista e devolve resultado compatível com a operação Rust já testada pelo oráculo.

#### Scenario: Falha de entrada controlada
- **WHEN** o cliente chama ferramenta desconhecida ou passa argumentos ausentes, de tipo incorreto ou não interpretáveis
- **THEN** o servidor devolve erro de protocolo ou da ferramenta sem encerrar nem processar como resultado válido.

### Requirement: Delimitação segura de capacidades
O servidor Rust SHALL distinguir explicitamente sua superfície experimental do servidor Python; SHALL preservar operações transacionais, manifesto, exportação e mídia fora do alcance de `serve`.

#### Scenario: Operação somente em memória
- **WHEN** o cliente consulta a lista de ferramentas do binário Rust
- **THEN** ferramentas de escrita transacional ou sincronização de mídia não aparecem na lista.

### Requirement: Compatibilidade e encerramento
O servidor SHALL preservar `version`, `health` e `oracle` e SHALL terminar de forma limpa quando o cliente fechar stdin, sem emitir resposta fictícia.

#### Scenario: EOF de stdio
- **WHEN** o cliente fecha stdin após uma ou mais chamadas
- **THEN** o processo encerra sem imprimir logs ou dados de aplicação em stdout.
