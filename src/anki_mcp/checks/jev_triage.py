"""checks/jev_triage.py — Hybrid Clinical Triage (HCT) with JEV System 1 & deterministic fallback.

Implements section-based semantic triage:
1. Sectional Slicing: slices clinical source material into SectionNodes by markdown headings or paragraphs.
2. JEV System 1 Inference: queries TypeSafe System One (Score question type) for calibrated Expected Value (EV).
3. Deterministic Fallback: heuristic clinical density estimator if offline, no key, or on API error.
4. Universal Invariant Audit (Must-Cover Matrix): evaluates 8 core clinical axes.
5. Batch Partitioning: calculates total EV and partitions into discrete batches of <= 20 cards.
"""

from __future__ import annotations

import asyncio
import json
import math
import os
import re
import statistics
import urllib.error
import urllib.request
from typing import Any, Literal

from anki_mcp.checks.decisions import plan_topic_budget
from anki_mcp.models.triage import (
    MustCoverInvariant,
    PrunedSectionRecord,
    SectionNode,
    SubSectionSpec,
    TopicBudgetEvaluation,
)

KNOWLINE_ENDPOINT = os.environ.get("KNOWLINE_SYSTEMONE_URL", "").strip()
DEFAULT_KNOWLINE_MODEL = os.environ.get("KNOWLINE_MODEL", "m")
TYPESAFE_ENDPOINT = os.environ.get("TYPESAFE_SYSTEMONE_URL", "https://api.typesafe.ai/v1/systemone")
DEFAULT_JEV_MODEL = os.environ.get("TYPESAFE_MODEL", "jev-1.13.0")
OPENROUTER_DECISIONS_ENDPOINT = os.environ.get(
    "OPENROUTER_DECISIONS_URL", "https://openrouter.ai/api/alpha/decisions"
)
DEFAULT_OPENROUTER_JEV_MODEL = os.environ.get("OPENROUTER_JEV_MODEL", "~typesafe/jev-latest")

# ── Type Aliases for Strict Model Compliance ───────────────────────────────────

CoverStatus = Literal["COVERED", "GAP", "PARTIAL", "NOT_APPLICABLE"]
PruneReason = Literal[
    "NO_ACTIONABLE_CONDUCT",
    "LOW_DENSITY_EPIDEMIOLOGY",
    "AUDIENCE_EXCLUDED",
    "REDUNDANT_AXIS",
    "SURPLUS_TRUNCATION",
]
ConvergenceLevel = Literal["HIGH", "MEDIUM", "LOW", "DEGRADED_FALLBACK"]
SaturationStatus = Literal["DEFICIT", "OPTIMAL", "SURPLUS"]
ActionRequired = Literal[
    "proceed_normal",
    "request_more_source",
    "continue_with_deficit",
    "split_topic",
    "prioritize_core",
]
EngineVerdict = Literal["jev_system1", "deterministic_fallback", "triangulated_hybrid"]

# ── Universal Must-Cover Clinical Invariants ───────────────────────────────────

UNIVERSAL_INVARIANTS = (
    (
        "reconhecimento",
        "Reconhecimento / Apresentação Clínica e Semiologia",
        (
            r"apresenta|quadro cl[íi]nic|sintom|sinais?|tr[íi]ade|t[ée]trade|achados? cl[íi]nic"
            r"|semiolog|exame f[íi]sico|inspe[çc][ãa]o|palpa[çc][ãa]o|ausculta|hist[óo]ria cl[íi]nica|manifesta"
        ),
    ),
    (
        "padrao_ouro",
        "Confirmação Diagnóstica / Padrão-Ouro / Critérios",
        (
            r"padr[ãa]o[- ]ouro|gold standard|crit[ée]rios?|confirma[çc][ãa]o|bi[óo]psia"
            r"|PCR|sorolog|resson[âa]ncia|tomografia|fundoscopia|eletroneuromiografia|endoscopia|ecocardiograma"
            # "exame" solto capturava "exame físico" (semiologia = reconhecimento),
            # roubando o eixo de confirmação diagnóstica de cartões descritivos.
            r"|exames?\s+complementa(?:r|res)|exame\s+(?:laboratorial|de\s+imagem)"
        ),
    ),
    (
        "red_flags",
        "Sinais de Alarme / Emergência / Red Flags",
        (
            # "alerta" solto capturava estado mental basal ("vigil e alerta"),
            # rotulando semiologia normal como emergência.
            r"sinais? de alarme|sinal de alerta|red flags?|alerta cl[íi]nico|emerg[êe]ncia|urg[êe]ncia"
            r"|gravidade|risco iminente"
            r"|interna[çc][ãa]o|admiss[ãa]o em UTI|choque|parada|deteriora[çc][ãa]o|endoftalmite|perda s[úu]bita"
        ),
    ),
    (
        "tratamento_1a_linha",
        "Tratamento de 1ª Linha / Droga de Escolha / Conduta Definitiva",
        (
            r"primeira[- ]linha|1[ªa] linha|droga de escolha|tratamento|conduta|medidas? iniciais"
            r"|facoemulsifica[çc][ãa]o|cirurgia|esquema preferencial|dose|posologia|terapia|riluzol|ventila[çc][ãa]o|laser"
        ),
    ),
    (
        "seguranca_farmacologica",
        "Segurança Farmacológica / Efeitos Adversos Graves / Contraindicações",
        (
            r"contraindica|efeitos? advers|eventos? advers|toxicidade|intera|QTc|hepatotox|nefrotox"
            r"|cautela|precau[çc][ãa]o|evitar|black[- ]box|risco de sangramento|complica[çc]"
        ),
    ),
    (
        "diferencial_critico",
        "Diagnóstico Diferencial Crítico / Armadilhas",
        (
            r"diagn[óo]stico diferencial|diferenci|mimetizador|armadilha|pitfall|confundid|versus|\bvs\.?\b"
        ),
    ),
    (
        "refratariedade_resgate",
        "Refratariedade / 2ª Linha / Conduta de Resgate",
        (
            r"refrat[áa]ri|falha terap[êe]utica|segunda[- ]linha|2[ªa] linha|resgate|terceira[- ]linha|intoler[âa]ncia"
            r"|tardia|capsulotomia|suporte"
        ),
    ),
    (
        "seguimento_metas",
        "Seguimento / Metas Terapêuticas / Complicações Tardias",
        (
            r"seguimento|monitoriza[çc][ãa]o|metas?|progn[óo]stico|complica[çc][ãa]o tardia|recorr[êe]ncia|recidiva"
            r"|reavalia[çc][ãa]o|sobrevida|VILI"
        ),
    ),
)


