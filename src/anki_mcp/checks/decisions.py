"""checks/decisions.py — decision-point triage, content filter and card budget.

Everything here is deterministic: regular expressions over pt-BR text, no LLM
and no network call at runtime. The same input always gives the same verdict,
so a budget can be re-derived and audited later.

Three layers, in the order a topic is processed:

1. Content filter — :func:`classify_content`
   Each candidate sentence gets ``CARD`` (it states a decision: a diagnostic
   criterion, a cut-off with number+unit, a first-line therapy, a
   contraindication, a red flag, a differential...), ``MAYBE`` (no decision
   trigger, or only a mechanism) or ``DISCARD`` (epidemiology described for its
   own sake, history of medicine, assistant chatter, biochemical detail). Every
   verdict carries a reason. Triggers are *phrase patterns*, never single
   words: "dose" or "tratamento" alone decide nothing. Negation is reported
   (``polarity``) and never used to lower a verdict: a contraindication is a
   negated claim and is often the highest-yield card.

2. Attribution — :func:`assign_primary`
   A decision-bearing sentence is counted under **one** primary decision type.
   Counting it under every type its words touched inflated coverage (a type
   with a target of 4 reached 33 candidates) and hid gaps. Secondary matches
   are still reported, but never counted.

3. Budget — :func:`budget_from_counts` / :func:`plan_topic_budget`
   Obligations set the target range ``[total_min, total_max]``. Real counts per
   type are compared against it, giving a per-type status (gap, below minimum,
   surplus, ok), orphans, a split into batches of at most 20 cards, and a
   justification where every line reads ``coberto X / alvo Y``. A number without
   a named obligation is never emitted, and no card is ever invented.

The top-down axis (obligations) and the bottom-up axis (sentence score from
:mod:`anki_mcp.checks.triage`) stay separate. Their disagreement is what the
gap / surplus / orphan report exposes.
"""

from __future__ import annotations

import math
import re
from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass, field
from typing import Any

from anki_mcp.checks.batch import MAX_BATCH
from anki_mcp.checks.triage import (
    _CHATTER_RE,
    _FILLER_RE,
    _LOW_YIELD_RE,
    _META_RE,
    detect_polarity,
)

__all__ = [
    "DECISION_TAXONOMY",
    "Budget",
    "BudgetRow",
    "ContentVerdict",
    "DecisionType",
    "Plan",
    "annotate_content",
    "applicable_types",
    "assign_primary",
    "budget_from_counts",
    "build_coverage",
    "classify_candidate",
    "classify_content",
    "evaluate_plan",
    "plan_topic_budget",
    "recover_gaps",
]


# ── Taxonomy ──────────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class DecisionType:
    """One kind of clinical decision a topic may require.

    Attributes:
        slug: Stable identifier used in reports and coverage keys.
        label: Human-readable Portuguese label.
        target_min: Fewest cards this decision legitimately deserves.
        target_max: Most cards before the topic becomes redundant.
        triggers: Regex alternation of *phrase* patterns (case-insensitive).
            A pattern must carry the decision, not a word that merely
            co-occurs with it.
        obrigatorio: True when any named disease owes this type (recognition).
            A gap in such a type is always an extraction failure.
        trigger_min: Distinct phrase matches needed before the type counts as
            *applicable* to a topic. Generic phrasings use 2; one unambiguous
            phrase (a named criterion) is enough with 1.
    """

    slug: str
    label: str
    target_min: int
    target_max: int
    triggers: str
    obrigatorio: bool = False
    trigger_min: int = 1

    def matches(self, text: str) -> bool:
        """True when *text* has at least ``trigger_min`` distinct phrase matches."""
        return self.hit_count(text) >= self.trigger_min

    def hit_count(self, text: str) -> int:
        """Number of distinct phrases of this type found in *text*."""
        return len({m.group(0).lower() for m in _compile(self.triggers).finditer(text)})


_CACHE: dict[str, re.Pattern[str]] = {}


def _compile(pattern: str) -> re.Pattern[str]:
    compiled = _CACHE.get(pattern)
    if compiled is None:
        compiled = re.compile(pattern, re.IGNORECASE)
        _CACHE[pattern] = compiled
    return compiled


#: Cut-off with number and unit: "maior que 35 repetições", "≥ 140 mmHg",
#: "acima de 2 mg/dL". A bare number ("35 pacientes") is not a cut-off.
_CUTOFF = (
    r"(?:>|≥|<|≤|maior (?:que|do que)|menor (?:que|do que)|acima de|abaixo de"
    r"|superior a|inferior a|igual ou (?:maior|superior|menor|inferior)(?: que| a)?"
    r"|ponto de corte de)\s*(?:de\s*)?\d+(?:[.,]\d+)?\s*"
    r"(?:%|repeti\w*|mg/dL|mg|mcg|ml|mmol|mmHg|mm|anos?|meses|dias|semanas|horas|h\b"
    r"|UI|ng|pg|ms|pontos?|cm|kg|g\b)"
)

