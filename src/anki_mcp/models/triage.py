"""models/triage.py — Pydantic models for HCT (Hybrid Clinical Triage) and budgeting."""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, Field


class SubSectionSpec(BaseModel):
    """Atomic sub-block proposed when a section breaches the k=3 JEV ceiling."""

    sub_id: str = Field(..., description="Sub-identifier (e.g. 'sec_02_sub01')")
    inferred_title: str = Field(..., description="Specific clinical focus heading")
    primary_obligation: str = Field(default="", description="Detected dominant obligation")
    token_estimate: int = Field(default=0, description="Estimated token count")
    ev_estimate: float = Field(default=0.0, description="Estimated cards for this sub-block")


class PrunedSectionRecord(BaseModel):
    """Audit record for a section excluded or pruned from the flashcard budget."""

    id: str = Field(..., description="ID da seção podada")
    title: str = Field(..., description="Título da seção")
    ev_score: float = Field(default=0.0, description="EV residual estimado antes da poda")
    token_estimate: int = Field(default=0, description="Tamanho em tokens")
    reason: Literal[
        "NO_ACTIONABLE_CONDUCT",
        "LOW_DENSITY_EPIDEMIOLOGY",
        "AUDIENCE_EXCLUDED",
        "REDUNDANT_AXIS",
        "SURPLUS_TRUNCATION",
    ] = Field(..., description="Motivo taxonômico da poda")
    detail: str = Field(default="", description="Justificativa clínica detalhada")


class SectionNode(BaseModel):
    """A semantic section extracted from source material for clinical triage."""

    id: str = Field(..., description="Unique section identifier (e.g. 'sec_01' or title slug)")
    title: str = Field(..., description="Section title or inferred clinical heading")
    content: str = Field(..., description="Text content of this section")
    level: int = Field(default=2, description="Heading level (1, 2, 3...)")
    token_estimate: int = Field(default=0, description="Estimated token count")
    ev_score: float = Field(
        default=0.0, description="Expected value of flashcards (EV) from JEV or heuristic"
    )
    confidence: float = Field(default=1.0, description="Confidence score [0.0, 1.0]")
    primary_obligation: str = Field(default="", description="Dominant clinical obligation detected")
    suggested_cards: int = Field(default=0, description="Rounded target cards for this section")
    probabilities: dict[str, float] = Field(
        default_factory=dict, description="Class probabilities from JEV"
    )

    # A-Cascade support for k=3 ceiling
    needs_subdivision: bool = Field(
        default=False, description="True se a seção excede o teto k=3 do JEV"
    )
    subsections: list[SubSectionSpec] = Field(
        default_factory=list, description="Subdivisões propostas para seções densas"
    )


class MustCoverInvariant(BaseModel):
    """An essential clinical invariant in the Universal Clinical Decision Matrix."""

    slug: str = Field(
        ..., description="Standardized invariant slug (e.g. 'reconhecimento', 'padrao_ouro')"
    )
    label: str = Field(..., description="Clinical label in pt-BR")
    status: Literal["COVERED", "GAP", "PARTIAL", "NOT_APPLICABLE"] = Field(
        default="GAP", description="Coverage status"
    )
    matched_sections: list[str] = Field(
        default_factory=list, description="IDs of sections covering this invariant"
    )
    explanation: str = Field(default="", description="Justification or recommendation")


