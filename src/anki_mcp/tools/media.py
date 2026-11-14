"""tools/media.py — image & table catalog, semantic candidate ranking, and media namespace MCP tools.

Tools:
 catalog_load      — load/index a catalog (catalog.json, central_images.json, anki_catalog.csv, catalog.md)
                     and optionally register it in the deck Manifesto
 image_namespace   — apply anti-collision slug prefix to a raw filename → namespaced name
 image_candidates  — token-based, accent-insensitive semantic search across figures and tables
 image_sync        — check consistency and transactionally copy media to collection.media
 image_report      — summary: total, figures, tables, noise, synced, missing per material
"""

from __future__ import annotations

import csv
import hashlib
import inspect
import json
import os
import re
import shutil
import tempfile
import unicodedata
from functools import wraps
from pathlib import Path
from typing import Any, Literal

from fastmcp import FastMCP
from filelock import FileLock, Timeout

from anki_mcp.core.deck import save_manifesto, transaction
from anki_mcp.core.safe import error_result, safe_tool
from anki_mcp.models import CatalogEntry, CatalogRef
from anki_mcp.models.draft import MediaFigure, clean_extractor_caption


def _collection_sync_lock(fn):
    """Serialize inspection, commit and rollback across workers for one collection."""

    @wraps(fn)
    def locked(*args, **kwargs):
        bound = inspect.signature(fn).bind(*args, **kwargs)
        try:
            explicit = bound.arguments.get("collection_media_dir")
            cm = (
                _collection_media(explicit) if explicit is not None else _collection_media()
            ).resolve()
        except FileNotFoundError as exc:
            return error_result("MEDIA.CATALOG_ERROR", str(exc), "Check collection_media_dir.")
        try:
            with FileLock(str(cm / ".anki-media-sync.lock"), timeout=5):
                return fn(*args, **kwargs)
        except Timeout:
            return error_result(
                "GOV.LOCK",
                "Media collection is busy.",
                "Retry after the current synchronization finishes.",
            )

    return locked


# In-memory cache of loaded catalogs so pipeline checks in the same session always find them
_ACTIVE_CATALOG_PATHS: set[Path] = set()
_ACTIVE_CATALOG_FILENAMES: set[str] = set()

# Noise patterns from PDF extraction (copyright pages, generated dummy captions, tiny icons)
_AUTO_CAPTION_RE = re.compile(
    r"^(?:imagem|figura|image|figure|tabela|quadro)\s+\d+\s+(?:da\s+p[áa]gina|of\s+page)\s+\d+\s*$",
    re.IGNORECASE,
)
_BIBLIO_NOISE_RE = re.compile(
    r"(?:todos\s+os\s+direitos\s+reservados|lei\s+n[ºo°]?\s*9\.?610|proibida\s+a\s+reprodu[çc][ãa]o"
    r"|licencia\s+creative\s+commons|equipo\s+editorial|diagramaci[óo]n\s*:|design\s+de\s+capa"
    r"|dados\s+internacionais\s+de\s+cataloga[çc][ãa]o|ficha\s+catalogr[áa]fica|isbn\s*[0-9\-]{8,}"
    r"|copyright\s*©|\bdoi\s*:\s*10\.|\bissn\s*:\s*\d{4})",
    re.IGNORECASE,
)

_PT_STOPWORDS = frozenset(
    {
        "a",
        "ao",
        "aos",
        "as",
        "com",
        "como",
        "da",
        "das",
        "de",
        "do",
        "dos",
        "e",
        "em",
        "entre",
        "na",
        "nas",
        "no",
        "nos",
        "o",
        "os",
        "ou",
        "para",
        "pela",
        "pelas",
        "pelo",
        "pelos",
        "por",
        "qual",
        "quais",
        "que",
        "se",
        "sem",
        "seu",
        "sua",
        "sobre",
        "um",
        "uma",
        "of",
        "in",
        "on",
        "the",
        "and",
        "or",
        "for",
        "to",
        "with",
        "by",
        "from",
        "at",
    }
)


def _strip_accents(s: str) -> str:
    return "".join(
        c for c in unicodedata.normalize("NFD", s.lower()) if unicodedata.category(c) != "Mn"
    )


def _tokenize_query(text: str) -> list[str]:
    norm = _strip_accents(text)
    tokens = re.findall(r"[a-z0-9]{2,}", norm)
    return [t for t in tokens if t not in _PT_STOPWORDS]


def _normalize_slug(raw_slug: str) -> str:
    """Produce a valid AnkiMBR slug starting with a lowercase ASCII letter."""
    s = _strip_accents(raw_slug.strip())
    s = re.sub(r"^images_", "", s)
    s = re.sub(r"[^a-z0-9_]+", "_", s).strip("_")
    s = re.sub(r"_+", "_", s)
    if not s:
        return "deck"
    if s[0].isdigit():
        s = f"doc_{s}"
    return s


