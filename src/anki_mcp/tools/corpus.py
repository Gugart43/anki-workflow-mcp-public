"""tools/corpus.py — source-material triage and coverage tools.

Exposes 4 MCP tools for evaluating study material before carding:

  corpus_load      — persist raw source text into the deck's corpus directory
  corpus_triage    — score candidates (inline text or a stored corpus file)
  corpus_coverage  — compare corpus vocabulary against the deck Manifesto
  corpus_digest    — THE unified report: worth carding / gaps / discard

All scoring is deterministic local logic (``anki_mcp.checks.triage``); no
network, no LLM.
"""

from __future__ import annotations

import re
import unicodedata
from pathlib import Path
from typing import Any, Literal

from fastmcp import FastMCP

from anki_mcp.checks.decisions import annotate_content
from anki_mcp.checks.triage import STOPWORDS, triage_text
from anki_mcp.core.deck import load_manifesto
from anki_mcp.core.safe import error_result, safe_tool
from anki_mcp.models import Manifesto

# ── Internal helpers ──────────────────────────────────────────────────────────

_CAMEL_RE = re.compile(r"(?<=[a-z])(?=[A-Z])")
_TOKEN_RE = re.compile(r"\b[a-z]{3,}\b")
_SENTENCE_RE = re.compile(r"(?<=\.)\s+")
_EVIDENCE_CHARS = 120
_MIN_TOKEN_CHARS = 4


def _safe_label(label: str) -> str:
    """Convert a free-form label into a safe filename stem."""
    normalized = unicodedata.normalize("NFD", label.lower())
    ascii_ish = "".join(c for c in normalized if unicodedata.category(c) != "Mn")
    return re.sub(r"[^a-z0-9]+", "-", ascii_ish).strip("-") or "corpus"


def _corpus_dir(deck_dir: str) -> Path:
    """Return the corpus subdirectory path (may not exist yet)."""
    return Path(deck_dir) / "corpus"


def _corpus_files(deck_dir: str) -> list[Path]:
    """List existing .md files in the corpus directory (empty list if absent)."""
    cdir = _corpus_dir(deck_dir)
    if not cdir.is_dir():
        return []
    return sorted(cdir.glob("*.md"))


def _read_corpus(files: list[Path]) -> list[tuple[str, str]]:
    """Read each corpus file, return list of (label, content) pairs."""
    results: list[tuple[str, str]] = []
    for f in files:
        try:
            content = f.read_text(encoding="utf-8")
            results.append((f.stem, content))
        except OSError:
            pass
    return results


def _label_tokens(text: str) -> set[str]:
    """Significant lowercase word tokens of *text* (no accents, no stopwords).

    camelCase and ``::`` tag separators are split so that a taxonomy node
    ``Neurologia::CriseConvulsiva`` yields ``neurologia`` and ``convulsiva``.
    Tokens shorter than ``_MIN_TOKEN_CHARS`` and stopwords are dropped, as are
    purely numeric tokens.
    """
    spaced = _CAMEL_RE.sub(" ", text.replace("::", " "))
    decomposed = unicodedata.normalize("NFD", spaced.lower())
    ascii_ish = "".join(c for c in decomposed if unicodedata.category(c) != "Mn")
    return {
        token
        for token in _TOKEN_RE.findall(ascii_ish)
        if len(token) >= _MIN_TOKEN_CHARS and token not in STOPWORDS
    }


def _covered_tokens(manifesto: Manifesto) -> set[str]:
    """All topical tokens found in the Manifesto (taxonomy + id_registry)."""
    covered: set[str] = set()
    tax = manifesto.taxonomy
    if tax:
        if tax.root:
            covered |= _label_tokens(tax.root)
        for part in tax.nodes:
            covered |= _label_tokens(part)
    for entry in manifesto.id_registry:
        if entry.topic:
            covered |= _label_tokens(entry.topic)
        if entry.anchor:
            covered |= _label_tokens(entry.anchor)
    return covered


def _evidence(text: str, term: str) -> str:
    """First sentence of *text* containing *term*, truncated for the report."""
    for sentence in _SENTENCE_RE.split(text):
        if term in _label_tokens(sentence) and sentence.strip():
            return sentence.strip()[:_EVIDENCE_CHARS]
    return text.strip()[:_EVIDENCE_CHARS]


def _triage_corpus(files: list[Path]) -> list[dict]:
    """Triage every corpus file, tagged with its label, sorted by composite desc.

    Sorting key: composite desc, then source label, then line number — fully
    deterministic across platforms and file orders.
    """
    items: list[dict] = []
    for label, content in _read_corpus(files):
        items.extend(annotate_content(triage_text(content, source_label=label)))
    items.sort(key=lambda item: (-item["composite"], item["source_label"], item["line"]))
    return items


