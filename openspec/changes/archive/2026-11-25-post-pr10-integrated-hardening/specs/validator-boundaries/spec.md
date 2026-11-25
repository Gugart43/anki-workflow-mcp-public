# validator-boundaries

## Purpose
Corrigir falsos positivos e negativos estruturais preservando texto Unicode e a política HTML5 fail-closed.

## ADDED Requirements

### Requirement: Cloze UTF8
Offsets Cloze SHALL ser bytes UTF-8 válidos, com paridade de escapes e preservação de LaTeX em todos os consumidores.

#### Scenario: Acentos e equação
- **WHEN** um Cloze aparece após acentos e emoji com chaves LaTeX
- **THEN** parse, renumber e revisão preservam texto sem panic

### Requirement: Validação de conteúdo visível
QA e Cloze SHALL aplicar heurísticas ao texto visível, sem tratar preposição em como sigla EM nem portador(a) como múltipla escolha.

#### Scenario: Markup legítimo
- **WHEN** um QA contém HTML seguro e contexto por extenso
- **THEN** a análise equivale ao texto visível

### Requirement: Tabelas e hierarquias
Tabelas SHALL considerar rowspan e aninhamento sem contaminar dimensões externas; tags SHALL rejeitar segmentos vazios.

#### Scenario: Rowspan válido
- **WHEN** uma tabela retangular usa rowspan válido
- **THEN** não ocorre TBL.RAGGED falso

#### Scenario: Segmento vazio
- **WHEN** uma tag contém A::::B
- **THEN** o validador emite diagnóstico
