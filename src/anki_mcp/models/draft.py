"""models/draft.py — Structured data contracts for LLM card generation, surgical patching, and quarantine.

Enforces TypeSafe System One contracts for generation:
 - DeepSeek-R1 / LLM S2 emits typed CardDrafts (JSON/JSONL), never raw CSV.
 - Supports both canonical AnkiMBR fields and Prompt Master JSONL aliases.
 - The MCP compiler handles layout, HTML escaping, delimiter encoding, table rendering, and AnkiMBR compliance.
"""

from __future__ import annotations

import html
import re
from typing import Any, Literal

from pydantic import BaseModel, Field, model_validator

from anki_mcp.core.note_blocks import NoteBlock
from anki_mcp.core.tables import TableSpec, markdown_tables_to_html

_EXTRACTOR_WRAPPER_RE = re.compile(
    r"</?(?:div|span)\b[^>]*(?:statdx-fig-body|anki-fig-title|table-title|table-subinfo|arrow-pill)[^>]*>",
    re.IGNORECASE,
)
_BASE64_IMG_RE = re.compile(r"""<img\b[^>]*src=["']data:image/[^"']+["'][^>]*>""", re.IGNORECASE)
_FIG_PARSE_RE = re.compile(
    r"""<figure\b[^>]*>\s*<img\b[^>]*src=["']([^"']+)["'][^>]*>\s*(?:<figcaption\b[^>]*>(.*?)</figcaption>)?\s*</figure>""",
    re.DOTALL | re.IGNORECASE,
)
_BARE_IMG_PARSE_RE = re.compile(
    r"""<img\b[^>]*src=["']([^"']+)["'][^>]*>""",
    re.IGNORECASE,
)

# Mapped styles that become semantic CSS classes rather than forbidden inline styles
_SEMANTIC_STYLE_CLASSES = {
    "clinical": "anki-figure",
    "figure": "anki-figure",
    "table": "anki-figure anki-table-figure",
    "quadro": "anki-figure anki-table-figure",
}


def clean_extractor_caption(text: str) -> str:
    """Remove raw extractor wrapper divs, base64 indicator icons, and boilerplate subinfo."""
    if not text:
        return ""
    s = _BASE64_IMG_RE.sub("", text)
    s = _EXTRACTOR_WRAPPER_RE.sub(" ", s)
    s = re.sub(
        r"<small>\s*Tabela\s*/\s*Quadro\s+estruturado\s*</small>", "", s, flags=re.IGNORECASE
    )
    s = re.sub(
        r"<small>\s*\d+\s*linhas?\s*[x×]\s*\d+\s*colunas?\s*</small>", "", s, flags=re.IGNORECASE
    )
    s = re.sub(r"\s+", " ", s).strip()
    return s


class ExtraNotes(BaseModel):
    """Rich clinical notes structure matching the AnkiMBR standard."""

    fisiopatologia: str | None = None
    raciocinio_clinico: str | None = None
    armadilhas_diferenciais: str | None = None
    correlacao_visual: str | None = None
    tabela: TableSpec | dict[str, Any] | None = None
    texto_livre: str | None = None
    blocos: list[NoteBlock] = Field(default_factory=list)

    def source_references(self) -> list[str]:
        """Keep block evidence in the references field, not in the visible prose."""
        return list(dict.fromkeys(s.reference() for b in self.blocos for s in b.fontes))

    def render_html(self) -> str:
        """Render the structured notes into AnkiMBR compliant HTML (including optional HTML table)."""
        parts: list[str] = []
        if self.fisiopatologia and self.fisiopatologia.strip():
            parts.append(f"• <b>Fisiopatologia &amp; Mecanismo:</b> {self.fisiopatologia.strip()}")
        if self.raciocinio_clinico and self.raciocinio_clinico.strip():
            parts.append(
                f"• <b>Raciocínio Clínico &amp; Prática:</b> {self.raciocinio_clinico.strip()}"
            )
        if self.armadilhas_diferenciais and self.armadilhas_diferenciais.strip():
            parts.append(
                f"• <b>Armadilhas &amp; Diferenciais:</b> {self.armadilhas_diferenciais.strip()}"
            )
        if self.correlacao_visual and self.correlacao_visual.strip():
            parts.append(
                f"• <b>Correlação de Imagem / Quadro:</b> {self.correlacao_visual.strip()}"
            )
        if self.tabela is not None:
            spec = (
                self.tabela
                if isinstance(self.tabela, TableSpec)
                else TableSpec.model_validate(self.tabela)
            )
            parts.append(spec.render_html())
        if self.texto_livre and self.texto_livre.strip():
            converted_free, _ = markdown_tables_to_html(self.texto_livre.strip())
            parts.append(converted_free)
        parts.extend(block.render_html() for block in self.blocos)
        return "<br>".join(parts)


