"""Error code registry and routing table.

Every diagnostic emitted by the MCP has:
  - code:       namespaced string  (e.g. "CLOZE.NONSEQ")
  - severity:   "CRITICAL" | "WARN" | "LOG"
  - owner:      the Anki skill responsible for fixing the issue
  - description: human-readable explanation
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class ErrorDef:
    code: str
    severity: str  # "CRITICAL" | "WARN" | "LOG"
    owner: str  # skill slug
    description: str


# ---------------------------------------------------------------------------
# Registry — all known codes
# ---------------------------------------------------------------------------
_CODES: list[ErrorDef] = [
    # ── Formatting / CSV structure ──────────────────────────────────────────
    ErrorDef(
        "FMT.HEADER_MISMATCH",
        "CRITICAL",
        "anki-formatting-guide",
        "Header ≠ note type (7 QA / 6 Cloze fields)",
    ),
    ErrorDef(
        "FMT.COLUMN_COUNT", "CRITICAL", "anki-formatting-guide", "Row has wrong number of columns"
    ),
    ErrorDef(
        "FMT.FIELD_SHIFTED",
        "CRITICAL",
        "anki-formatting-guide",
        "Row is structurally invalid (unescaped '|' or missing fields): semantic checks "
        "for this row are SKIPPED_STRUCTURAL because the values are misaligned",
    ),
    ErrorDef(
        "CHECK.SKIPPED",
        "LOG",
        "anki-export-validator",
        "Check could not be evaluated (missing input or unreadable catalog); not a PASS",
    ),
    ErrorDef(
        "FMT.RAW_PIPE",
        "CRITICAL",
        "anki-formatting-guide",
        "Raw pipe '|' inside field (must be &#124;)",
    ),
    ErrorDef(
        "FMT.RAW_NEWLINE",
        "CRITICAL",
        "anki-formatting-guide",
        r"Raw \r or \n inside field (use <br>)",
    ),
    ErrorDef(
        "FMT.ENTITY_MISSING",
        "CRITICAL",
        "anki-formatting-guide",
        "Raw '<' or '>' outside allowed tag",
    ),
    ErrorDef(
        "FMT.AMP_RAW", "CRITICAL", "anki-formatting-guide", "Raw '&' that is not a valid entity"
    ),
    ErrorDef(
        "FMT.CLASS_UNKNOWN",
        "WARN",
        "anki-formatting-guide",
        "CSS class not in the semantic vocabulary (renders unstyled)",
    ),
    ErrorDef(
        "FMT.ATTR_FORBIDDEN",
        "CRITICAL",
        "anki-formatting-guide",
        "HTML attribute not allowed (only class/src/alt are permitted)",
    ),
    ErrorDef(
        "FMT.TAG_FORBIDDEN",
        "CRITICAL",
        "anki-formatting-guide",
        "HTML tag outside whitelist or figure outside Imagens field",
    ),
    ErrorDef(
        "FMT.TAG_UNCLOSED",
        "WARN",
        "anki-formatting-guide",
        "Unclosed or unmatched HTML tag (e.g. <b> without matching </b>)",
    ),
    ErrorDef(
        "FMT.BOM",
        "CRITICAL",
        "anki-formatting-guide",
        "UTF-8 BOM detected (must be UTF-8 without BOM)",
    ),
    # ── MathJax ─────────────────────────────────────────────────────────────
    ErrorDef(
        "MATH.UNBALANCED",
        "CRITICAL",
        "anki-formatting-guide",
        r"Unbalanced \(..\) or \[..\] MathJax delimiters",
    ),
    ErrorDef(
        "MATH.DOLLAR",
        "CRITICAL",
        "anki-formatting-guide",
        "Raw '$' used for math (use \\( \\) instead)",
    ),
    # ── Cloze ───────────────────────────────────────────────────────────────
    ErrorDef(
        "CLOZE.NONSEQ",
        "CRITICAL",
        "anki-cloze-creator",
        "Cloze indices not sequential or malformed",
    ),
    ErrorDef("CLOZE.NESTED", "CRITICAL", "anki-cloze-creator", "Nested cloze deletions"),
    ErrorDef(
        "CLOZE.ID_IN_TEXT",
        "CRITICAL",
        "anki-cloze-creator",
        "ID visible in carrier sentence (must be in tags/metadata)",
    ),
    ErrorDef(
        "CLOZE.EXT_HINT_LEAK",
        "WARN",
        "anki-cloze-creator",
        "Hint or example outside cloze deletion gives away the answer",
    ),
    ErrorDef(
        "CLOZE.COTEXT_LEAK",
        "WARN",
        "anki-cloze-creator",
        "Co-text or visible examples leak the answer of an adjacent deletion",
    ),
    ErrorDef(
        "CLOZE.ACRONYM_LEAK",
        "CRITICAL",
        "anki-cloze-creator",
        "Visible acronym immediately adjacent to occluded full name (or vice-versa) leaks answer",
    ),
    ErrorDef(
        "CLOZE.TOO_LONG",
        "WARN",
        "anki-cloze-creator",
        "Cloze target deletion exceeds 120 characters (advisory review signal; length alone does not prove non-atomicity)",
    ),
    ErrorDef(
        "CLOZE.BOUNDARY",
        "WARN",
        "anki-cloze-creator",
        "Deletion swallows preposition/punctuation/unit",
    ),
    ErrorDef(
        "CLOZE.CONTEXT_WARN",
        "WARN",
        "anki-cloze-creator",
        "Carrier sentence lacks clinical context anchor",
    ),
    # ── Hints ───────────────────────────────────────────────────────────────
    ErrorDef("HINT.FORMAT", "CRITICAL", "anki-cloze-hints", "Invalid hint syntax"),
    ErrorDef(
        "HINT.SYNONYM",
        "CRITICAL",
        "anki-cloze-hints",
        "Hint is a synonym / direct clue for the target",
    ),
    ErrorDef(
        "HINT.ELIMINATION",
        "WARN",
        "anki-cloze-hints",
        "Hint enables binary/small-category elimination",
    ),
    ErrorDef(
        "HINT.ROOT_LEAK",
        "CRITICAL",
        "anki-cloze-hints",
        "Hint contains root/derivative of hidden target",
    ),
    # ── QA ──────────────────────────────────────────────────────────────────
    ErrorDef(
        "QA.CARDINALITY",
        "CRITICAL",
        "anki-qa-designer",
        "Answer has 3+ items — review unit (separate independent recalls or move optional details to notes; a single cloze can hide a list)",
    ),
    ErrorDef("QA.YESNO", "CRITICAL", "anki-qa-designer", "Yes/No question or answer"),
    ErrorDef(
        "QA.ID_IN_TEXT",
        "CRITICAL",
        "anki-qa-designer",
        "ID visible in stem or answer (must be in tags/metadata)",
    ),
    ErrorDef(
        "QA.MC_EMBED", "CRITICAL", "anki-qa-designer", "Multiple-choice options embedded in stem"
    ),
    ErrorDef(
        "QA.CONTEXT_WARN",
        "WARN",
        "anki-qa-designer",
        "Stem lacks clinical scenario/population/source",
    ),
    ErrorDef(
        "QA.NUMBER_UNIT",
        "WARN",
        "anki-qa-designer",
        "Numeric answer without unit or direction operator",
    ),
    ErrorDef(
        "QA.MULTI_OP",
        "WARN",
        "anki-qa-designer",
        "Stem appears to require two independent retrievals",
    ),
    ErrorDef(
        "QA.PAIR_HEURISTIC",
        "WARN",
        "anki-qa-designer",
        "Possible natural pair (drug+dose) — review atomicity",
    ),
    # `QA.INTERFERENCE_WARN` is retired: Rust emits `CARD.INTERFERENCE_PAIR_WARN`
    # (crates/anki-validate/src/interference.rs:17). Old payloads still resolve
    # through RETIRED_ALIASES below, so nothing silently loses its meaning.
    # ── Notes ───────────────────────────────────────────────────────────────
    ErrorDef(
        "NOTE.METADATA",
        "CRITICAL",
        "anki-notes-enricher",
        "Forbidden metadata header (ID:, Âncora:, Cross-ref:, Relevância:)",
    ),
    ErrorDef(
        "NOTE.DUP_ANSWER_WARN",
        "WARN",
        "anki-notes-enricher",
        "Notes field repeats the answer verbatim",
    ),
    ErrorDef(
        "NOTE.EXAM_ONLY_WARN",
        "WARN",
        "anki-notes-enricher",
        "Notes field contains only exam-appearance mention without pedagogical explanation",
    ),
    ErrorDef(
        "NOTE.LENGTH_WARN", "WARN", "anki-notes-enricher", "Notes field outside recommended length"
    ),
    ErrorDef(
        "NOTE.STYLE_WARN",
        "WARN",
        "anki-notes-enricher",
        "Bullets/style deviate from standard (• <b>…</b>)",
    ),
    ErrorDef(
        "NOTE.DRAFT_OVERFLOW",
        "WARN",
        "anki-card-generator",
        "Draft notes contain more than minimal (ID + Anchor)",
    ),
    # ── IDs ─────────────────────────────────────────────────────────────────
    ErrorDef(
        "ID.FORMAT",
        "CRITICAL",
        "anki-registry",
        "ID does not match pattern B{nn}-{QA|CL}-{nnn}[a-z?]",
    ),
    ErrorDef(
        "ID.DUPLICATE", "CRITICAL", "anki-registry", "Duplicate ID in batch or already in ledger"
    ),
    ErrorDef("ID.RETIRED_REUSE", "CRITICAL", "anki-registry", "Retired/merged ID reused"),
    # ── Cross-refs ──────────────────────────────────────────────────────────
    ErrorDef(
        "XREF.BROKEN",
        "CRITICAL",
        "anki-export-validator",
        "Cross-ref does not resolve to active or current-batch ID",
    ),
    # ── Tags ────────────────────────────────────────────────────────────────
    ErrorDef("TAG.UNREGISTERED", "CRITICAL", "anki-registry", "Tag not in Manifesto taxonomy"),
    ErrorDef("TAG.DEPTH", "CRITICAL", "anki-registry", "Tag exceeds maximum depth of 4 levels"),
    ErrorDef(
        "TAG.FORMAT", "CRITICAL", "anki-registry", "Tag contains spaces or invalid characters"
    ),
    # ── Images ──────────────────────────────────────────────────────────────
    ErrorDef(
        "IMG.PHANTOM", "CRITICAL", "anki-image-injector", "Image src not found in active catalog"
    ),
    ErrorDef(
        "IMG.MEDIA_MISSING",
        "CRITICAL",
        "anki-image-injector",
        "Image file missing from collection.media",
    ),
    ErrorDef(
        "IMG.CAP_OVER", "CRITICAL", "anki-image-injector", "More than 3 images in a single card"
    ),
    ErrorDef(
        "IMG.NAMESPACE",
        "CRITICAL",
        "anki-image-injector",
        "Image filename missing valid slug prefix [a-z0-9_]",
    ),
    ErrorDef(
        "IMG.SRC_MISMATCH",
        "CRITICAL",
        "anki-image-injector",
        "img src does not match namespaced filename",
    ),
    ErrorDef(
        "IMG.DUP_IN_CARD",
        "CRITICAL",
        "anki-image-injector",
        "Same image appears more than once in a card",
    ),
    ErrorDef(
        "IMG.STRUCTURE", "CRITICAL", "anki-image-injector", "Malformed <figure>/<figcaption> HTML"
    ),
    ErrorDef(
        "IMG.CAPTION",
        "WARN",
        "anki-image-injector",
        "Caption is empty or contains extraction noise",
    ),
    ErrorDef(
        "IMG.ENTRY_INVALID",
        "LOG",
        "anki-image-injector",
        "Catalog entry missing filename/anki_tag — quarantined",
    ),
    ErrorDef(
        "IMG.COLLISION_RESOLVED",
        "LOG",
        "anki-image-injector",
        "Filename collision between catalogs resolved by slug",
    ),
    ErrorDef("IMG.NOISE", "LOG", "anki-image-injector", "Bibliographic caption discarded as noise"),
    ErrorDef(
        "IMG.QUARANTINED",
        "LOG",
        "anki-image-injector",
        "Catalog entry quarantined (invalid fields)",
    ),
    ErrorDef(
        "IMG.BARE_IMG",
        "CRITICAL",
        "anki-image-injector",
        "<img> outside <figure> (every image needs a <figcaption>)",
    ),
    ErrorDef(
        "IMG.CAPTION_WEAK",
        "WARN",
        "anki-image-injector",
        "Caption is generic or too short to be clinically descriptive",
    ),
    ErrorDef(
        "IMG.INLINE_STYLE",
        "WARN",
        "anki-image-injector",
        "Inline style on figure/img/figcaption overrides the note type theme",
    ),
    ErrorDef(
        "IMG.ALT_MISSING",
        "LOG",
        "anki-image-injector",
        "<img> without alt text",
    ),
    # ── Tables ──────────────────────────────────────────────────────────────
    ErrorDef(
        "TBL.MALFORMED", "CRITICAL", "anki-formatting-guide", "Unbalanced or malformed <table> HTML"
    ),
    ErrorDef(
        "TBL.RAGGED", "CRITICAL", "anki-formatting-guide", "Rows have inconsistent column counts"
    ),
    ErrorDef("TBL.NESTED", "CRITICAL", "anki-formatting-guide", "Nested <table> is not allowed"),
    ErrorDef(
        "TBL.MARKDOWN_PIPE",
        "CRITICAL",
        "anki-formatting-guide",
        "Markdown table or raw pipe in field (collides with the CSV delimiter)",
    ),
    ErrorDef(
        "TBL.NO_HEADER",
        "WARN",
        "anki-formatting-guide",
        "Table without a header row (<thead>/<th>)",
    ),
    ErrorDef(
        "TBL.TOO_WIDE", "WARN", "anki-formatting-guide", "Too many columns for a mobile screen"
    ),
    ErrorDef("TBL.TOO_LONG", "WARN", "anki-formatting-guide", "Too many rows for one card"),
    ErrorDef("TBL.CELL_TOO_LONG", "WARN", "anki-formatting-guide", "Table cell text too long"),
    ErrorDef(
        "TBL.INLINE_STYLE",
        "WARN",
        "anki-formatting-guide",
        "Inline style/width/bgcolor on table overrides the note type theme",
    ),
    ErrorDef(
        "TBL.IN_CARD_FACE",
        "WARN",
        "anki-formatting-guide",
        "Table in Enunciado/Resposta/Sentença (move to Notas Adicionais or split the card)",
    ),
    # ── Duplicates ──────────────────────────────────────────────────────────
    ErrorDef("DUP.EXACT", "CRITICAL", "anki-post-curator", "Exact duplicate of an existing card"),
    ErrorDef(
        "DUP.SEMANTIC_WARN",
        "WARN",
        "anki-post-curator",
        "Suspected semantic duplicate based on topic overlap",
    ),
    # ── Batch ───────────────────────────────────────────────────────────────
    ErrorDef(
        "BATCH.OVER_LIMIT",
        "CRITICAL",
        "anki-cloze-creator",
        "Batch exceeds 20 cards (Cloze expansions counted)",
    ),
    # ── State & governance ──────────────────────────────────────────────────
    ErrorDef(
        "STATE.OUT_OF_ORDER",
        "CRITICAL",
        "anki-card-generator",
        "Tool called outside allowed pipeline phase",
    ),
    ErrorDef(
        "STATE.NO_RECEIPT",
        "CRITICAL",
        "anki-export-validator",
        "Export/commit attempted without a valid PASS receipt",
    ),
    ErrorDef(
        "GOV.SCHEMA",
        "CRITICAL",
        "anki-registry",
        "manifesto.json invalid or schema migration pending",
    ),
    ErrorDef("GOV.LOCK", "CRITICAL", "anki-registry", "Deck lock unavailable (concurrent write)"),
    # ── Corpus & triage ──────────────────────────────────────────────────────
    ErrorDef(
        "CORPUS.NOT_FOUND",
        "CRITICAL",
        "anki-card-generator",
        "Corpus directory or file does not exist",
    ),
    ErrorDef(
        "CORPUS.EMPTY",
        "WARN",
        "anki-card-generator",
        "Corpus contains no extractable candidate content",
    ),
    ErrorDef(
        "TRIAGE.LOW_YIELD",
        "INFO",
        "anki-card-generator",
        "Candidate has low flashcard potential",
    ),
    ErrorDef(
        "TRIAGE.UNSTABLE",
        "WARN",
        "anki-card-generator",
        "Candidate likely to become stale (statistic or time-bound fact)",
    ),
    ErrorDef(
        "COVERAGE.GAP",
        "INFO",
        "anki-registry",
        "Topic present in corpus but absent from deck coverage",
    ),
    # ── Card atomicity / length (emitted by checks/qa.py and checks/cloze.py,
    #    mirrored 1:1 by the Rust validators crates/anki-validate/src/{qa,cloze}.rs) ──
    ErrorDef(
        "CARD.SPLIT_QA_MULTI_QUESTION",
        "WARN",
        "anki-card-generator",
        "Stem asks more than one question (two obligations on one card)",
    ),
    ErrorDef(
        "CARD.SPLIT_CLOZE_INDICES",
        "CRITICAL",
        "anki-card-generator",
        "More cloze indices than the operational cap (one note expands to many cards)",
    ),
    ErrorDef(
        "CARD.TOO_LONG_STEM",
        "WARN",
        "anki-card-generator",
        "Stem length above the review threshold (advisory, not proof of multiple demands)",
    ),
    ErrorDef(
        "CARD.TOO_LONG_ANSWER",
        "WARN",
        "anki-card-generator",
        "Answer length above the review threshold (advisory)",
    ),
    ErrorDef(
        "CARD.TOO_LONG_SENTENCE",
        "WARN",
        "anki-card-generator",
        "Cloze carrier sentence above the review threshold (advisory)",
    ),
    ErrorDef(
        "CARD.TOO_LONG_DELETION",
        "WARN",
        "anki-card-generator",
        "Cloze deletion above the review threshold (advisory)",
    ),
    ErrorDef(
        "CARD.INTERFERENCE_PAIR_WARN",
        "WARN",
        "anki-card-generator",
        "Highly similar prompts charging distinct clinical targets (recommend mutual burying)",
    ),
    # ── Information profile advisories (checks/information.py advisory_signals) ──
    ErrorDef(
        "INFO.PROMPT_UNAVAILABLE",
        "INFO",
        "anki-card-generator",
        "Partial information profile: no stem/sentence available for evaluation",
    ),
    ErrorDef(
        "INFO.QA_HIDDEN_LIST",
        "INFO",
        "anki-qa-designer",
        "QA answer holds 3+ candidate items (review semantic independence)",
    ),
    ErrorDef(
        "INFO.CLOZE_HIDDEN_LIST",
        "INFO",
        "anki-cloze-creator",
        "Cloze deletion holds 3+ candidate items (review semantic independence)",
    ),
    # ── Corpus ──────────────────────────────────────────────────────────────
    ErrorDef(
        "CORPUS.NO_SOURCE",
        "WARN",
        "anki-card-generator",
        "No source text provided or found under deck_dir/corpus",
    ),
    # ── Pipeline phases and gates ───────────────────────────────────────────
    ErrorDef(
        "PIPELINE.TERMINAL",
        "CRITICAL",
        "anki-card-generator",
        "Batch is already exported; no further transitions are allowed",
    ),
    ErrorDef(
        "PIPELINE.INVALID_STATE",
        "CRITICAL",
        "anki-card-generator",
        "Batch state is unknown and has no allowed transition",
    ),
    ErrorDef(
        "PIPELINE.GATE_REQUIRED",
        "CRITICAL",
        "anki-card-generator",
        "Phase has no recorded PASS gate, so the advance is blocked",
    ),
    ErrorDef(
        "PIPELINE.GATE_INVALID",
        "CRITICAL",
        "anki-card-generator",
        "Recorded gate value is not a valid receipt hash",
    ),
    ErrorDef(
        "PIPELINE.RECEIPT_MISMATCH",
        "CRITICAL",
        "anki-card-generator",
        "receipt_hash does not match the gate recorded for the current phase",
    ),
    # ── Export ──────────────────────────────────────────────────────────────
    ErrorDef(
        "EXPORT.NO_RECEIPT",
        "CRITICAL",
        "anki-export-validator",
        "Export attempted without a receipt matching the exact CSV content",
    ),
    ErrorDef(
        "EXPORT.VALIDATION_FAIL",
        "CRITICAL",
        "anki-export-validator",
        "Re-validation verdict is not PASS at export time",
    ),
    ErrorDef(
        "EXPORT.BATCH_CONFLICT",
        "CRITICAL",
        "anki-export-validator",
        "Batch content differs from an already committed/exported receipt",
    ),
    # ── Media (tools/media.py, mirrors apps/.../cli/sync_media.rs) ──────────
    ErrorDef(
        "MEDIA.CATALOG_ERROR",
        "CRITICAL",
        "anki-image-injector",
        "Media catalog unreadable or malformed",
    ),
    ErrorDef(
        "MEDIA.COLLISION",
        "CRITICAL",
        "anki-image-injector",
        "Catalog entry collides with an existing file in collection.media",
    ),
    ErrorDef(
        "MEDIA.SOURCE_MISSING",
        "CRITICAL",
        "anki-image-injector",
        "Catalog source files are missing; nothing was copied",
    ),
    ErrorDef(
        "MEDIA.IO_ERROR",
        "CRITICAL",
        "anki-image-injector",
        "I/O failure while copying media (batch rolled back)",
    ),
    # ── Runtime / tool boundary (core/safe.py) ──────────────────────────────
    ErrorDef(
        "ARGUMENT.INVALID",
        "CRITICAL",
        "anki-card-generator",
        "Tool arguments invalid (malformed JSON, wrong type or value)",
    ),
    ErrorDef(
        "RUNTIME.ERROR",
        "CRITICAL",
        "anki-card-generator",
        "Operation could not complete (deliberate RuntimeError without coded prefix)",
    ),
    ErrorDef(
        "INTERNAL.UNEXPECTED",
        "CRITICAL",
        "anki-card-generator",
        "Unexpected exception escaped a tool (normalized, never a raw traceback)",
    ),
    ErrorDef(
        "DECK.NOT_FOUND",
        "CRITICAL",
        "anki-registry",
        "Deck directory (or manifesto.json) does not exist",
    ),
    ErrorDef(
        "FILE.NOT_FOUND",
        "CRITICAL",
        "anki-registry",
        "Requested file does not exist",
    ),
    ErrorDef(
        "HIERARCHY.CORRUPT",
        "CRITICAL",
        "anki-registry",
        "hierarchy.json unreadable or missing the 'decks' key",
    ),
    # ── Sanitizer guards (core/sanitize.py) ─────────────────────────────────
    ErrorDef(
        "SANITIZE.INPUT_TOO_LARGE",
        "CRITICAL",
        "anki-formatting-guide",
        "Field exceeds the sanitizer input cap; split the content before sanitizing",
    ),
]

# Retired codes kept as aliases so historical payloads still resolve to a real
# definition instead of raising KeyError.
RETIRED_ALIASES: dict[str, str] = {
    "QA.INTERFERENCE_WARN": "CARD.INTERFERENCE_PAIR_WARN",
}

# Build lookup dicts
_BY_CODE: dict[str, ErrorDef] = {e.code: e for e in _CODES}


def resolve(code: str) -> str:
    """Return the registered code that *code* maps to (following retired aliases)."""
    return RETIRED_ALIASES.get(code, code)


def get(code: str) -> ErrorDef:
    """Return the ErrorDef for *code*; raises KeyError if unknown."""
    return _BY_CODE[resolve(code)]


def severity(code: str) -> str:
    return _BY_CODE[resolve(code)].severity


def owner(code: str) -> str:
    return _BY_CODE[resolve(code)].owner


def is_critical(code: str) -> bool:
    return _BY_CODE[resolve(code)].severity == "CRITICAL"


def all_codes() -> list[str]:
    return list(_BY_CODE)
