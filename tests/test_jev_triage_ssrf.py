"""tests/test_jev_triage_ssrf.py — B30: SSRF e vazamento de Bearer no triage SystemOne.

Cobre os três requisitos do hardening do cliente SystemOne:

1. apenas `https://` público é aceito (`http://`, `file://`, `ftp://`, sem esquema
   e host privado/loopback/link-local são recusados com erro próprio);
2. nenhum 3xx é seguido, de modo que `Authorization: Bearer` nunca é reenviado ao
   host apontado pelo `Location`;
3. um endpoint `https://` válido continua funcionando.

Nenhum teste faz I/O de rede: o seam HTTP real (`HTTPSHandler.https_open`) é
substituído por respostas sintéticas, então a cadeia de handlers do `urllib`
continua sendo exercitada de verdade.
"""

from __future__ import annotations

import json
import socket
import urllib.request
from typing import Any

import pytest

from anki_mcp.checks import jev_triage

QUESTIONS: dict[str, Any] = {
    "card_count_expected": {"type": "score", "instructions": "quantos cartões?"}
}
SECRET_KEY = "synthetic-test-key"


class _FakeHeaders(dict[str, str]):
    """Cabeçalhos case-insensitive, como os que o `http.client` devolve."""

    def get(self, name: str, default: str | None = None) -> str | None:  # type: ignore[override]
        return super().get(name.lower(), default)


class _FakeResponse:
    """Resposta mínima consumível pela cadeia de handlers do `urllib`."""

    def __init__(
        self,
        code: int,
        *,
        payload: bytes = b"",
        headers: dict[str, str] | None = None,
    ) -> None:
        self.code = code
        self.status = code
        self.msg = f"synthetic {code}"
        self.headers = _FakeHeaders(headers or {})
        self._payload = payload

    def info(self) -> _FakeHeaders:
        return self.headers

    def read(self, *_args: Any, **_kwargs: Any) -> bytes:
        return self._payload

    def close(self) -> None:
        return None

    def __enter__(self) -> _FakeResponse:
        return self

    def __exit__(self, *_exc: object) -> bool:
        return False


def _public_dns(monkeypatch: pytest.MonkeyPatch) -> None:
    """Fixa a resolução DNS em um endereço público, sem tocar a rede."""

    def _resolve(*_args: Any, **_kwargs: Any) -> list[Any]:
        return [(socket.AF_INET, socket.SOCK_STREAM, 6, "", ("93.184.216.34", 0))]

    monkeypatch.setattr(jev_triage.socket, "getaddrinfo", _resolve)


def _patch_https_open(monkeypatch: pytest.MonkeyPatch, response: _FakeResponse) -> list[Any]:
    """Substitui só o passo de socket do `HTTPSHandler`, guardando os requests vistos."""
    seen: list[Any] = []

    def _https_open(self: Any, req: Any, *args: Any, **kwargs: Any) -> _FakeResponse:
        seen.append(req)
        return response

    monkeypatch.setattr(urllib.request.HTTPSHandler, "https_open", _https_open)
    return seen


# ── 1. Esquema inseguro ───────────────────────────────────────────────────────


@pytest.mark.parametrize(
    "endpoint",
    [
        "http://api.typesafe.ai/v1/systemone",
        "file:///etc/passwd",
        "ftp://api.typesafe.ai/v1/systemone",
        "//api.typesafe.ai/v1/systemone",
        "api.typesafe.ai/v1/systemone",
        "",
    ],
)
def test_non_https_endpoint_is_refused_before_any_request(
    monkeypatch: pytest.MonkeyPatch, endpoint: str
) -> None:
    monkeypatch.setattr(jev_triage, "TYPESAFE_ENDPOINT", endpoint)

    def _no_request(*_args: Any, **_kwargs: Any) -> Any:
        raise AssertionError("an insecure endpoint must never reach the HTTP layer")

    monkeypatch.setattr(jev_triage, "_open_systemone", _no_request)

    with pytest.raises(jev_triage.InsecureSystemOneEndpoint) as excinfo:
        jev_triage._post_systemone_with_fallback("estado", QUESTIONS, explicit_api_key=SECRET_KEY)

    message = str(excinfo.value)
    assert "https" in message, f"mensagem deve exigir https: {message}"
    assert SECRET_KEY not in message, "o erro não pode ecoar a credencial"


@pytest.mark.parametrize("address", ["127.0.0.1", "10.1.2.3", "169.254.169.254", "::1", "fd00::1"])
def test_endpoint_resolving_to_non_public_address_is_refused(
    monkeypatch: pytest.MonkeyPatch, address: str
) -> None:
    def _resolve(*_args: Any, **_kwargs: Any) -> list[Any]:
        host = (address, 0, 0, 0) if ":" in address else (address, 0)
        return [
            (socket.AF_INET6 if ":" in address else socket.AF_INET, socket.SOCK_STREAM, 6, "", host)
        ]

    monkeypatch.setattr(jev_triage.socket, "getaddrinfo", _resolve)

    with pytest.raises(jev_triage.InsecureSystemOneEndpoint) as excinfo:
        jev_triage._validate_systemone_endpoint("https://metadata.example/v1/systemone")

    assert address in str(excinfo.value)