class TopicBudgetEvaluation(BaseModel):
    """Complete budget evaluation and batch partitioning plan for a clinical topic."""

    topic: str = Field(default="", description="Name or title of the medical topic")
    domain: str = Field(default="Medicina", description="Clinical field or subspecialty")
    deck_dir: str = Field(default="", description="Path to the deck directory if bound")
    total_sections: int = Field(default=0, description="Number of parsed clinical sections")
    total_ev: float = Field(default=0.0, description="Sum of expected values (EV) across sections")
    total_planned_cards: int = Field(
        default=0, description="Total target cards planned (rounded EV)"
    )
    batch_size: int = Field(default=20, description="Maximum cards per batch (default: 20)")
    batches_needed: int = Field(default=1, description="Number of discrete batches needed")
    batches_planned: list[str] = Field(
        default_factory=list, description="Batch labels (e.g. ['B01', 'B02'])"
    )
    must_cover_matrix: list[MustCoverInvariant] = Field(
        default_factory=list, description="Universal clinical invariants status"
    )
    gaps: list[str] = Field(default_factory=list, description="Critical path omissions detected")
    sections: list[SectionNode] = Field(default_factory=list, description="Evaluated section nodes")
    source_tokens: int = Field(default=0, description="Total estimated source tokens")
    engine: Literal["jev_system1", "deterministic_fallback", "triangulated_hybrid"] = Field(
        default="triangulated_hybrid", description="Inference engine used for EV calibration"
    )
    # ── Triangulação Tripla de EV ─────────────────────────────────────────────
    ev_jev: float | None = Field(default=None, description="Expected Value via JEV System 1")
    ev_decisions: float = Field(
        default=0.0, description="Expected Value via deterministic regex rules"
    )
    ev_structural: float = Field(
        default=0.0, description="Expected Value via structural nodes (bullets, cutoffs, tables)"
    )
    ev_floor: int = Field(
        default=0, description="Piso inviolável de invariantes essenciais detectados"
    )
    convergence: Literal["HIGH", "MEDIUM", "LOW", "DEGRADED_FALLBACK"] = Field(
        default="HIGH", description="Convergência entre o modelo semântico e determinístico"
    )
    convergence_spread: float = Field(
        default=0.0, description="Dispersão relativa normalizada entre os estimadores"
    )
    degraded: bool = Field(
        default=False, description="True se operando sob fallback degradado (sem JEV ou timeout)"
    )
    degraded_reason: str = Field(
        default="", description="Justificativa técnica do modo degradado se aplicável"
    )

    # ── Saturação e Decisão de Lote ───────────────────────────────────────────
    saturation_status: Literal["DEFICIT", "OPTIMAL", "SURPLUS"] = Field(
        default="OPTIMAL", description="Estado da fonte em relação à faixa elástica da zona"
    )
    source_incomplete: bool = Field(default=False, description="True se EV < zona_min")
    source_exceeds_zone: bool = Field(default=False, description="True se EV > zona_max")
    action_required: Literal[
        "proceed_normal",
        "request_more_source",
        "continue_with_deficit",
        "split_topic",
        "prioritize_core",
    ] = Field(default="proceed_normal", description="Ação recomendada ao agente ou usuário")

    cognitive_zone: Literal[
        "ZONA_FOCAL", "ZONA_NUCLEAR", "ZONA_MULTIDIMENSIONAL", "ZONA_SINDROMICA"
    ] = Field(default="ZONA_NUCLEAR", description="Cognitive decision zone")
    zone_descriptor: str = Field(
        default="", description="Functional clinical descriptor of the zone"
    )
    elastic_range: tuple[int, int] = Field(
        default=(0, 0), description="Broad elastic reference range (min, max)"
    )
    taxonomy_scope: Literal[
        "MICRO_ACUTE", "ATOMIC_ENTITY", "CHRONIC_MULTISYSTEM", "UMBRELLA_SYNDROME"
    ] = Field(
        default="ATOMIC_ENTITY", description="Categorized clinical scope level (backwards-compat)"
    )
    target_audience: Literal["R1_GENERALISTA", "ESPECIALISTA", "FULL_COMPREHENSIVE"] = Field(
        default="R1_GENERALISTA", description="Target medical audience"
    )
    recommended_range: tuple[int, int] = Field(
        default=(0, 0), description="Recommended card count range (min, max)"
    )
    core_batch_budget: int = Field(
        default=0, description="Recommended card budget for the primary B01 batch"
    )
    suggested_splits: list[str] = Field(
        default_factory=list,
        description="Subentities suggested for dedicated child decks (if umbrella)",
    )
    actionability_pruned_count: int = Field(
        default=0, description="Number of low-actionability / pure epidemiology sections pruned"
    )
    pruned_sections: list[PrunedSectionRecord] = Field(
        default_factory=list, description="Audit log of pruned sections with clinical reason"
    )
    not_attended: list[str] = Field(
        default_factory=lambda: [
            "cascade_split_automatic_partitioning",
            "plan_accuracy_telemetry_in_manifesto",
            "retrospective_ground_truth_calibration",
        ],
        description="Features explicitly planned but deferred to future backlog",
    )
    known_risks: list[str] = Field(
        default_factory=lambda: [
            "zone_classification_non_deterministic_when_use_jev_true_without_cache"
        ],
        description="Known architectural trade-offs or constraints",
    )
    raw_summary: str = Field(default="", description="Human-readable executive summary")
