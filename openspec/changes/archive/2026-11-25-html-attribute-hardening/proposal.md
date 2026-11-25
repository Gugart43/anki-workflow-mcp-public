# Proposal

## Why

O sanitizador Python e Rust preserva atributos HTML arbitrários em tags permitidas, e `check_csv` não os rejeita. Um CSV com `onclick` ou `onerror` pode atravessar a validação e ser importado para o WebView do Anki; o escape localizado de `MediaFigure` não corrige essa fronteira.

## What Changes

- Sanitizar atributos de tags permitidas com uma política mínima compatível com AnkiMBR e reconstruir atributos seguros com escape.
- Rejeitar atributos executáveis, malformados e origens inseguras de imagem em CSV bruto com diagnóstico crítico `FMT.ATTR_FORBIDDEN` nos dois motores.
- Acrescentar regressões Python, Rust e diferenciais que preservem classes semânticas, tabelas, mídia local e inequações clínicas.
- **BREAKING**: HTML legado com estilos inline ou atributos fora da política não será mais aceito como válido no `check_csv`; a sanitização removerá esses atributos.

## Capabilities

### New Capabilities

- `html-field-safety`: contrato de atributos e diagnósticos para HTML em campos Anki no fluxo de compilação e validação CSV Python/Rust.

### Modified Capabilities

Nenhuma: não há specs de capacidades previamente publicadas neste repositório.

## Impact

`core/sanitize.py`, `checks/csv_structure.py`, `crates/anki-validate/src/sanitize.rs`, `crates/anki-validate/src/csv_structure.rs`, testes e documentação. Sem novos serviços ou acesso a baralhos pessoais. A compatibilidade será medida com testes existentes e paridade entre motores; não se promete garantia universal contra todo XSS.
