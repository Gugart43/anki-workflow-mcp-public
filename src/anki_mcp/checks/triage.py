"""checks/triage.py — deterministic content-triage scoring engine.

Given source material (textbook excerpt, article, pasted text), this module
extracts candidate statements and scores each across 5 clinical-relevance
dimensions, assigning one of three verdicts:

  CARD     composite >= CARD_THRESHOLD   -- worth carding immediately
  MAYBE    composite >= MAYBE_THRESHOLD  -- review before carding
  DISCARD  composite <  MAYBE_THRESHOLD  -- skip

Five dimensions, each in [0.0, 1.0], combined by :data:`DIMENSION_WEIGHTS`
into a composite weighted mean:

  atomicity        can the statement become ONE card?   (reuses ``_count_items``)
  factual_density  checkable specifics present          (numbers+units, doses,
                   named criteria/scales, anatomical/clinical terms)
  stability        durability of the fact               (mechanisms/definitions
                   score high; epidemiology, years, "currently" score low)
  yield            clinical/exam relevance              (keyword signal only)
  non_triviality   not a tautology / filler / meta note

No I/O, no network. All functions are pure over strings.
"""

from __future__ import annotations

import re
import unicodedata

from anki_mcp.checks.qa import _count_items

# ── Tunable constants ─────────────────────────────────────────────────────────

DIMENSION_WEIGHTS: dict[str, float] = {
    "atomicity": 0.20,
    "factual_density": 0.30,
    "stability": 0.15,
    "yield": 0.25,
    "non_triviality": 0.10,
}
"""Weights of the five dimensions in the composite score (sum = 1.0)."""

CARD_THRESHOLD = 0.65
"""Heuristic CARD threshold, not a clinically validated optimum.

Must exceed REVIEW_ONLY_COMPOSITE_CAP so ambiguous or case-bound text remains
review-only. Scores describe lexical patterns, not medical validity.
"""

MAYBE_THRESHOLD = 0.38
"""Heuristic review threshold; lower scores are labeled DISCARD.

This label is advisory, not proof that source material lacks educational value.
"""

ATOMICITY_CARD_FLOOR = 0.5
"""A statement below this atomicity can never reach "CARD"."""

REVIEW_ONLY_COMPOSITE_CAP = 0.55
"""Ceiling applied to statements that must never be auto-carded (-> "MAYBE").

Two classes qualify. (1) Non-atomic statements: without a cap,
``factual_density`` (0.30) + ``yield`` (0.25) can outvote a collapsed
``atomicity`` — a 230-word pasted paragraph carrying doses scored 0.79 "CARD".
(2) Case-bound statements ("Paciente B já preenche o critério..."), which are
true only of one presented vignette and cannot back a general reference card.
"""

MIN_CANDIDATE_CHARS = 25
"""Candidates shorter than this are discarded as too terse."""

_ATOMICITY_MAX_WORDS = 45
"""Statements beyond this word count receive an additional atomicity penalty."""

_ATOMICITY_BY_ITEMS: dict[int, float] = {1: 1.0, 2: 0.7, 3: 0.5, 4: 0.3}

_FUSED_ITEM_RE = re.compile(
    r"\.\s*-\s|\s[-\u2013\u2014]\s+(?=\S)|\s\|\s|[\u2502\u25bc\u2514\u250c\u2534\u251c\u2524]"
)
"""Separators that glue two list items / diagram nodes into one sentence.

``.-`` is the artifact a PDF-to-markdown export leaves between consecutive
bullets; ``│ ▼ └ ┌`` are the box-drawing glyphs of ASCII decision trees.
Neither is seen by :func:`anki_mcp.checks.qa._count_items`, so a fused pair of
facts used to pass as a single atomic statement.
"""


def _has_fused_items(text: str) -> bool:
    """True when *text* visibly glues two statements together."""
    return bool(_FUSED_ITEM_RE.search(text))


_CASE_DEIXIS_RE = re.compile(
    r"\bPaciente\s+[A-Z]\b|\beste\s+primeiro\b|\bo\s+paciente\s+acima\b"
    r"|\bno\s+caso\s+do\s+paciente\b"
)
"""References to one named/presented vignette, not to medicine in general.

Matched against the raw text (capital `Paciente A`): the lowercase "orientar o
paciente e a família" of an ordinary precept must not trip it.
"""


def _is_case_bound(text: str) -> bool:
    """True when the statement only holds for a specific presented case."""
    return bool(_CASE_DEIXIS_RE.search(text))


_TRAILING_CONNECTOR_RE = re.compile(
    r"(?:[;:,]|\bou|\be|\bde|\bcom|\bem|\bpara|\bque|\bsem|\bda|\bdo)\s*[.;]?$"
)
"""A statement cut by the semicolon/period split leaves a dangling connector."""