def _discover_env_secret(var_name: str) -> str | None:
    """Read only explicitly supplied process environment, never ambient secret files."""
    value = os.environ.get(var_name, "").strip()
    return value or None


def get_typesafe_api_key() -> str | None:
    """Safely discover SystemOne API key (KnowLine primary or TypeSafe fallback)."""
    return (
        _discover_env_secret("KNOWLINE_API_KEY")
        or _discover_env_secret("TYPESAFE_API_KEY")
        or _discover_env_secret("OPENROUTER_API_KEY")
    )


def _post_systemone_with_fallback(
    state: str,
    questions: dict[str, Any],
    *,
    explicit_api_key: str | None = None,
    timeout_s: float = 6.0,
) -> dict[str, Any]:
    """Call KnowLine-31B-Gen1 primary and fall back to Jev TypeSafe (direct -> OpenRouter)."""
    targets: list[tuple[str, str, str, dict[str, str]]]
    if explicit_api_key:
        targets = [(TYPESAFE_ENDPOINT, DEFAULT_JEV_MODEL, explicit_api_key, {})]
    else:
        targets = []
        kl_key = _discover_env_secret("KNOWLINE_API_KEY")
        if kl_key and KNOWLINE_ENDPOINT:
            targets.append(
                (
                    KNOWLINE_ENDPOINT,
                    DEFAULT_KNOWLINE_MODEL,
                    kl_key,
                    {"ngrok-skip-browser-warning": "true"},
                )
            )
        ts_key = _discover_env_secret("TYPESAFE_API_KEY")
        if ts_key:
            targets.append((TYPESAFE_ENDPOINT, DEFAULT_JEV_MODEL, ts_key, {}))
        or_key = _discover_env_secret("OPENROUTER_API_KEY")
        if or_key:
            targets.append(
                (
                    OPENROUTER_DECISIONS_ENDPOINT,
                    DEFAULT_OPENROUTER_JEV_MODEL,
                    or_key,
                    {},
                )
            )

    last_exc: Exception | None = None
    for endpoint, model, key, extra_hdrs in targets:
        payload = {"model": model, "state": state, "questions": questions}
        req = urllib.request.Request(
            endpoint,
            data=json.dumps(payload).encode("utf-8"),
            headers={
                "Authorization": f"Bearer {key}",
                "Content-Type": "application/json",
                "User-Agent": "anki-workflow-mcp/0.3.1",
                **extra_hdrs,
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=timeout_s) as resp:
                data = json.loads(resp.read().decode("utf-8"))
                if isinstance(data, dict) and isinstance(data.get("answers"), dict):
                    return data
        except Exception as exc:
            last_exc = exc
            continue
    if last_exc is not None:
        raise last_exc
    raise RuntimeError("No SystemOne backend available")


def split_into_sections(text: str) -> list[SectionNode]:
    """Slice raw markdown/text into structured SectionNodes by headings or structural blocks."""
    lines = text.splitlines()
    sections: list[SectionNode] = []
    current_title = "Introdução / Contexto Geral"
    current_level = 2
    current_lines: list[str] = []
    sec_counter = 1

    heading_re = re.compile(r"^(#{1,4})\s+(.+)$")

    def _flush(title: str, level: int, body_lines: list[str]):
        nonlocal sec_counter
        body = "\n".join(body_lines).strip()
        if not body:
            return
        token_est = max(1, len(body) // 4)
        sec_id = f"sec_{sec_counter:02d}"
        sections.append(
            SectionNode(
                id=sec_id,
                title=title or f"Seção {sec_counter}",
                content=body,
                level=level,
                token_estimate=token_est,
            )
        )
        sec_counter += 1

    for line in lines:
        m = heading_re.match(line)
        if m:
            if current_lines:
                _flush(current_title, current_level, current_lines)
                current_lines = []
            current_level = len(m.group(1))
            current_title = m.group(2).strip()
        else:
            current_lines.append(line)

    if current_lines:
        _flush(current_title, current_level, current_lines)

    # If no headers were found and text is long, split by double newlines into blocks
    if len(sections) <= 1 and len(text) > 600:
        paragraphs = [p.strip() for p in text.split("\n\n") if p.strip()]
        if len(paragraphs) > 1:
            sections = []
            for i, p in enumerate(paragraphs, start=1):
                first_line = p.splitlines()[0][:60]
                sections.append(
                    SectionNode(
                        id=f"sec_{i:02d}",
                        title=f"Bloco {i}: {first_line}",
                        content=p,
                        level=2,
                        token_estimate=max(1, len(p) // 4),
                    )
                )

    return sections


def heuristic_section_density(
    content: str, title: str = ""
) -> tuple[float, float, str, dict[str, float]]:
    """Deterministic fallback estimator for section density and expected cards (EV).

    Returns:
        (ev_score, confidence, primary_obligation, probabilities)
    """
    text_lower = f"{title} {content}".lower()

    # Pure noise / chatter / epidemiology check
    is_chatter = bool(
        re.search(
            r"posso formular um caso|espero que ajude|como assistente|no pr[óo]ximo relato|ol[áa]|bom dia",
            text_lower,
        )
    )
    is_pure_epidemiology = bool(
        re.search(
            r"afeta cerca de \d+|preval[êe]ncia varia|descrita por .* em \d{4}|hist[óo]ria da medicina|estudos epidemiol[óo]gicos apontam",
            text_lower,
        )
    )
    has_clinical_decision = bool(
        re.search(
            r"tratamento|dose|posologia|contraindica|diagn[óo]stic|crit[ée]rio|padr[ãa]o[- ]ouro|droga de escolha|sinal de alarme",
            text_lower,
        )
    )

    if (is_chatter or is_pure_epidemiology) and not has_clinical_decision:
        return 0.0, 0.95, "epidemiologia_descritiva", {"0": 1.0, "1": 0.0, "2": 0.0, "3": 0.0}

    # Count clinical obligation matches across universal invariants
    matched_invariants: list[str] = []
    for slug, _label, pattern in UNIVERSAL_INVARIANTS:
        if re.search(pattern, text_lower):
            matched_invariants.append(slug)

    match_count = len(matched_invariants)
    primary = matched_invariants[0] if matched_invariants else "geral"

    # Check for cutoffs or dose specs (numbers with units)
    has_cutoffs = bool(
        re.search(
            r"\b\d+(?:[.,]\d+)?\s*(?:mg|mcg|ml|g|%|mmHg|bpm|anos?|meses|dias|UI)\b",
            text_lower,
        )
    )

    if match_count >= 3 or (match_count >= 2 and has_cutoffs):
        return 2.8, 0.85, primary, {"0": 0.05, "1": 0.10, "2": 0.35, "3": 0.50}
    elif match_count == 2 or (match_count == 1 and has_cutoffs):
        return 1.9, 0.80, primary, {"0": 0.10, "1": 0.30, "2": 0.50, "3": 0.10}
    elif match_count == 1 or has_clinical_decision:
        return 1.1, 0.75, primary, {"0": 0.15, "1": 0.70, "2": 0.15, "3": 0.00}
    else:
        # Generic low density
        return 0.5, 0.60, "contexto", {"0": 0.60, "1": 0.30, "2": 0.10, "3": 0.00}


def call_jev_section_score(
    content: str,
    title: str = "",
    api_key: str | None = None,
    timeout_s: float = 6.0,
) -> tuple[float, float, dict[str, float], str]:
    """Call JEV System 1 Score question type for section card expectation.

    Returns:
        (score, confidence, probabilities, engine_status)
    """
    key = api_key or get_typesafe_api_key()
    if not key:
        ev, conf, _prim, probs = heuristic_section_density(content, title)
        return ev, conf, probs, "deterministic_fallback_no_key"

    clean_snippet = f"### {title}\n{content}".strip()[:1400]
    questions_payload: dict[str, Any] = {
        "card_count_expected": {
            "type": "score",
            "instructions": (
                "Quantas obrigações clínicas independentes, regras diagnósticas ou condutas essenciais "
                "estão contidas aqui que justificam a criação de flashcards Anki independentes para médicos e residentes?"
            ),
            "criteria": [
                "0 cards: conteúdo puramente introdutório ou epidemiológico sem decisão prática",
                "1 card: contém uma única decisão ou conduta atômica",
                "2 cards: contém duas decisões/condutas independentes",
                "3+ cards: alta densidade decisória com 3 ou mais condutas/indicações/complicações distintas",
            ],
        }
    }

    try:
        data = _post_systemone_with_fallback(
            clean_snippet,
            questions_payload,
            explicit_api_key=api_key,
            timeout_s=timeout_s,
        )
        answer = data.get("answers", {}).get("card_count_expected", {})
        score = float(answer.get("score", 1.0))
        conf = float(answer.get("confidence", 0.8))
        probs = answer.get("probabilities", {})
        return score, conf, probs, "jev_system1"
    except Exception:
        # Graceful fallback on network timeout or HTTP error
        ev, conf, _prim, probs = heuristic_section_density(content, title)
        return ev, conf, probs, "deterministic_fallback_error"


def evaluate_must_cover_invariants(
    sections: list[SectionNode],
) -> tuple[list[MustCoverInvariant], list[str]]:
    """Evaluate coverage across the 8 Universal Clinical Invariants."""
    invariants: list[MustCoverInvariant] = []
    gaps: list[str] = []

    for slug, label, pattern in UNIVERSAL_INVARIANTS:
        matched: list[str] = []
        for sec in sections:
            combined = f"{sec.title} {sec.content}".lower()
            if re.search(pattern, combined):
                matched.append(sec.id)

        status: CoverStatus
        if matched:
            status = "COVERED"
            expl = f"Coberto por {len(matched)} seção(ões): {', '.join(matched)}."
        else:
            status = "GAP"
            expl = "Nenhuma menção explícita a este eixo clínico na fonte."
            gaps.append(f"GAP.{slug.upper()}")

        invariants.append(
            MustCoverInvariant(
                slug=slug,
                label=label,
                status=status,
                matched_sections=matched,
                explanation=expl,
            )
        )

    return invariants, gaps


AUDIENCE_WEIGHTS: dict[str, dict[str, float]] = {
    "R1_GENERALISTA": {
        "red_flags": 1.0,
        "conduta_imediata": 1.0,
        "contraindicacao": 1.0,
        "padrao_ouro": 1.0,
        "reconhecimento": 0.90,
        "diagnostico_diferencial": 0.75,
        "segunda_linha": 0.40,
        "estadiamento_avancado": 0.20,
        "mecanismo_fisiopatologico": 0.20,
        "epidemiologia_geral": 0.0,
        "geral": 0.70,
        "contexto": 0.50,
        "seguimento_metas": 0.70,
        "refratariedade_resgate": 0.40,
        "epidemiologia_descritiva": 0.0,
    },
    "ESPECIALISTA": {
        "red_flags": 1.0,
        "conduta_imediata": 1.0,
        "contraindicacao": 1.0,
        "padrao_ouro": 1.0,
        "reconhecimento": 0.85,
        "diagnostico_diferencial": 1.0,
        "segunda_linha": 1.0,
        "estadiamento_avancado": 1.0,
        "mecanismo_fisiopatologico": 0.50,
        "epidemiologia_geral": 0.10,
        "geral": 0.90,
        "contexto": 0.70,
        "seguimento_metas": 0.90,
        "refratariedade_resgate": 1.0,
        "epidemiologia_descritiva": 0.10,
    },
    "FULL_COMPREHENSIVE": dict.fromkeys(
        [
            "red_flags",
            "conduta_imediata",
            "contraindicacao",
            "padrao_ouro",
            "reconhecimento",
            "diagnostico_diferencial",
            "segunda_linha",
            "estadiamento_avancado",
            "mecanismo_fisiopatologico",
            "epidemiologia_geral",
            "geral",
            "contexto",
            "seguimento_metas",
            "refratariedade_resgate",
            "epidemiologia_descritiva",
        ],
        1.0,
    ),
}

AUDIENCE_PRUNE_THRESHOLD: dict[str, float] = {
    "R1_GENERALISTA": 0.35,
    "ESPECIALISTA": 0.25,
    "FULL_COMPREHENSIVE": 0.15,
}


def compute_structural_ev(text: str, sections_count: int) -> float:
    """Deterministic structural estimator based on markdown blocks, bullets, tables, and cutoffs."""
    sections = len(re.findall(r"^#{1,3}\s+", text, re.MULTILINE)) or sections_count
    bullets = len(re.findall(r"^[ \t]*[-*•]\s+[A-Za-z0-9]", text, re.MULTILINE))
    numbered = len(re.findall(r"^[ \t]*\d+[\.)]\s+[A-Za-z0-9]", text, re.MULTILINE))
    tables = len(re.findall(r"\|[-:]+\|", text))
    cutoffs = len(
        re.findall(
            r"\b\d+(?:[\.,]\d+)?\s*(?:mg/dL|g/dL|mmHg|cmH2O|mL/kg|bpm|ms|mcg|UI|%)\b",
            text,
            re.IGNORECASE,
        )
    )
    sec_base = max(1.0, sections * 1.8)
    feature_bonus = (bullets * 0.5) + (numbered * 0.5) + (tables * 1.5) + (cutoffs * 0.4)
    return round(sec_base + feature_bonus, 1)


def check_and_subdivide_section(sec: SectionNode) -> list[SubSectionSpec]:
    """A-Cascade: Subdivide dense sections into atomic sub-blocks to prevent k=3 ceiling saturation."""
    if sec.token_estimate < 300:
        return []
    paragraphs = [p.strip() for p in sec.content.split("\n\n") if len(p.strip()) > 80]
    if len(paragraphs) <= 1:
        return []

    subsections: list[SubSectionSpec] = []
    for idx, p in enumerate(paragraphs, 1):
        p_tokens = max(1, len(p) // 4)
        _, _, p_prim, _ = heuristic_section_density(p, f"{sec.title} parte {idx}")
        p_ev, _, _, _ = heuristic_section_density(p, f"{sec.title} parte {idx}")
        subsections.append(
            SubSectionSpec(
                sub_id=f"{sec.id}_sub{idx:02d}",
                inferred_title=f"{sec.title} (Parte {idx})",
                primary_obligation=p_prim,
                token_estimate=p_tokens,
                ev_estimate=round(p_ev, 1),
            )
        )
    return subsections


CognitiveZone = Literal["ZONA_FOCAL", "ZONA_NUCLEAR", "ZONA_MULTIDIMENSIONAL", "ZONA_SINDROMICA"]

ZONE_DESCRIPTORS: dict[CognitiveZone, str] = {
    "ZONA_FOCAL": "Protocolo hiperfocado ou conduta imediata de urgência com algoritmo linear",
    "ZONA_NUCLEAR": "Entidade clínica delimitada com ciclo diagnóstico-terapêutico padrão",
    "ZONA_MULTIDIMENSIONAL": "Condição crônica multissistêmica com estadiamento, múltiplos órgãos e suporte continuado",
    "ZONA_SINDROMICA": "Síndrome guarda-chuva com triagem diagnóstica e múltiplos arquétipos clínicos",
}

ZONE_ELASTIC_RANGES: dict[CognitiveZone, tuple[int, int]] = {
    "ZONA_FOCAL": (5, 14),
    "ZONA_NUCLEAR": (10, 22),
    "ZONA_MULTIDIMENSIONAL": (18, 42),
    "ZONA_SINDROMICA": (16, 32),
}

ZONE_TO_TAXONOMY: dict[
    CognitiveZone,
    Literal["MICRO_ACUTE", "ATOMIC_ENTITY", "CHRONIC_MULTISYSTEM", "UMBRELLA_SYNDROME"],
] = {
    "ZONA_FOCAL": "MICRO_ACUTE",
    "ZONA_NUCLEAR": "ATOMIC_ENTITY",
    "ZONA_MULTIDIMENSIONAL": "CHRONIC_MULTISYSTEM",
    "ZONA_SINDROMICA": "UMBRELLA_SYNDROME",
}


def call_jev_zone_choice(
    text: str, topic: str = "", api_key: str | None = None, timeout_s: float = 6.0
) -> tuple[CognitiveZone | None, float, str]:
    """Call JEV System 1 Choice question to classify cognitive decision zone.

    Returns:
        (zone_name, confidence, engine_status)
    """
    key = api_key or get_typesafe_api_key()
    if not key:
        return None, 0.0, "deterministic_fallback_no_key"

    clean_snippet = f"Tema: {topic}\n{text[:1800]}".strip()
    questions_payload: dict[str, Any] = {
        "zona_cognitiva": {
            "type": "choice",
            "instructions": "Classifique este tema clínico em uma das 4 zonas de complexidade pedagógica e arquitetural:",
            "criteria": ZONE_DESCRIPTORS,
        }
    }

    try:
        data = _post_systemone_with_fallback(
            clean_snippet,
            questions_payload,
            explicit_api_key=api_key,
            timeout_s=timeout_s,
        )
        ans = data.get("answers", {}).get("zona_cognitiva", {})
        choice = ans.get("choice", "")
        conf = float(ans.get("confidence", 0.0))
        if choice in ZONE_DESCRIPTORS:
            return choice, conf, "jev_system1"  # type: ignore[return-value]
    except Exception:
        pass

    return None, 0.0, "fallback"


def classify_cognitive_zone(
    text: str, topic: str = "", use_jev: bool = True, api_key: str | None = None
) -> tuple[CognitiveZone, str, tuple[int, int], list[str], str]:
    """Determine clinical cognitive decision zone, descriptor, elastic range, and subentities.

    Returns:
        (cognitive_zone, zone_descriptor, elastic_range, suggested_splits, engine)
    """
    umbrella_entities: list[str] = []
    entity_regexes = [
        (r"guillain[- ]barr[ée]", "Síndrome de Guillain-Barré (SGB)"),
        (r"pidc|cidp|polineuropatia inflamat[óo]ria desmielinizante cr[ôo]nica", "PIDC / CIDP"),
        (r"t[úu]nel do carpo", "Síndrome do Túnel do Carpo"),
        (
            r"neuropatia diab[ée]tica|polineuropatia sim[ée]trica distal",
            "Polineuropatia Diabética (PSD)",
        ),
        (r"hansen[íi]ase|mycobacterium leprae", "Neuropatia da Hanseníase"),
        (r"radiculopatia|h[ée]rnia discal", "Radiculopatias Lombossacras e Cervicais"),
        (r"vasculite|neuropatia vascul[íi]tica", "Neuropatias Vasculíticas"),
        (r"parsonage[- ]turner", "Síndrome de Parsonage-Turner"),
        (r"charcot[- ]marie[- ]tooth|cmt\b", "Doença de Charcot-Marie-Tooth"),
    ]
    for pattern, name in entity_regexes:
        if re.search(pattern, text.lower()):
            umbrella_entities.append(name)

    # 1. Try JEV System 1 Choice classification if active
    if use_jev:
        choice, conf, eng = call_jev_zone_choice(text, topic, api_key=api_key)
        if choice and conf >= 0.70:
            return (
                choice,
                ZONE_DESCRIPTORS[choice],
                ZONE_ELASTIC_RANGES[choice],
                umbrella_entities if choice == "ZONA_SINDROMICA" else [],
                eng,
            )

    # 2. Deterministic Fallback based on clinical patterns
    combined = f"{topic} {text[:3000]}".lower()

    # Micro-emergency / Acute presentation
    acute_patterns = [
        r"\b(?:oacr|glaucoma agudo|epistaxe|status epilepticus|trombol[íi]tico|emerg[êe]ncia oftalmol[óo]gica|choque anafil[áa]tico|crise hipertensiva|apresen[çc][ãa]o aguda|perda s[úu]bita)\b"
    ]
    if any(re.search(p, combined) for p in acute_patterns):
        zone: CognitiveZone = "ZONA_FOCAL"
        return zone, ZONE_DESCRIPTORS[zone], ZONE_ELASTIC_RANGES[zone], [], "deterministic_fallback"

    # Chronic Multisystem
    multisystem_patterns = [
        r"\b(?:esclerose lateral amiotr[óo]fica|ela\b|insufici[êe]ncia card[íi]aca|l[úu]pus eritematoso|doen[çc]a renal cr[ôo]nica|sepse|parkinson)\b"
    ]
    if any(re.search(p, combined) for p in multisystem_patterns):
        zone = "ZONA_MULTIDIMENSIONAL"
        return zone, ZONE_DESCRIPTORS[zone], ZONE_ELASTIC_RANGES[zone], [], "deterministic_fallback"

    # Umbrella Syndrome
    is_plural_topic = bool(
        re.search(
            r"(?:neuropatias|cefaleias|glomerulopatias|artrites|diarreias|anemias|arritmias|miopatias)\b",
            topic.lower(),
        )
    )
    if is_plural_topic or len(umbrella_entities) >= 3:
        zone = "ZONA_SINDROMICA"
        return (
            zone,
            ZONE_DESCRIPTORS[zone],
            ZONE_ELASTIC_RANGES[zone],
            umbrella_entities,
            "deterministic_fallback",
        )

    zone = "ZONA_NUCLEAR"
    return zone, ZONE_DESCRIPTORS[zone], ZONE_ELASTIC_RANGES[zone], [], "deterministic_fallback"


def classify_taxonomy_scope(text: str, topic: str = "") -> tuple[str, list[str]]:
    """Backwards-compatible helper returning legacy taxonomy_scope string."""
    zone, _, _, splits, _ = classify_cognitive_zone(text, topic, use_jev=False)
    return ZONE_TO_TAXONOMY.get(zone, "ATOMIC_ENTITY"), splits


def evaluate_topic_budget(
    text: str,
    topic: str = "",
    domain: str = "Medicina",
    deck_dir: str = "",
    batch_size: int = 20,
    target_audience: Literal[
        "R1_GENERALISTA", "ESPECIALISTA", "FULL_COMPREHENSIVE"
    ] = "R1_GENERALISTA",
    use_jev: bool = True,
    api_key: str | None = None,
) -> TopicBudgetEvaluation:
    """Synchronous orchestrator for Hybrid Clinical Triage and Topic Budgeting."""
    if not text.strip():
        return TopicBudgetEvaluation(
            topic=topic or "Vazio",
            domain=domain,
            deck_dir=deck_dir,
            raw_summary="Texto vazio: 0 seções, 0 cartões planejados.",
        )

    sections = split_into_sections(text)
    total_ev = 0.0
    active_ev = 0.0
    engines_used: set[str] = set()
    pruned_sections: list[PrunedSectionRecord] = []
    active_sections: list[SectionNode] = []

    # 1. Sectional Scoring with A-Cascade for k=3 Ceiling
    for sec in sections:
        # A-Cascade: Check if dense section requires sub-block decomposition
        subsections = check_and_subdivide_section(sec)
        if subsections:
            sec.needs_subdivision = True
            sec.subsections = subsections
            score = sum(s.ev_estimate for s in subsections)
            conf = 0.85
            probs = {"0": 0.05, "1": 0.20, "2": 0.40, "3": 0.35}
            eng = "a_cascade_subdivided"
            sec.primary_obligation = subsections[0].primary_obligation
        else:
            if use_jev:
                score, conf, probs, eng = call_jev_section_score(
                    sec.content, sec.title, api_key=api_key
                )
            else:
                score, conf, _, probs = heuristic_section_density(sec.content, sec.title)
                eng = "deterministic_fallback"

            # Attribute primary clinical obligation
            _, _, primary, _ = heuristic_section_density(sec.content, sec.title)
            sec.primary_obligation = primary

        # 2. Audience Relevance Weighting: gamma(A, o)
        weight = AUDIENCE_WEIGHTS.get(target_audience, {}).get(sec.primary_obligation, 0.80)
        weighted_score = round(score * weight, 2)

        sec.ev_score = weighted_score
        sec.confidence = round(conf, 2)
        sec.probabilities = probs
        sec.suggested_cards = round(weighted_score)
        total_ev += weighted_score
        engines_used.add(eng)

        # 3. Transparent Pruning Check: tau_A
        prune_tau = AUDIENCE_PRUNE_THRESHOLD.get(target_audience, 0.35)
        if weighted_score < prune_tau:
            reason_code: PruneReason
            if sec.primary_obligation in ("epidemiologia_geral", "epidemiologia_descritiva"):
                reason_code = "LOW_DENSITY_EPIDEMIOLOGY"
            elif weight < 0.50:
                reason_code = "AUDIENCE_EXCLUDED"
            else:
                reason_code = "NO_ACTIONABLE_CONDUCT"

            pruned_sections.append(
                PrunedSectionRecord(
                    id=sec.id,
                    title=sec.title,
                    ev_score=weighted_score,
                    token_estimate=sec.token_estimate,
                    reason=reason_code,
                    detail=f"EV ponderado {weighted_score:.2f} abaixo do limiar {prune_tau} para {target_audience} (Obrigação: {sec.primary_obligation})",
                )
            )
        else:
            active_sections.append(sec)
            active_ev += weighted_score

    # 4. Universal Invariant Audit (Must-Cover Matrix)
    invariants, gaps = evaluate_must_cover_invariants(sections)
    covered_invariants = sum(1 for inv in invariants if inv.status == "COVERED")
    ev_floor = max(1, covered_invariants)

    # 5. Cognitive Decision Zone Classification
    cognitive_zone, zone_desc, elastic_range, suggested_splits, _zone_eng = classify_cognitive_zone(
        text=text, topic=topic, use_jev=use_jev, api_key=api_key
    )
    taxonomy_scope = ZONE_TO_TAXONOMY.get(cognitive_zone, "ATOMIC_ENTITY")

    # 6. Triangulação Tripla de EV com Mediana (JEV + Decisions + Structural)
    # 6a. Decisions estimator (deterministic regex)
    try:
        dt_plan = plan_topic_budget(text, batch_size=batch_size)
        ev_decisions = float(dt_plan.get("total_planned", 0.0))
    except Exception:
        ev_decisions = float(ev_floor)

    # 6b. Structural estimator (independent nodes, bullets, tables, cutoffs)
    ev_structural = compute_structural_ev(text, len(sections))

    # 6c. Semantic JEV estimator
    jev_active = "jev_system1" in engines_used
    ev_jev: float | None = round(active_ev, 2) if jev_active else None

    # 6d. Median synthesis & convergence
    convergence: ConvergenceLevel
    engine_verdict: EngineVerdict
    if ev_jev is not None:
        candidates = [ev_jev, ev_decisions, ev_structural]
        raw_median = statistics.median(candidates)
        spread = (max(candidates) - min(candidates)) / max(raw_median, 1.0)
        if spread <= 0.25:
            convergence = "HIGH"
        elif spread <= 0.55:
            convergence = "MEDIUM"
        else:
            convergence = "LOW"
        degraded = False
        degraded_reason = ""
        engine_verdict = "triangulated_hybrid"
    else:
        candidates = [ev_decisions, ev_structural]
        raw_median = statistics.median(candidates)
        spread = (max(candidates) - min(candidates)) / max(raw_median, 1.0)
        convergence = "DEGRADED_FALLBACK"
        degraded = True
        degraded_reason = "JEV offline, timeout ou sem API key; estimativa baseada em mediana estrutural e determinística"
        engine_verdict = "deterministic_fallback"

    total_ev_triangulated = max(float(ev_floor), round(raw_median, 2))
    total_planned = round(total_ev_triangulated)

    # 7. Saturação em Relação à Faixa Elástica
    zona_min, zona_max = elastic_range
    saturation_status: SaturationStatus
    action_required: ActionRequired
    if total_planned < zona_min:
        saturation_status = "DEFICIT"
        source_incomplete = True
        source_exceeds_zone = False
        action_required = "request_more_source"
    elif total_planned > zona_max:
        saturation_status = "SURPLUS"
        source_incomplete = False
        source_exceeds_zone = True
        action_required = "split_topic" if suggested_splits else "prioritize_core"
    else:
        saturation_status = "OPTIMAL"
        source_incomplete = False
        source_exceeds_zone = False
        action_required = "proceed_normal"

    # Core batch B01 budget
    if cognitive_zone == "ZONA_FOCAL":
        core_budget = total_planned
    elif cognitive_zone == "ZONA_SINDROMICA" and target_audience == "R1_GENERALISTA":
        core_budget = 12
    elif cognitive_zone == "ZONA_MULTIDIMENSIONAL" and target_audience == "R1_GENERALISTA":
        core_budget = min(16, total_planned)
    else:
        core_budget = min(total_planned, batch_size)

    batches_needed = max(1, math.ceil(total_planned / batch_size))
    batches_planned = [f"B{i + 1:02d}" for i in range(batches_needed)]
    total_tokens = sum(sec.token_estimate for sec in sections)

    summary_lines = [
        f"Orçamento Triangulado ({engine_verdict}): {total_planned} cartões planejados "
        f"[Mediana: {raw_median:.1f} | JEV: {ev_jev} | Decisões: {ev_decisions:.1f} | Estrutural: {ev_structural:.1f} | Piso: {ev_floor}].",
        f"Convergência: {convergence} (Dispersão: {spread:.2f}) | Saturação: {saturation_status} (Ação: {action_required}).",
        f"Zona Cognitiva: {cognitive_zone} [Faixa Elástica: {zona_min}-{zona_max}] (Foco: {target_audience}).",
        f"Descritor: {zone_desc}.",
        f"Seções: {len(sections)} avaliadas ({len(pruned_sections)} podadas por baixa relevância clínica).",
        f"Lotes planejados: {', '.join(batches_planned)} (limite {batch_size} cartões/lote).",
        f"Matriz de Cobertura Universal: {covered_invariants}/8 eixos cobertos. Lacunas críticas: {len(gaps)}.",
    ]
    if suggested_splits:
        summary_lines.append(
            f"Subentidades sugeridas para desmembramento em decks dedicados: {', '.join(suggested_splits[:5])}."
        )

    return TopicBudgetEvaluation(
        topic=topic,
        domain=domain,
        deck_dir=deck_dir,
        total_sections=len(sections),
        ev_jev=ev_jev,
        ev_decisions=ev_decisions,
        ev_structural=ev_structural,
        ev_floor=ev_floor,
        total_ev=total_ev_triangulated,
        total_planned_cards=total_planned,
        convergence=convergence,
        convergence_spread=round(spread, 2),
        degraded=degraded,
        degraded_reason=degraded_reason,
        saturation_status=saturation_status,
        source_incomplete=source_incomplete,
        source_exceeds_zone=source_exceeds_zone,
        action_required=action_required,
        batch_size=batch_size,
        batches_needed=batches_needed,
        batches_planned=batches_planned,
        must_cover_matrix=invariants,
        gaps=gaps,
        sections=sections,
        source_tokens=total_tokens,
        engine=engine_verdict,
        cognitive_zone=cognitive_zone,
        zone_descriptor=zone_desc,
        elastic_range=elastic_range,
        taxonomy_scope=taxonomy_scope,
        target_audience=target_audience,
        recommended_range=elastic_range,
        core_batch_budget=core_budget,
        suggested_splits=suggested_splits,
        actionability_pruned_count=len(pruned_sections),
        pruned_sections=pruned_sections,
        raw_summary=" ".join(summary_lines),
    )


async def evaluate_topic_budget_async(
    text: str,
    topic: str = "",
    domain: str = "Medicina",
    deck_dir: str = "",
    batch_size: int = 20,
    target_audience: Literal[
        "R1_GENERALISTA", "ESPECIALISTA", "FULL_COMPREHENSIVE"
    ] = "R1_GENERALISTA",
    use_jev: bool = True,
    api_key: str | None = None,
) -> TopicBudgetEvaluation:
    """Async wrapper for non-blocking execution in FastMCP tools."""
    loop = asyncio.get_running_loop()
    return await loop.run_in_executor(
        None,
        evaluate_topic_budget,
        text,
        topic,
        domain,
        deck_dir,
        batch_size,
        target_audience,
        use_jev,
        api_key,
    )
