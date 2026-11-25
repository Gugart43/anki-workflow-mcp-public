# Apresentação Temática e Camada Visual — Wave 2 (AnkiMBR Theme v2)

Este documento especifica a arquitetura da folha de estilos unificada, os princípios de design de interface, a matriz de compatibilidade com os note types nativos AnkiMBR e o ecossistema de ferramentas MCP (Rust e TypeScript) de auditoria e pré-visualização.

---

## 1. Escopo e Limitações Não Clínicas

> ### AVISO LEGAL E DIRETRIZ DE ESCOPO (NÃO CLÍNICO)
> 1. **Natureza Gráfica e Ergonômica:** As folhas de estilo (`ankimbr-presentation-v2.css`, `ankimbr-media-patch.css`), tokens de cor, callouts clínicos e ferramentas de auditoria visual têm como objetivo exclusivo a usabilidade, acessibilidade digital (WCAG 2.1) e ergonomia visual no aplicativo Anki.
> 2. **Ausência de Efeito Terapêutico ou Diagnóstico:** A rotulagem de callouts como *\"Pérola Clínica\"*, *\"Raciocínio Clínico\"* ou *\"Alerta Vermelho\"* atua exclusivamente como recurso mnemônico e pedagógico de software. Não constitui suporte à decisão clínica, validação diagnóstica ou certificação médica.
> 3. **Preservação de Dados Pessoais:** Nenhum arquivo CSS, script ou ferramenta MCP manipula ou armazena dados confidenciais ou prontuários de pacientes.

---

## 2. Anatomia Estrutural dos Note Types Nativos AnkiMBR

Os templates nativos utilizados no ecossistema AnkiMBR possuem duas variantes padronizadas que compartilham a mesma identidade visual e folha de estilo básica:
1. **`1 Básico AnkiMBR` (Pergunta e Resposta / QA)**
2. **`1 Omissão de Palavras AnkiMBR` (Cloze Deletions)**

### 2.1. Componentes Estruturais dos Templates Nativos

| Elemento / Seção | Seletor e Markup Nativo | Script Auxiliar Embutido | Finalidade Funcional |
|---|---|---|---|
| **Contêiner Central** | `.coluna`, `.card .coluna` | — | Centraliza o conteúdo com largura máxima de 640px. |
| **Cabeçalho / Taxonomia** | `<div class="cabecalho"><div class="tag-estudo">{{Campo de Estudo}}</div></div>` | Formata hierarquia (converte `::` em ` › `). | Badge de identificação temática no topo do cartão. |
| **Enunciado (QA)** | `<div class="coluna enunciado">{{Enunciado}}</div>` | — | Pergunta central exibida na frente e mantida no verso. |
| **Resposta (QA)** | `<div class="coluna resposta">{{Resposta}}</div>` | — | Caixa elevada com sombra (`--ambr-shadow`) e fundo diferenciado. |
| **Sentença / Omissão** | `<div class="coluna sentenca-box"><div class="sentenca">{{cloze:Sentença}}</div></div>` | — | Caixa destacada que abriga o texto com as omissões Cloze. |
| **Omissão Cloze** | `<span class="cloze">...</span>` | Nativo do Anki | Realce âmbar/azul com salvaguarda de quebra de linha. |
| **Separador de Frente/Verso** | `<hr id="answer">` ou `<hr>` | — | Linha discreta centralizada (largura delimitada a 60%). |
| **Container de Notas** | `<div class="hints-container coluna">` | — | Pilha vertical para agrupamento de seções expansíveis. |
| **Notas Adicionais** | `<details class="hint" open><summary>Notas</summary><div class="hint-corpo">{{Notas Adicionais}}</div></details>` | **Script P2:** converte marcadores `•` em `<ul class="ambr-notas"><li>`. | Aprofundamento conceitual e fisiopatológico. |
| **Imagens Relacionadas** | `<details class="hint" open><summary>Imagens</summary><div class="hint-corpo centro media">{{Imagens Relacionadas}}</div></details>` | Integração com o motor de zoom. | Apresentação de esquemas, fotos e figuras médicas. |
| **Referências Bibliográficas** | `<details class="hint hint-ref" open><summary>Referências</summary><div class="hint-corpo">{{Referências}}</div></details>` | — | Citações textuais alinhadas à direita com tipografia discreta. |
| **Marca d'Água** | `<div class="marca-dagua">AnkiMBR</div>` | — | Identificador de rodapé discreto. |
| **Visualizador de Zoom** | `#ambr-zoom-overlay`, `#ambr-zoom-viewport`, `#ambr-zoom-image` | **Engine JS Completa (~350 linhas):** pan, pinch, mousewheel, double-tap, touch e teclado (Esc). | Permite ampliação de alta fidelidade em tela cheia para análise de exames e ilustrações. |

---

## 3. Matriz Comparativa de Tokens: Legado Nativo vs. Theme v2

A folha de estilo nativa opera com 12 tokens principais. O `ankimbr-presentation-v2.css` expande o design system para mais de 55 tokens rigorosamente calibrados para contraste segundo as diretrizes WCAG 2.1 AA/AAA.