def _build_namespaced(slug: str, filename: str) -> str:
    """Ensure 100% lowercase ASCII namespaced filename without redundant double-prefixing."""
    clean_slug = _normalize_slug(slug) if slug else ""
    fn_norm = _strip_accents(filename.strip())
    # Separate stem and extension
    p = Path(fn_norm)
    ext = re.sub(r"[^a-z0-9.]+", "", p.suffix.lower())
    stem = re.sub(r"[^a-z0-9_]+", "_", p.stem.lower()).strip("_")
    stem = re.sub(r"_+", "_", stem)
    if not stem:
        stem = "img"

    if clean_slug:
        # Avoid redundant prefixing when extractor already prefixed the file with the first words of the PDF
        # e.g., slug='tratado_de_neurologia_da_academia_brasileira_2', stem='tratado_de_p38_5'
        slug_parts = clean_slug.split("_")
        short_prefix = "_".join(slug_parts[:2]) if len(slug_parts) >= 2 else clean_slug
        if stem.startswith(f"{clean_slug}_") or stem == clean_slug:
            final_stem = stem
        elif len(short_prefix) >= 5 and stem.startswith(f"{short_prefix}_"):
            final_stem = stem
        else:
            final_stem = f"{clean_slug}_{stem}"
    else:
        final_stem = stem

    if final_stem[0].isdigit():
        final_stem = f"doc_{final_stem}"

    return f"{final_stem}{ext}"


def _classify_noise(
    caption: str,
    source: str,
    width_px: int | None,
    height_px: int | None,
    page: int | None,
    item_type: str,
) -> tuple[bool, str]:
    """Detect extraction noise (dummy captions, copyright headers, tiny publisher logos)."""
    clean_cap = clean_extractor_caption(caption)
    if not clean_cap:
        return True, "empty_caption"
    if _AUTO_CAPTION_RE.match(clean_cap):
        return True, "auto_generated_caption"
    if _BIBLIO_NOISE_RE.search(clean_cap):
        return True, "bibliographic_or_copyright_noise"
    if (
        item_type == "figure"
        and width_px is not None
        and height_px is not None
        and width_px < 180
        and height_px < 180
    ):
        return True, f"tiny_icon_or_logo ({width_px}x{height_px}px)"
    return False, ""


def _infer_item_type(
    raw_type: str | None, filename: str, raw_caption: str
) -> Literal["figure", "table"]:
    if raw_type and str(raw_type).strip().lower() in {"table", "tabela", "quadro"}:
        return "table"
    if "_tabela_" in filename.lower() or "table-title" in raw_caption.lower():
        return "table"
    return "figure"


