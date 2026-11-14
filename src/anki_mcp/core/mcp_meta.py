"""core/mcp_meta.py — MCP protocol ergonomics: semantic tags, prompts and resources.

Implements H1-3 (Task 3):
- TOOL_TAGS taxonomy for all 49 registered tools.
- tag_tools(mcp) decorator / synchronous applier.
- phase_rules(phase) native MCP prompt.
- schema:// and vocab:// native MCP resources.
"""

from __future__ import annotations

import json
from typing import Any

from anki_mcp.core.vocab import vocabulary_cheatsheet
from anki_mcp.models import CardDraft, Manifesto, ValidationReport

TOOL_TAGS: dict[str, set[str]] = {
    # ── validate (11) ──
    "validate_csv_structure": {"validate"},
    "validate_cloze": {"validate"},
    "validate_hints": {"validate"},
    "validate_qa": {"validate"},
    "validate_notes": {"validate"},
    "validate_tags_field": {"validate"},
    "validate_crossrefs": {"validate"},
    "validate_images_field": {"validate"},
    "validate_tables_field": {"validate", "tables"},
    "validate_batch": {"validate"},
    "review_batch": {"validate", "pipeline"},
    # ── mechanics (4) ──
    "csv_build_row": {"mechanics"},
    "sanitize_card_field": {"mechanics"},
    "batch_auto_fix": {"mechanics"},
    "count_batch_cards": {"mechanics"},
    # ── state (7) ──
    "manifesto_init": {"state"},
    "manifesto_import": {"state"},
    "manifesto_state": {"state"},
    "manifesto_commit": {"state"},
    "manifesto_plan_set": {"state"},
    "taxonomy_check": {"state"},
    "id_ledger_query": {"state"},
    # ── pipeline (5) ──
    "pipeline_phase_advance": {"pipeline"},
    "pipeline_gate_check": {"pipeline"},
    "gap_request": {"pipeline"},
    "export_final": {"pipeline"},
    "deck_audit": {"pipeline"},
    # ── media (6) ──
    "catalog_load": {"media"},
    "image_namespace": {"media"},
    "image_candidates": {"media"},
    "image_sync": {"media"},
    "image_report": {"media"},
    "media_figure_build": {"media"},
    # ── compiler (4) ──
    "cards_compile_drafts": {"compiler"},
    "card_patch_apply": {"compiler"},
    "batch_patch_draft": {"compiler"},
    "card_quarantine": {"compiler"},
    # ── tables (2 additional standalone) ──
    "table_build": {"tables"},
    "table_from_markdown": {"tables"},
    # ── triage (6) ──
    "corpus_load": {"triage"},
    "corpus_triage": {"triage"},
    "corpus_coverage": {"triage"},
    "corpus_digest": {"triage"},
    "topic_triage_calibrate": {"triage"},
    "topic_budget_plan": {"triage"},
    # ── hierarchy (4) ──
    "deck_hierarchy_get": {"hierarchy"},
    "deck_hierarchy_search": {"hierarchy"},
    "deck_hierarchy_add": {"hierarchy"},
    "deck_hierarchy_sync": {"hierarchy"},
}