def _is_fragment(text: str) -> bool:
    """True for text nothing can be built from: cut mid-list or unclosed.

    The extractor splits on ";" — which is also the separator inside a criteria
    list — so "Repetir RM em 6-12 meses ou (Lesões Gd+ e Gd- juntas;" survives as
    a candidate and used to score a full 1.0 CARD.
    """
    stripped = text.strip()
    if stripped.count("(") != stripped.count(")"):
        return True
    return bool(_TRAILING_CONNECTOR_RE.search(stripped))


STOPWORDS: frozenset[str] = frozenset(
    """
    para como mais esse essa este esta isso aquilo pelo pela pelos pelas
    porque quando muito menos todas todos entre sobre depois antes ainda
    tambem apenas pode podem deve devem temos tiver sera serao numa num
    this that with from they there their these those which where when have
    been were will would should could about into over than then such also
    more most some only same very each both other others being does done
    """.split()
)
"""Accent-stripped lowercase words carrying no topical signal."""


def _normalize(text: str) -> str:
    """Lowercase, strip accents and collapse whitespace (for keyword matching)."""
    decomposed = unicodedata.normalize("NFD", text.lower())
    stripped = "".join(c for c in decomposed if unicodedata.category(c) != "Mn")
    return re.sub(r"\s+", " ", stripped).strip()


def _clamp(value: float) -> float:
    """Clamp *value* into [0.0, 1.0] and round to 4 decimals."""
    return round(max(0.0, min(1.0, value)), 4)


def _dedupe_found(matches: list[str]) -> list[str]:
    """Sorted unique lowercased matches — keeps ``reasons`` deterministic."""
    return sorted({m.strip().lower() for m in matches if m.strip()})


# ── Signal patterns ───────────────────────────────────────────────────────────

_HEADING_RE = re.compile(r"^\s{0,3}#{1,6}\s+(?P<title>.*\S)\s*#*\s*$")
_SECTION_NUM_RE = re.compile(
    r"^\s{0,3}(?P<num>\d{1,2})(?P<esc>\\+\s*)?[.)]\s+(?P<title>[^.!?;\n]{3,80})$"
)
_BULLET_RE = re.compile(r"^\s*[-*+\u2022\u00b7\u25cf\u25aa]\s+(?P<body>.*)$")
_NUMBERED_RE = re.compile(r"^\s{0,3}\d{1,3}[.)]\s+(?P<body>.*)$")
# Sentence boundaries: period/semicolon/question mark + space, newline, and the
# PDF-export artifact ".-" that glues consecutive bullets onto one line.
_SENTENCE_SPLIT_RE = re.compile(r"(?<=\.)\s+|(?<=;)\s+|(?<=\?)\s+|\.\s*-\s+|\n")
_MD_LINK_RE = re.compile(r"\[([^\]]+)\]\([^)]+\)")
_INLINE_MARKUP_RE = re.compile(r"[*_`~]{1,2}")
_SOURCE_CITE_RE = re.compile(r"\\*\[\s*Source:[^\]]*?\\*\]", re.IGNORECASE)
_MD_ESCAPE_RE = re.compile(r"\\([\[\]().*_`#|>-])")

