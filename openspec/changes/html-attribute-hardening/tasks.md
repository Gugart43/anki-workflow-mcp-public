# Tasks

## 1. Motor Python

- [x] 1.1 Adicionar regressões RED→GREEN e neutralização de atributos em `sanitize_field`, com allowlist, sintaxe malformada, src/dimensões seguros e preservação de inequações/entidades; verificar testes direcionados e idempotência.
- [x] 1.2 Fazer `check_csv` rejeitar atributos proibidos/malformados com `FMT.ATTR_FORBIDDEN` crítico, sem reescrever CSV; verificar testes direcionados e gate em caso sintético.
- [x] 1.3 Cobrir draft→CSV, atualizar documentação de segurança Python caso o comportamento mude e verificar suíte Python e Ruff.

## 2. Motor Rust experimental

- [x] 2.1 Adicionar regressões RED→GREEN e neutralização de atributos em `sanitize_field`, incluindo valores, sintaxe malformada e idempotência; verificar `cargo test -p anki-validate`.
- [x] 2.2 Fazer `check_csv` rejeitar atributos proibidos/malformados com diagnóstico crítico; verificar regressões e o gate via oráculo Rust.
- [x] 2.3 Cobrir draft→CSV e documentar limites no README; verificar testes Rust, formatação e Clippy.

## 3. Integração

- [x] 3.1 Adicionar regressões diferenciais Python↔Rust para entradas HTML sintéticas, recompilar o oráculo e verificar `pytest tests/differential/`.
- [x] 3.2 Revisar caminhos alternativos de emissão, confirmar ausência de bypass nas APIs testadas e executar gates finais Python/Rust/OpenSpec, sem usar dados privados ou publicar alterações.

## Workflow follow-up

- Revisar e, se apropriado, arquivar a mudança depois da aceitação do projeto.