#: The twelve decision types, in presentation order. Order also breaks ties in
#: :func:`assign_primary`. Cardinalities are tight on purpose: a type earns a
#: second card only when the decision has a second branch.
DECISION_TAXONOMY: tuple[DecisionType, ...] = (
    DecisionType(
        slug="marco_diagnostico",
        label="Marco diagnóstico (o que fecha o diagnóstico)",
        target_min=1,
        target_max=2,
        triggers=(
            r"crit[ée]rios? (?:diagn[óo]stic\w*|de (?:inclus[ãa]o|defini[çc][ãa]o|diagn[óo]stico))"
            r"|crit[ée]rios? (?:maiores?|menores?)"
            r"|preench\w* (?:o |os |a )?crit[ée]ri\w*"
            r"|fecha(?:m)? o diagn[óo]stico"
            r"|diagn[óo]stico (?:[ée] )?(?:confirmad|estabelecid|definid|fechad)\w*"
            r"|diagn[óo]stico cl[íi]nic\w* (?:[ée]|requer|exige|baseia|se baseia)"
            r"|define-se (?:como|por)|marco definidor|defini[çc][ãa]o de caso"
            r"|diagnostic criteria|baseia-se na presen[çc]a"
            rf"|{_CUTOFF}"
        ),
        trigger_min=1,
    ),
    DecisionType(
        slug="monitorizacao",
        label="Monitorização e escalas de seguimento",
        target_min=2,
        target_max=6,
        triggers=(
            r"escala (?:de |do |da )?[A-Z]{2,6}\b|escore (?:de |do |da )?[A-Z]{2,6}\b"
            r"|\b(?:EDSS|UHDRS|TFC|NEDA|SDMT)\b"
            r"|rastreio (?:anual|peri[óo]dic\w*)|monitor\w* (?:anual|peri[óo]dic\w*|cl[íi]nic\w*|de rotina)"
            r"|RM de (?:controle|rotina)|seguimento (?:anual|semestral|trimestral|peri[óo]dic\w*)"
            r"|acompanhamento (?:anual|semestral|peri[óo]dic\w*)"
            r"|estadiamento (?:funcional|cl[íi]nic\w*)|marcos? locomotores"
            r"|atrofia (?:cerebral|tal[âa]mica)"
        ),
        trigger_min=2,
    ),
    DecisionType(
        slug="bioetica",
        label="Aconselhamento genético e bioética",
        target_min=1,
        target_max=3,
        triggers=(
            r"aconselhamento gen[ée]tic\w*|conselho gen[ée]tic\w*"
            r"|teste gen[ée]tic\w* (?:pr[ée]-sintom\w*|preditiv\w*|pr[ée]-teste)"
            r"|aconselhamento pr[ée]-teste|pr[ée]-sintom[áa]tic\w*"
            r"|diretiva\w* antecipad\w*|consentimento (?:informado|livre)"
            r"|risco de suic[íi]dio|revela[çc][ãa]o (?:do|de) (?:diagn[óo]stico|resultado)"
            r"|direito de n[ãa]o saber|confidencialidade"
        ),
        trigger_min=2,
    ),
    DecisionType(
        slug="cenario_especial",
        label="Comorbidade / cenário especial",
        target_min=1,
        target_max=2,
        triggers=(
            r"(?:na|durante a|em) gesta[çc][ãa]o|gr[áa]vid\w*|puerp[ée]rio|amamenta[çc][ãa]o"
            r"|\bidos[oa]s?\b|pacientes? (?:pedi[áa]tric\w*|renais|hep[áa]ticos?|imunossuprimid\w*)"
            r"|insufici[êe]ncia (?:renal|hep[áa]tica|card[íi]aca)|comorbidade"
            r"|em (?:crian[çc]as|adolescentes|idosos)"
            r"|depress[ãa]o (?:com|e) idea[çc][ãa]o suicida|hist[óo]ria de (?:depress|tentativa)"
            r"|res[íi]duo p[óo]s-mict\w*|disfagia (?:grave|importante)"
        ),
        trigger_min=2,
    ),
    DecisionType(
        slug="prognostico",
        label="Prognóstico e causa de morte",
        target_min=1,
        target_max=2,
        triggers=(
            r"progn[óo]stic\w*|sobrevida (?:m[ée]dia|mediana|de \d)"
            r"|causa (?:mais )?(?:comum )?de (?:morte|[óo]bito)"
            r"|mortalidade (?:de|em|por|é)|expectativa de vida"
            r"|tempo (?:m[ée]dio )?at[ée] (?:o )?[óo]bito|evolu[çc][ãa]o natural"
            r"|preditor\w* de"
        ),
        trigger_min=2,
    ),
    DecisionType(
        slug="diferencial",
        label="Diferencial que inverte conduta",
        target_min=1,
        target_max=2,
        triggers=(
            r"diagn[óo]stico diferencial|diferenciar (?:de|entre|do|da)|distinguir (?:de|entre|do|da)"
            r"|mimetiz\w*|diferencial (?:com|de|inclui|imp[õo]e)"
            r"|(?:n[ãa]o|deve) (?:se )?confundir|confund\w* com|em vez de"
            r"|excluir (?:outras|outras causas)|exclus[ãa]o de (?:causas|doen[çc]a)"
            r"|\bLESP\b|\bLEMP\b"
        ),
        trigger_min=1,
    ),
    DecisionType(
        slug="red_flag",
        label="Armadilha / red flag / apresentação atípica",
        target_min=1,
        target_max=2,
        triggers=(
            r"red flags?|bandeira\w* vermelh\w*|sinais? de alarme"
            r"|(?:deve|devem|deveria) (?:alertar|levantar suspeita)|suspeitar (?:de|quando|em)"
            r"|(?:apresenta[çc][ãa]o|quadro) at[íi]pic\w*|armadilha|erro (?:comum|frequente|diagn[óo]stic\w*)"
            r"|(?:piora|agrava[çc][ãa]o) (?:r[áa]pid\w*|s[úu]bit\w*)"
            r"|sinal (?:de )?(?:gravidade|risco)"
        ),
        trigger_min=2,
    ),
    DecisionType(
        slug="seguranca_farmacologica",
        label="Segurança farmacológica (EAG / contraindicação)",
        target_min=1,
        target_max=4,
        triggers=(
            r"contraindica\w*|efeitos? advers\w*|eventos? advers\w*|toxicidade"
            r"|intera[çc][ãa]o (?:medicamentosa|droga|com)"
            r"|risco (?:de )?(?:hepatotox\w*|cardiotox\w*|arrit\w*|LEMP|edema macular|suic\w*|sangramento)"
            r"|prolongamento do (?:intervalo )?QT\w*|\bQTc?\b"
            r"|abstin[êe]ncia (?:grave|s[úu]bita)|rebote"
            r"|n[ãa]o (?:deve|devem) (?:ser )?(?:usad\w*|administrad\w*|prescrit\w*)"
            r"|cautela|precau[çc][ãa]o|evitar (?:o )?uso"
        ),
        trigger_min=2,
    ),
    DecisionType(
        slug="tratamento_1a_linha",
        label="Tratamento de primeira linha",
        target_min=2,
        target_max=6,
        triggers=(
            r"primeira[- ]linha|\b1[ªa]\.? linha|segunda[- ]linha|\b2[ªa]\.? linha|terceira[- ]linha"
            r"|tratamento de (?:escolha|primeira|manuten[çc][ãa]o|resgate)|droga de escolha"
            r"|(?:primeira|segunda) op[çc][ãa]o|\bresgate\b"
            r"|indicad[oa] (?:como|para) (?:tratamento|terapia)"
            r"|tratamento (?:sintom[áa]tic\w*|farmacol[óo]gic\w*|de suporte)"
            r"|(?:dose|doses|posologia) (?:inicial|de manuten[çc][ãa]o|m[áa]xim\w*|usual|recomendad\w*)"
            r"|\d+(?:[.,]\d+)?\s*(?:mg|mcg|g|ml|UI)\s*(?:/|por)\s*(?:dia|kg|d)\b"
            r"|tetrabenazina|deutetrabenazina|valbenazina|riluzol|amantadina|clonazepam|haloperidol"
        ),
        trigger_min=2,
    ),
    DecisionType(
        slug="confirmacao",
        label="Confirmação complementar (exame / achado característico)",
        target_min=2,
        target_max=8,
        triggers=(
            r"(?:RM|resson[âa]ncia(?: magn[ée]tica)?|tomografia|TC)\s+(?:de |do |da |cr[âa]nio|encef[áa]lic\w*|com |mostra|evidencia|revela|demonstra)"
            r"|achado\w* (?:caracter[íi]stic\w*|t[íi]pic\w*|patognom\w*)"
            r"|bandas oligoclonais|\bBOC\b"
            r"|teste gen[ée]tic\w*|testes? (?:sorol[óo]gic\w*|molecular\w*)"
            r"|atrofia (?:do |de |dos |das )?(?:n[úu]cleo caudado|estriado|caudado|putame)"
            r"|hipersinal|realce (?:pelo|por|com) (?:contraste|gadol[íi]nio)"
            r"|l[íi]quor (?:com|mostra|apresenta)|eletroneuromiografia|sorologia|VDRL"
            r"|bi[óo]psia"
        ),
        trigger_min=2,
    ),
    DecisionType(
        slug="mecanismo",
        label="Mecanismo / fisiopatologia",
        target_min=1,
        target_max=4,
        triggers=(
            r"mecanismo (?:de a[çc][ãa]o|fisiopatol[óo]gic\w*|molecular)|fisiopatolog\w*"
            r"|patog[êe]nes\w*|etiopatogen\w*"
            r"|ganho de fun[çc][ãa]o|huntingtina|gene HTT|muta[çc][ãa]o (?:no|do|da) gene"
            r"|neurodegenera\w*|desmieliniz\w*|autoimun\w*"
            r"|\bVMAT2\b|prote[íi]na (?:mutante|anormal)|barreira hematoencef[áa]lica"
        ),
        trigger_min=2,
    ),
    DecisionType(
        slug="reconhecimento",
        label="Reconhecimento (padrão de apresentação e quem adoece)",
        target_min=1,
        target_max=5,
        triggers=(
            r"quadro cl[íi]nic\w*|apresenta[çc][ãa]o (?:cl[íi]nic\w*|t[íi]pic\w*|cl[áa]ssic\w*|inicial|precoce)"
            r"|tr[íi]ade (?:cl[áa]ssic\w*|can[ôo]nic\w*|motora|cognitiv\w*)"
            r"|manifesta-se (?:por|como|com)|se manifesta (?:por|como|com)"
            r"|caracteriz\w* (?:por|como)"
            r"|sintomas? (?:iniciais|precoces|cardinais|mais comuns|t[íi]picos)"
            r"|idade de in[íi]cio|faixa et[áa]ria|entre os \d+ e os \d+ anos|por volta dos \d+ anos"
            r"|forma juvenil|(?:costuma|tipicamente) (?:se )?(?:inicia|come[çc]a|afeta|apresenta)"
            r"|(?:afeta|acomete) (?:mais )?(?:homens|mulheres|adultos|crian[çc]as|idosos)\b"
        ),
        obrigatorio=True,
        trigger_min=1,
    ),
)


