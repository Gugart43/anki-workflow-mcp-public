"""tools/state.py — manifesto & governance MCP tools.

Tools:
  manifesto_init       — initialize a new manifesto.json
  manifesto_import     — import legacy manifesto.md → manifesto.json
  manifesto_state      — read current manifesto state
  manifesto_commit     — transactional write (sole writer, requires receipt)
  taxonomy_check       — validate tags against registered taxonomy
  id_ledger_query      — query ID registry
"""

from __future__ import annotations

import json
import re
from datetime import date
from pathlib import Path
from typing import Any

from fastmcp import FastMCP

from anki_mcp.core.deck import load_manifesto, resolve_deck, save_manifesto, transaction
from anki_mcp.core.receipt import verify_receipt
from anki_mcp.core.render import render_manifesto_md
from anki_mcp.core.safe import error_result, safe_tool
from anki_mcp.models import (
    BatchMeta,
    CardStatus,
    ChangelogEntry,
    IDEntry,
    Manifesto,
    NoteType,
    TaxonomyNode,
)

# ── Helpers ───────────────────────────────────────────────────────────────────


def _flat_taxonomy(m: Manifesto) -> set[str]:
    """All registered tag paths from the taxonomy."""
    root = m.taxonomy.root
    tags = {root}
    for node in m.taxonomy.nodes:
        tags.add(f"{root}::{node}")
    return tags


# ── Tool registration ─────────────────────────────────────────────────────────