class MediaFigure(BaseModel):
    """Media figure specification with required clinical caption for AnkiMBR."""

    filename: str
    caption: str = ""
    title: str = ""
    context_link: str = ""
    item_type: Literal["figure", "table"] = "figure"
    style: str = ""
    caption_style: str = ""

    def render_html(self) -> str:
        """Render standard AnkiMBR <figure> tag with descriptive caption."""
        fn = self.filename.strip()
        if not fn:
            return ""
        raw_cap = clean_extractor_caption(self.caption)
        title_clean = clean_extractor_caption(self.title)
        ctx_clean = clean_extractor_caption(self.context_link)

        cap_parts: list[str] = []
        if title_clean and title_clean.lower() not in raw_cap.lower():
            cap_parts.append(f"<b>{title_clean}:</b>")
        if raw_cap:
            cap_parts.append(raw_cap)
        if ctx_clean and ctx_clean.lower() not in raw_cap.lower():
            cap_parts.append(f"<i>({ctx_clean})</i>")
        cap = " ".join(cap_parts).strip()

        clean_alt = re.sub(r"<[^>]+>", "", cap).strip()
        escaped_alt = html.escape(clean_alt, quote=True)
        alt_attr = f' alt="{escaped_alt}"'

        style_key = self.style.strip().lower()
        if not style_key:
            fig_attr = ' class="anki-figure anki-table-figure"' if self.item_type == "table" else ""
        elif style_key in _SEMANTIC_STYLE_CLASSES:
            fig_attr = f' class="{_SEMANTIC_STYLE_CLASSES[style_key]}"'
        else:
            escaped_style = html.escape(self.style.strip(), quote=True)
            fig_attr = f' style="{escaped_style}"'

        if cap:
            escaped_cap_style = html.escape(self.caption_style.strip(), quote=True)
            cap_style = f' style="{escaped_cap_style}"' if self.caption_style else ""
            cap_tag = f"<figcaption{cap_style}>{cap}</figcaption>"
        else:
            cap_tag = ""
        escaped_fn = html.escape(fn, quote=True)
        return f'<figure{fig_attr}><img src="{escaped_fn}"{alt_attr}>{cap_tag}</figure>'


def parse_images_html_to_figures(images_val: Any) -> list[MediaFigure]:
    """Parse an HTML string or list of dicts/strings into MediaFigure objects."""
    if not images_val:
        return []
    if isinstance(images_val, MediaFigure):
        return [images_val]
    if isinstance(images_val, dict):
        return [MediaFigure(**images_val)]
    if isinstance(images_val, list):
        out: list[MediaFigure] = []
        for item in images_val:
            out.extend(parse_images_html_to_figures(item))
        return out
    if not isinstance(images_val, str):
        return []

    s = images_val.strip()
    if not s:
        return []

    figures: list[MediaFigure] = []
    for m in _FIG_PARSE_RE.finditer(s):
        full_tag = m.group(0)
        fn = m.group(1).strip()
        cap = (m.group(2) or "").strip()
        style_val = "clinical" if "anki-figure" in full_tag else ""
        if fn:
            figures.append(MediaFigure(filename=fn, caption=cap, style=style_val))

    outside = _FIG_PARSE_RE.sub("", s)
    for m in _BARE_IMG_PARSE_RE.finditer(outside):
        fn = m.group(1).strip()
        if fn:
            rem_text = re.sub(r"<[^>]+>", " ", outside).strip()
            figures.append(MediaFigure(filename=fn, caption=rem_text))

    if (
        not figures
        and not ("<" in s or ">" in s)
        and re.search(r"\.(?:png|jpe?g|gif|webp|svg)$", s, re.I)
    ):
        figures.append(MediaFigure(filename=s, caption=""))

    return figures