# ── Content filter ────────────────────────────────────────────────────────────

_DISCARD_EPIDEMIOLOGY = re.compile(
    r"preval[êe]ncia|incid[êe]ncia|afeta (?:cerca de |aproximadamente |mais de |em torno de )?"
    r"\d[\d.,]*\s*(?:milh[õo]es?|mil\b|pessoas|%)"
    r"|\d[\d.,]*\s*(?:milh[õo]es|mil) de (?:pessoas|habitantes|casos)"
    r"|por 100\s?(?:mil|\.000)|no mundo\b|em todo o mundo|ao redor do mundo"
    r"|dados epidemiol[óo]gic\w*|estudos? epidemiol[óo]gic\w*",
    re.IGNORECASE,
)
_DISCARD_HISTORY = re.compile(
    r"descrit[oa] (?:pela primeira vez )?(?:em|por|pelo|pela|no ano de)\s+(?:\S+\s+){0,4}?(?:1[89]\d{2}|20[0-2]\d)"
    r"|foi (?:primeiro )?descrit\w* (?:por|em)|hist[óo]ria (?:da|do|das|dos)\s+(?:medicina|descri\w*|doen\w*)"
    r"|\bem (?:1[89]\d{2}),?\s|homenagem|primeir[oa] (?:relato|descri[çc][ãa]o)",
    re.IGNORECASE,
)
_DISCARD_BIOCHEM = re.compile(
    r"peso molecular|massa molar|f[óo]rmula (?:qu[íi]mica|molecular|estrutural)"
    r"|estrutura (?:qu[íi]mica|molecular|terci[áa]ria|cristalina)|\bpKa\b|ponto de fus[ãa]o|solubilidade",
    re.IGNORECASE,
)