_PHASE_RULES: dict[str, str] = {
    "drafted": """### Fase: drafted (Rascunho Inicial)
- **Objetivo**: Estruturação semântica preliminar em 3 camadas e formatação CSV pura.
- **Checks Obrigatórios**: `csv_structure` (header match, escape de pipes `&#124;`, parênteses MathJax, ausência de BOM).
- **Diretriz de Informação (3 Camadas)**: Distinga carga recuperada (alvo da pergunta ou omissão `cN`), contexto necessário na frente (dados que tornam a resposta única; contexto maior não é cobrança extra) e explicação opcional (`Notas Adicionais`, sem volume mínimo nem tetos rígidos novos). Em QA com lista ≥ 3, revise a unidade sem conversão automática QA->Cloze — uma única lacuna Cloze pode ocultar lista.
- **Tools Recomendadas**: `cards_compile_drafts`, `validate_csv_structure`, `pipeline_gate_check(phase='drafted')`.
- **Critério de Saída**: PASS no gate `csv_structure`.
""",
    "curated": """### Fase: curated (Curadoria Clínica & Atômica)
- **Objetivo**: Garantia de unidade decisória nas 3 camadas, cardinalidade QA e qualidade de omissão Cloze.
- **Checks Obrigatórios**: `csv_structure`, `cloze`, `hints`, `qa`, `notes`.
- **Regras Clínicas**:
  - **3 Camadas**: separe carga recuperada (o que o estudante precisa lembrar), contexto necessário (contexto da frente não é cobrança extra) e explicação opcional (`Notas Adicionais`). Comprimento em caracteres é apenas sinal de revisão (WARN), nunca prova de múltipla cobrança nem teto/mínimo rígido novo.
  - **QA**: 1-2 itens recuperados; lista ≥ 3 (`QA.CARDINALITY`) exige revisão da unidade (separar recuperações independentes ou mover detalhe opcional para notas), sem conversão automática QA->Cloze.
  - **Cloze**: omissões sequenciais `c1..cN` (máximo operacional de 3 índices; cada índice gera cartão próprio com os demais índices visíveis; uma única lacuna pode ocultar lista e contar índices/caracteres não prova atomicidade); sem dicas que revelem a resposta.
  - **Notas**: vazias ou curtas são permitidas (sem mínimo obrigatório), com profundidade variável na mesma unidade cobrada e sustentada por fonte autorizada; retomar o alvo sem explicar (`NOTE.DUP_ANSWER_WARN`) ou comentar apenas que caiu em prova (`NOTE.EXAM_ONLY_WARN`) não é explicação.
- **Tools Recomendadas**: `validate_cloze`, `validate_hints`, `validate_qa`, `validate_notes`, `review_batch`.
- **Critério de Saída**: Zero erros CRITICAL; PASS no gate `curated`.
""",
    "enriched": """### Fase: enriched (Enriquecimento Semântico & Notas)
- **Objetivo**: Aprofundamento opcional na camada de explicação (`Notas Adicionais`), mecanismos fisiopatológicos e quadros comparativos limpos.
- **Checks Obrigatórios**: `csv_structure`, `notes`.
- **Regras Clínicas**:
  - **3 Camadas**: `Notas Adicionais` é explicação opcional após a resposta (vazio ou curto permitido, sem volume mínimo nem tetos rígidos novos). A profundidade varia conforme a dificuldade, permanecendo na mesma unidade cobrada e sempre sustentada pela fonte autorizada — sem inventar explicações nem encher linguiça.
  - **Contexto vs. Cobrança & Sem Auto-Conversão**: contexto necessário da frente não é cobrança extra; uma única lacuna pode ocultar lista; não converter QA->Cloze automaticamente.
  - **Antipadrões em Notas**: mera repetição normalizada do alvo (`NOTE.DUP_ANSWER_WARN`) e menção exclusiva a prova (`NOTE.EXAM_ONLY_WARN`) não substituem explicação. Comprimento é apenas sinal de revisão, nunca bloqueio.
  - Usar HTML semântico real (`<p>`, `<blockquote>`, `<b>`, listas e tabelas limpas), não tags escapadas como `&lt;p&gt;`; cabeçalhos são opcionais, não robóticos. Tabelas limpas, sem inline styles; dimensões móveis são recomendações de legibilidade, não bloqueios de conteúdo.
- **Representação por objetivo**: Parágrafos para explicar; tabelas para comparar; listas para checklist; fluxogramas para condições e próximo passo; alertas para ressalvas. `notas_adicionais.blocos` aceita `paragrafo`, `lista`, `alerta`, `tabela`, `fluxograma`, com `titulo` opcional e `fontes` obrigatórias (`fonte`, `pagina`, `trecho`). Preservar campos legados. Não usar todos os formatos em cada nota nem impor volume mínimo. Fluxos são mapas HTML numerados sem JS/Mermaid: declarar rótulos das condições, não inventar relações. Blocos guardam evidência no draft e adicionam fonte/página às Referências; o compilador verifica estrutura, não fidelidade clínica. Revisar trechos e sentido na fonte. Tabelas de 2–4 cols/até 8 linhas por quadro são alvo de legibilidade, não teto de conhecimento; dividir quadros longos sem aumentar a cobrança. Priorizar explicação → comparação/decisão → exceção, evitando repetir um resumo completo em cada cartão.
- **Tools Recomendadas**: `cards_compile_drafts`, `table_build`, `table_from_markdown`, `validate_tables_field`, `pipeline_gate_check(phase='enriched')`.
- **Critério de Saída**: PASS no gate `enriched`.
""",
    "illustrated": """### Fase: illustrated (Ilustração & Mídia Clínica)
- **Objetivo**: Associação de figuras semiológicas, exames de imagem e esquemas semânticos.
- **Checks Obrigatórios**: `images`.
- **Regras Clínicas**: Usar `<figure>` e `<figcaption>` semânticas (`media_figure_build`); verificar se a imagem existe no catálogo (`anki_catalog.csv`); namespace `slug_arquivo.png`.
- **Tools Recomendadas**: `catalog_load`, `image_candidates`, `media_figure_build`, `pipeline_gate_check(phase='illustrated')`.
- **Critério de Saída**: Imagens verificadas contra o catálogo; sem imagens fantasmas (`IMG.PHANTOM`).
""",
    "validated": """### Fase: validated (Validação Final Completa de Lote)
- **Objetivo**: Auditoria holística antes de qualquer escrita de exportação.
- **Checks Obrigatórios**: `csv_structure`, `cloze`, `hints`, `qa`, `notes`, `tags`, `batch`, `images`.
- **Regras Clínicas**: Lote ≤ 20 cards no Anki; taxonomia registrada no Manifesto; recibo criptográfico gerado.
- **Tools Recomendadas**: `review_batch(detail='compact')` ou `validate_batch`.
- **Critério de Saída**: Status PASS com emissão de recibo (`sha256:...`).
""",
    "exported": """### Fase: exported (Consolidação em Disco & Cartório)
- **Objetivo**: Escrita transacional atômica nos arquivos finais e no Manifesto.
- **Checks Obrigatórios**: Verificação de recibo PASS (`verify_receipt`).
- **Tools Recomendadas**: `export_final` ou `manifesto_commit`.
- **Critério de Saída**: Arquivos CSV gravados em `exports/`, ledger e changelog atualizados sob lock transacional.
""",
}


