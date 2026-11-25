# Contratos (`contracts/`)

## Mapa

| Caminho | O que é | Quem gera |
| --- | --- | --- |
| `contracts/v2/schemas/*.schema.json` | JSON Schema Draft 2020-12 canônico (chaves ordenadas, LF) das structs Rust (`schemars`) | `export-schemas` |
| `contracts/v2/ts/index.d.ts` | Tipos TypeScript derivados dos schemas | `tools/codegen-ts/generate.mjs` |
| `contracts/v2/manifest.json` | Índice tool MCP → `args_schema` → `result_schema` (+ `output_schema_present`) | `export-schemas` |
| `contracts/v2/version.lock` | `contract_version` + sha256 do conjunto de schemas + política | `export-schemas` |
| `contracts/legacy/*.json` | Captura do servidor Python legado (49 tools); `served_by` diz quem serve cada nome hoje | `scripts/legacy-oracle/export_contracts.py` |

Regenerar tudo: `just codegen-contracts` (ou `cargo run --locked -p anki-workflow-mcp --bin export-schemas`
e depois `node tools/codegen-ts/generate.mjs`). Verificar sem escrever: `node tools/codegen-ts/generate.mjs --check`.

## Política do contrato vivo (v2)

- `contracts/v2` é **gerado, nunca editado à mão**: qualquer edição manual é desfeita na próxima execução e
  quebra `apps/anki-workflow-mcp/tests/contract_drift.rs` (compara schemas, `manifest.json` e `version.lock` byte a byte).
- `contract_version` é a versão maior do diretório: mudança **breaking** em um schema, no manifesto ou nos tipos
  TypeScript exige bump de `contract_version` e um novo diretório `contracts/vN` — nada de breaking in-place.
- `schema_digest` no `version.lock` é o sha256 do conjunto de schemas (nome + LF + bytes + LF, ordenado por nome):
  ele pina exatamente contra o que um consumidor compilou. Mudança aditiva mantém o `contract_version` e muda o
  digest, então o diff do lock é a revisão obrigatória da mudança.
- O codegen TypeScript **falha** (não avisa) em `$defs` órfão, `title` duplicado com corpos diferentes e
  `description` conflitante para um tipo compartilhado; corrija a origem (doc comments/structs em Rust).

## Legado

`contracts/legacy/tools.json` marca cada tool com `served_by` (`"rust"` quando o nome já é servido pelo servidor
Rust, `"python-legacy"` caso contrário). Dois nomes colidem entre as duas implementações com parâmetros
incompatíveis — `batch_patch_draft` e `cards_compile_drafts` — e cada entrada colide documentada em `collision`
(`also_served_by`, `params_compatible`, `reason`). O teste
`export_schemas::tests::test_legacy_tools_declare_served_by_and_document_incompatible_collisions` falha se um novo
nome for servido pelas duas implementações sem essa documentação.
