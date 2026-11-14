"""checks/images.py — anki-image-injector HTML structure & clinical caption validation.

Checks (structural and semantic, read from the Imagens Relacionadas field):
 IMG.CAP_OVER       — more than 3 images in one card
 IMG.PHANTOM        — src filename not in the active catalog
 IMG.NAMESPACE      — filename does not match valid namespaced prefix pattern
 IMG.DUP_IN_CARD    — same image appears twice in the same card
 IMG.STRUCTURE      — malformed <figure>/<figcaption>
 IMG.CAPTION        — empty or bibliographic noise caption
 IMG.BARE_IMG       — bare <img> tag outside <figure>
 IMG.CAPTION_WEAK   — caption too short (<20 chars), generic, auto-generated ("Imagem 1 da página 10"), or raw filename
 IMG.INLINE_STYLE   — inline style in <figure>/<img>/<figcaption>
 IMG.ALT_MISSING    — <img> tag missing alt attribute
"""

from __future__ import annotations

import re

from anki_mcp.models import Diagnostic

_FIG_RE = re.compile(r"<figure[^>]*>(.*?)</figure>", re.DOTALL | re.IGNORECASE)
_FIG_FULL_RE = re.compile(r"<figure\b[^>]*>.*?</figure>", re.DOTALL | re.IGNORECASE)
_IMG_TAG = re.compile(r"<img\b[^>]*>", re.IGNORECASE)
_IMG_SRC = re.compile(r"""<img\b[^>]*?\bsrc\s*=\s*["']([^"']+)["']""", re.IGNORECASE)
_ALT_ATTR = re.compile(r"""\balt\s*=""", re.IGNORECASE)
_INLINE_STYLE_ATTR = re.compile(r"""<(figure|img|figcaption)\b[^>]*?\bstyle\s*=""", re.IGNORECASE)
_FIGCAP = re.compile(r"<figcaption[^>]*>(.*?)</figcaption>", re.DOTALL | re.IGNORECASE)
# Allow lowercase alphanumeric prefix + underscore + lowercase filename
_NS_VALID = re.compile(r"^[a-z0-9][a-z0-9]*_[a-z0-9_.]+$")

# Noise patterns in captions (bibliographic garbage from PDF extraction)
_NOISE_RE = re.compile(
    r"(?:ISBN|Copyright|©|\bReceived\s*:|ISSN|\bDOI\s*:|\bCatalog\s+No\b|Todos\s+os\s+direitos\s+reservados)",
    re.IGNORECASE,
)

# Generic or extractor-boilerplate caption patterns (e.g. "Figura 3", "Imagem 1 da página 10", "Tabela / Quadro estruturado")
_GENERIC_CAPTION_RE = re.compile(
    r"^(?:fig(?:ura)?|imagem?|photo|foto|quadro|tabela)\.?\s*(?:\d+|[ivxlcdm]+)?(?:\s+da\s+p[áa]gina\s+\d+)?\s*[:.-]?\s*$"
    r"|^tabela\s*/\s*quadro\s+estruturado$"
    r"|^\d+\s*linhas?\s*[x×]\s*\d+\s*colunas?$",
    re.IGNORECASE,
)

# File extension pattern in caption (e.g. ends with .png, .jpg, etc.)
_FILE_EXT_RE = re.compile(
    r"\.(?:png|jpe?g|gif|webp|svg)$",
    re.IGNORECASE,
)


