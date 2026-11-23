"""SHARC queries over the decoded DSP image and its analysis tables."""

from __future__ import annotations

import hashlib
import json
import os
import re
import sqlite3
import tomllib
from contextlib import closing
from functools import cached_property
from typing import Any

from tools.firmware.query_db import (
    ATLAS_DIR,
    ROOT,
    SHARC_DIR,
    FwError,
    Graph,
    _has,
    clip,
    db_key,
    load_reference,
    open_ro,
    parse_addr,
    short_image,
)

DSP_CODE_BASE = 0x28000000


def anchor_sw(anchor: dict[str, Any]) -> int | None:
    """Convert a generated port anchor to its SHARC short-word code address."""
    if "pm" in anchor:
        return int(anchor["pm"])
    addr = (
        anchor.get("src", anchor["addr"])
        if anchor.get("kind") == "value"
        else anchor["addr"]
    )
    if addr < DSP_CODE_BASE:
        return None
    return (addr - DSP_CODE_BASE) // 2


def port_anchors(image: str) -> list[dict[str, Any]]:
    path = os.path.join(ROOT, "ports", image + ".toml")
    if not os.path.isfile(path):
        return []
    with open(path, "rb") as fh:
        return [
            a for a in tomllib.load(fh).get("anchor", []) if a.get("cpu") == "sharc"
        ]


def matches_term(term: str, *values: str | None) -> bool:
    words = term.casefold().replace("_", " ").split()
    text = " ".join(v for v in values if v).casefold().replace("_", " ")
    return all(word in text for word in words)


