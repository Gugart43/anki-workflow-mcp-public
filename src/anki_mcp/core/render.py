"""core/render.py — render manifesto.json → manifesto.md.

Preserves the 6-section format the user already knows from the existing
manifesto.md files, making the JSON-backed source transparent.
"""

from __future__ import annotations

from anki_mcp.models import Manifesto


def render_manifesto_md(m: Manifesto) -> str:
    lines: list[str] = []

    # Header
    lines += [
        "# Manifesto de Governança dos Flashcards",
        "",
        f"**Campo de Estudo:** `{m.campo_estudo}`  ",
        f"**Data de Inicialização:** {m.created}  ",
        f"**Última Atualização:** {m.updated}  ",
    ]
    if m.topic_plan:
        lines.append(
            f"**Plano Geral do Tema:** {m.topic_plan.total_planned} cartões planejados "
            f"({len(m.topic_plan.batches_planned)} lotes de {m.topic_plan.batch_size})  "
        )
        lines.append(f"**Lotes Previstos:** {', '.join(m.topic_plan.batches_planned)}  ")
        if m.topic_plan.rationale:
            lines.append(f"**Justificativa do Orçamento:** {m.topic_plan.rationale}  ")
    if m.last_validated_batch:
        qa = m.totals.get("qa", 0)
        cl = m.totals.get("cloze", 0)
        tot = m.totals.get("active", 0)
        lines.append(
            f"**Último Lote Validado:** {m.last_validated_batch} ({tot} cartões aprovados)"
        )
        lines.append(f"**Total de Cartões Ativos:** {tot} cartões ({qa} QA + {cl} Cloze)")
    lines += ["", "---", ""]

    # Section 1: ID Registry
    lines += [
        "## 1. Registro de Identificadores (ID Registry)",
        "",
        "| ID | Lote | Tipo | Status | Tópico | Âncora da Fonte |",
        "| :--- | :--- | :--- | :--- | :--- | :--- |",
    ]
    for entry in m.id_registry:
        lines.append(
            f"| `{entry.id}` | {entry.batch} | {entry.type.value} "
            f"| `{entry.status.value}` | {entry.topic} | {entry.anchor} |"
        )
    lines += [""]

    # Section 2: Topic Log
    lines += ["## 2. Log de Tópicos (Topic Log)", ""]
    for topic in m.topic_log:
        lines.append(f"- {topic}")
    lines += [""]

    # Section 3: Tag Taxonomy
    lines += ["## 3. Taxonomia de Etiquetas (Tag Taxonomy)", ""]
    lines.append(f"```\n{m.taxonomy.root}")
    for node in m.taxonomy.nodes:
        planned_for = m.taxonomy.planned.get(node, "")
        suffix = f" (Previsto para {planned_for})" if planned_for else ""
        lines.append(f"├── {node}{suffix}")
    lines.append("```")
    lines += [""]

    # Section 4: Image Catalog
    lines += ["## 4. Versão do Catálogo de Imagens & Media Namespaces", ""]
    for cat in m.catalogs:
        lines += [
            f"- **Material:** {cat.material}",
            f"  - **Diretório:** `{cat.path}`",
            f"  - **Prefixo:** `{cat.prefix}`",
            f"  - **Hash:** `{cat.version_hash}`",
            f"  - **Destino:** `{cat.media_dir}`",
        ]
    lines += [""]

    # Section 5: Changelog
    lines += ["## 5. Changelog de Mutações", ""]
    for clog in reversed(m.changelog):  # newest first
        ids = f" [{clog.id}]" if clog.id else ""
        lines.append(f"- **{clog.date} ({clog.action}{ids}):** {clog.reason}")
    lines += [""]

    # Section 6: Suggestion Queue
    lines += ["## 6. Fila de Sugestões (Suggestion Queue)", ""]
    if m.suggestion_queue:
        for item in m.suggestion_queue:
            lines.append(f"- {item}")
    else:
        lines.append("- *Vazia no momento.*")
    lines += [""]

    return "\n".join(lines)