# Factual density ─────────────────────────────────────────────────────────────
_NUMBER_UNIT_RE = re.compile(
    r"\b\d+(?:[.,]\d+)?\s*"
    r"(?:mg(?:/kg)?|mcg|g|kg|ml|mmol|meq|ui|kcal|cm|mm|hz|bpm|l|dl|ng|mhz|mv|s|min"
    r"|h|horas?|dias?|semanas?|mes(?:es)?|anos?|minutos?)\b",
    re.IGNORECASE,
)
# Leading endpoint of a numeric range ("85-95%", "3 a 5 dias"): the hyphen/`a`
# connector hides the first value from _NUMBER_UNIT_RE/_PERCENT_RE.
_RANGE_HEAD_RE = re.compile(
    r"\b(\d+(?:[.,]\d+)?)\s*(?:-|\u2013|\u2014|a|ate)\s*\d+(?:[.,]\d+)?\s*"
    r"(?:%|(?:mg(?:/kg)?|mcg|g|kg|ml|mmol|meq|ui|kcal|cm|mm|hz|bpm|l|dl|ng|mhz|mv|s|min"
    r"|h|horas?|dias?|semanas?|mes(?:es)?|anos?|minutos?)\b)"
)
_PERCENT_RE = re.compile(r"\b\d+(?:[.,]\d+)?\s*%")
# Counts spelled out in words ("quatro grandes classes", "tres criterios").
# The digit-based lexicons above are blind to them, so a definition that
# enumerates its elements scored as if it carried no countable specific —
# which is exactly the "classificacao em N classes" shape of a high-yield card.
_SPELLED_COUNT_RE = re.compile(
    r"\b(?:dois|duas|tres|quatro|cinco|seis|sete|oito|nove|dez)\s+"
    r"(?:grandes\s+|principais\s+|tipos\s+de\s+)?"
    r"(?:classes?|grupos?|tipos?|categorias?|criterios?|fases?|estagios?|graus?"
    r"|niveis?|etapas?|passos?|causas?|opcoes?|pilares?|eixos?|sinais"
    r"|sintomas|fatores|mecanismos?|formas?|padroes?)"
)
_DRUG_RE = re.compile(
    r"\b(?:fenitoina|fenitoína|levetiracetam|midazolam|lorazepam|diazepam"
    r"|fenobarbital|valproato|valproico|carbamazepina|oxcarbazepina|lamotrigina"
    r"|topiramato|gabapentina|pregabalina|clonazepam|nitrazepam|vigabatrina"
    r"|lacosamida|perampanel|brivaracetam|clobazam|etossuximida|primidona"
    r"|atropina|adenosina|amiodarona|lidocaina|procainamida|sotalol|verapamil"
    r"|diltiazem|digoxina|warfarina|heparina|enoxaparina|aspirina|clopidogrel"
    r"|ticagrelor|prasugrel|rivaroxabana|apixabana|dabigatrana|rtpa|alteplase"
    r"|metformina|insulina|glibenclamida|sitagliptina|empagliflozina|liraglutida"
    r"|metoprolol|atenolol|propranolol|captopril|enalapril|lisinopril|losartana"
    r"|anlodipino|nifedipino|furosemida|hidroclorotiazida|espironolactona"
    r"|atorvastatina|sinvastatina|rosuvastatina|omeprazol|pantoprazol|ranitidina"
    r"|ciprofloxacino|amoxicilina|azitromicina|metronidazol|vancomicina"
    r"|metilprednisolona|metilprednisona|prednisona|prednisolona|dexametasona"
    r"|plasmaferese|imunoglobulina|gamaglobulina|natalizumabe|ocrelizumabe"
    r"|rituximabe|ofatumumabe|ublituximabe|alemtuzumabe|fingolimode|siponimode"
    r"|ozanimode|teriflunomida|dimetilfumarato|cladribina|azatioprina"
    r"|micofenolato|metotrexato|ciclofosfamida|glatirâmer|glatiramer"
    r"|interferon|betainterferona|baclofeno|tizanidina|dantroleno|dalfampridina"
    r"|riluzol|edaravona|dextrometorfano|nusinersena|risdiplam|donepezila"
    r"|memantina|rivastigmina|galantamina|levodopa|carbidopa|pramipexol"
    r"|ropinirol|amantadina|biperideno|triexifenidil|sumatriptana|topiramato)\b",
    re.IGNORECASE,
)
_NAMED_CRITERIA_RE = re.compile(
    r"\b(?:criterios?\s+de\s+\w+|escala\s+de\s+\w+|score\s+de\s+\w+"
    r"|criterios?\s+\w+|glasgow|wells|light|cha2ds2\s*vasc|has\s*bled"
    r"|sofa|apache|rankin|nihss|saint\s*george|mmse|moca|minimental"
    r"|framingham|duke|jones|centor|fine|psi|curb\s*65|alvarado"
    r"|child\s*pugh|meld|bishop|apgar|brassey|mcnichol)\b",
    re.IGNORECASE,
)
_ANATOMY_RE = re.compile(
    r"\b(?:cortex|cortic|cerebell|hipocampo|amigdala|talamo|hipotalamo"
    r"|ganglio\s*basal|neoestri|putamen|caudado|globo\s*palido|substantia\s*nigra"
    r"|nucleo|neuronio|receptor|sinapse|axonio|dendrito|mielina|gaba|glutamato"
    r"|dopamina|serotonina|noradrenalina|acetilcolina|opioid|canal\s*de"
    r"|membrana|potencial\s*de\s*acao|potencial\s*de\s*repouso"
    r"|ventricul|aorta|arteria|veia|capilar|coracaoo|miocardio|pericardio"
    r"|pleura|alveolo|bronquio|traqueia|esofago|estomago|duodeno|pancrea"
    r"|figado|baco|rim|nefron|glomeru|tubul|bexiga|utero|ovario|testicul"
    r"|tireoide|suprarrenal|hipofise|epifise|timo|linfonodo|bazo"
    r"|liquor|lcr|bandas?\s+oligoclonais?|oligoclonal"
    r"|barreira\s+hematoencefalica|barreira\s+hematoencefálica|bhe"
    r"|cadeia\s+leve|neurofilamento|nfl|potencial\s+evocado"
    r"|eletroencefalograma|eletrencefalograma|eeg|ressonancia|tomografia"
    r"|desmieliniz\w*|mieliniz\w*|neurodegenera\w*|inflamassa?o|inflamacao)\b",
    re.IGNORECASE,
)