def _load_catalog_csv(path: Path) -> list[CatalogEntry]:
    """Universal loader supporting:
    1. catalog.json (Pdf-image-extractor-gm per-document JSON)
    2. central_images.json (Pdf-image-extractor-gm global hub JSON)
    3. 2-col anki_catalog.csv / anki_deck.csv from extractor
    4. catalog.md (Markdown table from extractor)
    5. Canonical 4+ col pipe-delimited CSV
    """
    entries: list[CatalogEntry] = []
    if not path.exists():
        return entries

    if path.is_dir():
        # Prefer catalog.json first because it carries complete metadata (type, page, dimensions, rows/cols)
        for cand_name in (
            "catalog.json",
            "anki_catalog.csv",
            "anki_deck.csv",
            "catalog.md",
            "central_images.json",
        ):
            cand = path / cand_name
            if cand.is_file():
                return _load_catalog_csv(cand)
        return entries

    suffix = path.suffix.lower()

    # ── 1 & 2: JSON formats (catalog.json or central_images.json) ─────────────
    if suffix == ".json":
        try:
            data = json.loads(path.read_text(encoding="utf-8-sig"))
        except Exception:
            return []

        # Case A: central_images.json -> {"images": [{"folder": ..., "doc_title": ..., "filename": ...}]}
        if isinstance(data, dict) and "images" in data and isinstance(data["images"], list):
            for it in data["images"]:
                fn = str(it.get("filename") or "").strip()
                if not fn:
                    continue
                doc_title = str(it.get("doc_title") or it.get("folder") or "").strip()
                slug = _normalize_slug(doc_title or fn.split("_p")[0])
                raw_cap = str(it.get("caption") or it.get("caption_html") or "").strip()
                clean_cap = clean_extractor_caption(raw_cap)
                itype = _infer_item_type(it.get("type"), fn, raw_cap)
                w_px = it.get("width_px")
                h_px = it.get("height_px")
                page = it.get("page")
                src_kind = str(it.get("source") or "")
                is_noise, n_reason = _classify_noise(clean_cap, src_kind, w_px, h_px, page, itype)
                namespaced = _build_namespaced(slug, fn)
                entries.append(
                    CatalogEntry(
                        filename=fn,
                        page=page if isinstance(page, int) else None,
                        caption=clean_cap,
                        anki_tag=str(it.get("anki_tag") or "").strip(),
                        material=doc_title,
                        slug=slug,
                        namespaced=namespaced,
                        item_type=itype,
                        title=it.get("title"),
                        source=src_kind,
                        width_px=w_px,
                        height_px=h_px,
                        rows=it.get("rows"),
                        cols=it.get("cols"),
                        continued=bool(it.get("continued", False)),
                        continuation_of=it.get("continuation_of"),
                        noise=is_noise,
                        noise_reason=n_reason,
                    )
                )
            return entries

        # Case B: per-document catalog.json -> {"pdf_name": ..., "items": [...]}
        items = (
            data.get("items", [])
            if isinstance(data, dict)
            else (data if isinstance(data, list) else [])
        )
        raw_slug = ""
        material_name = ""
        if isinstance(data, dict):
            material_name = str(data.get("pdf_name") or "").strip()
            raw_slug = str(data.get("slug") or material_name or path.parent.name)
        slug = _normalize_slug(raw_slug)

        for it in items:
            fn = str(it.get("filename") or "").strip()
            if not fn:
                continue
            raw_cap = str(it.get("caption") or it.get("caption_html") or "").strip()
            clean_cap = clean_extractor_caption(raw_cap)
            itype = _infer_item_type(it.get("type"), fn, raw_cap)
            w_px = it.get("width_px")
            h_px = it.get("height_px")
            page = it.get("page")
            src_kind = str(it.get("source") or "")
            is_noise, n_reason = _classify_noise(clean_cap, src_kind, w_px, h_px, page, itype)
            namespaced = _build_namespaced(slug, fn)
            entries.append(
                CatalogEntry(
                    filename=fn,
                    page=page if isinstance(page, int) else None,
                    caption=clean_cap,
                    anki_tag=str(it.get("anki_tag") or "").strip(),
                    material=material_name,
                    slug=slug,
                    namespaced=namespaced,
                    item_type=itype,
                    title=it.get("title"),
                    source=src_kind,
                    width_px=w_px,
                    height_px=h_px,
                    rows=it.get("rows"),
                    cols=it.get("cols"),
                    continued=bool(it.get("continued", False)),
                    continuation_of=it.get("continuation_of"),
                    noise=is_noise,
                    noise_reason=n_reason,
                )
            )
        return entries

    # If sibling catalog.json exists next to anki_catalog.csv, enrich from catalog.json!
    sibling_json = path.parent / "catalog.json"
    if (
        suffix in {".csv", ".md"}
        and sibling_json.is_file()
        and path.name in {"anki_catalog.csv", "anki_deck.csv", "catalog.md"}
    ):
        json_entries = _load_catalog_csv(sibling_json)
        if json_entries:
            return json_entries

    text = path.read_text(encoding="utf-8").lstrip("\ufeff")
    lines = [
        line.strip() for line in text.splitlines() if line.strip() and not line.startswith("#")
    ]
    if not lines:
        return entries

    # ── 3: Markdown catalog.md table ──────────────────────────────────────────
    if suffix == ".md":
        slug = _normalize_slug(path.parent.name)
        for line in lines:
            if not line.startswith("|") or "---" in line or " Tag Anki " in line:
                continue
            parts = [p.strip() for p in line.strip("|").split("|")]
            if len(parts) < 4:
                continue
            raw_type, img_col, page_col, cap_col = parts[0], parts[1], parts[2], parts[3]
            m_fn = re.search(r"\(([^)]+)\)", img_col)
            fn = m_fn.group(1).strip() if m_fn else img_col.strip()
            if not fn:
                continue
            page = int(page_col) if page_col.isdigit() else None
            clean_cap = clean_extractor_caption(cap_col)
            itype = _infer_item_type(raw_type, fn, cap_col)
            is_noise, n_reason = _classify_noise(clean_cap, "", None, None, page, itype)
            namespaced = _build_namespaced(slug, fn)
            entries.append(
                CatalogEntry(
                    filename=fn,
                    page=page,
                    caption=clean_cap,
                    material=path.parent.name.replace("images_", ""),
                    slug=slug,
                    namespaced=namespaced,
                    item_type=itype,
                    noise=is_noise,
                    noise_reason=n_reason,
                )
            )
        return entries

    # ── 4: Canonical CSV with header ──────────────────────────────────────────
    first_line = lines[0]
    has_header = "filename" in first_line.lower() or "slug" in first_line.lower()
    if has_header:
        reader = csv.DictReader(lines, delimiter="|")
        for row in reader:
            raw_slug = (row.get("slug") or "").strip()
            slug = _normalize_slug(raw_slug) if raw_slug else ""
            filename = (row.get("filename") or "").strip()
            if not filename:
                continue
            raw_cap = (row.get("caption") or "").strip()
            clean_cap = clean_extractor_caption(raw_cap)
            page_str = (row.get("page") or "").strip()
            page = int(page_str) if page_str.isdigit() else None
            itype = _infer_item_type(row.get("type"), filename, raw_cap)
            is_noise, n_reason = _classify_noise(clean_cap, "", None, None, page, itype)
            namespaced = _build_namespaced(slug, filename) if slug else filename.lower()
            entries.append(
                CatalogEntry(
                    filename=filename,
                    page=page,
                    caption=clean_cap,
                    anki_tag=(row.get("anki_tag") or "").strip(),
                    material=(row.get("material") or "").strip(),
                    slug=slug,
                    namespaced=namespaced,
                    item_type=itype,
                    noise=is_noise,
                    noise_reason=n_reason,
                )
            )
        return entries

    # ── 5: Headerless 2-column format from extractor: "<img src=""fn"">" | caption ──
    slug = _normalize_slug(path.parent.name)
    for line in lines:
        parts = [p.strip() for p in line.split("|")]
        col0 = parts[0]
        m = re.search(r"""src=["']*([^"'<>]+?)["']+""", col0)
        if not m:
            m = re.search(r"""src=["']?([^"'<>]+)""", col0)
        filename = m.group(1).strip() if m else col0.strip().strip('"').strip("'")
        raw_cap = parts[1].strip().strip('"').replace('""', '"') if len(parts) > 1 else ""
        if not filename:
            continue
        # Extract title if col0 has <div class="anki-fig-title"><b>...</b></div>
        m_title = re.search(
            r"anki-fig-title[^>]*>\s*<(?:b|strong)>(.*?)</(?:b|strong)>", col0, re.I
        )
        title = clean_extractor_caption(m_title.group(1)) if m_title else None
        clean_cap = clean_extractor_caption(raw_cap)
        itype = _infer_item_type(None, filename, raw_cap)
        # Extract page from filename pattern _p<page>_
        m_page = re.search(r"_p(\d+)_", filename, re.I)
        page = int(m_page.group(1)) if m_page else None
        is_noise, n_reason = _classify_noise(clean_cap, "", None, None, page, itype)
        namespaced = _build_namespaced(slug, filename)
        entries.append(
            CatalogEntry(
                filename=filename,
                page=page,
                caption=clean_cap,
                anki_tag="",
                material=path.parent.name.replace("images_", ""),
                slug=slug,
                namespaced=namespaced,
                item_type=itype,
                title=title,
                noise=is_noise,
                noise_reason=n_reason,
            )
        )
    return entries