### 3.1. Tabela de Correspondência e Mapeamento

| Token Nativo (Legado) | Valor Claro Nativo | Valor Escuro Nativo | Status no v2 | Solução Bridge (Compatibilidade) |
|---|---|---|:---:|---|
| `--ambr-accent` | `#0b5bd3` | `#7ab0ff` | Presente (recalibrado) | Mapeia para `#1d4ed8` / `#60a5fa`. |
| `--ambr-accent-soft` | `#eaf2ff` | `#273549` | Presente (recalibrado) | Mapeia para `#eff6ff` / `#172554`. |
| `--ambr-accent-line` | `#cfe1ff` | `#2f4470` | ⚠️ Omitido no v2 puro | **Bridge Obrigatória:** Definido como `var(--ambr-accent-line, #cfe1ff)` para não quebrar a borda da `.tag-estudo`. |
| `--ambr-ink` | `#111827` | `#e5e7eb` | Presente (recalibrado) | Mapeia para `#0f172a` / `#e8eef5`. |
| `--ambr-ink-soft` | `#4b5563` | `#b0b7c3` | ⚠️ Omitido no v2 puro | **Bridge Obrigatória:** Apontado como alias para `var(--ambr-ink-muted, #475569)`. |
| `--ambr-surface` | `#f8fafc` | `#1f2937` | Presente (recalibrado) | Fundo da resposta e caixas elevadas. |
| `--ambr-rule` | `#e5e7eb` | `#374151` | Presente (recalibrado) | Linhas de borda e divisores. |
| `--ambr-shadow` | `0 1px 3px rgba(0,0,0,.08)` | `0 1px 3px rgba(0,0,0,.4)` | ⚠️ Omitido no v2 puro | **Bridge Obrigatória:** Mantido para sustentação da profundidade de caixas nos note types nativos. |
| `--ambr-cloze` | `#fcd34d` (âmbar) | `#92400e` (âmbar escuro) | Renomeado (`--ambr-cloze-bg`) | **Bridge Obrigatória:** `--ambr-cloze: var(--ambr-cloze-bg, #dbeafe);`. |
| `--ambr-cloze-ink` | `#111827` | `#fef3c7` | Renomeado (`--ambr-cloze-fg`) | **Bridge Obrigatória:** `--ambr-cloze-ink: var(--ambr-cloze-fg, #1e3a8a);`. |

### 3.2. Novos Tokens Semânticos Introduzidos na Wave 2

* **Tabelas Clínicas:** `--anki-th-col-bg`, `--anki-th-row-bg`, `--anki-zebra-bg`.
* **Callouts Clínicos:**
  * Alertas / Armadilhas: `--anki-trap-bg`, `--anki-trap-border`, `--anki-trap-fg`
  * Pérolas Clínicas: `--anki-pearl-bg`, `--anki-pearl-border`, `--anki-pearl-fg`
  * Diagnóstico Diferencial: `--anki-diff-bg`, `--anki-diff-border`, `--anki-diff-fg`
  * Raciocínio Clínico: `--anki-reason-bg`, `--anki-reason-border`, `--anki-reason-fg`
  * Alarme / Red Flag: `--anki-redflag-bg`, `--anki-redflag-border`, `--anki-redflag-fg`
  * Correlação Clínica: `--anki-corr-bg`, `--anki-corr-border`, `--anki-corr-fg`
* **Fluxogramas de Conduta:** `--anki-flow-pos-bg`, `--anki-flow-neg-bg`, `--anki-flow-urg-bg`, `--anki-flow-dec-bg`.

---

## 4. Diretrizes de Coexistência e Camada Bridge (Retrocompatibilidade)

Para que a implementação do CSS v2 não cause quebra visual na base pré-existente de flashcards nem nos cartões novos gerados na Wave 2, a integração deve obedecer à seguinte arquitetura de coexistência:

```
┌─────────────────────────────────────────────────────────────┐
│                 Folha de Estilos Unificada                  │
├──────────────────────────────┬──────────────────────────────┤
│ Camada Nativa AnkiMBR        │ Extensões Wave 2 (v2)        │
│ • Seletores: .coluna,        │ • Seletores: .ambr-card,     │
│   .enunciado, .resposta,     │   .anki-table, .anki-callout │
│   .sentenca, .hint, etc.     │ • Callouts Semânticos        │
│ • Engine JS de Zoom          │ • Tabelas Zebradas WCAG      │
│ • Script P2 de Bullets       │ • Modificadores de Mídia     │
├──────────────────────────────┴──────────────────────────────┤
│ Camada Bridge de Variáveis (Aliasing Bidirecional)          │
│ --ambr-cloze: var(--ambr-cloze-bg);                         │
│ --ambr-ink-soft: var(--ambr-ink-muted);                     │
│ --ambr-accent-line: #cfe1ff / #2f4470 (dark);               │
│ --ambr-shadow: 0 1px 3px rgba(0,0,0,.08);                   │
└─────────────────────────────────────────────────────────────┘
```