def _ci(rx: re.Pattern[str]) -> re.Pattern[str]:
    """Case-insensitive copy: sentence-initial words ("Posso") must still match."""
    return re.compile(rx.pattern, rx.flags | re.IGNORECASE)


_DISCARD_GROUPS: tuple[tuple[str, re.Pattern[str]], ...] = (
    ("epidemiologia descritiva", _DISCARD_EPIDEMIOLOGY),
    ("história da medicina", _DISCARD_HISTORY),
    ("detalhe bioquímico", _DISCARD_BIOCHEM),
    ("meta / conversa", _ci(_META_RE)),
    ("meta / conversa", _ci(_CHATTER_RE)),
    ("meta / conversa", _ci(_LOW_YIELD_RE)),
    ("meta / conversa", _ci(_FILLER_RE)),
)

#: Decision types that never earn a CARD alone. Mechanism is owed by every
#: disease, but a mechanism sentence changes a decision only when it is the
#: reason behind one. Alone it is MAYBE.
_NOT_SUFFICIENT_ALONE: frozenset[str] = frozenset({"mecanismo"})

#: Types whose negated claims carry the decision. The writer must preserve the
#: negation; the verdict is not lowered because of it.
_NEGATION_SENSITIVE: frozenset[str] = frozenset({"seguranca_farmacologica", "cenario_especial"})

_CONTRAINDICATION_RE = re.compile(r"contraindica\w*", re.IGNORECASE)


@dataclass(frozen=True)
class ContentVerdict:
    """Deterministic verdict for one candidate sentence, with its reason.

    Attributes:
        verdict: ``CARD`` (states a decision), ``MAYBE`` (no decision trigger,
            or only a mechanism) or ``DISCARD`` (non-decisional content).
        reason: Justification for the verdict (pt-BR).
        decisions: Every decision type whose phrase pattern the text hits.
        primary: The one type that counts for coverage (CARD only), or None.
        discard: Discard groups matched (epidemiology, history, ...).
        polarity: ``affirmed`` or ``negated`` (see triage.detect_polarity).
    """

    verdict: str
    reason: str
    decisions: tuple[str, ...]
    primary: str | None
    discard: tuple[str, ...]
    polarity: str


def _hits(text: str, taxonomy: Sequence[DecisionType]) -> dict[str, int]:
    return {dt.slug: dt.hit_count(text) for dt in taxonomy}


def _pick_primary(hits: Mapping[str, int], taxonomy: Sequence[DecisionType]) -> str | None:
    """Most-hit type, preferring non-mechanism types; ties go to taxonomy order."""
    order = {dt.slug: i for i, dt in enumerate(taxonomy)}
    strong = [s for s, n in hits.items() if n >= 1 and s not in _NOT_SUFFICIENT_ALONE]
    pool = strong or [s for s, n in hits.items() if n >= 1]
    if not pool:
        return None
    return min(pool, key=lambda s: (-hits[s], order[s]))


def assign_primary(
    text: str,
    taxonomy: Sequence[DecisionType] = DECISION_TAXONOMY,
) -> str | None:
    """Return the single decision type a text is counted under, or None."""
    return _pick_primary(_hits(text, taxonomy), taxonomy)