def get_cached_catalog_filenames() -> set[str]:
    """Return all filenames and namespaced names loaded in the current session."""
    return set(_ACTIVE_CATALOG_FILENAMES)


def _register_catalog_in_memory(path: Path, entries: list[CatalogEntry]) -> None:
    _ACTIVE_CATALOG_PATHS.add(path.resolve())
    for e in entries:
        if e.namespaced:
            _ACTIVE_CATALOG_FILENAMES.add(e.namespaced)
            _ACTIVE_CATALOG_FILENAMES.add(e.namespaced.lower())
        if e.filename:
            _ACTIVE_CATALOG_FILENAMES.add(e.filename)
            _ACTIVE_CATALOG_FILENAMES.add(e.filename.lower())


def _hash_dir(p: Path) -> str:
    """Stable hash of a directory listing (for version comparison)."""
    target_dir = p if p.is_dir() else p.parent
    names = sorted(f.name for f in target_dir.iterdir() if f.is_file())
    return "sha256:" + hashlib.sha256("\n".join(names).encode()).hexdigest()[:16]


def _collection_media(collection_media_dir: str | Path | None = None) -> Path:
    """Path to Anki collection.media for the configured profile or explicit staging directory.

    Order of resolution:
    1. Explicit collection_media_dir argument if provided.
    2. ANKI_COLLECTION_MEDIA environment variable if set.
    3. Windows APPDATA discovery under APPDATA/Anki2/<profile>/collection.media.
    """
    if collection_media_dir is not None:
        p = Path(collection_media_dir)
        if p.is_dir():
            return p
        raise FileNotFoundError(f"Specified collection_media_dir not found: {collection_media_dir}")

    env_dir = os.environ.get("ANKI_COLLECTION_MEDIA")
    if env_dir:
        p = Path(env_dir)
        if p.is_dir():
            return p
        raise FileNotFoundError(f"ANKI_COLLECTION_MEDIA directory not found: {env_dir}")

    appdata_env = os.environ.get("APPDATA")
    appdata = Path(appdata_env) if appdata_env else (Path.home() / "AppData" / "Roaming")
    candidate = appdata / "Anki2" / "Usuário 1" / "collection.media"
    if candidate.is_dir():
        return candidate
    candidate_en = appdata / "Anki2" / "User 1" / "collection.media"
    if candidate_en.is_dir():
        return candidate_en
    anki2 = appdata / "Anki2"
    if anki2.is_dir():
        for profile_dir in anki2.iterdir():
            if profile_dir.is_dir():
                cm = profile_dir / "collection.media"
                if cm.is_dir():
                    return cm
    raise FileNotFoundError(
        "Anki collection.media not found. Set ANKI_COLLECTION_MEDIA or pass collection_media_dir."
    )


