# MCP input contract
## Purpose
Definir schemas e erros coerentes com o runtime de ferramentas Rust, preservando separação entre preparação e avaliação editorial.
## ADDED Requirements
### Requirement: Contrato de drafts explícito
O servidor SHALL expor schema completo dos drafts aceitos e rejeitar tipo desconhecido em vez de convertê-lo silenciosamente em QA.
#### Scenario: Tipo digitado incorretamente
- **WHEN** cards_compile_drafts recebe tipo CLOZ
- **THEN** retorna erro explícito e não compila cartão QA
### Requirement: Preparação não recebe revisões
O servidor SHALL rejeitar reviews em quality_prepare e aceitar revisões somente em quality_evaluate.
#### Scenario: Revisões prematuras
- **WHEN** quality_prepare recebe reviews
- **THEN** retorna erro de argumento desconhecido
### Requirement: Violações completas
O servidor SHALL devolver erro estruturado com código e todas as violações de range identificadas, sem truncar informações no limite antigo de 400 caracteres.
#### Scenario: Múltiplos ranges inválidos
- **WHEN** bindings e reviews incluem linhas e índices inválidos
- **THEN** cada campo inválido aparece em violations com caminho e mensagem