def classify_content(
    text: str,
    taxonomy: Sequence[DecisionType] = DECISION_TAXONOMY,
) -> ContentVerdict:
    """Classify one candidate as CARD, MAYBE or DISCARD, with a reason.

    Rules, first match wins:

    1. A decision type other than mechanism is hit -> ``CARD``. A discard
       pattern in the same sentence does not override it: "a prevalência é
       alta, mas o diagnóstico exige RM" still states a decision.
    2. Only mechanism is hit -> ``MAYBE`` (owed, but not a decision by itself).
    3. No decision hit, but a discard group matches -> ``DISCARD``.
    4. Nothing matches -> ``MAYBE`` (no decision trigger; a human decides).

    Polarity is reported for the writer. It never changes the verdict.
    """
    hits = _hits(text, taxonomy)
    decisions = tuple(s for s, n in hits.items() if n >= 1)
    primary = _pick_primary(hits, taxonomy)
    discard = tuple(dict.fromkeys(name for name, rx in _DISCARD_GROUPS if rx.search(text)))
    polarity = detect_polarity(text)
    if polarity == "affirmed" and _CONTRAINDICATION_RE.search(text):
        # triage.detect_polarity misses "contraindicação" (only "contraindicado").
        # A contraindication is a negative claim by definition.
        polarity = "negated"
    strong = tuple(s for s in decisions if s not in _NOT_SUFFICIENT_ALONE)

    if strong:
        verdict = "CARD"
        reason = "gatilho decisório: " + ", ".join(strong)
        if discard:
            reason += f"; também contém {', '.join(discard)} (mantido pelo gatilho decisório)"
    elif decisions:
        verdict = "MAYBE"
        reason = "só mecanismo: não muda decisão por si; revisar se a conduta depende dele"
    elif discard:
        verdict = "DISCARD"
        reason = "não-decisório: " + ", ".join(discard)
    else:
        verdict = "MAYBE"
        reason = "sem gatilho decisório por frase; decisão humana"

    if polarity == "negated" and any(s in _NEGATION_SENSITIVE for s in strong):
        reason += "; preservar negação (contraindicação/exclusão não pode virar afirmação)"
    return ContentVerdict(
        verdict=verdict,
        reason=reason,
        decisions=decisions,
        primary=primary if verdict == "CARD" else None,
        discard=discard,
        polarity=polarity,
    )


def annotate_content(
    items: Iterable[dict[str, Any]],
    taxonomy: Sequence[DecisionType] = DECISION_TAXONOMY,
) -> list[dict[str, Any]]:
    """Add content-filter keys to triage items in place and return them.

    Keys added: ``content_verdict``, ``content_reason``, ``content_primary``,
    ``content_discard``. The triage ``verdict`` is left untouched so the two
    gates stay separately auditable.
    """
    out: list[dict[str, Any]] = []
    for item in items:
        cv = classify_content(str(item.get("text", "")), taxonomy)
        item["content_verdict"] = cv.verdict
        item["content_reason"] = cv.reason
        item["content_primary"] = cv.primary
        item["content_discard"] = list(cv.discard)
        out.append(item)
    return out


# ── Applicability ─────────────────────────────────────────────────────────────


def applicable_types(
    source_text: str,
    taxonomy: Sequence[DecisionType] = DECISION_TAXONOMY,
) -> tuple[DecisionType, ...]:
    """Return the decision types the source material raises.

    A type applies when it is ``obrigatorio`` or when at least ``trigger_min``
    distinct phrases of it occur in *source_text*. Applicability is per topic,
    so a rare disease without a comorbidity section is not charged for one.
    """
    return tuple(dt for dt in taxonomy if dt.obrigatorio or dt.matches(source_text))


# ── Assignment ────────────────────────────────────────────────────────────────


def classify_candidate(
    text: str,
    taxonomy: Sequence[DecisionType] = DECISION_TAXONOMY,
) -> tuple[str, ...]:
    """Return every decision type *text* mentions (for reporting, not counting)."""
    return tuple(dt.slug for dt in taxonomy if dt.hit_count(text) >= 1)


def build_coverage(
    candidates: Sequence[Mapping[str, object]],
    taxonomy: Sequence[DecisionType] = DECISION_TAXONOMY,
) -> dict[str, list[int]]:
    """Map each decision slug to the candidates counted under it (primary only).

    A candidate counts only when the triage verdict is ``CARD`` **and** the
    content filter says ``CARD``. A MAYBE is not yet a card; a DISCARD is not
    content. Each counted candidate appears under exactly one slug.
    """
    coverage: dict[str, list[int]] = {dt.slug: [] for dt in taxonomy}
    for idx, cand in enumerate(candidates):
        if cand.get("verdict") != "CARD":
            continue
        cv = classify_content(str(cand.get("text", "")), taxonomy)
        if cv.verdict == "CARD" and cv.primary is not None:
            coverage[cv.primary].append(idx)
    return coverage


# ── Budget ────────────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class BudgetRow:
    """One decision type's budget line: coverage against its target.

    ``covered`` is None when no cards were supplied (planning mode). Coverage
    was then not assessed, and no status other than ``NAO_AVALIADO`` is claimed.
    """

    slug: str
    label: str
    covered: int | None
    target_min: int
    target_max: int
    status: str
    retained: int
    trim: int
    justification: str