class CardDraft(BaseModel):
    """Canonical structured representation of a single card draft."""

    id: str | None = None
    tipo: Literal["QA", "CLOZE"]
    enunciado_ou_sentenca: str
    resposta: str | None = None
    campo_estudo: str = ""
    notas_adicionais: ExtraNotes | str = Field(default_factory=ExtraNotes)
    tabela_notas: TableSpec | dict[str, Any] | None = None
    figura: MediaFigure | None = None
    figuras: list[MediaFigure] = Field(default_factory=list)
    referencias: list[str] | str = Field(default_factory=list)
    etiquetas: list[str] | str = Field(default_factory=list)

    @model_validator(mode="before")
    @classmethod
    def _normalize_aliases(cls, data: Any) -> Any:
        """Accept both canonical AnkiMBR keys and Prompt Master JSONL aliases seamlessly."""
        if not isinstance(data, dict):
            return data
        d = dict(data)

        if "tipo" not in d and "note_type" in d:
            nt = str(d.pop("note_type") or "").strip().upper()
            d["tipo"] = "CLOZE" if nt in {"CLOZE", "CL"} else "QA"
        elif "tipo" in d and isinstance(d["tipo"], str):
            nt = d["tipo"].strip().upper()
            d["tipo"] = "CLOZE" if nt in {"CLOZE", "CL"} else "QA"

        if "enunciado_ou_sentenca" not in d:
            if "pergunta" in d:
                d["enunciado_ou_sentenca"] = d.pop("pergunta")
            elif "sentenca" in d:
                d["enunciado_ou_sentenca"] = d.pop("sentenca")
            elif "enunciado" in d:
                d["enunciado_ou_sentenca"] = d.pop("enunciado")

        if "notas_adicionais" not in d and "notas" in d:
            d["notas_adicionais"] = d.pop("notas")

        if "referencias" not in d and "fonte" in d:
            d["referencias"] = d.pop("fonte")

        if "etiquetas" not in d and "tags" in d:
            d["etiquetas"] = d.pop("tags")

        if "campo_estudo" not in d:
            for c_alias in (
                "campo_de_estudo",
                "campo",
                "area",
                "study_field",
                "field_of_study",
                "especialidade",
            ):
                if d.get(c_alias):
                    d["campo_estudo"] = d.pop(c_alias)
                    break

        for img_alias in ("images", "imagens_relacionadas", "imagens"):
            if img_alias in d:
                raw_imgs = d.pop(img_alias)
                parsed_figs = parse_images_html_to_figures(raw_imgs)
                if parsed_figs:
                    existing = list(d.get("figuras") or [])
                    existing.extend(parsed_figs)
                    d["figuras"] = existing

        return d

    def all_figures(self) -> list[MediaFigure]:
        """Combine self.figura and self.figuras preserving order, deduplicating by filename."""
        result: list[MediaFigure] = []
        seen_filenames: set[str] = set()

        candidates: list[MediaFigure] = []
        if self.figura is not None:
            if isinstance(self.figura, MediaFigure):
                candidates.append(self.figura)
            elif isinstance(self.figura, dict):
                candidates.append(MediaFigure(**self.figura))
        for f in self.figuras:
            if isinstance(f, MediaFigure):
                candidates.append(f)
            elif isinstance(f, dict):
                candidates.append(MediaFigure(**f))

        for fig in candidates:
            fn = fig.filename.strip()
            if fn and fn not in seen_filenames:
                seen_filenames.add(fn)
                result.append(fig)
        return result

    def renumber_cloze(self) -> None:
        """Automatically renumber cloze deletions cN to strictly sequential c1..cK in order of appearance."""
        if self.tipo != "CLOZE":
            return
        text = self.enunciado_ou_sentenca
        from anki_mcp.checks.cloze import _find_cloze_spans

        spans = _find_cloze_spans(text)
        if not spans:
            return

        mapping: dict[int, int] = {}
        next_idx = 1
        for orig_idx, _, _, _, _ in spans:
            if orig_idx not in mapping:
                mapping[orig_idx] = next_idx
                next_idx += 1

        for orig_idx, target, hint, start, end in reversed(spans):
            new_idx = mapping[orig_idx]
            hint_part = f"::{hint}" if hint is not None else ""
            replacement = f"{{{{c{new_idx}::{target}{hint_part}}}}}"
            text = text[:start] + replacement + text[end:]
        self.enunciado_ou_sentenca = text


class CardPatch(BaseModel):
    """Surgical patch targeting a single card for granular repair."""

    card_index: int | None = None
    card_id: str | None = None
    updates: dict[str, Any] = Field(default_factory=dict)
    reason: str = ""


class QuarantineRecord(BaseModel):
    """Record of a quarantined card that failed validation after repair budget."""

    card_index: int
    card: CardDraft
    diagnostics: list[dict[str, Any]] = Field(default_factory=list)
    reason: str = ""
    timestamp: str = ""
