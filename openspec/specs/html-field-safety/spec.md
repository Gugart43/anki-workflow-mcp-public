# html-field-safety Specification

## Purpose

Estabelece a fronteira de segurança para atributos HTML em campos de flashcards Anki compilados e validados por ambos os motores, preservando a apresentação sem permitir comportamento ativo.

## Requirements

### Requirement: Política mínima de atributos
O sistema SHALL admitir em tags permitidas somente `class`; em `img`, também `src`, `alt`, `width`, `height`; e em `th`/`td`, também `colspan`, `rowspan`. Valores de atributos admitidos SHALL ser reconstruídos/interpretados sem permitir criar atributos adicionais. Estilos inline e eventos SHALL ser proibidos.

#### Scenario: Formatação legítima
- **WHEN** um campo contém `<span class="alerta">A</span>`, `<td colspan="2">B</td>` ou uma figura com imagem local e `alt`
- **THEN** a sanitização preserva os atributos admitidos com valores seguros e a verificação CSV não emite `FMT.ATTR_FORBIDDEN`

#### Scenario: Atributos executáveis
- **WHEN** uma tag permitida contém `onclick`, `onerror`, `onfocus`, `style` ou atributos booleanos arbitrários
- **THEN** a sanitização retira os atributos proibidos e a validação do CSV original emite `FMT.ATTR_FORBIDDEN` com severidade `CRITICAL`

### Requirement: Valores restritos em atributos estruturais
O sistema SHALL aceitar `src` de imagem somente para arquivo local simples sem protocolo/esquema nem travessia de diretório; atributos numéricos de dimensão ou extensão SHALL ser inteiros positivos; atributos com sintaxe ambígua ou duplicada SHALL falhar fechados.

#### Scenario: Imagem com origem perigosa
- **WHEN** `src` contém `javascript:`, `data:`, URL remota, entidade codificada que mascara um esquema ou travessia de diretório
- **THEN** a sanitização não preserva esse `src` e a validação do CSV original emite `FMT.ATTR_FORBIDDEN` crítico

#### Scenario: Atributos malformados
- **WHEN** uma tag permitida possui barra como separador, atributo sem valor, aspas quebradas ou nome duplicado
- **THEN** o parser não preserva o atributo problemático e a validação não aprova o campo como seguro

### Requirement: Neutralização e diagnóstico sem adulteração de CSV bruto
O sistema SHALL retornar HTML neutralizado ao sanitizar drafts e SHALL inspecionar o texto original ao validar CSV. `check_csv` SHALL emitir diagnóstico crítico para atributo proibido sem alterar o CSV fornecido.

#### Scenario: CSV externo com handler
- **WHEN** `check_csv` recebe um cartão com `<b onclick="alert(1)">x</b>`
- **THEN** retorna `FMT.ATTR_FORBIDDEN` crítico associado ao campo/linha, sem reescrever a entrada

#### Scenario: Draft compilado
- **WHEN** um draft contém tag permitida com atributo executável
- **THEN** o CSV gerado pelo compilador não contém o atributo executável e as correções são rastreáveis

### Requirement: Preservação de conteúdo inerte
O sistema SHALL manter entidades HTML já estruturadas e preservar inequações clínicas como texto seguro, sem interpretá-las como tags ou produzir atributos ativos.

#### Scenario: Inequação e entidade
- **WHEN** o campo contém `p < 0.05`, `Na > 135` ou `&amp;`
- **THEN** a sanitização preserva o sentido textual com escape sem duplo escape e a checagem não confunde a inequação com uma tag HTML

### Requirement: Paridade de fronteira Python e Rust
Os motores Python e Rust SHALL tomar a mesma decisão de permitir, neutralizar ou rejeitar atributos nos caminhos equivalentes de sanitização e validação CSV.

#### Scenario: Casos de regressão diferenciais
- **WHEN** entradas sintéticas comuns são processadas por ambos os motores
- **THEN** os dois retornam a mesma saída sanitizada, fixes e diagnósticos serializados para os casos cobertos