@dataclass
class Budget:
    """Card budget for a topic, with the justification for every number."""

    rows: list[BudgetRow]
    total_min: int
    total_max: int
    retained: int
    orphans: int
    batch_size: int
    batches_needed: int
    batches_retained: int
    batches: list[dict[str, Any]]
    stopping: dict[str, bool]
    status: str

    @property
    def gaps(self) -> list[str]:
        return [r.slug for r in self.rows if r.status == "LACUNA"]

    @property
    def excess(self) -> dict[str, int]:
        return {r.slug: r.trim for r in self.rows if r.trim > 0}

    @property
    def stopped(self) -> bool:
        return bool(self.stopping) and all(self.stopping.values())

    def justification(self) -> str:
        """Pt-BR justification: one line per decision type, then totals and stop state."""
        if self.status == "SEM_DECISOES":
            return "Nenhum tipo de decisão aplicável ao texto-fonte: nenhum cartão planejado."
        lines = [r.justification for r in self.rows]
        lines.append(
            f"Total: {self.retained} cartões retidos; faixa {self.total_min}–{self.total_max}; "
            f"horizonte {self.batches_needed} lote(s) de até {self.batch_size}."
        )
        if self.status == "SEM_CARTOES":
            lines.append(
                "Nenhum cartão CARD cobre o tema: orçamento não fechado; nada foi inventado."
            )
        if not self.stopping:
            lines.append("Parada: não avaliada (sem cartões informados).")
        else:
            unmet = [k for k, ok in self.stopping.items() if not ok]
            lines.append(
                "Parada: fechada." if not unmet else f"Parada NÃO atingida: {', '.join(unmet)}."
            )
        return "\n".join(lines)


def _row_justification(
    label: str,
    covered: int | None,
    tmin: int,
    tmax: int,
    status: str,
    trim: int,
    waiting: int = 0,
) -> str:
    alvo = f"alvo {tmin}–{tmax}"
    if status == "NAO_AVALIADO" or covered is None:
        return f"{label}: {alvo} → cobertura não avaliada (nenhum cartão informado)."
    head = f"{label}: coberto {covered} / {alvo} → "
    if status == "LACUNA":
        return head + "LACUNA: nenhum CARD nem candidato pendente; voltar à fonte (recover_gaps)."
    if status == "PENDENTE":
        return (
            head
            + f"PENDENTE: {waiting} candidato(s) MAYBE aguardam revisão; não é lacuna da fonte."
        )
    if status == "ABAIXO_DO_MINIMO":
        return head + f"ABAIXO DO MÍNIMO: faltam {tmin - covered}; procurar mais material."
    if status == "EXCEDENTE":
        return head + f"EXCEDENTE: aparar {trim} (ranquear por score; excedente não é lixo)."
    return head + "OK."


def _pack(rows: Sequence[BudgetRow], batch_size: int) -> list[dict[str, Any]]:
    """Split retained cards into batches of at most *batch_size*, in type order."""
    batches: list[dict[str, Any]] = []
    current: dict[str, Any] | None = None
    for row in rows:
        remaining = row.retained
        while remaining > 0:
            if current is None or current["total"] >= batch_size:
                current = {"id": f"B{len(batches) + 1:02d}", "total": 0, "types": {}}
                batches.append(current)
            take = min(batch_size - current["total"], remaining)
            current["types"][row.slug] = current["types"].get(row.slug, 0) + take
            current["total"] += take
            remaining -= take
    return batches


def budget_from_counts(
    applicable: Sequence[DecisionType],
    counts: Mapping[str, int] | None = None,
    orphans: int = 0,
    batch_size: int = MAX_BATCH,
    pending: Mapping[str, int] | None = None,
) -> Budget:
    """Build the budget from applicable types and the CARD count per type.

    Args:
        applicable: Decision types the topic raises.
        counts: Slug -> cards counted under it (primary attribution). None when
            no cards were supplied; coverage is then ``NAO_AVALIADO``.
        orphans: CARD candidates that serve no applicable type.
        batch_size: Cards per batch (default 20, the hard ceiling).
        pending: Slug -> MAYBE candidates whose primary is that type. A type
            with no CARD but pending candidates is ``PENDENTE`` (human review),
            not ``LACUNA``: the source has material; the filter has not decided.

    Status per type: ``LACUNA`` (0 cards, nothing pending), ``PENDENTE``,
    ``ABAIXO_DO_MINIMO`` (below target_min), ``EXCEDENTE`` (above target_max:
    trim), or ``OK``. The topic is *closed* (``FECHADO``) only when no type is a
    gap or pending, there are no orphans, and the retained total lies within
    ``[total_min, total_max]``. With no CARD, ``retained`` is 0 and the status is
    ``SEM_CARTOES``: nothing is invented.
    """
    total_min = sum(dt.target_min for dt in applicable)
    total_max = sum(dt.target_max for dt in applicable)
    rows: list[BudgetRow] = []
    for dt in applicable:
        if counts is None:
            rows.append(
                BudgetRow(
                    slug=dt.slug,
                    label=dt.label,
                    covered=None,
                    target_min=dt.target_min,
                    target_max=dt.target_max,
                    status="NAO_AVALIADO",
                    retained=0,
                    trim=0,
                    justification=_row_justification(
                        dt.label, None, dt.target_min, dt.target_max, "NAO_AVALIADO", 0
                    ),
                )
            )
            continue
        covered = int(counts.get(dt.slug, 0))
        waiting = int((pending or {}).get(dt.slug, 0))
        if covered == 0:
            status = "PENDENTE" if waiting else "LACUNA"
        elif covered < dt.target_min:
            status = "ABAIXO_DO_MINIMO"
        elif covered > dt.target_max:
            status = "EXCEDENTE"
        else:
            status = "OK"
        trim = max(0, covered - dt.target_max)
        rows.append(
            BudgetRow(
                slug=dt.slug,
                label=dt.label,
                covered=covered,
                target_min=dt.target_min,
                target_max=dt.target_max,
                status=status,
                retained=min(covered, dt.target_max),
                trim=trim,
                justification=_row_justification(
                    dt.label, covered, dt.target_min, dt.target_max, status, trim, waiting
                ),
            )
        )

    retained = sum(r.retained for r in rows)
    batches = _pack(rows, batch_size)
    batches_needed = math.ceil(total_max / batch_size) if total_max else 0

    stopping: dict[str, bool] = {}
    if not applicable:
        status = "SEM_DECISOES"
    else:
        if counts is not None:
            stopping = {
                "cobertura": not any(r.status in ("LACUNA", "PENDENTE") for r in rows),
                "exaustao": orphans == 0,
                "orcamento": total_min <= retained <= total_max,
            }
        if counts is not None and retained == 0:
            status = "SEM_CARTOES"
        elif stopping and all(stopping.values()):
            status = "FECHADO"
        else:
            status = "ABERTO"

    return Budget(
        rows=rows,
        total_min=total_min,
        total_max=total_max,
        retained=retained,
        orphans=orphans,
        batch_size=batch_size,
        batches_needed=batches_needed,
        batches_retained=len(batches),
        batches=batches,
        stopping=stopping,
        status=status,
    )