def _discard_reason(reasons: object) -> str:
    """Pick the most informative discard reason without assuming it exists.

    ``reasons`` is populated by the scoring layer, but a DISCARD item can reach
    this point with an empty (or missing) list; the previous ``[-1]`` raised
    ``IndexError`` and killed the whole ``corpus_digest`` call.
    """
    if not isinstance(reasons, list) or not reasons:
        return "no reason recorded"
    tail = [str(r) for r in reasons if str(r).strip()]
    if not tail:
        return "no reason recorded"
    return tail[-2] if len(tail) >= 2 else tail[-1]


def _counts(items: list[dict]) -> dict[str, int]:
    """Verdict counts for a triage result set."""
    return {
        "CARD": sum(1 for i in items if i["verdict"] == "CARD"),
        "MAYBE": sum(1 for i in items if i["verdict"] == "MAYBE"),
        "DISCARD": sum(1 for i in items if i["verdict"] == "DISCARD"),
    }


def _gaps(manifesto: Manifesto, files: list[Path]) -> list[dict]:
    """Corpus terms absent from the deck's covered vocabulary.

    Each gap is ``{"term", "code": "COVERAGE.GAP", "evidence"}`` where evidence
    is ``"<corpus label>: <first sentence containing the term>"``.

    Only considers clinically significant tokens (length >= 6, not in an
    extended stopword set). This avoids flooding the report with noise like
    "apresenta", "capitulo", "completa".
    """
    # Extended stopwords: functional/textual words that are never clinical gaps
    _gap_stopwords: set[str] = set(STOPWORDS) | {
        "apresenta",
        "capitulo",
        "contexto",
        "geral",
        "conceitos",
        "comparativa",
        "completa",
        "aproximadamente",
        "texto",
        "material",
        "visao",
        "tema",
        "resumo",
        "definido",
        "definida",
        "tabela",
        "figura",
        "grafico",
        "secao",
        "topico",
        "parte",
        "exemplo",
        "forma",
        "tipo",
        "tipos",
        "grupo",
        "grupos",
        "fase",
        "fases",
        "nivel",
        "niveis",
        "caso",
        "casos",
        "alem",
        "assim",
        "entao",
        "porem",
        "contudo",
        "todavia",
        "portanto",
        "embora",
        "gerais",
        "principais",
        "comum",
        "comuns",
        "importante",
        "importancia",
        "fundamental",
        "necessario",
        "possivel",
        "provavel",
        "maior",
        "menor",
        "melhor",
        "pior",
        "primeira",
        "primeiro",
        "segunda",
        "segundo",
        "terceiro",
        "quarto",
        "quinto",
        "outras",
        "outros",
        "outra",
        "outro",
        "cada",
        "toda",
        "todo",
        "grande",
        "pequena",
        "longo",
        "curto",
        "alta",
        "baixa",
        "drogas",
        "maximo",
        "minutos",
        # Common verbs / adverbs with no clinical specificity
        "envolve",
        "levando",
        "resume",
        "seguir",
        "veremos",
        "entender",
        "demonstra",
        "indica",
        "mostra",
        "sugere",
        "ocorre",
        "existe",
        "apresentam",
        "resulta",
        "requer",
        "possui",
        "consiste",
        "inclui",
        "corresponde",
        "constitui",
        "devemos",
        "podemos",
        "recentes",
        "publicados",
        "publicado",
        "descrito",
        "descrita",
        "infundida",
        # Portuguese demonstratives (all forms)
        "deste",
        "neste",
        "desse",
        "nesse",
        "desta",
        "nesta",
        "dessa",
        "nessa",
        "daquele",
        "naquele",
        "daquela",
        "naquela",
        # Generic clinical words that appear in every deck
        "continua",
        "ataque",
        "estado",
        "efeito",
        "perda",
        "crises",
        "estudos",
    }
    _MIN_GAP_LEN = 6  # clinical terms are usually 6+ chars
    covered = _covered_tokens(manifesto)
    seen: dict[str, tuple[str, str]] = {}
    for label, content in _read_corpus(files):
        for term in _label_tokens(content):
            if (
                len(term) >= _MIN_GAP_LEN
                and term not in _gap_stopwords
                and term not in covered
                and term not in seen
            ):
                seen[term] = (label, _evidence(content, term))
    return [
        {"term": term, "code": "COVERAGE.GAP", "evidence": f"{label}: {ev}"}
        for term, (label, ev) in sorted(seen.items())
    ]