def register_state_tools(mcp: FastMCP) -> None:

    @mcp.tool(
        description=(
            "Initialize a new manifesto.json for a deck. "
            "deck_dir: absolute path to the deck folder. "
            "campo_estudo: e.g. 'Medicina::Neurologia::ELA'. "
            "deck_slug: short identifier (e.g. 'ela'). "
            "taxonomy_root: root tag (e.g. 'ELA'). "
            "Will NOT overwrite an existing manifesto.json."
        )
    )
    @safe_tool
    def manifesto_init(
        deck_dir: str,
        campo_estudo: str,
        deck_slug: str,
        taxonomy_root: str,
    ) -> dict:
        p = Path(deck_dir)
        json_path = p / "manifesto.json"
        if json_path.exists():
            return {
                "status": "SKIPPED",
                "reason": "manifesto.json already exists",
                "path": str(json_path),
            }

        with transaction(deck_dir, require_manifesto=False):
            m = Manifesto(
                deck_slug=deck_slug,
                campo_estudo=campo_estudo,
                created=str(date.today()),
                updated=str(date.today()),
                taxonomy=TaxonomyNode(root=taxonomy_root),
            )
            save_manifesto(deck_dir, m)
            md_path = p / "manifesto.md"
            md_path.write_text(render_manifesto_md(m), encoding="utf-8")
        return {
            "status": "CREATED",
            "manifesto_json": str(json_path),
            "manifesto_md": str(md_path),
        }

    @mcp.tool(
        description=(
            "Import a legacy manifesto.md into manifesto.json format. "
            "Parses the markdown file and creates manifesto.json. "
            "deck_dir: folder containing the manifesto.md. "
            "manifesto_md_name: filename (default 'manifesto.md'). "
            "Reports all parsed sections and any parsing issues."
        )
    )
    @safe_tool
    def manifesto_import(
        deck_dir: str,
        manifesto_md_name: str = "manifesto.md",
    ) -> dict:
        md_path = Path(deck_dir) / manifesto_md_name
        if not md_path.exists():
            return error_result(
                "FILE.NOT_FOUND", f"File not found: {md_path}", "Check the path and the CSV format."
            )

        text = md_path.read_text(encoding="utf-8")

        issues: list[str] = []
        parsed: dict[str, Any] = {}

        # ── Parse campo and dates ─────────────────────────────────────────────
        m_campo = re.search(r"\*\*Campo de Estudo:\*\*\s*`([^`]+)`", text)
        m_created = re.search(r"\*\*Data de Inicializa[çc][aã]o:\*\*\s*(\S+)", text)
        m_updated = re.search(r"\*\*[ÚU]ltima Atualiza[çc][aã]o:\*\*\s*(\S+)", text)

        campo = m_campo.group(1) if m_campo else ""
        created = m_created.group(1) if m_created else str(date.today())
        updated = m_updated.group(1) if m_updated else str(date.today())
        if not campo:
            issues.append("Campo de Estudo not found in markdown")
        parsed["campo_estudo"] = campo

        # ── Parse ID Registry (table rows) ───────────────────────────────────
        id_entries: list[IDEntry] = []
        # Look for rows like: | `B01-QA-001` | B01 | QA | `active` | topic | anchor |
        id_rows = re.findall(
            r"\|\s*`(B\d+-(?:QA|CL)-\d+[a-z]?)`\s*\|"
            r"\s*(\S+)\s*\|\s*(QA|CL)\s*\|"
            r"\s*`?(\w+)`?\s*\|\s*([^|]+)\|\s*([^|]+)\|",
            text,
        )
        for row in id_rows:
            card_id, batch, ctype, status_raw, topic, anchor = row
            try:
                status = CardStatus(status_raw.strip())
            except ValueError:
                status = CardStatus.ACTIVE
                issues.append(f"Unknown status '{status_raw}' for {card_id}, defaulted to active")
            id_entries.append(
                IDEntry(
                    id=card_id.strip(),
                    batch=batch.strip(),
                    type=NoteType(ctype.strip()),
                    status=status,
                    topic=topic.strip(),
                    anchor=anchor.strip(),
                )
            )
        parsed["id_count"] = len(id_entries)

        # ── Parse Topic Log ───────────────────────────────────────────────────
        topic_log: list[str] = []
        in_topics = False
        for line in text.splitlines():
            if "## 2." in line and "Tópico" in line:
                in_topics = True
                continue
            if in_topics and line.startswith("## "):
                break
            if in_topics and line.startswith("- "):
                topic_log.append(line[2:].strip())
        parsed["topic_count"] = len(topic_log)

        # ── Parse Taxonomy ────────────────────────────────────────────────────
        root = ""
        nodes: list[str] = []
        planned: dict[str, str] = {}
        in_taxonomy = False
        for line in text.splitlines():
            if "## 3." in line:
                in_taxonomy = True
                continue
            if in_taxonomy and line.startswith("## "):
                break
            if in_taxonomy:
                stripped = line.strip().strip("`")
                if (
                    stripped
                    and not stripped.startswith("├")
                    and not stripped.startswith("└")
                    and not stripped.startswith("│")
                    and not root
                ):
                    root = stripped
                elif stripped.startswith(("├", "└")):
                    node_name = re.sub(r"^[├└─ ]+", "", stripped)
                    planned_m = re.search(r"\(Previsto para (B\d+)\)", node_name)
                    node_name = re.sub(r"\s*\(Previsto.*\)", "", node_name).strip()
                    if node_name:
                        nodes.append(node_name)
                    if planned_m and node_name:
                        planned[node_name] = planned_m.group(1)
        parsed["taxonomy_root"] = root
        parsed["taxonomy_nodes"] = nodes

        # ── Parse Suggestion Queue ────────────────────────────────────────────
        queue: list[str] = []
        in_queue = False
        for line in text.splitlines():
            if "## 6." in line:
                in_queue = True
                continue
            if in_queue and line.startswith("## "):
                break
            if in_queue and line.startswith("- ") and "Vazia" not in line:
                queue.append(line[2:].strip())

        # ── Derive slug from campo ────────────────────────────────────────────
        slug = re.sub(r"\W+", "_", campo.split("::")[-1].lower()).strip("_") if campo else "deck"

        # ── Build Manifesto ───────────────────────────────────────────────────
        manifesto = Manifesto(
            deck_slug=slug,
            campo_estudo=campo,
            created=created,
            updated=updated,
            id_registry=id_entries,
            topic_log=topic_log,
            taxonomy=TaxonomyNode(root=root, nodes=nodes, planned=planned),
            suggestion_queue=queue,
            totals={
                "active": sum(1 for e in id_entries if e.status == CardStatus.ACTIVE),
                "qa": sum(
                    1 for e in id_entries if e.type == NoteType.QA and e.status == CardStatus.ACTIVE
                ),
                "cloze": sum(
                    1
                    for e in id_entries
                    if e.type == NoteType.CLOZE and e.status == CardStatus.ACTIVE
                ),
            },
        )
        with transaction(deck_dir, require_manifesto=False):
            save_manifesto(deck_dir, manifesto)
            md_out = Path(deck_dir) / "manifesto.md"
            md_out.write_text(render_manifesto_md(manifesto), encoding="utf-8")

        return {
            "status": "IMPORTED",
            "slug": slug,
            "ids_parsed": len(id_entries),
            "topics_parsed": len(topic_log),
            "taxonomy_root": root,
            "issues": issues,
            "manifesto_json": str(Path(deck_dir) / "manifesto.json"),
        }

    @mcp.tool(
        description=(
            "Read the current state of the Manifesto. "
            "deck_dir: absolute path to the deck folder. "
            "sections: comma-separated subset to return "
            "('all', 'ids', 'topics', 'taxonomy', 'catalogs', 'changelog', 'queue', 'batches')."
        )
    )
    @safe_tool
    def manifesto_state(
        deck_dir: str,
        sections: str = "all",
    ) -> dict:
        m = load_manifesto(deck_dir)
        result: dict[str, Any] = {
            "deck_slug": m.deck_slug,
            "campo": m.campo_estudo,
            "updated": m.updated,
            "totals": m.totals,
            "last_batch": m.last_validated_batch,
        }
        want = {s.strip() for s in sections.split(",")}
        if "all" in want or "ids" in want:
            result["id_registry"] = [e.model_dump(mode="json") for e in m.id_registry]
        if "all" in want or "topics" in want:
            result["topic_log"] = m.topic_log
        if "all" in want or "taxonomy" in want:
            result["taxonomy"] = m.taxonomy.model_dump(mode="json")
        if "all" in want or "catalogs" in want:
            result["catalogs"] = [c.model_dump(mode="json") for c in m.catalogs]
        if "all" in want or "changelog" in want:
            result["changelog"] = [e.model_dump(mode="json") for e in m.changelog]
        if "all" in want or "queue" in want:
            result["suggestion_queue"] = m.suggestion_queue
        if "all" in want or "batches" in want:
            result["batches"] = {k: v.model_dump(mode="json") for k, v in m.batches.items()}
        if m.topic_plan:
            result["topic_plan"] = m.topic_plan.model_dump(mode="json")
        return result

    @mcp.tool(
        description=(
            "Register the top-down total card budget plan for a topic into the Manifesto before drafting. "
            "Enforces that the TOTAL quantity of cards is planned before complete production, "
            "chunked strictly into batches of 20 (MAX_BATCH = 20)."
        )
    )
    @safe_tool
    def manifesto_plan_set(
        deck_dir: str,
        total_planned: int,
        batches_planned: str,
        rationale: str = "",
        breakdown_json: str = "{}",
        batch_size: int = 20,
    ) -> dict[str, Any]:
        p = resolve_deck(deck_dir)
        batches = [b.strip() for b in batches_planned.split(",") if b.strip()]
        # Malformed input is an argument error, never a silently-replaced value (M1).
        breakdown = json.loads(breakdown_json) if breakdown_json.strip() else {}
        if not isinstance(breakdown, dict):
            raise ValueError("breakdown_json must be a JSON object")

        from anki_mcp.models import TopicPlan

        with transaction(p) as m:
            m.topic_plan = TopicPlan(
                total_planned=total_planned,
                batch_size=batch_size,
                batches_planned=batches,
                breakdown=breakdown,
                rationale=rationale,
            )
            m.updated = str(date.today())
            save_manifesto(p, m)
            md_text = render_manifesto_md(m)
            (p / "manifesto.md").write_text(md_text, encoding="utf-8")
        return {
            "status": "OK",
            "deck_dir": str(p),
            "total_planned": total_planned,
            "batch_size": batch_size,
            "batches_planned": batches,
            "topic_plan": m.topic_plan.model_dump(mode="json"),
        }

    @mcp.tool(
        description=(
            "Sole writer of the Manifesto. Commits a batch's data transactionally. "
            "Requires a valid receipt (content_hash from validate_batch PASS). "
            "Parameters: deck_dir, batch_id, receipt_hash, qa_csv, cloze_csv "
            "(for receipt verification), new_ids (JSON array of IDEntry dicts), "
            "new_topics (JSON array of strings), new_tags (comma-separated), "
            "changelog_entries (JSON array), queue_updates (JSON array of strings to add)."
        )
    )
    @safe_tool
    def manifesto_commit(
        deck_dir: str,
        batch_id: str,
        receipt_hash: str,
        qa_csv: str,
        cloze_csv: str,
        new_ids: str = "[]",  # JSON
        new_topics: str = "[]",  # JSON
        new_tags: str = "",  # comma-separated
        changelog_entries: str = "[]",  # JSON
        queue_updates: str = "[]",  # JSON
        catalog_updates: str = "[]",  # JSON (list of CatalogRef dicts)
    ) -> dict:
        """Transactional commit — the only tool that writes the Manifesto."""
        # ── Verify receipt ────────────────────────────────────────────────────
        if not verify_receipt(receipt_hash, qa_csv, cloze_csv):
            return {
                "status": "BLOCKED",
                "code": "STATE.NO_RECEIPT",
                "message": "Receipt hash does not match the provided CSV content. "
                "Re-run validate_batch and use the returned content_hash.",
            }

        from anki_mcp.checks.batch import count_batch

        with transaction(deck_dir) as m:
            today = str(date.today())

            # A CSV receipt identifies content, not the metadata mutation payload.
            prior = m.batches.get(batch_id)
            ids_parsed = json.loads(new_ids)
            entries = [IDEntry.model_validate(raw) for raw in ids_parsed]
            if len({entry.id for entry in entries}) != len(entries):
                return {
                    "status": "BLOCKED",
                    "code": "ID.DUPLICATE",
                    "message": "Duplicate IDs in payload.",
                }
            if prior is not None and any(
                prior.gates.get(gate) not in (None, receipt_hash)
                for gate in ("committed", "exported")
            ):
                return {
                    "status": "BLOCKED",
                    "code": "EXPORT.BATCH_CONFLICT",
                    "message": "Batch content differs from an existing receipt.",
                }
            known_ids = {entry.id: entry.model_dump(mode="json") for entry in m.id_registry}
            requested_nodes = {
                tag.strip()[len(m.taxonomy.root) + 2 :].split("::")[0]
                for tag in new_tags.split(",")
                if tag.strip().startswith(m.taxonomy.root + "::")
            }
            metadata_present = (
                all(
                    known_ids.get(raw.get("id"))
                    == IDEntry.model_validate(raw).model_dump(mode="json")
                    for raw in ids_parsed
                )
                and all(topic in m.topic_log for topic in json.loads(new_topics))
                and requested_nodes.issubset(m.taxonomy.nodes)
                and all(item in m.suggestion_queue for item in json.loads(queue_updates))
                and all(
                    raw in [entry.model_dump(mode="json") for entry in m.catalogs]
                    for raw in json.loads(catalog_updates)
                )
                and all(
                    ChangelogEntry.model_validate(raw).model_dump(mode="json")
                    in [entry.model_dump(mode="json") for entry in m.changelog]
                    for raw in json.loads(changelog_entries)
                )
            )
            if (
                prior is not None
                and prior.gates.get("committed") == receipt_hash
                and metadata_present
            ):
                return {
                    "status": "ALREADY_EXPORTED",
                    "batch_id": batch_id,
                    "message": "Same receipt already committed; nothing was written.",
                }

            if prior is not None and prior.gates.get("exported") not in (None, receipt_hash):
                return {
                    "status": "BLOCKED",
                    "code": "EXPORT.BATCH_CONFLICT",
                    "message": "Batch already exported with different content.",
                }

            # ── Register new IDs ──────────────────────────────────────────────────
            existing_ids = {e.id for e in m.id_registry}
            ids_parsed = json.loads(new_ids)
            added_ids: list[str] = []
            for raw in ids_parsed:
                entry = IDEntry.model_validate(raw)
                if entry.id in existing_ids:
                    existing = next(e for e in m.id_registry if e.id == entry.id)
                    if prior is not None and existing == entry and existing.batch == batch_id:
                        continue
                    return {
                        "status": "BLOCKED",
                        "code": "ID.DUPLICATE",
                        "message": f"ID {entry.id} already exists in the ledger",
                    }
                m.id_registry.append(entry)
                added_ids.append(entry.id)
                existing_ids.add(entry.id)

            # ── Update topic log ──────────────────────────────────────────────────
            for topic in json.loads(new_topics):
                if topic not in m.topic_log:
                    m.topic_log.append(topic)

            # ── Register new tags ─────────────────────────────────────────────────
            for tag in (t.strip() for t in new_tags.split(",") if t.strip()):
                if tag.startswith(m.taxonomy.root + "::"):
                    node = tag[len(m.taxonomy.root) + 2 :].split("::")[0]
                    if node not in m.taxonomy.nodes:
                        m.taxonomy.nodes.append(node)

            # ── Catalog updates ───────────────────────────────────────────────────
            from anki_mcp.models import CatalogRef

            known_slugs = {c.slug for c in m.catalogs}
            for raw in json.loads(catalog_updates):
                cat = CatalogRef.model_validate(raw)
                if cat.slug not in known_slugs:
                    m.catalogs.append(cat)

            # ── Changelog ────────────────────────────────────────────────────────
            for raw in json.loads(changelog_entries):
                item = ChangelogEntry.model_validate(raw)
                if item not in m.changelog:
                    m.changelog.append(item)

            # ── Suggestion queue ──────────────────────────────────────────────────
            for item in json.loads(queue_updates):
                if item not in m.suggestion_queue:
                    m.suggestion_queue.append(item)

            # ── Update totals & batch state ───────────────────────────────────────
            active_ids = [e for e in m.id_registry if e.status == CardStatus.ACTIVE]
            m.totals = {
                "active": len(active_ids),
                "qa": sum(1 for e in active_ids if e.type == NoteType.QA),
                "cloze": sum(1 for e in active_ids if e.type == NoteType.CLOZE),
            }
            m.last_validated_batch = batch_id
            m.updated = today

            # Record batch state
            m.batches[batch_id] = BatchMeta(
                state="exported"
                if prior is not None and prior.state == "exported"
                else "validated",
                cards=count_batch(qa_csv, cloze_csv, batch_id).total,
                gates={**(prior.gates if prior else {}), "committed": receipt_hash},
            )

            # ── Atomic write ──────────────────────────────────────────────────────
            save_manifesto(deck_dir, m)
            md_path = Path(deck_dir) / "manifesto.md"
            md_path.write_text(render_manifesto_md(m), encoding="utf-8")

        return {
            "status": "COMMITTED",
            "batch_id": batch_id,
            "ids_added": len(added_ids),
            "totals": m.totals,
            "md_rendered": str(md_path),
            "idempotent": False,
        }

    @mcp.tool(
        description=(
            "Check tags against the Manifesto taxonomy. "
            "tags: space-separated list. "
            "Returns ok/unregistered/depth/format per tag."
        )
    )
    @safe_tool
    def taxonomy_check(deck_dir: str, tags: str) -> dict:
        m = load_manifesto(deck_dir)
        flat = _flat_taxonomy(m)
        results = {}
        for tag in tags.split():
            tag = tag.strip()
            if not tag:
                continue
            depth = tag.count("::") + 1
            if depth > 4:
                results[tag] = {"status": "FAIL", "code": "TAG.DEPTH", "depth": depth}
            elif " " in tag:
                results[tag] = {"status": "FAIL", "code": "TAG.FORMAT"}
            elif tag not in flat:
                results[tag] = {
                    "status": "FAIL",
                    "code": "TAG.UNREGISTERED",
                    "registered": sorted(flat),
                }
            else:
                results[tag] = {"status": "OK"}
        return {"results": results, "taxonomy_flat": sorted(flat)}

    @mcp.tool(
        description=(
            "Query the ID ledger. "
            "filters: comma-separated values like 'batch=B01', 'status=active', 'type=QA'. "
            "Returns matching ID entries and a summary."
        )
    )
    @safe_tool
    def id_ledger_query(deck_dir: str, filters: str = "") -> dict:
        m = load_manifesto(deck_dir)
        entries = m.id_registry
        for filt in filters.split(","):
            filt = filt.strip()
            if "=" not in filt:
                continue
            key, val = filt.split("=", 1)
            key, val = key.strip(), val.strip()
            if key == "batch":
                entries = [e for e in entries if e.batch == val]
            elif key == "status":
                entries = [e for e in entries if e.status.value == val]
            elif key == "type":
                entries = [e for e in entries if e.type.value == val]
        return {
            "count": len(entries),
            "entries": [e.model_dump(mode="json") for e in entries],
        }
