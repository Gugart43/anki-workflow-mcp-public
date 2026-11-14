"""Pydantic v2 models — the shared data contracts for all MCP tools."""

from __future__ import annotations

from enum import StrEnum
from typing import Any, Literal

from pydantic import BaseModel, Field

# ── Enumerations ─────────────────────────────────────────────────────────────


class NoteType(StrEnum):
    QA = "QA"
    CLOZE = "CL"
    CLOZE_LONG = "CLOZE"


class CardStatus(StrEnum):
    ACTIVE = "active"
    RETIRED = "retired"
    MERGED = "merged"


class Severity(StrEnum):
    CRITICAL = "CRITICAL"
    WARN = "WARN"
    INFO = "INFO"
    LOG = "LOG"


class ValidationResult(StrEnum):
    PASS = "PASS"
    FAIL = "FAIL"
    INCOMPLETE = "INCOMPLETE"  # no CRITICAL, but a check could not run (no receipt)


# ── Diagnostics ──────────────────────────────────────────────────────────────


class Diagnostic(BaseModel):
    """A single validation finding (error or warning)."""

    code: str
    severity: Severity
    owner_skill: str
    message: str
    row: int | None = None  # 1-based CSV row (None = whole-file)
    field: str | None = None  # field name (e.g. "Sentença")
    suggestion: str | None = None  # optional fix hint
    auto_fix_available: bool = False
    fix_preview: str | None = None

    @classmethod
    def from_code(cls, code: str, message: str, **kwargs: Any) -> Diagnostic:
        from anki_mcp.core.codes import get as _get

        defn = _get(code)
        return cls(
            code=code,
            severity=Severity(defn.severity),
            owner_skill=defn.owner,
            message=message,
            **kwargs,
        )


# ── CSV / Card fields ─────────────────────────────────────────────────────────

QA_FIELDS = (
    "Enunciado",
    "Resposta",
    "Campo de Estudo",
    "Notas Adicionais",
    "Imagens Relacionadas",
    "Referências",
    "Etiquetas",
)
CLOZE_FIELDS = (
    "Sentença",
    "Notas Adicionais",
    "Campo de Estudo",
    "Imagens Relacionadas",
    "Referências",
    "Etiquetas",
)


class CardRow(BaseModel):
    """A single parsed row from a QA or Cloze CSV."""

    row_number: int
    note_type: NoteType
    fields: dict[str, str]  # field_name -> value (raw, never stripped)
    raw: str  # original CSV line (for error reporting)
    column_count: int = 0
    structural_ok: bool = True  # False → semantic checks are SKIPPED_STRUCTURAL


# ── Validation Report ─────────────────────────────────────────────────────────


class CheckStatus(BaseModel):
    name: str
    result: Literal["PASS", "FAIL", "SKIP", "WARN"]
    diagnostics: list[Diagnostic] = Field(default_factory=list)
    reason: str | None = None  # why a check was SKIPPED (never a silent PASS)


class ValidationReport(BaseModel):
    """Output of validate_batch — the gate before export_final."""

    result: ValidationResult
    batch_id: str
    deck_slug: str | None = None
    checks: list[CheckStatus] = Field(default_factory=list)
    errors: list[Diagnostic] = Field(default_factory=list)  # CRITICAL only
    warnings: list[Diagnostic] = Field(default_factory=list)  # WARN only
    # Receipt token — only present on PASS
    content_hash: str | None = None  # sha256 of (qa_csv + cloze_csv)
    tool_version: str = "0.1.0"


# ── Manifesto models ──────────────────────────────────────────────────────────


class IDEntry(BaseModel):
    id: str
    batch: str
    type: NoteType
    status: CardStatus = CardStatus.ACTIVE
    topic: str
    anchor: str


class CatalogRef(BaseModel):
    material: str
    slug: str
    path: str
    version_hash: str
    media_dir: str
    prefix: str


class ChangelogEntry(BaseModel):
    date: str
    batch: str
    action: str  # VALIDAÇÃO, SPLIT, MERGE, DELETE, FLAG…
    id: str | None = None
    reason: str


class BatchMeta(BaseModel):
    state: str  # started | drafted | curated | enriched | illustrated | validated | exported
    cards: int
    gates: dict[str, str] = Field(default_factory=dict)


class TaxonomyNode(BaseModel):
    root: str
    nodes: list[str] = Field(default_factory=list)
    planned: dict[str, str] = Field(default_factory=dict)  # node -> batch_id


class TopicPlan(BaseModel):
    total_planned: int = 0
    batch_size: int = 20
    batches_planned: list[str] = Field(default_factory=list)
    breakdown: dict[str, int] = Field(default_factory=dict)
    rationale: str = ""


class Manifesto(BaseModel):
    schema_version: str = "1.0"
    deck_slug: str
    campo_estudo: str
    created: str
    updated: str
    last_validated_batch: str | None = None
    totals: dict[str, int] = Field(default_factory=dict)  # active, qa, cloze
    id_registry: list[IDEntry] = Field(default_factory=list)
    topic_log: list[str] = Field(default_factory=list)
    taxonomy: TaxonomyNode = Field(default_factory=lambda: TaxonomyNode(root=""))
    catalogs: list[CatalogRef] = Field(default_factory=list)
    changelog: list[ChangelogEntry] = Field(default_factory=list)
    suggestion_queue: list[str] = Field(default_factory=list)
    batches: dict[str, BatchMeta] = Field(default_factory=dict)
    topic_plan: TopicPlan | None = None


# ── Catalog ───────────────────────────────────────────────────────────────────


class CatalogEntry(BaseModel):
    filename: str
    page: int | None = None
    caption: str
    anki_tag: str = ""
    material: str = ""
    slug: str = ""
    namespaced: str = ""  # slug_filename
    item_type: Literal["figure", "table"] = "figure"
    title: str | None = None
    source: str = ""
    width_px: int | None = None
    height_px: int | None = None
    rows: int | None = None
    cols: int | None = None
    continued: bool = False
    continuation_of: str | None = None
    quarantined: bool = False
    noise: bool = False
    noise_reason: str = ""


# Re-exports (package API). Imported after the base classes: draft.py depends on them.
import anki_mcp.models.draft as _draft  # noqa: E402
import anki_mcp.models.triage as _triage  # noqa: E402

CardDraft = _draft.CardDraft
CardPatch = _draft.CardPatch
ExtraNotes = _draft.ExtraNotes
MediaFigure = _draft.MediaFigure
QuarantineRecord = _draft.QuarantineRecord

SectionNode = _triage.SectionNode
MustCoverInvariant = _triage.MustCoverInvariant
TopicBudgetEvaluation = _triage.TopicBudgetEvaluation

# ── Batch counting ───────────────────────────────────────────────────────────


class BatchCount(BaseModel):
    qa_cards: int
    cloze_notes: int
    cloze_indices: int  # sum of distinct cN per Cloze note = actual Anki cards
    total: int  # qa_cards + cloze_indices
    within_limit: bool
    excess: int = 0
    ext_batch_suggested: str | None = None  # e.g. "B03-EXT"
