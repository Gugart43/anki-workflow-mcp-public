# quality-review Specification

## Purpose
Definir o contrato observável da revisão de lotes QA/Cloze com evidência textual e revisões editoriais declaradas, sem confundir aprovação técnica com certificação clínica ou publicação.

## Requirements

### Requirement: Cobertura individual de recuperações
O sistema SHALL inventariar cada QA e cada índice Cloze separadamente, sem teto numérico de vinte cartões no fluxo quality_v1. Evidência e revisão obrigatórias ausentes SHALL impedir PASS.

#### Scenario: Cloze com duas recuperações
- **WHEN** um lote contém c1 e c2, mas apenas c1 tem vínculo e revisão
- **THEN** o inventário contém duas recuperações e a avaliação não retorna PASS

#### Scenario: Lote acima do teto legado
- **WHEN** um lote estruturalmente válido contém mais de vinte recuperações
- **THEN** a revisão não o rejeita apenas pela contagem

### Requirement: Snapshot vincula conteúdo e contexto
O sistema SHALL vincular revisões ao snapshot dos CSVs, fontes, obrigações e vínculos. Alteração de qualquer desses elementos SHALL invalidar revisões do snapshot anterior.

#### Scenario: Revisão obsoleta
- **WHEN** um CSV ou trecho de fonte muda após a preparação
- **THEN** a avaliação rejeita a revisão com o hash anterior

### Requirement: Evidência literal e limitações de confiança
O sistema SHALL verificar referência de fonte, trecho literal e cobertura das obrigações; SHALL declarar revisores como atestações não autenticadas e manter export_authorized e clinical_correctness_certified falsos em todos os veredictos.

#### Scenario: Trecho inexistente
- **WHEN** um vínculo cita texto ausente na fonte fornecida
- **THEN** a avaliação não retorna PASS

#### Scenario: Aprovação técnica completa
- **WHEN** estrutura, evidência e revisões declaradas satisfazem o gate
- **THEN** PASS mantém ambas as autorizações falsas e não grava estado ou mídia

### Requirement: Conteúdo adicional seguro e preservado
O sistema SHALL preservar tabelas e figuras estruturadas nas notas durante normalização e compilação, com HTML seguro e legendas descritivas. Comprimento das notas isoladamente SHALL NOT causar falha de atomicidade.

#### Scenario: Tabelas e figuras nas notas
- **WHEN** drafts válidos contêm tabelas_notas e figuras_notas
- **THEN** o CSV compilado mantém conteúdo seguro dessas extensões e respeita as colunas AnkiMBR

#### Scenario: Atributos ativos e mídia remota
- **WHEN** um draft ou CSV contém eventos HTML, CSS ativo, atributos não permitidos ou src de mídia remoto/embutido
- **THEN** o compilador rejeita o draft ou o gate retorna FAIL com QUALITY.UNSAFE_HTML, sem modificar o oráculo legado

#### Scenario: Notas explicativas extensas
- **WHEN** uma resposta focada possui notas longas e seguras
- **THEN** extensão das notas não é classificada como violação crítica de atomicidade

### Requirement: Contrato MCP puro e estrito
O servidor SHALL expor seis ferramentas puras em memória e uma exportação local explícita, rejeitar argumentos desconhecidos e reservar stdout ao protocolo JSON-RPC. batch_patch_draft SHALL retornar um draft alterado sem gravar staging Python.

#### Scenario: Argumento desconhecido
- **WHEN** uma chamada MCP inclui um parâmetro não declarado
- **THEN** a chamada retorna erro sem executar mutação

#### Scenario: Lifecycle stdio
- **WHEN** um cliente inicializa, lista ferramentas, compila, prepara, avalia e encerra a sessão
- **THEN** recebe respostas JSON-RPC válidas e sete ferramentas, sem banners em stdout

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
