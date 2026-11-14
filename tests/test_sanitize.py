"""tests/test_sanitize.py — Comprehensive tests for core/sanitize.py."""

from anki_mcp.checks.csv_structure import check_csv
from anki_mcp.core.sanitize import auto_fix_csv_block, sanitize_field


def test_sanitize_inequalities():
    raw = "Idade < 50 anos, creatinina > 2.0 mg/dL, clearance <= 30 ml/min e PAS >= 140 mmHg (p < 0.05)."
    clean, fixes = sanitize_field(raw)
    assert "<" not in clean
    assert ">" not in clean
    assert "&lt; 50" in clean
    assert "&gt; 2.0" in clean
    assert "&le; 30" in clean
    assert "&ge; 140" in clean
    assert "p &lt; 0.05" in clean
    assert "escaped_inequality_lt" in fixes
    assert "escaped_inequality_gt" in fixes
    assert "escaped_inequality_le" in fixes
    assert "escaped_inequality_ge" in fixes


def test_sanitize_preserves_allowed_html():
    raw = '<b>Achado clínico:</b> <span class="destaque-clinico">Disfagia</span> grave.<br>Ver figura.'
    clean, fixes = sanitize_field(raw)  # noqa: RUF059
    assert "<b>Achado clínico:</b>" in clean
    assert '<span class="destaque-clinico">Disfagia</span>' in clean
    assert "<br>" in clean
    assert "&lt;b&gt;" not in clean  # Must NOT escape allowed tags!


def test_sanitize_bare_amp_no_double_escape():
    raw = "Sinal A & Sintoma B &amp; Critério C &#124; D"
    clean, fixes = sanitize_field(raw)  # noqa: RUF059
    assert "Sinal A &amp; Sintoma B" in clean
    assert "&amp;amp;" not in clean
    assert clean.count("&amp;") == 2
    assert "&#124;" in clean


def test_sanitize_markdown_conversion():
    raw = "**Diagnóstico provável:** *Miastenia Gravis* com anti-AChR positivo."
    clean, fixes = sanitize_field(raw)  # noqa: RUF059
    assert "<b>Diagnóstico provável:</b>" in clean
    assert "<i>Miastenia Gravis</i>" in clean


def test_sanitize_unclosed_tag_autofix():
    raw = "<b>Diagnóstico sem fechar tag"
    clean, fixes = sanitize_field(raw)
    assert clean == "<b>Diagnóstico sem fechar tag</b>"
    assert "auto_closed_tag_b" in fixes


def test_auto_fix_csv_block_qa():
    raw_csv = (
        "Qual o ponto de corte?|PA > 140/90 mmHg|Medicina::Cardio|Notas & detalhes com < 10 mg|img.jpg|Ref 1|Tag1\n"
        "Qual outro corte?|Glicemia >= 126 mg/dL|Medicina::Endo|Detalhe com | pipe acidental|img2.jpg|Ref 2|Tag2\n"
    )
    fixed_csv, fixes = auto_fix_csv_block(raw_csv, "QA")  # noqa: RUF059
    # Validate with check_csv to prove ZERO critical diagnostics
    diags = check_csv(fixed_csv, "QA")
    criticals = [d for d in diags if d.severity.value == "CRITICAL"]
    assert not criticals, f"Found critical diagnostics: {criticals}"
    assert (
        "Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas"
        in fixed_csv
    )
    assert "&gt; 140/90" in fixed_csv
    assert "&lt; 10" in fixed_csv
    assert "&amp;" in fixed_csv


def test_auto_fix_csv_block_cloze():
    raw_csv = "No paciente com {{c1::idade > 60 anos}}, a conduta é imediata.|Notas & obs com < 5%|Medicina::Neuro||Ref|Tag\n"
    fixed_csv, fixes = auto_fix_csv_block(raw_csv, "CLOZE")  # noqa: RUF059
    diags = check_csv(fixed_csv, "CL")
    criticals = [d for d in diags if d.severity.value == "CRITICAL"]
    assert not criticals, f"Found critical diagnostics: {criticals}"
    assert "{{c1::idade &gt; 60 anos}}" in fixed_csv