def score_catalog_entry(entry: CatalogEntry, query: str) -> float:
    """Rank a CatalogEntry against a clinical query (token + phrase + title + tag matching)."""
    if not query or not query.strip():
        return 1.0 if not entry.noise else 0.1
    q_norm = _strip_accents(query.strip())
    q_tokens = _tokenize_query(query)
    if not q_tokens:
        q_tokens = [q_norm]

    cap_norm = _strip_accents(entry.caption or "")
    title_norm = _strip_accents(entry.title or "")
    tag_norm = _strip_accents(entry.anki_tag or "")
    fn_norm = _strip_accents(entry.filename or "")
    mat_norm = _strip_accents(entry.material or "")
    combined = f"{title_norm} {cap_norm} {tag_norm} {fn_norm} {mat_norm}"

    score = 0.0
    # 1. Exact phrase match bonus
    if len(q_norm) >= 3 and q_norm in combined:
        score += 5.0

    # 2. Per-token weighted match (supports prefix/stem matching >= 4 chars)
    matched_tokens = 0
    for tok in q_tokens:
        stem = tok[: max(4, len(tok) - 2)] if len(tok) >= 6 else tok
        tok_matched = False
        if tok in title_norm or (len(stem) >= 4 and stem in title_norm):
            score += 3.5
            tok_matched = True
        if tok in cap_norm or (len(stem) >= 4 and stem in cap_norm):
            score += 2.5
            tok_matched = True
        if tok in tag_norm or tok in fn_norm:
            score += 2.0
            tok_matched = True
        if tok_matched:
            matched_tokens += 1

    if matched_tokens == 0 and score == 0.0:
        return 0.0

    # Coverage multiplier (reward matching multiple query concepts)
    coverage = matched_tokens / len(q_tokens)
    score *= 0.5 + 0.5 * coverage

    # Penalize noise entries heavily
    if entry.noise:
        score *= 0.15

    return round(score, 3)


# ── Tool registration ─────────────────────────────────────────────────────────