# ── Plan ──────────────────────────────────────────────────────────────────────


@dataclass
class Plan:
    """The reconciliation between clinical obligation and source support.

    Attributes:
        applicable: Slugs of the decision types this topic raises.
        target_min: Sum of ``target_min`` over applicable types.
        target_max: Sum of ``target_max`` over applicable types. Not clamped: a
            topic above one batch is split across batches (see ``budget``).
        coverage: Slug -> candidate indices counted under it (primary only).
        gaps: Applicable slugs with no counted candidate.
        surplus: Slug -> candidate indices beyond ``target_max``.
        orphans: CARD candidates that serve no applicable type.
        discarded: Triage-CARD candidates the content filter rejected, with reasons.
        kept: Candidate indices the plan retains.
        stopping: The three stopping conditions and whether all hold.
        budget: The justified budget behind the numbers above.
    """

    applicable: tuple[str, ...]
    target_min: int
    target_max: int
    coverage: dict[str, list[int]]
    gaps: list[str] = field(default_factory=list)
    surplus: dict[str, list[int]] = field(default_factory=dict)
    orphans: list[int] = field(default_factory=list)
    discarded: list[dict[str, str]] = field(default_factory=list)
    kept: list[int] = field(default_factory=list)
    stopping: dict[str, bool] = field(default_factory=dict)
    budget: Budget | None = None

    @property
    def stopped(self) -> bool:
        """True when every stopping condition holds."""
        return bool(self.stopping) and all(self.stopping.values())

    @property
    def card_count(self) -> int:
        """Number of cards the plan retains."""
        return len(self.kept)

    def summary(self) -> str:
        """Pt-BR justification: per-type ``coberto X / alvo Y`` and the stop state."""
        head = (
            f"Tipos de decisão aplicáveis: {len(self.applicable)} de {len(DECISION_TAXONOMY)}; "
            f"lote máximo: {MAX_BATCH}."
        )
        body = self.budget.justification() if self.budget else ""
        extra: list[str] = []
        if self.discarded:
            extra.append(
                f"Filtro de conteúdo descartou {len(self.discarded)} candidato(s) com CARD do triage."
            )
        return "\n".join([head, body, *extra])


def evaluate_plan(
    source_text: str,
    candidates: Sequence[Mapping[str, object]],
    taxonomy: Sequence[DecisionType] = DECISION_TAXONOMY,
    max_cards: int = MAX_BATCH,
) -> Plan:
    """Reconcile *candidates* against the decisions *source_text* raises.

    ``max_cards`` is the batch size, not a cap on the topic: a topic whose
    obligations exceed one batch is split into more batches (see ``budget``).
    """
    applicable = applicable_types(source_text, taxonomy)
    applicable_slugs = {dt.slug for dt in applicable}
    coverage = build_coverage(candidates, taxonomy)

    discarded: list[dict[str, str]] = []
    orphans: list[int] = []
    for idx, cand in enumerate(candidates):
        if cand.get("verdict") != "CARD":
            continue
        cv = classify_content(str(cand.get("text", "")), taxonomy)
        if cv.verdict != "CARD":
            discarded.append({"index": str(idx), "verdict": cv.verdict, "reason": cv.reason})
        elif cv.primary not in applicable_slugs:
            orphans.append(idx)

    counts = {dt.slug: len(coverage[dt.slug]) for dt in applicable}
    pending: dict[str, int] = {}
    for cand in candidates:
        if cand.get("verdict") != "CARD":
            continue
        text = str(cand.get("text", ""))
        cv = classify_content(text, taxonomy)
        if cv.verdict == "MAYBE":
            owner = assign_primary(text, taxonomy)
            if owner in applicable_slugs:
                pending[owner] = pending.get(owner, 0) + 1
    budget = budget_from_counts(
        applicable, counts, orphans=len(orphans), batch_size=max_cards, pending=pending
    )

    surplus: dict[str, list[int]] = {}
    kept: list[int] = []
    for dt in applicable:
        covered = coverage[dt.slug]
        if len(covered) > dt.target_max:
            surplus[dt.slug] = covered[dt.target_max :]
        kept.extend(covered[: dt.target_max])

    return Plan(
        applicable=tuple(dt.slug for dt in applicable),
        target_min=budget.total_min,
        target_max=budget.total_max,
        coverage=coverage,
        gaps=budget.gaps,
        surplus=surplus,
        orphans=orphans,
        discarded=discarded,
        kept=sorted(kept),
        stopping=budget.stopping,
        budget=budget,
    )