def test_localhost_hostname_is_refused() -> None:
    with pytest.raises(jev_triage.InsecureSystemOneEndpoint):
        jev_triage._validate_systemone_endpoint("https://localhost:8443/v1/systemone")


def test_unresolvable_host_is_not_reported_as_insecure(monkeypatch: pytest.MonkeyPatch) -> None:
    """Falha de DNS (offline) não é veredito de segurança: a rede reporta o erro real."""

    def _fail(*_args: Any, **_kwargs: Any) -> list[Any]:
        raise socket.gaierror("name resolution unavailable")

    monkeypatch.setattr(jev_triage.socket, "getaddrinfo", _fail)

    assert (
        jev_triage._validate_systemone_endpoint("https://api.typesafe.ai/v1/systemone")
        == "https://api.typesafe.ai/v1/systemone"
    )


# ── 2. Redirect bloqueado ────────────────────────────────────────────────────


def test_opener_replaces_the_default_redirect_handler() -> None:
    handlers = jev_triage._build_systemone_opener().handlers
    redirect_handlers = [h for h in handlers if isinstance(h, urllib.request.HTTPRedirectHandler)]
    assert redirect_handlers, "um handler de redirect precisa existir para ser bloqueante"
    assert all(isinstance(h, jev_triage._BlockRedirects) for h in redirect_handlers), (
        f"o handler padrão do urllib não pode estar presente: {redirect_handlers}"
    )


@pytest.mark.parametrize("code", [301, 302, 303, 307, 308])
def test_redirect_status_is_never_followed(monkeypatch: pytest.MonkeyPatch, code: int) -> None:
    _public_dns(monkeypatch)
    seen = _patch_https_open(
        monkeypatch,
        _FakeResponse(code, headers={"location": "https://attacker.example/collect"}),
    )
    monkeypatch.setattr(jev_triage, "TYPESAFE_ENDPOINT", "https://api.typesafe.ai/v1/systemone")

    with pytest.raises(jev_triage.SystemOneRedirectBlocked) as excinfo:
        jev_triage._post_systemone_with_fallback("estado", QUESTIONS, explicit_api_key=SECRET_KEY)

    message = str(excinfo.value)
    assert str(code) in message, f"a mensagem deve citar o status: {message}"
    assert "attacker.example" in message, f"a mensagem deve citar o destino recusado: {message}"
    assert SECRET_KEY not in message, "o erro não pode ecoar a credencial"
    assert len(seen) == 1, "o destino do Location nunca pode ser requisitado"


def test_redirect_does_not_fall_back_to_another_provider(monkeypatch: pytest.MonkeyPatch) -> None:
    """Um 3xx é erro do provedor: a credencial não é tentada em outro host."""
    _public_dns(monkeypatch)
    monkeypatch.setenv("KNOWLINE_API_KEY", "knowline-key")
    monkeypatch.setattr(jev_triage, "KNOWLINE_ENDPOINT", "https://knowline.example/v1/systemone")
    calls: list[Any] = []

    def _https_open(self: Any, req: Any, *args: Any, **kwargs: Any) -> _FakeResponse:
        calls.append(req)
        if "knowline" in req.full_url:
            return _FakeResponse(302, headers={"location": "https://attacker.example/collect"})
        return _FakeResponse(200, payload=json.dumps({"answers": {"x": {}}}).encode())

    monkeypatch.setattr(urllib.request.HTTPSHandler, "https_open", _https_open)

    with pytest.raises(jev_triage.SystemOneRedirectBlocked):
        jev_triage._post_systemone_with_fallback("estado", QUESTIONS)

    assert len(calls) == 1, f"nenhum fallback pode ser tentado após 3xx: {calls}"


# ── 3. https válido continua funcionando ─────────────────────────────────────


def test_valid_https_endpoint_still_succeeds_and_sends_the_bearer(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _public_dns(monkeypatch)
    answers = {"answers": {"card_count_expected": {"score": 0.75, "confidence": 0.9}}}
    seen = _patch_https_open(monkeypatch, _FakeResponse(200, payload=json.dumps(answers).encode()))
    monkeypatch.setattr(jev_triage, "TYPESAFE_ENDPOINT", "https://api.typesafe.ai/v1/systemone")

    data = jev_triage._post_systemone_with_fallback(
        "estado", QUESTIONS, explicit_api_key=SECRET_KEY
    )

    assert data == answers
    assert len(seen) == 1, "uma única requisição, sem redirect"
    request = seen[0]
    assert request.full_url == "https://api.typesafe.ai/v1/systemone"
    assert request.get_header("Authorization") == f"Bearer {SECRET_KEY}"
    assert request.get_method() == "POST"


def test_call_jev_section_score_degrades_gracefully_on_insecure_endpoint(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """O hardening não derruba o pipeline: só força o fallback determinístico."""
    monkeypatch.setattr(jev_triage, "TYPESAFE_ENDPOINT", "http://127.0.0.1:8080/v1/systemone")

    _score, _conf, _probs, engine = jev_triage.call_jev_section_score(
        "# Seção\nConduta A: metilprednisolona 1 g/dia.", api_key=SECRET_KEY
    )

    assert engine == "deterministic_fallback_error"
