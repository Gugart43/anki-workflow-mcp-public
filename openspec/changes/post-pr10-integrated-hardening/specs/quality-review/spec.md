# Quality review delta
## MODIFIED Requirements
### Requirement: Contrato MCP puro e estrito
O servidor SHALL expor seis ferramentas puras em memória e uma exportação local explícita, rejeitar argumentos desconhecidos e reservar stdout ao protocolo JSON-RPC. batch_patch_draft SHALL retornar um draft alterado sem gravar staging Python.

#### Scenario: Argumento desconhecido
- **WHEN** uma chamada MCP inclui um parâmetro não declarado
- **THEN** a chamada retorna erro sem executar mutação

#### Scenario: Lifecycle stdio
- **WHEN** um cliente inicializa, lista ferramentas, compila, prepara, avalia e encerra a sessão
- **THEN** recebe respostas JSON-RPC válidas e sete ferramentas, sem banners em stdout
## ADDED Requirements
### Requirement: Snapshot canônico
O snapshot SHALL ignorar permutações equivalentes de fontes, obrigações e vínculos sem ignorar mudanças semânticas de conteúdo.
#### Scenario: Permutação equivalente
- **WHEN** arrays semanticamente equivalentes são reordenados
- **THEN** o snapshot permanece idêntico
### Requirement: Quick não é certificação editorial
O modo quick SHALL executar validação estrutural e HTML sem alegar revisão estrita, autorização de exportação transacional ou certificação clínica. Strict SHALL ser o padrão.
#### Scenario: Modo omitido
- **WHEN** o chamador não informa mode
- **THEN** aplicam-se todas as exigências editoriais strict