def get_phase_rules(phase: str = "validated") -> str:
    """Return concise phase markdown instructions for LLM client guidance."""
    norm = phase.strip().lower()
    if norm in _PHASE_RULES:
        return _PHASE_RULES[norm]
    valid = ", ".join(_PHASE_RULES.keys())
    return f"Fase desconhecida '{phase}'. Fases válidas: {valid}."


def tag_tools(mcp: Any) -> None:
    """Assign semantic tags to all registered FastMCP tools."""
    lp = getattr(mcp, "_local_provider", None)
    if lp and hasattr(lp, "_components"):
        for comp in lp._components.values():
            name = getattr(comp, "name", None)
            if name and name in TOOL_TAGS:
                comp.tags = set(TOOL_TAGS[name])


def register_prompts_and_resources(mcp: Any) -> None:
    """Register phase_rules prompt and schema/vocab resources on FastMCP server."""

    @mcp.prompt(name="phase_rules", description="Regras da fase atual do pipeline de flashcards")
    def phase_rules_prompt(phase: str = "validated") -> str:
        return get_phase_rules(phase)

    @mcp.resource(
        "schema://card-draft",
        name="CardDraft Schema",
        description="JSON Schema do CardDraft para geração de cards clínicos",
    )
    def card_draft_schema() -> str:
        return json.dumps(CardDraft.model_json_schema(), indent=2)

    @mcp.resource(
        "schema://manifesto",
        name="Manifesto Schema",
        description="JSON Schema do Manifesto de governança",
    )
    def manifesto_schema() -> str:
        return json.dumps(Manifesto.model_json_schema(), indent=2)

    @mcp.resource(
        "vocab://classes",
        name="Semantic CSS Classes",
        description="Cheatsheet das classes semânticas CSS do AnkiMBR",
    )
    def vocab_classes() -> str:
        return vocabulary_cheatsheet()

    @mcp.resource(
        "schema://report",
        name="ValidationReport Schema",
        description="JSON Schema do relatório de validação",
    )
    def report_schema() -> str:
        return json.dumps(ValidationReport.model_json_schema(), indent=2)
