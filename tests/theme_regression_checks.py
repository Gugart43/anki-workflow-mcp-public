#!/usr/bin/env python3
"""
Theme Regression Checks for AnkiMBR Presentation v2 Stylesheet.

Validates that ankimbr-presentation-v2.css strictly adheres to:
1. color-scheme safety guards (Android WebView Force Dark blocker).
2. dark auto opt-in isolation (zero unguarded prefers-color-scheme).
3. !important restrictions (exclusively confined to @media print).
4. Full token symmetry (100% parity across Light, Dark, and Auto definitions).
5. Comprehensive presence of native AnkiMBR selectors (bridge compatibility).

Usage:
    python tests/theme_regression_checks.py [optional_path_to_css]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Resolve repository root dynamically regardless of current working directory
REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CSS_PATH = REPO_ROOT / "docs" / "reference-theme" / "ankimbr-presentation-v2.css"


def load_stylesheet(css_path: Path | None = None) -> tuple[Path, str]:
    """Resolve and load the theme stylesheet with absolute/repo-relative independence."""
    if css_path is None:
        if len(sys.argv) > 1 and sys.argv[1].strip():
            css_path = Path(sys.argv[1]).resolve()
        else:
            css_path = DEFAULT_CSS_PATH

    assert css_path.exists(), (
        f"Theme stylesheet not found at resolved path: {css_path}\n"
        f"Repository root resolved to: {REPO_ROOT}\n"
        "Please ensure 'docs/reference-theme/ankimbr-presentation-v2.css' exists."
    )

    content = css_path.read_text(encoding="utf-8")
    assert len(content.strip()) > 0, f"Stylesheet at {css_path} is empty."
    return css_path, content


def check_color_scheme(css: str) -> None:
    """Verify explicit color-scheme declarations and Android Force Dark guard."""
    assert "color-scheme: only light;" in css, (
        "Missing Android Force Dark guard ('color-scheme: only light;'). "
        "Without this, Android WebView may invert light-mode card surfaces uncontrollably."
    )

    assert "color-scheme: light;" in css, (
        "Missing baseline 'color-scheme: light;' in light theme declaration."
    )

    assert "color-scheme: dark;" in css, (
        "Missing 'color-scheme: dark;' in dark mode theme declaration."
    )


def check_dark_auto_opt_in(css: str) -> None:
    """Ensure @media (prefers-color-scheme: dark) is isolated to explicit [data-theme='auto']."""
    blocks = re.findall(
        r"@media\s*\(\s*prefers-color-scheme\s*:\s*dark\s*\)\s*\{([\s\S]*?\n\s*\})",
        css,
    )
    assert len(blocks) > 0, (
        "Expected at least one @media (prefers-color-scheme: dark) block in the stylesheet."
    )

    for idx, block in enumerate(blocks, start=1):
        has_auto = '[data-theme="auto"]' in block or "[data-theme='auto']" in block
        assert has_auto, (
            f"@media (prefers-color-scheme: dark) block #{idx} lacks explicit opt-in "
            "selector '[data-theme=\"auto\"]'. Unguarded media queries hijack the user's "
            "explicit Light Mode setting on Dark Mode operating systems."
        )

        # Ensure :root is never used unguarded inside prefers-color-scheme
        clean_block = re.sub(r"/\*[\s\S]*?\*/", "", block)
        assert ":root" not in clean_block, (
            f"@media (prefers-color-scheme: dark) block #{idx} targets ':root'. "
            "This dangerously overrides base tokens for all themes."
        )


def check_important_usage(css: str) -> None:
    """Ensure !important is forbidden everywhere outside @media print blocks."""
    # Strip comments to prevent false matches in annotations/documentation
    clean_css = re.sub(r"/\*[\s\S]*?\*/", "", css)

    # Split on actual @media print query block
    parts = re.split(r"@media\s+print\s*\{", clean_css)
    before_print = parts[0]
    assert "!important" not in before_print, (
        "Unexpected '!important' declaration outside '@media print'. "
        "Runtime flashcard themes must maintain natural CSS cascade precedence."
    )

    if len(parts) > 1:
        # Check that no !important appears after the closing brace of @media print
        after_print_brace = parts[1]
        depth = 1
        end_idx = len(after_print_brace)
        for i, char in enumerate(after_print_brace):
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    end_idx = i + 1
                    break
        after_print = after_print_brace[end_idx:]
        assert "!important" not in after_print, (
            "Unexpected '!important' declaration encountered after '@media print' block."
        )


def extract_theme_tokens(css: str) -> tuple[set[str], set[str], set[str]]:
    """Extract custom property variable sets from Light, Dark, and Auto definitions."""
    clean_css = re.sub(r"/\*[\s\S]*?\*/", "", css)

    # 1. Light theme block
    m_light = re.search(
        r"(?::root|\.card|\[data-theme=[\"']light[\"']\])[^{]*\{([^}]*)\}",
        clean_css,
    )
    assert m_light, "Light theme token declaration block not found in stylesheet."
    light_tokens = set(re.findall(r"(--[a-zA-Z0-9_-]+)\s*:", m_light.group(1)))

    # 2. Dark theme block
    m_dark = re.search(
        r"(?:html\.nightMode|\.nightMode|\[data-theme=[\"']dark[\"']\])[^{]*\{([^}]*)\}",
        clean_css,
    )
    assert m_dark, "Dark theme token declaration block not found in stylesheet."
    dark_tokens = set(re.findall(r"(--[a-zA-Z0-9_-]+)\s*:", m_dark.group(1)))

    # 3. Auto theme block (inside prefers-color-scheme: dark)
    m_auto = re.search(
        r"@media\s*\(\s*prefers-color-scheme\s*:\s*dark\s*\)\s*\{[^{]*\[data-theme=[\"']auto[\"']\][^{]*\{([^}]*)\}",
        clean_css,
    )
    assert m_auto, (
        "Auto theme token declaration block inside @media (prefers-color-scheme: dark) not found."
    )
    auto_tokens = set(re.findall(r"(--[a-zA-Z0-9_-]+)\s*:", m_auto.group(1)))

    return light_tokens, dark_tokens, auto_tokens


def check_token_symmetry(css: str) -> None:
    """Verify 100% token parity between Light, Dark, and Auto themes."""
    light_tokens, dark_tokens, auto_tokens = extract_theme_tokens(css)

    assert len(light_tokens) > 0, "No CSS custom properties found in Light theme definition."

    # Compare Light vs Dark
    missing_in_dark = light_tokens - dark_tokens
    extra_in_dark = dark_tokens - light_tokens
    assert not missing_in_dark, (
        f"Token asymmetry: {len(missing_in_dark)} tokens defined in Light but MISSING in Dark: "
        f"{sorted(missing_in_dark)}"
    )
    assert not extra_in_dark, (
        f"Token asymmetry: {len(extra_in_dark)} tokens defined in Dark but MISSING in Light: "
        f"{sorted(extra_in_dark)}"
    )

    # Compare Light vs Auto
    missing_in_auto = light_tokens - auto_tokens
    extra_in_auto = auto_tokens - light_tokens
    assert not missing_in_auto, (
        f"Token asymmetry: {len(missing_in_auto)} tokens defined in Light but MISSING in Auto: "
        f"{sorted(missing_in_auto)}"
    )
    assert not extra_in_auto, (
        f"Token asymmetry: {len(extra_in_auto)} tokens defined in Auto but MISSING in Light: "
        f"{sorted(extra_in_auto)}"
    )

    # Legacy bridge tokens mandatory for AnkiMBR note types
    required_legacy_tokens = [
        "--ambr-accent-line",
        "--ambr-ink-soft",
        "--ambr-control",
        "--ambr-control-line",
        "--ambr-shadow",
        "--ambr-cloze",
        "--ambr-cloze-ink",
    ]
    for token in required_legacy_tokens:
        assert token in light_tokens, (
            f"Mandatory legacy bridge token '{token}' is missing from theme definition. "
            "Existing AnkiMBR flashcards will suffer visual degradation."
        )


def check_native_selectors(css: str) -> None:
    """Verify that all essential native AnkiMBR selectors and bridge hooks exist."""
    required_selector_groups = {
        "Root / Layout Containers": [
            ".coluna",
            ".ambr-card",
        ],
        "Field of Study Header": [
            ".cabecalho",
            ".tag-estudo",
            ".ambr-field-study",
        ],
        "QA Note Type Elements": [
            ".enunciado",
            ".resposta",
            ".ambr-question",
            ".ambr-answer",
        ],
        "Cloze Note Type Elements": [
            ".sentenca",
            ".sentenca-box",
            ".cloze",
        ],
        "Card Dividers": [
            "hr#answer",
        ],
        "Expandable Hints & Notes": [
            ".hints-container",
            ".hint",
            ".hint-corpo",
            ".hint-ref",
            ".hint-details",
        ],
        "Bullet Points List": [
            ".ambr-notas",
        ],
        "Media & Figures Container": [
            ".media figure",
            ".anki-figure",
            ".anki-figcaption",
        ],
        "Interactive Zoom Module": [
            ".ambr-zoom-overlay",
            ".ambr-zoom-viewport",
            ".ambr-zoom-image",
            ".ambr-zoom-close",
        ],
        "Watermark Signature": [
            ".marca-dagua",
        ],
        "Dark Mode Compatibility Selectors": [
            ".nightMode",
            ".night_mode",
            '[data-theme="dark"]',
        ],
        "Mathematical Notation Hooks": [
            ".katex-display",
            "mjx-container",
        ],
    }

    for group_name, selectors in required_selector_groups.items():
        for sel in selectors:
            assert sel in css, (
                f"Missing required native AnkiMBR selector '{sel}' in group '{group_name}'. "
                "Failing to declare this selector breaks installed AnkiMBR card templates."
            )


def run_all_checks(css_path: Path | None = None) -> None:
    """Execute all theme regression checks sequentially."""
    resolved_path, css = load_stylesheet(css_path)
    print(f"[*] Running theme regression checks against: {resolved_path}")

    print("[1/5] Checking color-scheme guards...")
    check_color_scheme(css)
    print("      -> color-scheme: only light and mode declarations verified.")

    print("[2/5] Checking dark auto opt-in isolation...")
    check_dark_auto_opt_in(css)
    print("      -> prefers-color-scheme correctly guarded by [data-theme='auto'].")

    print("[3/5] Checking !important restriction...")
    check_important_usage(css)
    print("      -> Zero !important declarations outside @media print verified.")

    print("[4/5] Checking token symmetry and legacy bridge variables...")
    check_token_symmetry(css)
    light_tokens, _, _ = extract_theme_tokens(css)
    print(f"      -> 100% token symmetry verified across all {len(light_tokens)} variables.")

    print("[5/5] Checking native AnkiMBR selectors and bridge hooks...")
    check_native_selectors(css)
    print("      -> All native AnkiMBR selectors and zoom hooks verified.")

    print("PASS: ankimbr-presentation-v2.css theme regression checks")


if __name__ == "__main__":
    run_all_checks()
