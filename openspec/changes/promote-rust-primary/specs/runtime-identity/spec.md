# Spec Delta

## Purpose

Permitir ao operador identificar a implementação e versão do executável Rust e a origem declarada do build, sem atribuir garantias de procedência a um número de versão ou metadado não autenticado.

## ADDED Requirements

### Requirement: Identidade coerente da implementação
O executável SHALL disponibilizar versão legível via --version e identidade estruturada via comando build-info. A identidade SHALL informar implementação Rust e versão do pacote coerente com a inicialização MCP.

#### Scenario: Consulta sem servidor
- **WHEN** o operador executa --version ou build-info
- **THEN** o processo termina com sucesso sem iniciar transporte e informa sua versão e implementação

#### Scenario: Identidade do servidor
- **WHEN** um cliente realiza initialize MCP
- **THEN** serverInfo identifica a mesma versão de pacote que a CLI

### Requirement: Origem ausente explícita
A identidade estruturada SHALL distinguir revisão de origem conhecida de desconhecida e estado sujo de limpo ou desconhecido. Um build sem metadados de Git SHALL continuar possível e SHALL NOT inventar um SHA ou estado limpo.

#### Scenario: Build sem repositório
- **WHEN** o executável é compilado sem Git disponível ou sem metadados de origem
- **THEN** build-info indica revisão e estado desconhecidos sem impedir o build

#### Scenario: Build rastreável
- **WHEN** o build dispõe de revisão Git e estado verificáveis
- **THEN** build-info expõe a revisão e informa alterações locais detectadas, sem expor caminhos pessoais