# Stability ───────────────────────────────────────────────────────────────────
_STAT_MARKER_RE = re.compile(
    r"\b(?:prevalenc\w*|incidenc\w*|epidemiolog\w*|mortalidade|letalidade|morbidade"
    r"|seroprevalenc\w*|taxa\s+de|estudo\w*\s+recent\w*|recent\w*\s+estudo\w*"
    r"|atualmente|actualmente|currently|nos\s+dias\s+de\s+hoje|hoje\s+em\s+dia"
    r"|por\s+enquanto|ate\s+o\s+momento|ate\s+hoje|em\s+vigor|versao|edicao"
    r"|recem[\s-]?aprovad\w*|newly\s+approved|ultim\w*\s+anos|desde\s+\d{4})\b"
)
_YEAR_RE = re.compile(r"\b(?:19|20)\d{2}\b(?!\s*(?:mg|mcg|g|kg|ml|l|ui|kcal|%|kg))")
_STABLE_MARKER_RE = re.compile(
    r"\b(?:mecanismo|fisiopatolog\w*|patogen\w*|definic\w*|define-se|definida\s+como"
    r"|caracteriza-se|caracterizada\s+por|criteri\w*|anatom\w*|inerva\w*|irriga\w*"
    r"|drena\w*|origina-se|funcao\s+d\w*|estrutura|classifica-se|fisiolog\w*"
    r"|embriolog\w*|histolog\w*|constituid\w*|formad\w*\s+por)\b"
)

# Yield ───────────────────────────────────────────────────────────────────────
_HIGH_YIELD_RE = re.compile(
    r"\b(?:diagno[st]\w*|tratamento|terapia|terapeutic\w*|criteri\w*|conduta"
    r"|diferencial|indicac\w*|contraindicac\w*|mecanismo|fisiopatolog\w*"
    r"|patogen\w*|sinal\b|sindrome|sintoma|profilax\w*|rastreamento|screening"
    r"|emergencia|urgencia|prognos\w*|complicac\w*|efeito\s+adverso"
    r"|interac\w*\s+medicamentosa|dose\b|posologia"
    r"|classifica\w*|classes?|categor\w*|tipos?|subtipos?|estadiamento|estagios?"
    r"|taxonom\w*|graduac\w*|etiolog\w*|manifestac\w*|apresentac\w*)\b",
    re.IGNORECASE,
)
_LOW_YIELD_RE = re.compile(
    r"\b(?:histor\w*|curiosidade|contexto\s+historic\w*|prefacio|agradecimento"
    r"|sumario|referencia\s+bibliograf\w*|apresentacao\s+do\s+livro"
    r"|este\s+livro|esta\s+obra|o\s+autor|os\s+autores)\b",
    re.IGNORECASE,
)

# Non-triviality ──────────────────────────────────────────────────────────────
_FILLER_RE = re.compile(
    r"\b(?:e\s+importante|e\s+fundamental|e\s+essencial|deve-se\s+sempre"
    r"|sempre\s+deve|de\s+maneira\s+geral|de\s+modo\s+geral|pode-se\s+afirmar"
    r"|observa-se\s+que|nota-se\s+que|em\s+geral|normalmente|geralmente"
    r"|e\s+necessario|costuma\s+ser|na\s+maioria\s+dos\s+casos"
    r"|vale\s+destacar|e\s+valido|deve-se\s+lembrar)\b"
)
_META_RE = re.compile(
    r"\b(?:(?:este|esse|aquele|neste|nesse|naquele|deste|desse|daquele|esta|essa"
    r"|aquela|nesta|nessa|naquela|desta|dessa|daquela)s?\s+"
    r"(?:capitulo|topico|secao|texto|material|livro|artigo|unidade|modulo|documento))"
    r"|\b(?:vamos\s+ver|veremos|a\s+seguir|como\s+foi\s+dito|conforme\s+visto"
    r"|como\s+visto|figura|tabela|grafico|observe\s+a|veja\s+a|no\s+decorrer\s+deste"
    r"|nos\s+proximos|o\s+leitor|podemos\s+concluir|em\s+resumo|resumindo)\b"
)
"""Table/figure talk and cross-references to the surrounding document."""

