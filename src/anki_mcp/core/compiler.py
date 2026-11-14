"""core/compiler.py — Deterministic compiler from structured CardDrafts to AnkiMBR-compliant CSVs.

Bridge between LLM generative synthesis (JSON/JSONL) and Anki CSV storage:
 - Handles AnkiMBR field order:
   QA: Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas
   Cloze: Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas
 - Automatically converts Markdown tables or TableSpec objects in Notas Adicionais into clean HTML tables
 - Enforces HTML entity escaping, line breaks (<br>), and delimiter safety (| -> &#124;)
 - Guarantees sequential cloze indices (c1..cN)
 - Automatically purges ID leaks from text
 - Supports granular card-level patches and quarantine
"""

from __future__ import annotations

import re
from collections.abc import Sequence
from typing import Any

from anki_mcp.core.sanitize import sanitize_field
from anki_mcp.core.tables import TableSpec, markdown_tables_to_html
from anki_mcp.models import CLOZE_FIELDS, QA_FIELDS
from anki_mcp.models.draft import (
    CardDraft,
    CardPatch,
    ExtraNotes,
    MediaFigure,
    QuarantineRecord,
)

# Regex matching leaked card ID tokens (e.g. B01-QA-01, B02-CL-03, OF-B01-001)
_LEAKED_ID_RE = re.compile(
    r"\b(?:[A-Z]{2,4}-)?B\d{2}-(?:QA|CL|CLOZE)-\d{2,3}\b|\b[A-Z]{2,4}-B\d{2}-\d{3}\b",
    re.IGNORECASE,
)


def purge_id_leak(text: str) -> str:
    """Strip bare ID tokens that the model may have accidentally pasted in card text."""
    if not text:
        return ""
    cleaned = _LEAKED_ID_RE.sub("", text)
    return re.sub(r" {2,}", " ", cleaned).strip()


def _infer_campo_estudo(etiquetas: list[str] | str) -> str:
    """Infer hierarchical Campo de Estudo (Medicina::<Especialidade>::<Tema>) from taxonomy tags."""
    tag_list = etiquetas if isinstance(etiquetas, list) else str(etiquetas or "").split()
    for raw_tag in tag_list:
        tag = raw_tag.strip().lstrip("#")
        if not tag or "::" not in tag:
            continue
        if tag.startswith(("id::", "FocoClinico::", "Fonte::", "foco::", "tag::")):
            continue
        parts = [p.strip().replace("_", " ") for p in tag.split("::") if p.strip()]
        if not parts:
            continue
        if parts[0].lower() == "medicina":
            return "::".join(parts[:3])
        if len(parts) >= 2:
            return f"Medicina::{parts[0]}::{parts[1]}"
        if len(parts) == 1:
            return f"Medicina::{parts[0]}"
    return ""


