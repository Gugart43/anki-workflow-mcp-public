# Design

## Context

Ver `proposal.md` e `specs/html-field-safety/spec.md`. Os validadores e sanitizadores Python/Rust usam regex divergentes para reconhecer tags e preservam os atributos sem filtro. `vocab.py` já define a allowlist por tag. O gerador Rust de figuras possui escape localizado, mas os emitters de notas/figuras podem receber conteúdo HTML; a fronteira de campos precisa ser verificada independentemente. As alterações locais anteriores ao presente trabalho não devem ser descartadas.

## Goals / Non-Goals

**Goals:** política verificável para tag, atributo e valor; comportamento reproduzível entre Python/Rust; diagnósticos explícitos para CSV bruto e neutralização de drafts; proteção contra tags com sintaxe ambígua sem quebrar equações, classes, figuras e tabelas legítimas.

**Non-Goals:** certificar HTML arbitrário como seguro em qualquer navegador; substituir o transporte Python pelo Rust, alterar templates AnkiMBR, processar baralhos reais ou tentar corrigir conteúdo clínico.

## Decisions

1. **Reutilizar o vocabulário de atributos de `vocab.py` como contrato:** `class` global; `img` recebe `src`, `alt`, dimensões; `td`/`th` recebem spans. Rust espelha esse contrato em testes diferenciais. Alternativa rejeitada: bloquear apenas `on*`, pois permite novas superfícies de execução/estilo.
2. **Dois destinos distintos:** sanitização recompõe a tag sem atributos proibidos e informa fixes; `check_csv` inspeciona a entrada crua e emite `FMT.ATTR_FORBIDDEN` crítico, com sugestão/preview quando possível. Alternativa rejeitada: aceitar HTML perigoso e somente avisar, pois o gate pode tratar avisos como aprovação.
3. **Tokenização com estado de aspas e falha fechada:** evitar depender somente de regex de tags para atributos. Separadores por barra, aspas não fechadas, atributos booleanos e duplicados não podem ser interpretados como texto confiável. Para atributos permitidos, codificar valores antes de remontar a tag. Não reinterpretar entidades como markup ativo.
4. **Restrições de valores:** `src` é nome de arquivo local simples; rejeitar esquemas e travessia, inclusive codificação por entidade. Valores dimensionais/spans são inteiros positivos. `class` preserva tokens sem autorização para atributos extras; classes desconhecidas continuam gerando `FMT.CLASS_UNKNOWN` de aviso como contrato existente.
5. **Fluxos e testes:** cada motor implementa os mesmos casos sintéticos com RED→GREEN; depois integrar comparação Python↔Rust via oráculo recompilado, rodar suíte Python/Rust, formatação e Clippy. Se o contrato exigir expansão (por exemplo, outro atributo legítimo), documentar e testar antes de permitir.

## Risks / Trade-offs

- [HTML malformado é interpretado de modo diferente pelo WebView] → falhar fechado e documentar limites do parser; não alegar segurança universal sem ensaios contra o navegador real.
- [HTML legado com `style` ou atributos desconhecidos muda de aparência] → documentar incompatibilidade intencional e preservar convenções sem estilos inline por classes semânticas.
- [Geradores de HTML que interpolam strings não sanitizadas] → exercitar draft→CSV; se existir caminho direto que bypassa o sanitizador, acrescentar proteção nesse ponto antes de declarar encerramento.
- [Testes diferenciais podem usar binário desatualizado] → `cargo build -p anki-workflow-mcp` antes dos testes.

## Migration Plan

Manter Python como servidor operacional e Rust como experimento em memória; rodar regressões locais em CSVs sintéticos. Em caso de regressão, reverter somente alterações deste change; preservar alterações preexistentes na árvore. Não implantar automaticamente.