_CHATTER_RE = re.compile(
    r"\b(?:posso|podemos|quer\s+que\s+eu|precisa\s+que\s+eu)\s+"
    r"(?:formular|montar|criar|elaborar|preparar|trazer|ajudar|gerar|simular|detalhar)\b"
    r"|\b(?:caso\s+clinico\s+simulado|seu\s+plantao|sua\s+pratica|como\s+posso\s+ajudar"
    r"|se\s+preferir|me\s+diga\s+se|deseja\s+que|espero\s+ter\s+ajudado|bons\s+estudos)\b"
)
"""Assistant talk addressed to the user, not content ("Posso formular um caso simulado").

It carries clinical vocabulary ("critérios", "conduta", "terapêutica") and so used
to score ``yield`` 1.0: pure chatter ranked above real facts.
"""
_SHORT_DEFINITION_RE = re.compile(r"^\s*(?:\S+\s+){1,3}(?:e|sao|significa|consiste\s+em)\s+")
_TAUTOLOGY_RE = re.compile(
    r"\b(\w{4,})\s+(?:e\s+um\s+tipo\s+de|e\s+uma\s+forma\s+de|e\s+um\s+caso\s+de)\s+\1\b",
    re.IGNORECASE,
)
_WORDS_RE = re.compile(r"\b\w{5,}\b")


# ── Candidate extraction ──────────────────────────────────────────────────────


def _clean(chunk: str) -> str:
    """Strip markdown noise and collapse whitespace in a candidate statement.

    Removes PDF-export source citations (``[Source: 7: x.pdf]``) and the
    backslash escaping that PDF-to-markdown exporters emit, so the citation
    debris never reaches the card text or the scoring lexicons.
    """
    text = _SOURCE_CITE_RE.sub(" ", chunk)
    text = _MD_LINK_RE.sub(r"\1", text)
    text = _MD_ESCAPE_RE.sub(r"\1", text)
    text = _INLINE_MARKUP_RE.sub("", text)
    text = re.sub(r"^\s*\|+\s*", "", text)
    text = re.sub(r"\s*\|+\s*$", "", text)
    text = re.sub(r"\s*\|+\s*", "; ", text)
    text = re.sub(r"\s+", " ", text)
    return text.strip(" \t-\u2013\u2014\u2022*#").strip()


def _split_sentences(chunk: str) -> list[str]:
    """Split a line/paragraph into sentences (period, semicolon, newline)."""
    return [part.strip() for part in _SENTENCE_SPLIT_RE.split(chunk) if part.strip()]


def extract_candidates(text: str) -> list[dict]:
    """Split *text* into candidate statements worth considering as flashcards.

    Recognises markdown headings (structural only: they label the section),
    dash/asterisk/bullet/round-bullet lists, ``1.`` / ``1)`` numbered lists and
    plain paragraphs. Paragraphs and list items are further split into
    sentences on period+space, semicolon+space and newlines.

    Statements shorter than :data:`MIN_CANDIDATE_CHARS` are dropped, as are
    heading-only lines and lines without any alphanumeric content.

    Returns a list of dicts, each with keys:
      ``text``         the cleaned statement
      ``line``         1-based line number where the statement starts
      ``source_label`` nearest preceding markdown heading ("" when none)
    """
    candidates: list[dict] = []
    section = ""
    parts: list[str] = []
    start_line = 1

    def flush() -> None:
        nonlocal parts
        if parts:
            for sentence in _split_sentences(" ".join(parts)):
                cleaned = _clean(sentence)
                if len(cleaned) < MIN_CANDIDATE_CHARS or not re.search(r"\w", cleaned):
                    continue
                candidates.append({"text": cleaned, "line": start_line, "source_label": section})
            parts = []

    for index, raw in enumerate(text.splitlines(), start=1):
        stripped = raw.strip()
        if not stripped:
            flush()
            continue
        heading_match = _HEADING_RE.match(raw)
        if heading_match:
            flush()
            section = _clean(heading_match.group("title"))
            continue
        section_match = _SECTION_NUM_RE.match(raw)
        if section_match:
            title = section_match.group("title").strip()
            # "1\\. Nomenclatura" (escaped numbering of the PDF exports) or an
            # ALL-CAPS label is a section; "1) O mecanismo envolve..." is a list item.
            escaped_numbering = bool(section_match.group("esc"))
            all_caps = not re.search(r"[a-zà-öø-ÿ]", title)
            if escaped_numbering or all_caps:
                flush()
                section = _clean(title)
                continue
        list_match = _BULLET_RE.match(raw) or _NUMBERED_RE.match(raw)
        if list_match:
            flush()
            parts = [list_match.group("body")]
            start_line = index
            continue
        if not parts:
            start_line = index
        parts.append(stripped)

    flush()
    return candidates


