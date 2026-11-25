# legacy-sunset

## Purpose
Permitir migração operacional verificável ao executável Rust sem apagar prematuramente a referência de compatibilidade Python.

## ADDED Requirements

### Requirement: Legado explícito
A documentação e o executável Python SHALL declarar depreciação e encaminhar o fluxo padrão para Rust, mantendo execução de compatibilidade opt-in.

#### Scenario: Entrada legada
- **WHEN** o operador inicia o executável Python legado
- **THEN** recebe aviso em stderr, nunca banner em stdout MCP

### Requirement: Retirada condicionada
O legado SHALL NOT ser removido antes de verificação de paridade e ciclo CI Linux/Windows comprovados.

#### Scenario: CI ainda não executado
- **WHEN** somente testes locais estão disponíveis
- **THEN** o código de referência permanece e a pendência externa é documentada