def register_media_tools(mcp: FastMCP) -> None:

    @mcp.tool(
        description=(
            "Load and index an extractor catalog (catalog.json, anki_catalog.csv, catalog.md, or central_images.json) "
            "into active memory and optionally register it in the deck's manifesto.json. "
            "Returns counts of figures, tables, valid clinical items, filtered extraction noise, and version hash."
        )
    )
    @safe_tool
    def catalog_load(catalog_path: str, deck_dir: str | None = None) -> dict[str, Any]:
        p = Path(catalog_path)
        if not p.exists():
            return error_result(
                "MEDIA.CATALOG_ERROR",
                f"File or directory not found: {catalog_path}",
                "Check the path to catalog.json or anki_catalog.csv.",
            )

        try:
            entries = _load_catalog_csv(p)
        except Exception as exc:
            return error_result(
                "MEDIA.CATALOG_ERROR", str(exc), "Check the path and the catalog format."
            )

        _register_catalog_in_memory(p, entries)

        slugs = sorted({e.slug for e in entries if e.slug})
        figures_count = sum(1 for e in entries if e.item_type == "figure")
        tables_count = sum(1 for e in entries if e.item_type == "table")
        noise_entries = [e for e in entries if e.noise]
        valid_entries = [e for e in entries if not e.noise]
        no_slug = [e.filename for e in entries if not e.slug]
        no_cap = [e.filename for e in entries if not e.caption]

        version_hash = _hash_dir(p)
        registered_in_manifesto = False

        # Persist CatalogRef into manifesto.json if deck_dir is provided
        if deck_dir:
            deck_path = Path(deck_dir)
            if (deck_path / "manifesto.json").is_file():
                try:
                    with transaction(deck_dir) as m:
                        primary_slug = slugs[0] if slugs else m.deck_slug
                        material_name = (
                            next((e.material for e in entries if e.material), "") or primary_slug
                        )
                        resolved_str = str(p.resolve())
                        existing_idx = next(
                            (
                                i
                                for i, c in enumerate(m.catalogs)
                                if c.path == resolved_str or c.slug == primary_slug
                            ),
                            None,
                        )
                        ref = CatalogRef(
                            material=material_name,
                            slug=primary_slug,
                            path=resolved_str,
                            version_hash=version_hash,
                            media_dir=str((p if p.is_dir() else p.parent).resolve()),
                            prefix=f"{primary_slug}_",
                        )
                        if existing_idx is not None:
                            m.catalogs[existing_idx] = ref
                        else:
                            m.catalogs.append(ref)
                        save_manifesto(deck_dir, m)
                        registered_in_manifesto = True
                except Exception:
                    registered_in_manifesto = False

        return {
            "status": "OK",
            "count": len(entries),
            "valid_clinical_count": len(valid_entries),
            "figures_count": figures_count,
            "tables_count": tables_count,
            "noise_filtered_count": len(noise_entries),
            "slugs": slugs,
            "no_slug": no_slug[:20],
            "no_caption": no_cap[:20],
            "version_hash": version_hash,
            "catalog_path": str(p.resolve()),
            "registered_in_manifesto": registered_in_manifesto,
        }

    @mcp.tool(
        description=(
            "Apply the slug prefix to convert a raw filename to its Anki-safe namespaced form. "
            "Avoids double-prefixing when extractor filenames already contain the document prefix."
        )
    )
    @safe_tool
    def image_namespace(slug: str, filename: str) -> dict[str, Any]:
        clean_slug = _normalize_slug(slug) if slug.strip() else ""
        filename = filename.strip()
        namespaced = _build_namespaced(clean_slug, filename)

        result: dict[str, Any] = {
            "slug": clean_slug,
            "original": filename,
            "namespaced": namespaced,
        }

        try:
            cm = _collection_media()
            result["in_collection"] = (cm / namespaced).exists()
            result["collection_media"] = str(cm)
        except FileNotFoundError as exc:
            result["in_collection"] = None
            result["collection_media_error"] = str(exc)

        return result

    @mcp.tool(
        description=(
            "Rank and retrieve relevant figure and table entries from an extractor catalog. "
            "Uses token-based, accent-insensitive clinical matching across title, caption, tags, and filename, "
            "automatically filtering out extraction noise (dummy captions, copyright headers, tiny logos). "
            "item_type: 'all' | 'figure' | 'table'. "
            "Returns ranked candidates with ready-to-use namespaced filename, table dimensions, and <figure> HTML."
        )
    )
    @safe_tool
    def image_candidates(
        catalog_path: str,
        query: str,
        max_results: int = 10,
        item_type: Literal["all", "figure", "table"] = "all",
        include_noise: bool = False,
    ) -> dict[str, Any]:
        p = Path(catalog_path)
        if not p.exists():
            return error_result(
                "MEDIA.CATALOG_ERROR",
                f"File or directory not found: {catalog_path}",
                "Check the path to catalog.json or anki_catalog.csv.",
            )

        entries = _load_catalog_csv(p)
        _register_catalog_in_memory(p, entries)

        scored: list[tuple[float, CatalogEntry]] = []
        for e in entries:
            if not include_noise and e.noise:
                continue
            if item_type != "all" and e.item_type != item_type:
                continue
            sc = score_catalog_entry(e, query)
            if sc > 0:
                scored.append((sc, e))

        scored.sort(key=lambda pair: (-pair[0], pair[1].page or 9999))
        top = scored[:max_results]

        out_entries: list[dict[str, Any]] = []
        for sc, e in top:
            fig_html = MediaFigure(
                filename=e.namespaced,
                caption=e.caption,
                title=e.title or "",
                item_type=e.item_type,
            ).render_html()
            item_dict: dict[str, Any] = {
                "score": sc,
                "item_type": e.item_type,
                "filename": e.filename,
                "namespaced": e.namespaced,
                "page": e.page,
                "title": e.title,
                "caption": e.caption,
                "tag": e.anki_tag,
                "slug": e.slug,
                "figure_html": fig_html,
            }
            if e.item_type == "table":
                item_dict["table_meta"] = {
                    "rows": e.rows,
                    "cols": e.cols,
                    "continued": e.continued,
                    "recommendation": (
                        "Converter em <table> HTML nativo em Notas Adicionais (<=4 cols, <=8 linhas)"
                        if (e.cols or 0) <= 4 and (e.rows or 0) <= 8
                        else "Manter como quadro visual <figure> em Imagens Relacionadas (>4 cols ou >8 linhas)"
                    ),
                }
            out_entries.append(item_dict)

        return {
            "status": "OK",
            "query": query,
            "item_type_filter": item_type,
            "count": len(out_entries),
            "entries": out_entries,
        }

    @mcp.tool(
        description=(
            "Check consistency between a catalog and Anki collection.media. "
            "If apply=True, executes transactional copying of missing files (under their namespaced name) "
            "to collection.media, skipping noise entries unless copy_noise=True."
        )
    )
    @safe_tool
    @_collection_sync_lock
    def image_sync(
        catalog_path: str,
        apply: bool = False,
        only_filenames: str = "",
        copy_noise: bool = False,
        collection_media_dir: str | None = None,
    ) -> dict[str, Any]:
        p = Path(catalog_path)
        if not p.exists():
            return error_result(
                "MEDIA.CATALOG_ERROR",
                f"File not found: {catalog_path}",
                "Check the path and the CSV/JSON format.",
            )

        try:
            if collection_media_dir is not None:
                cm = _collection_media(collection_media_dir)
            else:
                cm = _collection_media()
        except FileNotFoundError as exc:
            return error_result(
                "MEDIA.CATALOG_ERROR", str(exc), "Check Anki collection.media directory."
            )

        source_dir = p.parent if p.is_file() else p
        entries = _load_catalog_csv(p)
        _register_catalog_in_memory(p, entries)

        filter_set = {f.strip().lower() for f in only_filenames.split(",") if f.strip()}
        synced, missing, copied, collisions = [], [], [], []
        skipped_noise = 0
        to_copy: list[tuple[Path, Path, str]] = []
        planned: dict[str, str] = {}

        # 1. Complete validation upfront
        for e in entries:
            if not e.namespaced:
                continue
            if not copy_noise and e.noise and not filter_set:
                skipped_noise += 1
                continue
            if (
                filter_set
                and e.namespaced.lower() not in filter_set
                and e.filename.lower() not in filter_set
            ):
                continue

            target = cm / e.namespaced
            src_file = source_dir / e.filename
            if not src_file.exists():
                src_file = source_dir / e.namespaced

            key = str(target.resolve()).casefold()
            if src_file.is_file():
                digest = hashlib.sha256(src_file.read_bytes()).hexdigest()
                if key in planned:
                    if planned[key] != digest:
                        collisions.append(
                            {
                                "namespaced": e.namespaced,
                                "source": str(src_file),
                                "target": str(target),
                                "source_sha256": digest,
                                "target_sha256": planned[key],
                            }
                        )
                    continue
                planned[key] = digest
            if target.exists():
                if src_file.exists():
                    src_sha = hashlib.sha256(src_file.read_bytes()).hexdigest()
                    dst_sha = hashlib.sha256(target.read_bytes()).hexdigest()
                    if src_sha != dst_sha:
                        collisions.append(
                            {
                                "namespaced": e.namespaced,
                                "source": str(src_file),
                                "target": str(target),
                                "source_sha256": src_sha,
                                "target_sha256": dst_sha,
                            }
                        )
                        continue
                synced.append(e.namespaced)
            else:
                if src_file.exists():
                    to_copy.append((src_file, target, e.namespaced))
                else:
                    missing.append({"namespaced": e.namespaced, "caption": e.caption})

        # If not applying or if collisions block copy, candidates remain missing
        if not apply or collisions:
            for _, _, ns in to_copy:
                cap = next((e.caption for e in entries if e.namespaced == ns), "")
                missing.append({"namespaced": ns, "caption": cap})

            active_total = len(synced) + len(missing) + len(collisions)
            ratio = round(len(synced) / active_total, 4) if active_total else 1.0

            if collisions:
                return error_result(
                    "MEDIA.COLLISION",
                    f"Found {len(collisions)} collision(s) between catalog and collection.media",
                    "Resolve filename collisions or update catalog before syncing.",
                    catalog=str(p),
                    collection=str(cm),
                    total_catalog=len(entries),
                    active_evaluated=active_total,
                    skipped_noise=skipped_noise,
                    synced=len(synced),
                    copied=0,
                    missing=len(missing),
                    collisions=collisions,
                    sync_ratio=ratio,
                    missing_list=missing[:50],
                )

            return {
                "status": "OK",
                "catalog": str(p),
                "collection": str(cm),
                "total_catalog": len(entries),
                "active_evaluated": active_total,
                "skipped_noise": skipped_noise,
                "synced": len(synced),
                "copied": 0,
                "missing": len(missing),
                "collisions": collisions,
                "sync_ratio": ratio,
                "missing_list": missing[:50],
            }

        if missing:
            return error_result(
                "MEDIA.SOURCE_MISSING",
                "Source files are missing; no files were copied.",
                "Restore all source files before applying the batch.",
                missing=len(missing),
                missing_list=missing,
                copied=0,
            )

        # 2. Transactional copy with temp + replace and batch rollback on OSError
        batch_copied: list[Path] = []
        for src_file, target, namespaced in to_copy:
            tmp_target = None
            try:
                fd, name = tempfile.mkstemp(prefix=".tmp_", dir=cm)
                os.close(fd)
                tmp_target = Path(name)
                shutil.copy2(src_file, tmp_target)
                os.replace(tmp_target, target)
                batch_copied.append(target)
                copied.append(namespaced)
                synced.append(namespaced)
            except OSError as exc:
                rollback_errors = []
                rolled_back = 0
                if tmp_target is not None and tmp_target.exists():
                    try:
                        tmp_target.unlink()
                    except OSError as cleanup_exc:
                        rollback_errors.append({"path": str(tmp_target), "error": str(cleanup_exc)})
                for rollback_file in batch_copied:
                    try:
                        if rollback_file.exists():
                            rollback_file.unlink()
                        rolled_back += 1
                    except OSError as cleanup_exc:
                        rollback_errors.append(
                            {"path": str(rollback_file), "error": str(cleanup_exc)}
                        )
                remaining_missing = list(missing)
                for _, _, ns in to_copy:
                    if ns not in [m["namespaced"] for m in remaining_missing]:
                        cap = next((e.caption for e in entries if e.namespaced == ns), "")
                        remaining_missing.append({"namespaced": ns, "caption": cap})
                initial_synced = len(synced) - len(batch_copied)
                active_total = initial_synced + len(remaining_missing) + len(collisions)
                ratio = round(initial_synced / active_total, 4) if active_total else 1.0
                return error_result(
                    "MEDIA.IO_ERROR",
                    f"I/O error copying {src_file.name} to collection.media ({exc}); rolled back {rolled_back} file(s)",
                    "Check write permissions and free disk space in collection.media.",
                    catalog=str(p),
                    collection=str(cm),
                    failed_file=str(target),
                    rolled_back_count=rolled_back,
                    rollback_errors=rollback_errors,
                    rollback_complete=not rollback_errors,
                    total_catalog=len(entries),
                    active_evaluated=active_total,
                    skipped_noise=skipped_noise,
                    synced=initial_synced + len(batch_copied) - rolled_back,
                    copied=len(batch_copied) - rolled_back,
                    missing=len(remaining_missing),
                    collisions=collisions,
                    sync_ratio=ratio,
                    missing_list=remaining_missing[:50],
                )

        active_total = len(synced) + len(missing) + len(collisions)
        ratio = round(len(synced) / active_total, 4) if active_total else 1.0

        return {
            "status": "OK",
            "catalog": str(p),
            "collection": str(cm),
            "total_catalog": len(entries),
            "active_evaluated": active_total,
            "skipped_noise": skipped_noise,
            "synced": len(synced),
            "copied": len(copied),
            "missing": len(missing),
            "collisions": collisions,
            "sync_ratio": ratio,
            "missing_list": missing[:50],
        }

    @mcp.tool(
        description=(
            "Summary report across one or more catalogs: totals, figures, tables, noise, sync ratios."
        )
    )
    @safe_tool
    def image_report(
        catalog_paths: str,
        collection_media_dir: str | None = None,
    ) -> dict[str, Any]:
        sep = "\n" if "\n" in catalog_paths else ","
        paths = [p.strip() for p in catalog_paths.split(sep) if p.strip()]
        rows: list[dict[str, Any]] = []
        grand_total = grand_synced = grand_missing = 0

        try:
            if collection_media_dir is not None:
                cm = _collection_media(collection_media_dir)
            else:
                cm = _collection_media()
            cm_str = str(cm)
        except FileNotFoundError as exc:
            return error_result(
                "MEDIA.CATALOG_ERROR", str(exc), "Check Anki collection.media directory."
            )

        for path_str in paths:
            p = Path(path_str)
            if not p.exists():
                rows.append({"catalog": path_str, "status": "NOT_FOUND"})
                continue
            entries = _load_catalog_csv(p)
            valid_entries = [e for e in entries if not e.noise]
            ns_names = {e.namespaced for e in valid_entries if e.namespaced}
            synced_n = sum(1 for n in ns_names if (cm / n).exists())
            missing_n = len(ns_names) - synced_n
            ratio = round(synced_n / len(ns_names), 4) if ns_names else 1.0
            grand_total += len(valid_entries)
            grand_synced += synced_n
            grand_missing += missing_n
            rows.append(
                {
                    "catalog": path_str,
                    "slug": next((e.slug for e in entries if e.slug), ""),
                    "total_raw": len(entries),
                    "valid_clinical": len(valid_entries),
                    "figures": sum(1 for e in valid_entries if e.item_type == "figure"),
                    "tables": sum(1 for e in valid_entries if e.item_type == "table"),
                    "noise_filtered": len(entries) - len(valid_entries),
                    "synced": synced_n,
                    "missing": missing_n,
                    "sync_ratio": ratio,
                }
            )

        return {
            "status": "OK",
            "collection": cm_str,
            "catalogs": rows,
            "grand_total": grand_total,
            "grand_synced": grand_synced,
            "grand_missing": grand_missing,
            "grand_sync_ratio": round(grand_synced / grand_total, 4) if grand_total else 1.0,
        }