# ── Dimension 1: atomicity ────────────────────────────────────────────────────


def score_atomicity(text: str) -> float:
    """Can this statement become exactly ONE flashcard? (0.0-1.0).

    Delegates item-counting to :func:`anki_mcp.checks.qa._count_items`.
    Statements that fuse two list items / diagram nodes with a separator
    ``_count_items`` cannot see (".-", " - ", " | ", box-drawing glyphs) are
    capped, as are statements longer than :data:`_ATOMICITY_MAX_WORDS` words:
    a 40-word sentence that carries several facts is not one card.
    """
    items = _count_items(text)
    if _has_fused_items(text):
        items = max(items, 2)
    base = _ATOMICITY_BY_ITEMS.get(items, 0.15 if items >= 5 else 0.3)
    if len(text.split()) > _ATOMICITY_MAX_WORDS or _has_fused_items(text):
        base = min(base, 0.35)
    return _clamp(base)


# ── Dimension 2: factual density ──────────────────────────────────────────────


def _specifics(norm_text: str) -> dict[str, list[str]]:
    """Categorised checkable specifics found in already-normalised *norm_text*."""
    return {
        "numeric": _dedupe_found(
            _NUMBER_UNIT_RE.findall(norm_text)
            + _PERCENT_RE.findall(norm_text)
            + _RANGE_HEAD_RE.findall(norm_text)
            + _SPELLED_COUNT_RE.findall(norm_text)
        ),
        "drug": _dedupe_found(_DRUG_RE.findall(norm_text)),
        "criteria": _dedupe_found(_NAMED_CRITERIA_RE.findall(norm_text)),
        "anatomy": _dedupe_found(_ANATOMY_RE.findall(norm_text)),
    }


def score_factual_density(text: str) -> float:
    """How many checkable specifics does the statement carry? (0.0-1.0).

    Specifics are: numbers with units, percentages, drug names/doses, named
    criteria/scales and anatomical/clinical terms. Numbers, drugs and criteria
    count 1.0 each; anatomy/clinical terms count 0.5 each.

    Technical vocabulary (fisiopatologia, mecanismo, receptor, etc.) adds
    +0.3 per match (capped at +0.9) when numeric specifics are scarce — clinical
    definitions often lack numbers but carry high factual density.
    """
    norm = _normalize(text)
    found = _specifics(norm)
    weighted = (
        len(found["numeric"])
        + len(found["drug"])
        + len(found["criteria"])
        + 0.5 * len(found["anatomy"])
    )
    # Bonus for stable-marker technical vocabulary when numbers are absent/low
    tech_words = len(_STABLE_MARKER_RE.findall(norm))
    if weighted < 1.5 and tech_words > 0:
        weighted += min(tech_words * 0.3, 0.9)

    if weighted >= 3:
        return 1.0
    if weighted >= 2:
        return 0.7
    if weighted >= 1:
        return 0.45
    if weighted > 0:
        return 0.2
    return 0.0


# ── Dimension 3: stability ────────────────────────────────────────────────────


def _stability_penalties(norm_text: str) -> list[str]:
    """Time-bound / statistical markers found in *norm_text*."""
    hits = list(_STAT_MARKER_RE.findall(norm_text))
    if _YEAR_RE.search(norm_text):
        hits.append("year-reference")
    return _dedupe_found(hits)


def _stability_rewards(norm_text: str) -> list[str]:
    """Durable-fact markers found in *norm_text*."""
    return _dedupe_found(_STABLE_MARKER_RE.findall(norm_text))


def score_stability(text: str) -> float:
    """How durable is this fact over time? (0.0-1.0).

    Mechanisms, definitions, anatomy and pathophysiology score high.
    Epidemiology statistics, year references and "currently" score low.
    """
    norm = _normalize(text)
    base = 0.5
    rewards = _stability_rewards(norm)
    penalties = _stability_penalties(norm)
    score = base + 0.25 * len(rewards) - 0.15 * len(penalties)
    return _clamp(score)


# ── Dimension 4: yield ────────────────────────────────────────────────────────


def score_yield(text: str) -> float:
    """Clinical / exam relevance signal. (0.0-1.0).

    Rewards diagnosis, treatment, pathophysiology, criteria and management
    keywords. Penalises historical, biographical and bibliographic content.
    """
    norm = _normalize(text)
    high = _HIGH_YIELD_RE.findall(norm)
    low = _LOW_YIELD_RE.findall(norm) + _CHATTER_RE.findall(norm)
    score = 0.4 + 0.2 * len(high) - 0.2 * len(low)
    return _clamp(score)


