# contract-distribution

## Purpose
Distribuir contratos derivados do núcleo Rust e validar interoperabilidade externa sem runtime Node no servidor.

## ADDED Requirements

### Requirement: Geração determinística
Contratos JSON Schema e declarações TypeScript SHALL derivar dos tipos Rust e ser idênticos byte a byte em duas gerações da mesma origem.

#### Scenario: Regeração
- **WHEN** o gerador executa duas vezes com o mesmo código
- **THEN** nenhum arquivo de contrato sofre drift

### Requirement: Launcher íntegro sem scripts
O launcher SHALL selecionar pacote opcional exato por plataforma, validar SHA-256 e delegar stdio sem downloads runtime nem postinstall.

#### Scenario: Integridade corrompida
- **WHEN** o binário ou seu digest não corresponde ao manifesto
- **THEN** o launcher falha com stdout vazio

#### Scenario: Override explícito
- **WHEN** ANKI_WORKFLOW_MCP_BIN aponta para executável local válido
- **THEN** o launcher executa esse caminho sem consultar registro

### Requirement: Interoperabilidade MCP
O harness SHALL usar cliente oficial real e testar handshake, schemas, erros, anotações e encerramento com processo real.

#### Scenario: Sessão completa
- **WHEN** o harness conecta ao servidor compilado
- **THEN** as sete ferramentas e seus contratos são validados e o processo encerra
