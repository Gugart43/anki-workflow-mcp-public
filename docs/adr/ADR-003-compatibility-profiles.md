# ADR-003: Perfis de Compatibilidade (`legacy_compat` vs `strict_v2`)

- **Status:** Aceito
- **Data:** 2026-11-14
- **Relacionado:** ADR-001, Seção 1.4 e Seção 3.4 do `PLANO_MESTRE_ANKI_WORKFLOW_MCP_RUST_V2.md`

## Contexto

A auditoria estática do servidor Python v0.3.1 revelou comportamentos que precisam ser preservados para compatibilidade na transição, mas endurecidos na versão 2:

- `manifesto_commit` aceita um digest SHA-256 computável sem comprovar que todos os checks de gate foram aprovados sobre aquele snapshot.
- `export_final` pode concluir com `ledger_complete=false` quando há lacunas de IDs.
- `_PHASE_GATES` descreve checks por fase, porém `pipeline_gate_check` executa avaliação integral.

## Decisão

Separar explicitamente dois perfis de execução:

1. **`legacy_compat`**:
   - Preserva contratos públicos, nomes, parâmetros, defaults, diagnósticos e semântica de `PASS`/`FAIL`/`INCOMPLETE` das 49 ferramentas.
   - Mantém `compute_receipt_v1` (`sha256:` sobre CSV normalizado) e contagem de lote `MAX_BATCH = 20` (Cloze por índices distintos).
   - Emite avisos estruturados de depreciação quando um fluxo depender de comportamento legado permissivo (`ledger_complete=false`).
   - Nunca desabilita controles essenciais de segurança (allowlist de roots, proteção contra traversal/symlinks, stdout limpo).

2. **`strict_v2`** (padrão para novos fluxos v2):
   - Exige `ValidationAttestation` válida e não revogada no SQLite do workflow antes de `manifesto_commit` e `export_final`.
   - Bloqueia `export_final` (`BLOCKED`) se houver IDs obrigatórios ausentes no ledger.
   - Aplica matriz explícita e testada de gates por fase (`started` → `drafted` → `curated` → `enriched` → `illustrated` → `validated` → `exported`).
   - Não impõe teto numérico de cartões por lote: registra a contagem total e revisa cada cartão (cada índice Cloze separadamente). Prioriza recuperação obrigatória, contexto necessário e explicação opcional; sinais automáticos são consultivos e não certificam validade clínica, fidelidade ou atomicidade sem revisão semântica.
   - Qualquer check obrigatório indisponível resulta em `INCOMPLETE`, nunca `PASS`.

## Teste que valida esta decisão

- Suíte diferencial (`tests/differential/test_rust_python_parity.py`) validando paridade 1:1 no modo `legacy_compat`.
- Testes negativos de gate em `anki-state` / `anki-application` garantindo bloqueio de bypass no modo `strict_v2`.
