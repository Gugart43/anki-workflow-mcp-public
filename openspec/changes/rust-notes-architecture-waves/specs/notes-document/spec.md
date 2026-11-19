# Notes document
## Purpose
Definir notas adicionais estruturadas e políticas configuráveis sem confundir extensão textual com atomicidade da recuperação.
## ADDED Requirements
### Requirement: Documento semântico e compilação unificada
O sistema SHALL aceitar blocos tipados, fontes e modos legado/v2, preservar o conteúdo aceito e renderizar HTML seguro em linha única sem separadores br entre blocos nem pipes literais.
#### Scenario: Documento com tabela fluxo e figura
- **WHEN** um draft contém documento válido com tabela, fluxo e figura local
- **THEN** a compilação preserva os blocos, legenda e fontes e mantém as colunas CSV canônicas
### Requirement: Invariantes estritas e erros sem perda silenciosa
O sistema SHALL rejeitar ciclos, órfãos, referências inválidas e payloads v2 inválidos. O modo estrito SHALL rejeitar campos legados.
#### Scenario: Fluxo cíclico
- **WHEN** um documento contém um ciclo
- **THEN** a compilação retorna erro explícito sem renderizar documento parcial
### Requirement: Políticas declarativas e métricas
O sistema SHALL fornecer perfis med_br e general, carregar perfil JSON validado, contar texto visível e separar prosa, listas, tabelas, legendas e citações.
#### Scenario: Perfil neutro
- **WHEN** notas citam apenas banca brasileira e o perfil selecionado é general
- **THEN** não há aviso específico de banca, preservando demais regras habilitadas