# ── Dimension 5: non-triviality ───────────────────────────────────────────────


def _is_tautology(norm_text: str) -> bool:
    """Return True if the statement looks like 'X is a type of X'."""
    if _TAUTOLOGY_RE.search(norm_text):
        return True
    words = [w for w in _WORDS_RE.findall(norm_text) if len(w) >= 5 and w not in STOPWORDS]
    if len(words) >= 2:
        # Simple tautology: same word appears as subject and predicate complement
        first, *rest = words
        return any(first == w for w in rest[:3])
    return False


def _is_short_definition(norm_text: str) -> bool:
    """Return True for very short definitional stubs (< 8 content words)."""
    if not _SHORT_DEFINITION_RE.match(norm_text):
        return False
    content_words = [w for w in norm_text.split() if len(w) >= 4]
    return len(content_words) < 8


def _triviality_flags(text: str, norm_text: str) -> list[str]:
    """Filler / meta-commentary / tautology flags for *text*."""
    flags = [f"filler:{m}" for m in _dedupe_found(_FILLER_RE.findall(norm_text))]
    flags += [f"meta:{m}" for m in _dedupe_found(_META_RE.findall(norm_text))]
    flags += [f"chatter:{m}" for m in _dedupe_found(_CHATTER_RE.findall(norm_text))]
    if _is_tautology(norm_text):
        flags.append("tautology")
    if _is_short_definition(norm_text):
        flags.append("short-definition")
    content_words = [w for w in text.split() if len(w) >= 3]
    if len(content_words) < 5:
        flags.append("too-short")
    return flags


def score_non_triviality(text: str) -> float:
    """Is the statement a genuine factual claim rather than filler? (0.0-1.0).

    Penalises filler phrases ("e importante"), meta-commentary ("neste
    capitulo", "figura"), tautological patterns and very short definitions.
    Each filler/meta flag subtracts 0.3; tautology subtracts 0.35;
    short-definition subtracts 0.55; too-short subtracts 0.25.
    """
    norm = _normalize(text)
    flags = _triviality_flags(text, norm)
    score = 1.0
    for flag in flags:
        if flag.startswith(("filler:", "meta:", "chatter:")):
            score -= 0.3
        elif flag == "tautology":
            score -= 0.35
        elif flag == "short-definition":
            score -= 0.55
        elif flag == "too-short":
            score -= 0.25
    return _clamp(score)


# ── Composite & verdict ───────────────────────────────────────────────────────

_DIMENSION_ORDER = ("atomicity", "factual_density", "stability", "yield", "non_triviality")


def _verdict(composite: float) -> str:
    """Map a composite score to a verdict string."""
    if composite >= CARD_THRESHOLD:
        return "CARD"
    if composite >= MAYBE_THRESHOLD:
        return "MAYBE"
    return "DISCARD"


_NEGATION_RE = re.compile(
    r"\bn[ãa]o\b|\bnunca\b|\bjamais\b|\baus[êe]ncia d|\bnegativo para\b"
    r"|\bcontraindicad|\bn[ãa]o deve|\bexceto\b|\bsalvo\b|\bincompat[íi]vel\b"
    r"|\bwithout\b|\bnever\b|\bcannot\b|\bnot\b",
    re.IGNORECASE,
)

#: "não só A, mas também B" affirms both. Treated as an exception rather than
#: folded into the negation pattern so the common case stays a single search.
_NOT_ONLY_RE = re.compile(r"n[ãa]o s[óo]|not only", re.IGNORECASE)


def detect_polarity(text: str) -> str:
    """Return ``"negated"`` or ``"affirmed"`` for the claim in *text*.

    Every other dimension scores a statement and its negation identically --
    measured: "o tratamento é feito com metilprednisolona 1 g/dia" and its
    negation both score 0.825 and both come out CARD, with no dimension
    separating them. In clinical text negation carries the decision ("não há
    indicação de trombólise", "não transforma uma crise em status"), so a
    pipeline blind to polarity can emit a card asserting the opposite of its
    source. That is the one failure mode where a wrong card is worse than no
    card, because it is memorised as a fact.

    This reports polarity and deliberately does **not** lower the score.
    Negated statements are frequently the highest-yield cards in the batch --
    a contraindication is a classic exam trap -- so demoting them would trade
    a safety bug for a recall collapse. The signal belongs to the writer,
    which must preserve the negation when drafting, and to the review gate,
    which should not let a negated claim through unreviewed.
    """
    norm = _normalize(text)
    hits = _NEGATION_RE.findall(norm)
    if not hits:
        return "affirmed"
    # A single negation inside "não só ... mas também" affirms rather than denies.
    if len(hits) == 1 and _NOT_ONLY_RE.search(norm):
        return "affirmed"
    return "negated"