### 4.1. Regras Críticas de Preservação:
1. **Isolamento do Módulo de Zoom:** Os seletores `#ambr-zoom-overlay`, `#ambr-zoom-viewport`, `#ambr-zoom-close` e `#ambr-zoom-image` devem ser integralmente mantidos com suas propriedades de `position: fixed; inset: 0; z-index: 999999; backdrop-filter: blur(8px)`.
2. **Quebra de Cloze no Mobile:** A regra `.cloze` deve reter obrigatoriamente `box-decoration-break: clone; -webkit-box-decoration-break: clone;`, assegurando que lacunas quebradas em duas linhas em telas de smartphones preservem suas margens e arredondamentos.
3. **Agrupamento de Contêineres:** Seletores de layout raiz devem ser declarados conjuntamente:
   ```css
   .coluna, .ambr-card {
     max-width: 640px;
     margin-left: auto;
     margin-right: auto;
     min-width: 0;
   }
   ```

---

## 5. Patch de Mídia Tema-Segura (`ankimbr-media-patch.css`)

O patch de mídia implementa renderização estrita de imagens médicas com proteção contra distorções do modo noturno em diferentes clientes (Anki Desktop, AnkiMobile e AnkiDroid):

### 5.1. Classes Modificadoras Médicas
* `.anki-figure--preserve`: Impede a inversão de cores. Essencial para lesões dermatológicas, traçados eletrocardiográficos (ECG) coloridos, tomografias, ressonâncias e peças anatômicas.
* `.anki-figure--frame-light`: Adiciona uma moldura sutil com fundo claro translúcido em modo escuro para gráficos que possuem fundo branco natural, evitando ofuscamento sem distorcer o conteúdo.
* `.anki-figure--invert-dark`: Aplica o filtro `filter: invert(0.92) hue-rotate(180deg)` exclusivamente em diagramas lineares esquemáticos monocromáticos (preto e branco).

### 5.2. Barreiras contra Distorção em WebViews Móveis
* **Bloqueio de Force Dark no Android:** Declaração explícita de `color-scheme: only light;` no escopo claro impede que a engine do Android force temas escuros artificiais que estragam o contraste de figuras.
* **Proteção contra Sequestro de Tema Claro:** Regras de `@media (prefers-color-scheme: dark)` **nunca** devem ser aplicadas soltas no escopo global; elas são estritamente delimitadas a seletores opt-in como `[data-theme="auto"]`.

---

## 6. Servidores MCP Auxiliares (Rust e TypeScript)

A validação contínua da qualidade estética e acessibilidade é assistida por dois servidores auxiliares integrados ao protocolo MCP.

### 6.1. Servidor Rust de Auditoria (`tools/anki-theme-audit-mcp`)
Binário nativo implementado com a biblioteca `rmcp` focado em checagens estáticas rápidas:
* `audit_css`: Analisa arquivos CSS submetidos:
  - Detecta e bloqueia regras `@media (prefers-color-scheme: dark)` desprovidas de escopo `[data-theme="auto"]`.
  - Garante a presença da salvaguarda `color-scheme: only light`.
  - Verifica a ausência da diretiva `!important` fora de blocos `@media print`.
  - Audita a simetria de variáveis CSS entre os blocos `:root` e `.nightMode`.
* `audit_html`: Detecta marcações incompatíveis com Light/Dark Mode em rascunhos (ex.: atributos obsoletos `bgcolor=`, `<font>`, tags `<svg>` soltas ou propriedades inline `style="color:..."`).

### 6.2. Servidor TypeScript de Pré-visualização (`tools/anki-theme-preview-mcp`)
Servidor Node.js baseado em `@modelcontextprotocol/server` e `@modelcontextprotocol/node` com suporte aos transportes Stdio e HTTP Streamable:
* `validate_contrast_matrix`: Calcula a razão de contraste fotométrico (algoritmo WCAG 2.1) entre pares de texto e fundo para temas Claro e Escuro, validando thresholds mínimos (4.5:1 para texto normal, 3.0:1 para componentes visuais grandes).
* `generate_theme_preview`: Gera um documento HTML completo e autocontido com layout em grade (lado a lado), exibindo o mesmo cartão sob o Tema Claro (`data-theme="light" class="card"`) e sob o Tema Escuro (`data-theme="dark" class="card nightMode"`).
* Recurso MCP exposto: `ankimbr://preview/harness`.

---

## 7. Procedimentos e Comandos de Verificação do Tema

Os comandos descritos abaixo são fornecidos como diretriz formal de execução por pipelines de integração contínua (CI) e testes manuais da camada de apresentação.

> **NOTA DE CONFORMIDADE:** Os comandos abaixo foram executados nesta máquina e o resultado real de cada um está em [`docs/migration/wave2-acceptance.md`](migration/wave2-acceptance.md) (§2).

### 7.1. Verificação do Servidor TypeScript de Visualização
```bash
cd tools/anki-theme-preview-mcp
npm ci --ignore-scripts --no-audit --no-fund
npm run build
npm run typecheck
npm test
```

### 7.2. Verificação do Servidor Rust de Auditoria
```bash
cargo test -p anki-theme-audit-mcp
```

### 7.3. Script de Checagem Regressiva do CSS
```bash
python tests/theme_regression_checks.py
```