# ── Tool registration ─────────────────────────────────────────────────────────


def register_corpus_tools(mcp: FastMCP) -> None:
    """Register all corpus-triage tools onto *mcp*."""

    @mcp.tool(
        name="corpus_load",
        description="Persist source material into the deck's corpus directory for later triage.",
    )
    @safe_tool
    def corpus_load(deck_dir: str, text: str, label: str) -> dict[str, Any]:
        """Persist *text* into ``<deck_dir>/corpus/<safe_label>.md`` (UTF-8, no BOM).

        Creates the corpus subdirectory if it does not exist. If a file with the
        same label already exists, the new content is appended after a separator.

        Parameters
        ----------
        deck_dir:
            Absolute path to the deck directory (must contain manifesto.json).
        text:
            Source material to save (markdown, plain text, or pasted excerpt).
        label:
            Descriptive name for this source (e.g. 'harrison_cap4', 'notebooklm_ep').
            Converted automatically to a safe filename.

        Returns
        -------
        status:   "OK"
        path:     absolute path of the written file
        bytes:    bytes written (append mode counts only the new content)
        label:    sanitised label used as the filename stem
        """
        cdir = _corpus_dir(deck_dir)
        cdir.mkdir(parents=True, exist_ok=True)
        safe = _safe_label(label)
        dest = cdir / f"{safe}.md"
        separator = "\n\n---\n\n" if dest.exists() else ""
        content_bytes = (separator + text).encode("utf-8")
        with dest.open("ab") as fh:
            fh.write(content_bytes)
        return {"status": "OK", "path": str(dest), "bytes": len(content_bytes), "label": safe}

    @mcp.tool(
        name="corpus_triage",
        description="Score source material for flashcard potential. Pass inline text or a stored label.",
    )
    @safe_tool
    def corpus_triage(deck_dir: str, text: str = "", label: str = "") -> dict[str, Any]:
        """Score candidate statements from source material.

        Accepts either inline *text* or a previously stored corpus *label*. When
        both are given, inline *text* takes priority.

        Parameters
        ----------
        deck_dir:
            Absolute path to the deck directory.
        text:
            Inline source material to score. Takes priority over *label*.
        label:
            Label of a previously stored corpus file (via ``corpus_load``).

        Returns
        -------
        status:     "OK" | "EMPTY"
        counts:     dict with keys CARD, MAYBE, DISCARD
        candidates: list of scored candidates (top 50, sorted by composite desc)
        """
        if text:
            source = text
            src_label = label or "inline"
        elif label:
            safe = _safe_label(label)
            dest = _corpus_dir(deck_dir) / f"{safe}.md"
            if not dest.exists():
                return {
                    "status": "EMPTY",
                    "code": "CORPUS.NOT_FOUND",
                    "counts": {"CARD": 0, "MAYBE": 0, "DISCARD": 0},
                    "candidates": [],
                }
            source = dest.read_text(encoding="utf-8")
            src_label = safe
        else:
            return {
                "status": "EMPTY",
                "code": "CORPUS.EMPTY",
                "counts": {"CARD": 0, "MAYBE": 0, "DISCARD": 0},
                "candidates": [],
            }

        results = annotate_content(triage_text(source, source_label=src_label))
        return {
            "status": "OK",
            "counts": _counts(results),
            "candidates": results[:50],
        }

    @mcp.tool(
        name="corpus_coverage",
        description="Compare corpus vocabulary against the deck Manifesto to identify coverage gaps.",
    )
    @safe_tool
    def corpus_coverage(deck_dir: str) -> dict[str, Any]:
        """Report topics present in the corpus but absent from the deck Manifesto.

        Reads all corpus files and the deck Manifesto, tokenises both to
        lowercase word sets, and reports tokens that appear in the corpus
        but are not covered by any taxonomy node or id_registry entry.

        Noise is filtered: purely functional vocabulary, demonstratives and
        generic verbs are excluded. Only tokens of 6+ characters are reported.

        Parameters
        ----------
        deck_dir:
            Absolute path to the deck directory (must contain manifesto.json).

        Returns
        -------
        status:         "OK" | "EMPTY"
        covered_terms:  count of Manifesto vocabulary tokens
        gaps:           list of {term, code: "COVERAGE.GAP", evidence}
        corpus_files:   number of corpus files read
        """
        files = _corpus_files(deck_dir)
        if not files:
            cdir = _corpus_dir(deck_dir)
            code = "CORPUS.NOT_FOUND" if not cdir.exists() else "CORPUS.EMPTY"
            return {
                "status": "EMPTY",
                "code": code,
                "covered_terms": 0,
                "gaps": [],
                "corpus_files": 0,
            }
        manifesto = load_manifesto(deck_dir)
        covered = _covered_tokens(manifesto)
        gaps = _gaps(manifesto, files)
        return {
            "status": "OK",
            "covered_terms": len(covered),
            "gaps": gaps,
            "corpus_files": len(files),
        }

    @mcp.tool(
        name="corpus_digest",
        description="Unified triage + coverage report for all corpus files in the deck.",
    )
    @safe_tool
    def corpus_digest(deck_dir: str) -> dict[str, Any]:
        """Produce the unified triage + coverage digest for all corpus files.

        Combines scoring and gap analysis into a single structured report with
        three action-oriented sections:

        - **card**    candidates that score CARD (composite >= 0.60) — card these
        - **gaps**    topics in the corpus absent from the deck — investigate these
        - **discard** candidates to skip, each with its discard reason

        Parameters
        ----------
        deck_dir:
            Absolute path to the deck directory (must contain manifesto.json).

        Returns
        -------
        status:   "OK" | "EMPTY"
        summary:  {corpus_files, total_candidates, CARD, MAYBE, DISCARD, gaps}
        card:     list of CARD candidates sorted by composite desc
        gaps:     list of coverage gaps {term, code, evidence}
        discard:  list of DISCARD candidates with their primary reason
        """
        files = _corpus_files(deck_dir)
        if not files:
            cdir = _corpus_dir(deck_dir)
            code = "CORPUS.NOT_FOUND" if not cdir.exists() else "CORPUS.EMPTY"
            return {
                "status": "EMPTY",
                "code": code,
                "summary": {
                    "corpus_files": 0,
                    "total_candidates": 0,
                    "CARD": 0,
                    "MAYBE": 0,
                    "DISCARD": 0,
                    "gaps": 0,
                },
                "card": [],
                "gaps": [],
                "discard": [],
            }
        manifesto = load_manifesto(deck_dir)
        all_items = _triage_corpus(files)
        cts = _counts(all_items)
        gaps = _gaps(manifesto, files)

        card_items = [i for i in all_items if i["verdict"] == "CARD"]
        discard_items = [
            {**i, "reason": _discard_reason(i.get("reasons"))}
            for i in all_items
            if i["verdict"] == "DISCARD"
        ]

        return {
            "status": "OK",
            "summary": {
                "corpus_files": len(files),
                "total_candidates": len(all_items),
                **cts,
                "gaps": len(gaps),
            },
            "card": card_items,
            "gaps": gaps,
            "discard": discard_items,
        }

    @mcp.tool(
        description=(
            "Calculate top-down total card budget for a topic based on source text and clinical decisions. "
            "Plans total cards upfront and chunks strictly into batches of 20 (MAX_BATCH = 20)."
        )
    )
    @safe_tool
    def topic_budget_plan(
        deck_dir: str,
        text: str = "",
        label: str = "",
        batch_size: int = 20,
        card_texts: list[str] | None = None,
    ) -> dict[str, Any]:
        """Budget with justification. ``card_texts`` (optional) enables per-type coverage."""
        from anki_mcp.checks.decisions import plan_topic_budget

        source_text = text
        if not source_text and label:
            for ext in (".txt", ".md"):
                candidate = Path(deck_dir) / "corpus" / f"{label}{ext}"
                if candidate.is_file():
                    source_text = candidate.read_text(encoding="utf-8")
                    break
        if not source_text:
            corpus_dir = Path(deck_dir) / "corpus"
            if corpus_dir.is_dir():
                pieces: list[str] = []
                for ext in ("*.txt", "*.md"):
                    pieces.extend(
                        f.read_text(encoding="utf-8", errors="ignore")
                        for f in sorted(corpus_dir.glob(ext))
                    )
                source_text = "\n\n".join(pieces)

        if not source_text:
            return error_result(
                "CORPUS.NO_SOURCE",
                "No source text provided or found in deck corpus.",
                "Pass text=... or label=... with a corpus file under deck_dir/corpus/.",
            )

        plan = plan_topic_budget(source_text, batch_size=batch_size, card_texts=card_texts)
        # Augment with Must-Cover Universal Invariants floors
        from anki_mcp.checks.jev_triage import evaluate_must_cover_invariants, split_into_sections

        sections = split_into_sections(source_text)
        invariants, gaps = evaluate_must_cover_invariants(sections)
        plan["must_cover_matrix"] = [inv.model_dump() for inv in invariants]
        plan["must_cover_gaps"] = gaps
        plan["must_cover_covered_count"] = sum(1 for inv in invariants if inv.status == "COVERED")

        return {"status": "OK", "deck_dir": deck_dir, **plan}

    @mcp.tool(
        name="topic_triage_calibrate",
        description=(
            "Calibrate topic flashcard budget via Sectional HCT with JEV System 1 & Must-Cover Matrix. "
            "Slices source into clinical sections, computes expected cards (EV), and partitions into batches <= 20."
        ),
    )
    @safe_tool
    def topic_triage_calibrate(
        deck_dir: str = "",
        text: str = "",
        label: str = "",
        domain: str = "Medicina",
        batch_size: int = 20,
        target_audience: Literal[
            "R1_GENERALISTA", "ESPECIALISTA", "FULL_COMPREHENSIVE"
        ] = "R1_GENERALISTA",
        use_jev: bool = True,
    ) -> dict[str, Any]:
        """Hybrid Clinical Triage (HCT) tool: Sectional JEV calibration + Universal Invariants."""
        from anki_mcp.checks.jev_triage import evaluate_topic_budget

        source_text = text
        if not source_text and label and deck_dir:
            for ext in (".txt", ".md"):
                candidate = Path(deck_dir) / "corpus" / f"{label}{ext}"
                if candidate.is_file():
                    source_text = candidate.read_text(encoding="utf-8")
                    break
        if not source_text and deck_dir:
            corpus_dir = Path(deck_dir) / "corpus"
            if corpus_dir.is_dir():
                pieces: list[str] = []
                for ext in ("*.txt", "*.md"):
                    pieces.extend(
                        f.read_text(encoding="utf-8", errors="ignore")
                        for f in sorted(corpus_dir.glob(ext))
                    )
                source_text = "\n\n".join(pieces)

        if not source_text:
            return error_result(
                "CORPUS.NO_SOURCE",
                "No source text provided or found in deck corpus.",
                "Pass text=... or label=... with a corpus file under deck_dir/corpus/.",
            )

        topic_name = label or (Path(deck_dir).name if deck_dir else "Tópico Clínico")
        eval_result = evaluate_topic_budget(
            text=source_text,
            topic=topic_name,
            domain=domain,
            deck_dir=deck_dir,
            batch_size=batch_size,
            target_audience=target_audience,
            use_jev=use_jev,
        )

        return {
            "status": "OK",
            "deck_dir": deck_dir,
            "topic": eval_result.topic,
            "domain": eval_result.domain,
            "cognitive_zone": eval_result.cognitive_zone,
            "zone_descriptor": eval_result.zone_descriptor,
            "elastic_range": eval_result.elastic_range,
            "taxonomy_scope": eval_result.taxonomy_scope,
            "target_audience": eval_result.target_audience,
            "total_sections": eval_result.total_sections,
            "total_ev": eval_result.total_ev,
            "ev_jev": eval_result.ev_jev,
            "ev_decisions": eval_result.ev_decisions,
            "ev_structural": eval_result.ev_structural,
            "ev_floor": eval_result.ev_floor,
            "convergence": eval_result.convergence,
            "convergence_spread": eval_result.convergence_spread,
            "degraded": eval_result.degraded,
            "degraded_reason": eval_result.degraded_reason,
            "saturation_status": eval_result.saturation_status,
            "source_incomplete": eval_result.source_incomplete,
            "source_exceeds_zone": eval_result.source_exceeds_zone,
            "action_required": eval_result.action_required,
            "total_planned_cards": eval_result.total_planned_cards,
            "recommended_range": eval_result.recommended_range,
            "core_batch_budget": eval_result.core_batch_budget,
            "suggested_splits": eval_result.suggested_splits,
            "actionability_pruned_count": eval_result.actionability_pruned_count,
            "pruned_sections": [p.model_dump() for p in eval_result.pruned_sections],
            "batch_size": eval_result.batch_size,
            "batches_needed": eval_result.batches_needed,
            "batches_planned": eval_result.batches_planned,
            "must_cover_matrix": [inv.model_dump() for inv in eval_result.must_cover_matrix],
            "gaps": eval_result.gaps,
            "sections": [sec.model_dump() for sec in eval_result.sections],
            "source_tokens": eval_result.source_tokens,
            "engine": eval_result.engine,
            "not_attended": eval_result.not_attended,
            "known_risks": eval_result.known_risks,
            "summary": eval_result.raw_summary,
        }