class Sharc:
    cpu = "sharc"
    address_width = 6

    def __init__(self, image: str):
        self.image = short_image(image)
        self.path = os.path.join(SHARC_DIR, self.image + ".sqlite")
        self.db = open_ro(self.path)
        self.meta = (
            dict(self.db.execute("SELECT key, value FROM meta").fetchall())
            if _has(self.db, "meta")
            else {}
        )
        self.meta = {k.split("|")[-1]: v for k, v in self.meta.items()} or self.meta
        rows = (
            self.db.execute(
                "SELECT key, value FROM meta WHERE image=?", (self.image,)
            ).fetchall()
            if _has(self.db, "meta")
            else []
        )
        if rows:
            self.meta = dict(rows)
        self._graph: Graph | None = None
        self.graph_state = ""
        notes = os.path.join(SHARC_DIR, self.image + ".notes.sqlite")
        self.notes = open_ro(notes) if os.path.exists(notes) else None

    def close(self) -> None:
        if self.notes is not None:
            self.notes.close()
        self.db.close()

    @cached_property
    def atlas_roles(self) -> dict[int, str]:
        filename = os.path.join(ATLAS_DIR, self.image + "-dsp.sqlite")
        if not os.path.isfile(filename):
            return {}
        with closing(open_ro(filename)) as db:
            if not _has(db, "function_index_meta"):
                return {}
            stamp = dict(db.execute("SELECT key, value FROM function_index_meta"))
            if any(
                stamp.get(k) != self.meta.get(k)
                for k in ("image_sha256", "build_input_sha256", "min_depth", "blocks")
            ):
                return {}
            return dict(db.execute("SELECT addr, inferred_role FROM function_index"))

    def func_row(self, sw: int) -> tuple[Any, ...] | None:
        return self.db.execute(
            "SELECT entry_sw, end_sw, n_insns, block, name, label, entry_kind, is_leaf FROM functions "
            "WHERE entry_sw<=? AND end_sw>? ORDER BY entry_sw DESC LIMIT 1",
            (sw, sw),
        ).fetchone()

    def resolve(self, token: str) -> int:
        addr = parse_addr(token)
        if addr is None and token in self.anchor_names:
            addr = self.anchor_names[token]
        if addr is None:
            row = self.db.execute(
                "SELECT entry_sw FROM functions WHERE name=?", (token,)
            ).fetchone()
            if row is None:
                raise FwError(
                    "no SHARC function named %r (try: search %s)" % (token, token)
                )
            addr = row[0]
        f = self.func_row(addr)
        if f is None:
            raise FwError("0x%x is not inside a known SHARC function" % addr)
        return f[0]

    @cached_property
    def anchors(self) -> dict[int, list[tuple[str, int, str, str]]]:
        """Declared names and annotated sites, grouped by containing function."""
        docs = {
            a["name"]: a.get("doc") or ""
            for a in load_reference()
            if a.get("cpu") == "sharc"
        }
        grouped: dict[int, list[tuple[str, int, str, str]]] = {}
        for anchor in port_anchors(self.image):
            sw = anchor_sw(anchor)
            row = self.func_row(sw) if sw is not None else None
            if row is not None and sw is not None:
                name = anchor["name"]
                grouped.setdefault(row[0], []).append(
                    (name, sw, anchor.get("mark", "?"), docs.get(name, ""))
                )
        return grouped

    @cached_property
    def anchor_names(self) -> dict[str, int]:
        return {
            name: sw
            for group in self.anchors.values()
            for name, sw, _mark, _doc in group
        }

    def function_index(self) -> list[tuple[int, int, str, str, str, str, str, str]]:
        """One row per recovered function, including unnamed entries."""
        notes = (
            {
                sw: (role, summary, confidence)
                for sw, role, summary, confidence in self.notes.execute(
                    "SELECT function_sw, role, summary, confidence FROM notes WHERE image=?",
                    (self.image,),
                )
            }
            if self.notes is not None and _has(self.notes, "notes")
            else {}
        )
        rows = []
        for sw, end, name, label in self.db.execute(
            "SELECT entry_sw, end_sw, name, label FROM functions WHERE image=? ORDER BY entry_sw",
            (self.image,),
        ):
            role, summary, confidence = notes.get(sw, ("", "", ""))
            anchors = ", ".join(a[0] for a in self.anchors.get(sw, []))
            rows.append(
                (
                    sw,
                    end,
                    name,
                    label or "",
                    anchors,
                    role or "",
                    summary or "",
                    confidence or "",
                )
            )
        return rows

    def note(self, entry: int) -> tuple[str, str] | None:
        if self.notes is None:
            return None
        try:
            row = self.notes.execute(
                "SELECT role, summary FROM notes WHERE image=? AND function_sw=?",
                (self.image, entry),
            ).fetchone()
        except sqlite3.Error:
            return None
        return row

    def label(self, entry: int) -> str:
        row = self.db.execute(
            "SELECT name, label FROM functions WHERE entry_sw=?", (entry,)
        ).fetchone()
        n = self.note(entry)
        extra = ""
        if n:
            extra = " {%s}" % n[0]
        elif row and row[1]:
            extra = " (%s)" % row[1]
        names = [
            name
            for name, pc, _mark, _doc in self.anchors.get(entry, [])
            if pc == entry and ".fn." in name
        ]
        alias = "  [" + ", ".join(names) + "]" if names else ""
        return "%06x %s%s%s" % (
            entry,
            row[0] if row else "FUN_%06x" % entry,
            extra,
            alias,
        )

    def callers(self, entry: int) -> list[tuple[int | None, int, str]]:
        return self.db.execute(
            "SELECT from_function, from_sw, kind FROM edges WHERE (to_sw=? OR to_function=?) "
            "AND kind IN ('call','jump','indirect') AND IFNULL(from_function,0)<>? ORDER BY 1, 2",
            (entry, entry, entry),
        ).fetchall()

    def callees(self, entry: int) -> list[tuple[int, str]]:
        return self.db.execute(
            "SELECT DISTINCT to_function, kind FROM edges WHERE from_function=? AND kind='call' "
            "AND to_function IS NOT NULL ORDER BY 1",
            (entry,),
        ).fetchall()

    def graph(self) -> Graph:
        if self._graph is None:
            self._graph, self.graph_state = Graph.load(
                "sharc-" + self.image,
                db_key(self.db, self.path),
                self.db.execute(
                    "SELECT DISTINCT from_function, to_function FROM edges WHERE kind='call' "
                    "AND from_function IS NOT NULL AND to_function IS NOT NULL"
                ),
            )
        return self._graph


