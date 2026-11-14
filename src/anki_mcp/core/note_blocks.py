"""Optional, source-anchored study blocks; independent of recall atomicity."""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator

from anki_mcp.core.sanitize import sanitize_field
from anki_mcp.core.tables import TableSpec


def _text(value: str) -> str:
    return sanitize_field(value, convert_markdown=False)[0]


class NoteSource(BaseModel):
    """Audit evidence supplied by the author, not an automatic fidelity verdict."""

    model_config = ConfigDict(extra="forbid", str_strip_whitespace=True)
    fonte: str = Field(min_length=1)
    pagina: int = Field(ge=1)
    trecho: str = Field(min_length=1)

    def reference(self) -> str:
        return f"{self.fonte}, p. PDF {self.pagina}."


class FlowNode(BaseModel):
    model_config = ConfigDict(extra="forbid")
    id: str = Field(min_length=1)
    texto: str = Field(min_length=1)


class FlowEdge(BaseModel):
    model_config = ConfigDict(extra="forbid")
    origem: str
    destino: str
    rotulo: str = Field(min_length=1)


class NoteFlow(BaseModel):
    """Explicit graph rendered as a numbered, accessible decision map."""

    model_config = ConfigDict(extra="forbid")
    inicio: str
    nos: list[FlowNode] = Field(min_length=1)
    arestas: list[FlowEdge] = Field(default_factory=list)

    @model_validator(mode="after")
    def check_graph(self) -> NoteFlow:
        ids = {n.id for n in self.nos}
        if len(ids) != len(self.nos) or self.inicio not in ids:
            raise ValueError("Flow requires unique node IDs and a valid inicio")
        links = [(e.origem, e.destino, e.rotulo) for e in self.arestas]
        if len(set(links)) != len(links):
            raise ValueError("Duplicate flow edges")
        if any(e.origem not in ids or e.destino not in ids for e in self.arestas):
            raise ValueError("Flow edge refers to a missing node")
        reached = {self.inicio}
        while True:
            following = reached | {e.destino for e in self.arestas if e.origem in reached}
            if following == reached:
                break
            reached = following
        if reached != ids:
            raise ValueError("Every flow node must be reachable from inicio")
        return self

    def render_html(self) -> str:
        nodes = sorted(self.nos, key=lambda n: n.id != self.inicio)
        numbers = {n.id: i for i, n in enumerate(nodes, 1)}
        labels = {n.id: n.texto for n in nodes}
        parts = ["<ol>"]
        for node in nodes:
            parts.append(f"<li><b>{_text(node.texto)}</b>")
            edges = [e for e in self.arestas if e.origem == node.id]
            if edges:
                parts.append("<ul>")
                for edge in edges:
                    parts.append(
                        f"<li>{_text(edge.rotulo)} → {numbers[edge.destino]}. {_text(labels[edge.destino])}</li>"
                    )
                parts.append("</ul>")
            parts.append("</li>")
        return "".join(parts) + "</ol>"


class NoteBlock(BaseModel):
    """One optional study block. Do not fill every format on every card."""

    model_config = ConfigDict(extra="forbid")
    tipo: Literal["paragrafo", "lista", "alerta", "tabela", "fluxograma"]
    titulo: str = ""
    texto: str = ""
    itens: list[str] = Field(default_factory=list)
    tabela: TableSpec | None = None
    fluxograma: NoteFlow | None = None
    fontes: list[NoteSource] = Field(min_length=1)

    @model_validator(mode="after")
    def check_content(self) -> NoteBlock:
        if self.tipo in {"paragrafo", "alerta"}:
            valid = (
                bool(self.texto.strip())
                and not self.itens
                and self.tabela is None
                and self.fluxograma is None
            )
        elif self.tipo == "lista":
            valid = (
                bool(self.itens)
                and all(x.strip() for x in self.itens)
                and not self.texto
                and self.tabela is None
                and self.fluxograma is None
            )
        elif self.tipo == "fluxograma":
            valid = (
                self.fluxograma is not None
                and self.tabela is None
                and not self.texto
                and not self.itens
            )
        else:
            valid = (
                self.tabela is not None
                and self.fluxograma is None
                and not self.texto
                and not self.itens
            )
        if not valid:
            raise ValueError(
                "Block content must match tipo; empty or competing payloads are invalid"
            )
        return self

    def render_html(self) -> str:
        title = f"<p><b>{_text(self.titulo)}</b></p>" if self.titulo else ""
        if self.tipo == "tabela":
            assert self.tabela is not None
            return title + self.tabela.render_html()
        if self.tipo == "fluxograma":
            assert self.fluxograma is not None
            return title + self.fluxograma.render_html()
        if self.tipo == "lista":
            return title + "<ul>" + "".join(f"<li>{_text(x)}</li>" for x in self.itens) + "</ul>"
        tag = "blockquote" if self.tipo == "alerta" else "p"
        return title + f"<{tag}>{_text(self.texto)}</{tag}>"