def check_images_field(
    html: str,
    catalog_filenames: set[str] | None = None,
    *,
    row: int | None = None,
    field: str = "Imagens Relacionadas",
) -> list[Diagnostic]:
    """Validate the Imagens Relacionadas HTML field."""
    diags: list[Diagnostic] = []

    if not html or not html.strip():
        return diags  # empty field is fine

    # ── IMG.BARE_IMG ─────────────────────────────────────────────────────────
    outside_figures = _FIG_FULL_RE.sub("", html)
    bare_imgs = list(_IMG_TAG.finditer(outside_figures))
    for bare_m in bare_imgs:
        diags.append(
            Diagnostic.from_code(
                "IMG.BARE_IMG",
                f"Bare <img> tag found outside <figure>: '{bare_m.group(0)}'",
                row=row,
                field=field,
                suggestion='Wrap all images in semantic <figure class="anki-figure"><img ...><figcaption>...</figcaption></figure>',
            )
        )
        if re.search(r"\bstyle\s*=", bare_m.group(0), re.IGNORECASE):
            diags.append(
                Diagnostic.from_code(
                    "IMG.INLINE_STYLE",
                    f"Bare <img> tag contains inline style '{bare_m.group(0)}'",
                    row=row,
                    field=field,
                    suggestion="Remove inline style attribute; styling is handled by AnkiMBR CSS variables",
                )
            )

    # ── IMG.CAP_OVER ─────────────────────────────────────────────────────────
    all_img_srcs = [m.group(1) for m in _IMG_SRC.finditer(html)]
    if len(all_img_srcs) > 3:
        diags.append(
            Diagnostic.from_code(
                "IMG.CAP_OVER",
                f"Card has {len(all_img_srcs)} images (maximum 3)",
                row=row,
                field=field,
                suggestion="Remove lower-ranked images to stay within the 3-image cap",
            )
        )

    catalog_lower = (
        {c.lower() for c in catalog_filenames} if catalog_filenames is not None else None
    )

    seen_srcs: set[str] = set()
    for idx, block in enumerate(_FIG_RE.finditer(html), start=1):
        full_block = block.group(0)
        inner = block.group(1)
        img_m = _IMG_SRC.search(inner)
        cap_m = _FIGCAP.search(inner)

        # ── IMG.INLINE_STYLE ─────────────────────────────────────────────────
        for style_m in _INLINE_STYLE_ATTR.finditer(full_block):
            diags.append(
                Diagnostic.from_code(
                    "IMG.INLINE_STYLE",
                    f"Figure #{idx}: inline style in <{style_m.group(1)}> tag breaks theme CSS",
                    row=row,
                    field=field,
                    suggestion='Remove inline style attribute; use class="anki-figure" and AnkiMBR CSS variables',
                )
            )

        # ── IMG.STRUCTURE (img) ──────────────────────────────────────────────
        if not img_m:
            diags.append(
                Diagnostic.from_code(
                    "IMG.STRUCTURE",
                    f"Figure #{idx}: no <img src=...> found inside <figure>",
                    row=row,
                    field=field,
                )
            )
            continue

        src = img_m.group(1)

        # ── IMG.ALT_MISSING ──────────────────────────────────────────────────
        img_tag_m = _IMG_TAG.search(inner)
        if img_tag_m and not _ALT_ATTR.search(img_tag_m.group(0)):
            diags.append(
                Diagnostic.from_code(
                    "IMG.ALT_MISSING",
                    f"Figure #{idx}: <img> tag missing alt attribute",
                    row=row,
                    field=field,
                    suggestion='Add an alt attribute describing the image: <img src="..." alt="...">',
                )
            )

        # ── IMG.NAMESPACE ────────────────────────────────────────────────────
        if not _NS_VALID.match(src):
            diags.append(
                Diagnostic.from_code(
                    "IMG.NAMESPACE",
                    f"Figure #{idx}: src '{src}' does not match required lowercase namespace pattern <slug>_<filename>",
                    row=row,
                    field=field,
                    suggestion="Run image_namespace(slug, filename) so the filename is lowercase and prefixed (e.g. 'ceratite_p14_1.png')",
                )
            )

        # ── IMG.PHANTOM ──────────────────────────────────────────────────────
        if (
            catalog_filenames is not None
            and src not in catalog_filenames
            and (catalog_lower is None or src.lower() not in catalog_lower)
        ):
            diags.append(
                Diagnostic.from_code(
                    "IMG.PHANTOM",
                    f"Figure #{idx}: src '{src}' not found in the active catalog",
                    row=row,
                    field=field,
                    suggestion="Only inject images whose filenames exist in the loaded catalog (use image_candidates + image_namespace)",
                )
            )

        # ── IMG.DUP_IN_CARD ──────────────────────────────────────────────────
        if src in seen_srcs:
            diags.append(
                Diagnostic.from_code(
                    "IMG.DUP_IN_CARD",
                    f"Image '{src}' appears more than once in this card",
                    row=row,
                    field=field,
                )
            )
        seen_srcs.add(src)

        # ── IMG.STRUCTURE (figcaption) ───────────────────────────────────────
        if not cap_m:
            diags.append(
                Diagnostic.from_code(
                    "IMG.STRUCTURE",
                    f"Figure #{idx}: missing <figcaption>",
                    row=row,
                    field=field,
                    suggestion="Add a contextual clinical caption inside <figcaption>",
                )
            )
            continue

        caption = cap_m.group(1).strip()

        # ── IMG.CAPTION & IMG.CAPTION_WEAK ────────────────────────────────────
        if not caption:
            diags.append(
                Diagnostic.from_code(
                    "IMG.CAPTION",
                    f"Figure #{idx}: empty caption",
                    row=row,
                    field=field,
                )
            )
        elif _NOISE_RE.search(caption):
            diags.append(
                Diagnostic.from_code(
                    "IMG.CAPTION",
                    f"Figure #{idx}: caption appears to contain bibliographic noise (ISBN/Copyright/…)",
                    row=row,
                    field=field,
                    suggestion="Remove noise and keep only the descriptive clinical text",
                )
            )
        else:
            text_caption = re.sub(r"<[^>]+>", "", caption).strip()
            if not text_caption:
                diags.append(
                    Diagnostic.from_code(
                        "IMG.CAPTION",
                        f"Figure #{idx}: empty caption",
                        row=row,
                        field=field,
                    )
                )
            elif (
                len(text_caption) < 20
                or _GENERIC_CAPTION_RE.match(text_caption)
                or _FILE_EXT_RE.search(text_caption)
            ):
                diags.append(
                    Diagnostic.from_code(
                        "IMG.CAPTION_WEAK",
                        f"Figure #{idx}: caption '{text_caption}' is weak (too short, generic, or raw extractor placeholder)",
                        row=row,
                        field=field,
                        suggestion="Provide a contextual clinical caption (>= 20 chars) explaining the visual finding and its connection to the card",
                    )
                )

    return diags