# ── Gap recovery ──────────────────────────────────────────────────────────────

_BULLET = re.compile(r"^[\s\u2022\u25cf\u25e6\u25aa\u00b7\-\*\u2013\u2014\u25a0\u2b1c]+")
_WS = re.compile(r"\s+")


def _paragraphs(source_text: str, min_chars: int = 60) -> list[str]:
    """Split *source_text* into bullet-stripped, whitespace-collapsed blocks.

    Sentence extraction in ``triage.py`` drops material it cannot parse as
    prose (table rows, heading runs, bulleted criteria). Gap recovery reads at
    paragraph granularity instead and strips list markers rather than
    rejecting them.
    """
    out: list[str] = []
    for block in re.split(r"\n\s*\n", source_text):
        cleaned = _WS.sub(" ", _BULLET.sub("", block.strip())).strip()
        if len(cleaned) >= min_chars:
            out.append(cleaned)
    return out


def recover_gaps(
    source_text: str,
    gaps: Iterable[str],
    taxonomy: Sequence[DecisionType] = DECISION_TAXONOMY,
    already: Iterable[str] = (),
) -> dict[str, list[str]]:
    """Return source paragraphs that could close each gap in *gaps*.

    A gap has two possible causes with opposite fixes: the source does not
    discuss the decision (nothing to do), or the extractor missed it. This
    re-reads the raw text at paragraph level and returns the paragraphs whose
    phrases raise the gap's type, excluding text already carded.
    """
    by_slug = {dt.slug: dt for dt in taxonomy}
    seen = {_WS.sub(" ", t).strip().lower() for t in already}
    recovered: dict[str, list[str]] = {}
    for slug in gaps:
        dt = by_slug.get(slug)
        if dt is None:
            continue
        recovered[slug] = [
            para
            for para in _paragraphs(source_text)
            if dt.matches(para) and para.lower() not in seen
        ]
    return recovered


# ── Public budget entry point (used by the MCP tool) ─────────────────────────


def plan_topic_budget(
    source_text: str,
    taxonomy: Sequence[DecisionType] = DECISION_TAXONOMY,
    batch_size: int = MAX_BATCH,
    card_texts: Sequence[str] | None = None,
) -> dict[str, Any]:
    """Top-down card budget for a topic, with the justification for each number.

    Without *card_texts* the range and batch horizon are planned, but coverage
    is not assessed (each type is ``NAO_AVALIADO``). With *card_texts* (drafted
    cards, one per entry) each card is attributed to its primary type and the
    per-type ``coberto X / alvo Y`` report is produced. Planning without cards
    never claims a card count.
    """
    applicable = applicable_types(source_text, taxonomy)
    counts: dict[str, int] | None = None
    orphans = 0
    if card_texts is not None:
        counts = {dt.slug: 0 for dt in applicable}
        for text in card_texts:
            primary = assign_primary(text, taxonomy)
            if primary in counts:
                counts[primary] += 1
            else:
                orphans += 1
    budget = budget_from_counts(applicable, counts, orphans=orphans, batch_size=batch_size)
    horizon = max(1, budget.batches_needed)
    batches_planned = [f"B{i + 1:02d}" for i in range(horizon)]

    return {
        "applicable_decisions": [dt.slug for dt in applicable],
        "decision_count": len(applicable),
        "total_min": budget.total_min,
        "total_max": budget.total_max,
        "total_planned": budget.total_max,
        "retained": budget.retained,
        "batch_size": batch_size,
        "batches_needed": horizon,
        "batches_planned": batches_planned,
        "batches_retained": budget.batches_retained,
        "batches": budget.batches,
        "breakdown": {dt.slug: dt.target_max for dt in applicable},
        "rows": [
            {
                "decision": r.slug,
                "covered": r.covered,
                "target": f"{r.target_min}–{r.target_max}",
                "status": r.status,
                "justification": r.justification,
            }
            for r in budget.rows
        ],
        "gaps": budget.gaps,
        "excess": budget.excess,
        "orphans": orphans,
        "budget_status": budget.status,
        "stopping": budget.stopping,
        "justification": budget.justification(),
        "summary": (
            f"Tema levanta {len(applicable)} decisões clínicas ativas. "
            f"Orçamento: {budget.total_max} cartões planejados (faixa {budget.total_min}–{budget.total_max}), "
            f"horizonte de {horizon} lote(s) de até {batch_size}."
        ),
    }