def compile_drafts_to_csv(
    drafts: Sequence[CardDraft],
    *,
    auto_renumber_cloze: bool = True,
    auto_purge_id_leaks: bool = True,
    default_campo_estudo: str = "",
) -> tuple[str, str, dict[str, Any]]:
    """Compile a list of CardDraft objects into standard (qa_csv, cloze_csv) strings.

    Returns:
        (qa_csv_text, cloze_csv_text, stats_dict)
    """
    qa_rows: list[list[str]] = []
    cloze_rows: list[list[str]] = []
    stats = {
        "total_drafts": len(drafts),
        "qa_count": 0,
        "cloze_count": 0,
        "renumbered_cloze": 0,
        "purged_id_leaks": 0,
        "illustrated_cards": 0,
        "tables_rendered": 0,
    }

    for draft in drafts:
        card = draft.model_copy(deep=True)

        # 1. Purge ID leaks if requested
        if auto_purge_id_leaks:
            orig_text = card.enunciado_ou_sentenca
            card.enunciado_ou_sentenca = purge_id_leak(orig_text)
            if card.enunciado_ou_sentenca != orig_text:
                stats["purged_id_leaks"] += 1

            if card.resposta:
                orig_resp = card.resposta
                card.resposta = purge_id_leak(orig_resp)
                if card.resposta != orig_resp:
                    stats["purged_id_leaks"] += 1

        # 2. Sequential Cloze renumbering
        if auto_renumber_cloze and card.tipo == "CLOZE":
            orig_sent = card.enunciado_ou_sentenca
            card.renumber_cloze()
            if card.enunciado_ou_sentenca != orig_sent:
                stats["renumbered_cloze"] += 1

        # 3. Format 'Notas Adicionais' (with automatic Markdown table conversion & TableSpec support)
        if isinstance(card.notas_adicionais, ExtraNotes):
            if card.notas_adicionais.tabela is not None:
                stats["tables_rendered"] += 1
            stats["tables_rendered"] += sum(
                block.tipo == "tabela" for block in card.notas_adicionais.blocos
            )
            rendered_notes = card.notas_adicionais.render_html()
        elif isinstance(card.notas_adicionais, str):
            rendered_notes, tbl_count = markdown_tables_to_html(card.notas_adicionais)
            stats["tables_rendered"] += tbl_count
        else:
            rendered_notes = ""

        if card.tabela_notas is not None:
            spec = (
                card.tabela_notas
                if isinstance(card.tabela_notas, TableSpec)
                else TableSpec.model_validate(card.tabela_notas)
            )
            tbl_html = spec.render_html()
            rendered_notes = f"{rendered_notes}<br>{tbl_html}" if rendered_notes else tbl_html
            stats["tables_rendered"] += 1

        clean_notes, _ = sanitize_field(rendered_notes, convert_markdown=True)

        # 4. Format 'Imagens Relacionadas'
        figs: list[MediaFigure] = card.all_figures()
        if figs:
            rendered_img = "".join(f.render_html() for f in figs)
            stats["illustrated_cards"] += 1
        else:
            rendered_img = ""
        clean_img, _ = sanitize_field(rendered_img, is_images_field=True, convert_markdown=False)

        # 5. Format References and Tags
        if isinstance(card.referencias, list):
            refs_str = " &#124; ".join([r.strip() for r in card.referencias if r.strip()])
        else:
            refs_str = str(card.referencias or "").strip()
        if isinstance(card.notas_adicionais, ExtraNotes):
            known_refs = {r.strip() for r in refs_str.split(" &#124; ")}
            for ref in card.notas_adicionais.source_references():
                if ref not in known_refs:
                    refs_str = f"{refs_str} &#124; {ref}" if refs_str else ref
                    known_refs.add(ref)
        clean_refs, _ = sanitize_field(refs_str, convert_markdown=False)

        if isinstance(card.etiquetas, list):
            tags_str = " ".join([t.strip().replace(" ", "_") for t in card.etiquetas if t.strip()])
        else:
            tags_str = str(card.etiquetas or "").strip()

        # Injetar id::<CARD_ID> nas etiquetas se o card possuir ID definido
        c_id = getattr(card, "id", None)
        card_id_tag = f"id::{c_id.strip()}" if isinstance(c_id, str) and c_id.strip() else ""
        if card_id_tag and card_id_tag not in tags_str:
            tags_str = f"{tags_str} {card_id_tag}".strip() if tags_str else card_id_tag

        # 6. Sanitize main fields
        clean_stem, _ = sanitize_field(card.enunciado_ou_sentenca, convert_markdown=False)

        raw_campo = (card.campo_estudo or "").strip()
        if not raw_campo and default_campo_estudo:
            raw_campo = default_campo_estudo.strip()
        if not raw_campo:
            raw_campo = _infer_campo_estudo(card.etiquetas)

        clean_field_study, _ = sanitize_field(raw_campo, convert_markdown=False)

        if card.tipo == "QA":
            stats["qa_count"] += 1
            clean_resp, _ = sanitize_field(card.resposta or "", convert_markdown=False)
            row = [
                clean_stem,
                clean_resp,
                clean_field_study,
                clean_notes,
                clean_img,
                clean_refs,
                tags_str,
            ]
            qa_rows.append(row)
        else:
            stats["cloze_count"] += 1
            row = [
                clean_stem,
                clean_notes,
                clean_field_study,
                clean_img,
                clean_refs,
                tags_str,
            ]
            cloze_rows.append(row)

    def _write_csv(headers: Sequence[str], rows: list[list[str]]) -> str:
        lines = ["|".join(headers)]
        for r in rows:
            lines.append("|".join(r))
        return "\n".join(lines)

    qa_csv = _write_csv(QA_FIELDS, qa_rows) if qa_rows else "|".join(QA_FIELDS)
    cloze_csv = _write_csv(CLOZE_FIELDS, cloze_rows) if cloze_rows else "|".join(CLOZE_FIELDS)

    return qa_csv, cloze_csv, stats


def apply_patch_to_draft(draft: CardDraft, patch: CardPatch) -> CardDraft:
    """Apply surgical field updates to a single CardDraft."""
    card_dict = draft.model_dump()
    for k, v in patch.updates.items():
        if k == "notas_adicionais" and isinstance(v, dict):
            card_dict["notas_adicionais"] = v
        else:
            card_dict[k] = v
    return CardDraft(**card_dict)


def partition_quarantine(
    drafts: list[CardDraft],
    failed_indices: Sequence[int],
    diagnostics: list[dict[str, Any]] | None = None,
    reason: str = "Validation failed after repair budget",
) -> tuple[list[CardDraft], list[QuarantineRecord]]:
    """Partition drafts into valid list and quarantined records."""
    valid: list[CardDraft] = []
    quarantined: list[QuarantineRecord] = []
    failed_set = set(failed_indices)

    diags_map: dict[int, list[dict[str, Any]]] = {}
    if diagnostics:
        for d in diagnostics:
            row = d.get("row")
            if row is not None:
                diags_map.setdefault(row - 1, []).append(d)

    for idx, card in enumerate(drafts):
        if idx in failed_set:
            q_rec = QuarantineRecord(
                card_index=idx,
                card=card,
                diagnostics=diags_map.get(idx, []),
                reason=reason,
            )
            quarantined.append(q_rec)
        else:
            valid.append(card)

    return valid, quarantined
