# Runtime identity delta
## ADDED Requirements
### Requirement: CLI previsível
A CLI SHALL retornar ajuda com sucesso, rejeitar argumentos extras com código 2 e iniciar runtime assíncrono somente em serve.
#### Scenario: Ajuda
- **WHEN** o operador executa help ou --help
- **THEN** recebe usage em stdout com código zero
#### Scenario: Argumento excedente
- **WHEN** o operador executa health extra
- **THEN** recebe erro com código 2 sem iniciar servidor