def score_candidate(text: str) -> dict:
    """Score *text* across all five dimensions and return a results dict.

    Returns a dict with keys:
      atomicity, factual_density, stability, yield, non_triviality,
      composite, verdict, reasons, polarity
    """
    norm = _normalize(text)
    dims: dict[str, float] = {
        "atomicity": score_atomicity(text),
        "factual_density": score_factual_density(text),
        "stability": score_stability(text),
        "yield": score_yield(text),
        "non_triviality": score_non_triviality(text),
    }
    composite = round(sum(DIMENSION_WEIGHTS[name] * dims[name] for name in _DIMENSION_ORDER), 4)
    review_only: list[str] = []
    if dims["atomicity"] < ATOMICITY_CARD_FLOOR:
        review_only.append("non-atomic")
    if _is_case_bound(text):
        review_only.append("case-bound")
    if _is_fragment(text):
        review_only.append("fragment")
    if review_only:
        composite = round(min(composite, REVIEW_ONLY_COMPOSITE_CAP), 4)
    verdict = _verdict(composite)

    # Build human-readable reasons
    reasons: list[str] = []
    items = _count_items(text)
    if items == 1:
        reasons.append("single atomic statement")
    elif items >= 2:
        reasons.append(f"bundles {items} items (atomicity penalty)")
    found = _specifics(norm)
    spec_parts = []
    if found["numeric"]:
        spec_parts.append(f"{len(found['numeric'])} numeric")
    if found["drug"]:
        spec_parts.append(f"{len(found['drug'])} drug")
    if found["criteria"]:
        spec_parts.append(f"{len(found['criteria'])} criteria")
    if found["anatomy"]:
        spec_parts.append(f"{len(found['anatomy'])} anatomy")
    if spec_parts:
        reasons.append(f"specifics: {', '.join(spec_parts)}")
    else:
        reasons.append("no checkable specifics")
    stable_hits = _stability_rewards(norm)
    unstable_hits = _stability_penalties(norm)
    if stable_hits:
        reasons.append(f"durable-fact markers: {', '.join(stable_hits[:3])}")
    if unstable_hits:
        reasons.append(f"instability signals: {', '.join(unstable_hits[:3])}")
    high = _HIGH_YIELD_RE.findall(norm)
    if high:
        reasons.append(f"high-yield signals: {', '.join(_dedupe_found(high)[:3])}")
    flags = _triviality_flags(text, norm)
    if flags:
        reasons.append(f"triviality flags: {', '.join(flags[:4])}")
    if review_only:
        reasons.append(
            f"review-only ({', '.join(review_only)}): composite capped at "
            f"{REVIEW_ONLY_COMPOSITE_CAP:.2f} (CARD blocked)"
        )
    polarity = detect_polarity(text)
    if polarity == "negated":
        reasons.append(
            "NEGATED claim: the writer must preserve the negation; a card "
            "asserting the affirmative is worse than no card"
        )
    reasons.append(f"composite {composite:.2f} -> {verdict}")
    return {
        **dims,
        "composite": composite,
        "verdict": verdict,
        "reasons": reasons,
        "polarity": polarity,
    }


# ── Public entry point ────────────────────────────────────────────────────────


def triage_text(text: str, source_label: str = "") -> list[dict]:
    """Extract, score and sort candidates from *text*.

    Returns a list of dicts (one per extracted candidate), sorted by
    composite score descending (highest potential first). Each dict merges
    the candidate metadata with the score dict and adds a ``code`` key:

      ``TRIAGE.UNSTABLE``   if verdict is DISCARD due to instability signals
      ``TRIAGE.LOW_YIELD``  if verdict is DISCARD or MAYBE (low composite)
      ``""``                for CARD candidates

    :param text:         raw source material to triage
    :param source_label: optional label (file name, notebook title, …)
    """
    candidates = extract_candidates(text)
    results: list[dict] = []
    for cand in candidates:
        scored = score_candidate(cand["text"])
        if source_label:
            cand["source_label"] = cand["source_label"] or source_label
        verdict = scored["verdict"]
        unstable = bool(_stability_penalties(_normalize(cand["text"])))
        if verdict == "DISCARD" and unstable:
            code = "TRIAGE.UNSTABLE"
        elif verdict in ("DISCARD", "MAYBE"):
            code = "TRIAGE.LOW_YIELD"
        else:
            code = ""
        results.append({**cand, **scored, "code": code})

    results.sort(key=lambda item: (-item["composite"], item["source_label"], item["line"]))
    return results