def _sha256_file(path: str) -> str:
    digest = hashlib.sha256()
    try:
        with open(path, "rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as exc:
        raise FwError(f"cannot read provenance input {path}: {exc}") from exc
    return digest.hexdigest()


def cmd_stores(
    img: Sharc, roots: list[str], expected_sha: str | None, blob: str | None
) -> dict[str, Any]:
    """One read-only transaction; file identity is not a SQLite snapshot hash."""
    img.db.execute("BEGIN")
    try:
        return _store_inventory(img, roots, expected_sha, blob)
    finally:
        img.db.rollback()


def _store_inventory(
    img: Sharc, roots: list[str], expected_sha: str | None, blob: str | None
) -> dict[str, Any]:
    """Inventory recorded operands, not runtime addresses, extents or ownership."""
    import networkx as nx

    from tools.sharc_dev.sharcdb.schema import DB_VERSION

    required = {
        "meta": "image key value",
        "functions": "image entry_sw end_sw name",
        "edges": "image from_sw to_sw kind delayed cond from_function to_function note",
        "insn": "image sw form fields confidence function_sw raw",
        "mem_access": "image sw space direction base_reg modifier u form width abs_address",
        "ptr": "image sw base_reg base_value address direction width",
    }
    for table, columns in required.items():
        found = {r[1] for r in img.db.execute(f"PRAGMA table_info({table})")}
        if not set(columns.split()) <= found:
            raise FwError(
                f"stores: missing/stale schema for {table}; no rebuild attempted"
            )
    meta = dict(
        img.db.execute(
            "SELECT key,value FROM meta WHERE image=? ORDER BY key", (img.image,)
        )
    )
    if any(
        not meta.get(k)
        for k in ("image", "cpu", "db_version", "image_sha256", "blob_path")
    ):
        raise FwError(
            "stores: missing per-image provenance metadata; no rebuild attempted"
        )
    if (
        meta["image"] != img.image
        or meta["cpu"] != "sharc"
        or meta["db_version"] != str(DB_VERSION)
    ):
        raise FwError(
            "stores: stale/mismatched image, CPU or schema version; no rebuild attempted"
        )
    sha = meta["image_sha256"]
    if not re.fullmatch(r"[0-9a-f]{64}", sha):
        raise FwError("stores: malformed image SHA-256 metadata")
    if expected_sha is not None and expected_sha.lower() != sha:
        raise FwError("stores: expected image SHA-256 differs from database metadata")
    selected_blob = os.path.abspath(blob) if blob else None
    if selected_blob is not None and _sha256_file(selected_blob) != sha:
        raise FwError(
            "stores: image bytes differ from database SHA-256; no rebuild attempted"
        )

    def rows(table: str) -> list[dict[str, Any]]:
        cursor = img.db.execute(
            f"SELECT rowid AS evidence_rowid,* FROM {table} WHERE image=? ORDER BY rowid",
            (img.image,),
        )
        names = [c[0] for c in cursor.description]
        return [dict(zip(names, r, strict=True)) for r in cursor]

    functions = sorted(rows("functions"), key=lambda r: r["entry_sw"])
    entries = {f["entry_sw"] for f in functions}
    owner_cache: dict[int, list[int]] = {}

    def owners(pc: int | None) -> list[int]:
        if pc is None:
            return []
        if pc not in owner_cache:
            owner_cache[pc] = [
                f["entry_sw"] for f in functions if f["entry_sw"] <= pc < f["end_sw"]
            ]
        return owner_cache[pc]

    root_rows = []
    for token in roots:
        pc = parse_addr(token)
        if pc is None:
            raise FwError(f"stores: root must be an explicit PC, not {token!r}")
        containing = owners(pc)
        if len(containing) != 1:
            raise FwError(
                f"stores: root 0x{pc:x} has {len(containing)} containing functions"
            )
        root_rows.append(
            {
                "requested_pc": pc,
                "containing_function": containing[0],
                "is_function_entry": pc == containing[0],
            }
        )
    root_rows.sort(key=lambda r: (r["requested_pc"], r["containing_function"]))
    edges = rows("edges")
    graph = nx.DiGraph()
    graph.add_nodes_from(entries)
    for edge in edges:
        edge["source_containing_functions"] = owners(edge["from_sw"])
        edge["target_containing_functions"] = owners(edge["to_sw"])
        edge["target_is_recorded_entry"] = (
            edge["to_sw"] in entries if edge["to_sw"] is not None else None
        )
        eligible = (
            edge["kind"] == "call"
            and edge["from_function"] in entries
            and edge["to_function"] in entries
            and edge["source_containing_functions"] == [edge["from_function"]]
            and edge["target_containing_functions"] == [edge["to_function"]]
        )
        edge["followed_by_policy"] = eligible
        if eligible:
            graph.add_edge(edge["from_function"], edge["to_function"])
    reachable: set[int] = set()
    for root in root_rows:
        entry = root["containing_function"]
        reachable.add(entry)
        reachable.update(nx.descendants(graph, entry))
    selected_edges = []
    for edge in edges:
        if (
            not ({edge["from_function"]} | set(edge["source_containing_functions"]))
            & reachable
        ):
            continue
        if edge["followed_by_policy"]:
            reason = None
        elif edge["source_containing_functions"] != [edge["from_function"]]:
            reason = "source_function_metadata_missing_or_mismatched"
        elif edge["kind"] == "call":
            reason = "unresolved_or_mismatched_call_target"
        elif edge["kind"] == "indirect":
            reason = "unresolved_indirect_target"
        elif edge["kind"] == "return":
            reason = "return_not_followed"
        elif edge["to_function"] not in (None, edge["from_function"]):
            reason = "non_call_cross_function_edge"
        else:
            reason = "non_call_edge_not_followed"
        edge["not_followed_reason"] = reason
        selected_edges.append(edge)
    selected_edges.sort(key=lambda r: (r["from_sw"], r["evidence_rowid"]))

    instructions = {r["sw"]: r for r in rows("insn")}
    pointers: dict[int, list[dict[str, Any]]] = {}
    for pointer in rows("ptr"):
        pointers.setdefault(pointer["sw"], []).append(pointer)
    all_stores = [r for r in rows("mem_access") if r["direction"] == "store"]
    stores: list[dict[str, Any]] = []
    for mem in sorted(all_stores, key=lambda r: (r["sw"], r["evidence_rowid"])):
        insn = instructions.get(mem["sw"])
        scope = set(owners(mem["sw"]))
        if insn and insn["function_sw"] in entries:
            scope.add(insn["function_sw"])
        if not scope & reachable:
            continue
        site_ptr = pointers.get(mem["sw"], [])
        compatible = [
            p
            for p in site_ptr
            if p["direction"] == "store"
            and p["base_reg"] == mem["base_reg"]
            and p["width"] == mem["width"]
        ]
        competing = [
            m
            for m in all_stores
            if m["sw"] == mem["sw"]
            and m["base_reg"] == mem["base_reg"]
            and m["width"] == mem["width"]
        ]
        unknown = []
        if len(scope) != 1 or (insn and insn["function_sw"] not in owners(mem["sw"])):
            unknown.append("function_ownership_metadata_ambiguous")
        if not insn:
            unknown.append("instruction_evidence_missing")
        matched = None
        if mem["abs_address"] is not None:
            resolution = "database_recorded_absolute_operand"
        elif len(compatible) > 1 or (compatible and len(competing) > 1):
            resolution = "unknown"
            unknown.append("ambiguous_pointer_to_operand_matching")
        elif len(compatible) == 1:
            matched = compatible[0]
            resolution = (
                "static_first_arrival_estimate"
                if matched["address"] is not None
                else "base_only_estimate"
            )
            if matched["address"] is None:
                unknown.append("effective_address_unresolved")
        else:
            resolution = "unknown"
            unknown.append(
                "no_compatible_pointer_evidence"
                if not site_ptr
                else "pointer_base_or_width_mismatch_possible_bank_metadata_gap"
            )
        labels = []
        if mem["base_reg"] in ("I6", "I7"):
            labels.append("stack_register_candidate")
        if mem["space"] == "PM":
            labels.append("program_memory_operand")
        if mem["form"] == "dual":
            labels.append("dual_memory_operand")
        stores.append(
            {
                "operand_id": len(stores),
                "function_candidates": sorted(scope),
                "mem_access": mem,
                "instruction": insn,
                "ptr_site_evidence": site_ptr,
                "compatible_ptr_rowids": [p["evidence_rowid"] for p in compatible],
                "matched_ptr_rowid": matched["evidence_rowid"] if matched else None,
                "address_estimate": matched["address"] if matched else None,
                "resolution": resolution,
                "confidence": {
                    "instruction": insn["confidence"] if insn else None,
                    "address": "limited_static_estimate_not_bounds"
                    if matched
                    else resolution,
                },
                "unknown_reasons": unknown,
                "candidate_labels": labels,
            }
        )
    schema = list(
        img.db.execute(
            "SELECT type,name,sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY type,name"
        )
    )
    return {
        "report_version": 1,
        "provenance": {
            "image": img.image,
            "meta": meta,
            "database_path": os.path.abspath(img.path),
            "database_file_sha256": _sha256_file(img.path),
            "database_file_hash_scope": "main file identity only, not transaction/WAL snapshot or concurrent replacement proof",
            "journal_mode": img.db.execute("PRAGMA journal_mode").fetchone()[0],
            "schema_sha256": hashlib.sha256(
                json.dumps(schema, separators=(",", ":")).encode()
            ).hexdigest(),
            "selected_blob_path": selected_blob,
            "image_verification": "bytes_verified"
            if selected_blob
            else "metadata_only_not_bytes_verified",
            "verified_image_sha256": sha if selected_blob else None,
            "expected_image_sha256": expected_sha,
        },
        "policy": {
            "edges": "recorded CALL only with unambiguous containing target; whole containing functions, including interior targets",
            "roots": "requested PC is retained; traversal starts at its containing function",
            "pointer_evidence": "limited constant propagation; loop first arrival, not runtime bounds; stack labels are candidates, not exclusions",
            "footprint": "raw width tokens only; no byte extent, SIMD or alias inference",
            "coverage": "recorded functions/operands only; unresolved/non-CALL edges prevent full reachability or ownership claims",
        },
        "roots": root_rows,
        "reachable_functions": [f for f in functions if f["entry_sw"] in reachable],
        "edges": selected_edges,
        "stores": stores,
        "counts": {
            "database_store_operands": len(all_stores),
            "reachable_functions": len(reachable),
            "reachable_store_operands": len(stores),
            "reachable_store_sites": len({r["mem_access"]["sw"] for r in stores}),
            "other_store_operands": len(all_stores) - len(stores),
            "edge_rows": len(selected_edges),
            "followed_call_edge_rows": sum(
                e["followed_by_policy"] for e in selected_edges
            ),
            "unresolved_call_edge_rows": sum(
                e["kind"] == "call" and not e["followed_by_policy"]
                for e in selected_edges
            ),
            "unresolved_indirect_edge_rows": sum(
                e["kind"] == "indirect" for e in selected_edges
            ),
            "not_followed_edge_rows": sum(
                not e["followed_by_policy"] for e in selected_edges
            ),
            "stack_candidate_operands": sum(
                "stack_register_candidate" in r["candidate_labels"] for r in stores
            ),
            "pm_operands": sum(r["mem_access"]["space"] == "PM" for r in stores),
            "dual_operands": sum(
                "dual_memory_operand" in r["candidate_labels"] for r in stores
            ),
        },
    }


def cmd_fn(img: Sharc, token: str) -> list[str]:
    entry = img.resolve(token)
    f = img.func_row(entry)
    assert f is not None
    out = [
        "%s  insns=%d end=%06x block=%s %s%s"
        % (
            img.label(entry),
            f[2],
            f[1],
            f[3],
            "leaf" if f[7] else "non-leaf",
            " " + f[6] if f[6] else "",
        )
    ]
    n = img.note(entry)
    if n:
        out.append("note: %s: %s" % (n[0], clip(n[1] or "", 140)))
    callers = img.db.execute(
        "SELECT DISTINCT from_function FROM edges WHERE to_function=? AND from_function IS NOT NULL "
        "AND from_function<>? AND kind IN ('call','jump','indirect')",
        (entry, entry),
    ).fetchall()
    callees = img.db.execute(
        "SELECT DISTINCT to_function FROM edges WHERE from_function=? AND kind='call' AND to_function IS NOT NULL",
        (entry,),
    ).fetchall()
    out.append("callers: %d" % len(callers))
    out.extend("  <- " + img.label(c[0]) for c in callers[:5])
    out.append("callees: %d" % len(callees))
    out.extend("  -> " + img.label(c[0]) for c in callees[:6])
    lit = img.db.execute(
        "SELECT sw, value, form FROM literals WHERE sw>=? AND sw<? ORDER BY sw LIMIT 6",
        (entry, f[1]),
    ).fetchall()
    if lit:
        out.append("literals: " + ", ".join("%x:%#x" % (a, b) for a, b, _ in lit))
    out.extend(sharc_disasm(img, entry, 8)[:9])
    return out


def _cost_pairs(text: str | None, opt: str) -> list[tuple[str, int]]:
    pairs: list[tuple[str, int]] = []
    for part in (text or "").split(","):
        if not part.strip():
            continue
        name, sep, val = part.partition("=")
        try:
            if not sep or not name.strip():
                raise ValueError(part)
            pairs.append((name.strip(), int(val, 0)))
        except ValueError:
            raise FwError("bad %s entry %r (want NAME=INT)" % (opt, part)) from None
    return pairs


def cmd_cost(
    img: Sharc,
    token: str,
    vars_text: str | None = None,
    follow_calls: bool = True,
    vars_all_text: str | None = None,
) -> list[str]:
    from tools.sharc_dev.sharc_cost import fn_cost, split_sym

    pairs = _cost_pairs(vars_text, "--vars")
    pairs_all = _cost_pairs(vars_all_text, "--vars-all")
    entry = img.resolve(token)
    c = fn_cost(img.db, img.image, entry, follow_calls=follow_calls)
    present = sorted(c.total.symbols() | c.self_cost.symbols())
    values: dict[str, int] = {}
    for name, val in pairs:
        sym = name if "@" in name else "%s@%x" % (name, entry)
        if "@" in name:
            base, _, fn = name.rpartition("@")
            try:
                sym = "%s@%x" % (base, int(fn, 16))
            except ValueError:
                raise FwError(
                    "bad --vars function in %r (want NAME@HEXADDR)" % name
                ) from None
        if sym not in present:
            raise FwError(
                "--vars %s matches no symbol; symbols present: %s"
                % (name, " ".join(present) or "(none)")
            )
        values[sym] = val
    for name, val in pairs_all:
        hit = [x for x in present if split_sym(x)[0] == name]
        if not hit:
            raise FwError(
                "--vars-all %s matches no symbol; symbols present: %s"
                % (name, " ".join(present) or "(none)")
            )
        for x in hit:
            values[x] = val
    c_max = fn_cost(img.db, img.image, entry, follow_calls=follow_calls, values=values)
    out = ["%s insns %d self %s" % (img.label(entry), c.n_insns, c.self_cost)]
    for lp in c.loops:
        out.append(
            "  loop 0x%x x %s body %d depth %d"
            % (lp.do_sw, lp.trip, lp.body_insns, lp.depth)
        )
    out.append("  dm loads %s stores %s" % (c.dm_loads, c.dm_stores))
    for (callee, mult, tail), sub in zip(c.calls, c.call_totals, strict=True):
        out.append(
            "  -> 0x%x x %s  total %s%s"
            % (
                callee,
                mult,
                "cut" if sub is None else sub,
                " (tail alternative)" if tail else "",
            )
        )
    if c.unknown_calls:
        out.append("  -> indirect: %d" % c.unknown_calls)
    out.extend("  note: " + n for n in c.notes)
    fns = {split_sym(x)[1] for x in present}
    if len(fns) > 1:
        out.append(
            "  note: total has symbols of %d functions (NAME@FN is one function's"
            " register; --vars-all NAME=V sets it in all)" % len(fns)
        )
    out.append("total %s" % c.total)
    if values:
        shown = ",".join("%s=%d" % kv for kv in values.items())
        out.append("with %s: %s" % (shown, c.total.evaluate(values)))
        if c_max.total_max is not None:
            out.append("with tail targets by max (tighter bound): %s" % c_max.total_max)
    return out


def sharc_disasm(img: Sharc, entry: int, n: int | None = None) -> list[str]:
    f = img.func_row(entry)
    if f is None:
        raise FwError("no SHARC function at 0x%x" % entry)
    rows = img.db.execute(
        "SELECT sw, mnemonic FROM insn WHERE aligned=1 AND sw>=? AND sw<? ORDER BY sw",
        (entry, f[1]),
    ).fetchall()
    out = ["  %06x  %s" % (sw, m) for sw, m in rows]
    return out[:n] if n else out


def cmd_disasm(img: Sharc, token: str) -> list[str]:
    entry = img.resolve(token)
    return [img.label(entry)] + sharc_disasm(img, entry)


def cmd_decomp(img: Sharc, token: str) -> list[str]:
    raise FwError(f"no decompiler for SHARC; use: dossier {token} or disasm {token}")


def cmd_xref(img: Sharc, token: str) -> list[str]:
    addr = parse_addr(token)
    if addr is None:
        addr = img.anchor_names.get(token)
    if addr is None:
        addr = img.resolve(token)
    out = ["xref %06x" % addr]
    for fn, site, kind in img.db.execute(
        "SELECT from_function, from_sw, kind FROM edges WHERE to_sw=? AND kind IN ('call','jump','indirect') "
        "ORDER BY from_sw",
        (addr,),
    ):
        out.append(
            "  code %-9s %06x in %s" % (kind, site, img.label(fn) if fn else "?")
        )
    for sw, direction, space in img.db.execute(
        "SELECT sw, direction, space FROM mem_access WHERE image=? AND abs_address=? ORDER BY sw",
        (img.image, addr),
    ):
        f = img.func_row(sw)
        out.append(
            "  data %-9s %06x in %s (%s absolute)"
            % (direction, sw, img.label(f[0]) if f else "?", space)
        )
    for sw, direction, base in img.db.execute(
        "SELECT sw, direction, base_reg FROM ptr WHERE address=? ORDER BY sw LIMIT 200",
        (addr,),
    ):
        f = img.func_row(sw)
        out.append(
            "  data %-9s %06x in %s (%s)"
            % (direction, sw, img.label(f[0]) if f else "?", base)
        )
    for sw, form in img.db.execute(
        "SELECT sw, form FROM literals WHERE value=? ORDER BY sw LIMIT 100", (addr,)
    ):
        f = img.func_row(sw)
        out.append(
            "  literal %s %06x in %s" % (form, sw, img.label(f[0]) if f else "?")
        )
    if len(out) == 1:
        out.append("  no references recorded (indirect targets may be unresolved)")
    return out


def _matches(a: Sharc, b: Sharc, entry: int) -> list[str]:
    matches: dict[int, list[str]] = {}
    for name, sw, _mark, _doc in a.anchors.get(entry, []):
        if sw != entry or ".fn." not in name or name not in b.anchor_names:
            continue
        target = b.anchor_names[name]
        if target != b.resolve(name):
            continue
        matches[target] = ["declared function"]

    hashes = a.db.execute(
        "SELECT exact_hash, reloc_hash FROM func_hash WHERE image=? AND entry_sw=?",
        (a.image, entry),
    ).fetchone()
    for e, exact in b.db.execute(
        "SELECT dst.entry_sw, dst.exact_hash=? FROM func_hash dst WHERE dst.reloc_hash=? "
        "AND dst.image=? ORDER BY 2 DESC, dst.entry_sw",
        (*(hashes or (None, None)), b.image),
    ):
        matches.setdefault(e, []).append("exact hash" if exact else "reloc hash")
    return [
        f"  match ({', '.join(evidence)}) {b.label(addr)}"
        for addr, evidence in matches.items()
    ]


def cmd_port(a: Sharc, b: Sharc, name: str) -> list[str]:
    entry = a.resolve(name)
    return [f"{a.label(entry)}  ({a.image}) -> {b.image}"] + (
        _matches(a, b, entry) or ["  no match"]
    )


def cmd_dossier(
    img: Sharc, token: str, other: Sharc | None, n_callers: int, listing_n: int
) -> list[str]:
    from tools.firmware.catalog import suggest_rule
    from tools.sharc_dev.sharc import Image

    entry = img.resolve(token)
    out = [f"dossier {img.image} {img.label(entry)}"]
    anchors = img.anchors.get(entry, [])
    if anchors:
        out.append("anchors:")
        for name, sw, mark, doc in anchors:
            out.append(
                f"  {name} [{mark}]+0x{sw - entry:x}"
                + (f"  {clip(doc, 130)}" if doc else "")
            )
    row = img.db.execute(
        "SELECT reloc_hash FROM func_hash WHERE image=? AND entry_sw=?",
        (img.image, entry),
    ).fetchone()
    if row and row[0]:
        funcs = [
            sw
            for (sw,) in img.db.execute(
                "SELECT entry_sw FROM func_hash WHERE image=? AND reloc_hash=? ORDER BY entry_sw",
                (img.image, row[0]),
            )
        ]
        kind, ordinal = suggest_rule(entry, funcs, False, "sharc_func")
        suffix = f" ordinal={ordinal}" if ordinal is not None else ""
        out.append(f"reloc hash: {row[0][:12]} ({len(funcs)} with this hash)")
        out.append(f"rule (computed): {kind}{suffix}")
    if other is not None:
        out.append(f"pair on {other.image}:")
        out.extend(_matches(img, other, entry) or ["  no match"])

    with closing(
        Image(img.image, img.path, img.meta.get("blob_path"), read_only=True)
    ) as analysis:
        out.extend(
            analysis.card(
                entry, listing_n=max(0, listing_n), caller_limit=max(0, n_callers)
            ).splitlines()
        )
    return out


def cmd_atlas(
    img: Sharc,
    term: str,
    per: int = 15,
    *,
    names: bool = True,
    notes: bool = True,
    atlas: bool = True,
) -> list[str]:
    rows = []
    for (
        entry,
        _end,
        name,
        label,
        anchors,
        role,
        summary,
        confidence,
    ) in img.function_index():
        matched_name = names and matches_term(term, name, label, anchors)
        matched_note = notes and matches_term(term, role, summary)
        inferred_role = img.atlas_roles.get(entry, "") if atlas else ""
        docs = [a[3] for a in img.anchors.get(entry, [])] if atlas else []
        if (
            matched_name
            or matched_note
            or (atlas and matches_term(term, inferred_role, *docs))
        ):
            anchor_hit = names and bool(anchors) and matches_term(term, anchors)
            rows.append(
                (not anchor_hit, entry, summary, confidence, anchors, inferred_role)
            )
    out = [f"functions: {len(rows)}"]
    for _, entry, summary, confidence, anchors, inferred_role in sorted(rows)[
        : per or None
    ]:
        role_suffix = f"  (atlas role: {inferred_role})" if inferred_role else ""
        out.append("  " + img.label(entry) + role_suffix)
        if anchors:
            out.append("    anchors: " + anchors)
        if summary:
            out.append(f"    note [{confidence or '?'}]: {clip(summary, 110)}")
    return out


def _strings(img: Sharc, term: str, limit: int) -> list[str]:
    if not _has(img.db, "strings"):
        raise FwError(
            "DSP strings require a current database; rebuild with tools.sharc_dev.sharcdb"
        )
    return [
        "  byte=0x%x block=%d %r" % (addr, block, clip(text, 90))
        for addr, block, text in img.db.execute(
            "SELECT byte_addr, block, text FROM strings WHERE image=? "
            "AND instr(lower(text), lower(?)) ORDER BY byte_addr LIMIT ?",
            (img.image, term, limit),
        )
    ]


def cmd_str(img: Sharc, term: str) -> list[str]:
    return ["DSP printable runs (candidates):"] + _strings(img, term, 200)


def cmd_search(img: Sharc, term: str, where: str | None, cap: int) -> list[str]:
    parts = set((where or "names,notes,atlas").split(","))
    invalid = parts - {"names", "notes", "atlas", "strings", "disasm", "decomp"}
    if invalid:
        raise FwError("unknown SHARC search sources: " + ", ".join(sorted(invalid)))
    per = max(6, cap // 4) if cap else 0
    out = []
    if parts & {"names", "notes", "atlas"}:
        out.extend(
            cmd_atlas(
                img,
                term,
                per,
                names=bool(parts & {"names", "atlas"}),
                notes=bool(parts & {"notes", "atlas"}),
                atlas="atlas" in parts,
            )
        )
    if "disasm" in parts:
        out.append("disassembly:")
        out.extend(
            f"  {sw:06x} {mnemonic}"
            for sw, mnemonic in img.db.execute(
                "SELECT sw, mnemonic FROM insn WHERE image=? AND aligned=1 "
                "AND instr(lower(mnemonic), lower(?)) ORDER BY sw LIMIT ?",
                (img.image, term, per or -1),
            )
        )
    if "strings" in parts:
        out.extend(
            ["DSP printable runs (candidates):", *_strings(img, term, per or -1)]
        )
    if "decomp" in parts:
        out.append("SHARC decompiled C unavailable; search with --in disasm")
    return out
