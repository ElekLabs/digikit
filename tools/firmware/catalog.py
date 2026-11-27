"""catalog.py: the CPU-independent part of the anchor catalogue.

The ColdFire collector (tools/coldfire/atlas.py) and the SHARC collector
(tools/sharc_dev/sharc_catalog.py) build rows of candidate anchor functions.
This module holds what they share: rule suggestion, grade, call-graph groups
and the sqlite, TSV and JSON writers. Rows are dicts keyed by column name.

It is also the way from a firmware function to a checked declaration in ports/fw/:

    uv run python -m tools.firmware.catalog emit-rule IMAGE ADDR [--len N] [--other IMAGE]
    uv run python -m tools.firmware.catalog declare IMAGE ADDR NAME [--table T] [--other IMAGE] [--doc TEXT] [--write]
"""

from __future__ import annotations

import argparse
import collections
import json
import re
import sqlite3
import sys
import tomllib
from collections.abc import Iterable, Sequence
from contextlib import closing
from pathlib import Path
from typing import Any

from tools.firmware.artifacts import FirmwareArtifacts

# Rule kinds that find a function by its hash.
FUNC_KINDS = ("cf_func", "sharc_func")
# Pair methods that count as a reliable match between the two devices.
STRONG_PAIR = frozenset({"strict", "loose", "strings", "vtable"})
MAX_SAME_HASH = (
    4  # up to this many functions with one hash, the address order picks one
)
# Louvain resolution: 2.0 gives about 50 groups of 10+ rows on DT2 1.17 (1.0 gives 23, with two of 600+ rows).
GROUP_RESOLUTION = 2.0

GROUP_COLUMNS = (
    ("id", "INTEGER"),
    ("size", "INTEGER"),
    ("anchored", "INTEGER"),
    ("grade", "INTEGER"),
    ("top_classes", "TEXT"),
    ("top_strings", "TEXT"),
    ("top_tags", "TEXT"),
    ("best", "INTEGER"),
)


def suggest_rule(
    entry: int,
    same_hash_entries_sorted: Sequence[int],
    is_vfunc: bool,
    func_kind: str = "cf_func",
) -> tuple[str, int | None]:
    """Rule kind and ordinal that would find `entry` again on another image."""
    n = len(same_hash_entries_sorted)
    if n <= 1:
        return (func_kind, None)
    if n <= MAX_SAME_HASH and entry in same_hash_entries_sorted:
        return (func_kind, list(same_hash_entries_sorted).index(entry))
    if n > MAX_SAME_HASH and is_vfunc:
        return ("cf_vtable_slot", None)
    return ("none", None)


def catalog_grade(
    rule_kind: str, stable: int | None, pair_method: str | None, rule_both: int | None
) -> int:
    """1 when the function can be an anchor on both devices by hash."""
    return int(
        rule_kind in FUNC_KINDS
        and stable == 1
        and pair_method in STRONG_PAIR
        and rule_both == 1
    )


def number_groups(communities: Iterable[Iterable[int]]) -> dict[int, int]:
    """Address -> group id. Biggest community first, then lowest address; singletons get -1."""
    sets = sorted((sorted(c) for c in communities), key=lambda c: (-len(c), c[0]))
    out: dict[int, int] = {}
    gid = 0
    for members in sets:
        if len(members) < 2:
            ids = -1
        else:
            ids = gid
            gid += 1
        for a in members:
            out[a] = ids
    return out


def count_list(counts: dict[str, int], limit: int) -> list[tuple[str, int]]:
    return sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[:limit]


def group_edges(
    addrs: Iterable[int], callee_n: dict[int, collections.Counter[int]]
) -> dict[tuple[int, int], int]:
    """Undirected edges (low, high) -> call sites in either direction, between `addrs`."""
    nodes = set(addrs)
    out: dict[tuple[int, int], int] = {}
    for f in nodes:
        for t, n in callee_n.get(f, {}).items():
            if t in nodes and t != f:
                key = (f, t) if f < t else (t, f)
                out[key] = out.get(key, 0) + n
    return out


def louvain_groups(
    addrs: Iterable[int],
    edges: dict[tuple[int, int], int],
    resolution: float = GROUP_RESOLUTION,
) -> dict[int, int]:
    import networkx as nx

    g = nx.Graph()
    g.add_nodes_from(sorted(addrs))
    g.add_weighted_edges_from((a, b, w) for (a, b), w in sorted(edges.items()))
    comms = nx.community.louvain_communities(
        g, weight="weight", resolution=resolution, seed=0
    )
    return number_groups(comms)


def parse_counts(text: str) -> list[tuple[str, int]]:
    # Class names can hold commas (template arguments), so match "name:count" up to the next "," or the end.
    return [(k, int(n)) for k, n in re.findall(r"(.+?):(\d+)(?:,|$)", text)]


def catalog_group_rows(
    rows: list[dict[str, Any]], class_col: str = "class", hint_col: str = "hint"
) -> list[dict[str, Any]]:
    """One row per group (id >= 0) with a summary of its members.

    `class_col` and `hint_col` name the row fields counted as top_classes and top_strings.
    """
    members: dict[int, list[dict[str, Any]]] = {}
    for r in rows:
        if r["group"] >= 0:
            members.setdefault(r["group"], []).append(r)
    out = []
    for gid in sorted(members):
        ms = members[gid]
        classes: dict[str, int] = {}
        hints: dict[str, int] = {}
        tags: dict[str, int] = {}
        for r in ms:
            if r[class_col]:
                classes[r[class_col]] = classes.get(r[class_col], 0) + 1
            for c, n in parse_counts(r.get("caller_classes") or ""):
                classes[c] = classes.get(c, 0) + n
            if r[hint_col]:
                hints[r[hint_col]] = hints.get(r[hint_col], 0) + 1
            for t in r["tags"].split(","):
                if t:
                    k = t.split(":")[0]
                    tags[k] = tags.get(k, 0) + 1
        best = min(ms, key=lambda r: (-r["score"], r["addr"]))
        out.append(
            {
                "id": gid,
                "size": len(ms),
                "anchored": sum(1 for r in ms if r["anchors"]),
                "grade": sum(1 for r in ms if r["anchor_grade"] == 1),
                "top_classes": ",".join("%s:%d" % kv for kv in count_list(classes, 5)),
                "top_strings": " | ".join(k for k, _n in count_list(hints, 3)),
                "top_tags": ",".join("%s:%d" % kv for kv in count_list(tags, 5)),
                "best": best["addr"],
            }
        )
    return out


def _write_table(
    target: str | sqlite3.Connection,
    table: str,
    columns: Sequence[tuple[str, str]],
    rows: list[dict[str, Any]],
) -> None:
    """Replace a generated table, joining the caller's transaction if supplied."""
    if isinstance(target, str):
        with closing(sqlite3.connect(target)) as con, con:
            con.execute("BEGIN")
            _write_table(con, table, columns, rows)
        return
    target.execute("DROP TABLE IF EXISTS %s" % table)
    target.execute(
        "CREATE TABLE %s (%s)" % (table, ", ".join('"%s" %s' % c for c in columns))
    )
    quoted = ",".join('"%s"' % c for c, _t in columns)
    target.executemany(
        "INSERT INTO %s(%s) VALUES (%s)"
        % (table, quoted, ",".join("?" * len(columns))),
        (tuple(r[c] for c, _t in columns) for r in rows),
    )
    if table == "catalog":
        target.execute("CREATE INDEX IF NOT EXISTS catalog_score ON catalog(score)")


def write_catalog(
    db_path: str | sqlite3.Connection,
    tsv_path: str,
    rows: list[dict[str, Any]],
    columns: Sequence[tuple[str, str]],
    addr_fmt: str = "0x%08x",
) -> None:
    """Write table `catalog` and the catalog TSV, best score first."""
    _write_table(db_path, "catalog", columns, rows)
    rows = sorted(rows, key=lambda r: (-r["score"], r["addr"]))
    with open(tsv_path, "w") as fh:
        fh.write("\t".join(c for c, _t in columns) + "\n")
        for r in rows:
            cells = []
            for c, _t in columns:
                v = r[c]
                if v is None:
                    cells.append("")
                elif c in ("addr", "pair_addr"):
                    cells.append(addr_fmt % v)
                elif c == "score":
                    cells.append("%.2f" % v)
                else:
                    cells.append(str(v))
            fh.write("\t".join(cells) + "\n")


def write_catalog_groups(
    image: str,
    rows: list[dict[str, Any]],
    edges: dict[tuple[int, int], int],
    db_path: str | sqlite3.Connection,
    tsv_path: str,
    graph_path: str,
    addr_fmt: str = "0x%08x",
    class_col: str = "class",
    hint_col: str = "hint",
) -> int:
    """Write table catalog_groups, the groups TSV and the graph JSON; return the group count."""
    groups = catalog_group_rows(rows, class_col, hint_col)
    _write_table(db_path, "catalog_groups", GROUP_COLUMNS, groups)
    with open(tsv_path, "w") as fh:
        fh.write("\t".join(c for c, _t in GROUP_COLUMNS) + "\n")
        for g in groups:
            fh.write(
                "\t".join(
                    addr_fmt % g[c] if c == "best" else str(g[c])
                    for c, _t in GROUP_COLUMNS
                )
                + "\n"
            )
    graph = {
        "image": image,
        "nodes": [
            {
                "addr": addr_fmt % r["addr"],
                "name": r["name"],
                "work_name": r.get("work_name", ""),
                "class": r[class_col],
                "group": r["group"],
                "score": round(r["score"], 2),
                "anchors": r["anchors"],
            }
            for r in sorted(rows, key=lambda r: r["addr"])
        ],
        "edges": [
            [addr_fmt % a, addr_fmt % b, w] for (a, b), w in sorted(edges.items())
        ],
    }
    with open(graph_path, "w") as fh:
        json.dump(graph, fh, indent=1)
        fh.write("\n")
    return len(groups)


# --- from a firmware function to a declaration (ports/fw/) -------------------------------------

SHARC_LIMIT = (
    0x1000000  # a SHARC pc is below this; ColdFire code is at 0x40000000 and up
)
DEFAULT_LEN = {"cf": 4, "sharc": 6}
NAME_CPU = {
    "cf": "cf",
    "sharc": "dsp",
}  # the cpu of an entry -> the first part of its name


def cpu_of(addr: int, override: str | None = None) -> str:
    return override or ("sharc" if addr < SHARC_LIMIT else "cf")


def device_of(image: str) -> str:
    return image.split("-")[0]


def rule_at(image: str, addr: int, length: int, cpu: str, root: Path) -> dict:
    """The rule for the LENGTH bytes at ADDR on the stock image IMAGE."""
    from tools.patchkit import port_resolve

    if cpu == "cf":
        return port_resolve.cf_rule_at(port_resolve.load_cf(image, root), addr, length)
    return port_resolve.sharc_rule_at(port_resolve.load_dsp(image, root), addr, length)


def catalog_pair(image: str, cpu: str, entry: int, root: Path) -> int | None:
    """The function on the other device that the atlas catalogue pairs with the function at ENTRY, or None."""
    out = root / "out"
    paths = [FirmwareArtifacts(image, cpu, out).database_path()]
    if cpu == "cf":
        paths.append(out / "atlas" / f"{image}.sqlite")
    for db in paths:
        if not db.is_file():
            continue
        with closing(
            sqlite3.connect(db.resolve().as_uri() + "?mode=ro", uri=True)
        ) as con:
            exists = con.execute(
                "SELECT 1 FROM sqlite_schema WHERE name='catalog'"
            ).fetchone()
            if exists:
                row = con.execute(
                    "SELECT pair_addr FROM catalog WHERE addr=?", (entry,)
                ).fetchone()
                return row[0] if row and row[0] is not None else None
    return None


def analyse(
    image: str, addr: int, length: int, cpu: str, other: str | None, root: Path
) -> dict:
    """The rule for ADDR on IMAGE (checked to resolve to ADDR) and, with OTHER, where it resolves there
    (`other`, or `other_error`), the catalogue pair of ADDR (`pair`: the pair of the function entry plus the same
    offset) and `same`: the rule finds the pair."""
    from tools.patchkit import port_resolve

    rule = rule_at(image, addr, length, cpu, root)
    got = port_resolve.check_rule(image, rule, cpu, root)
    if got != addr:
        raise port_resolve.ResolveError(
            f"the rule resolves to {got:#x} on {image}, not {addr:#x}"
        )
    out: dict = {"rule": rule, "other": None, "other_error": None, "pair": None}
    if other:
        pair = catalog_pair(image, cpu, addr - rule["offset"], root)
        out["pair"] = None if pair is None else pair + rule["offset"]
        try:
            out["other"] = port_resolve.check_rule(other, rule, cpu, root)
        except port_resolve.ResolveError as e:
            out["other_error"] = str(e)
    out["same"] = out["pair"] is not None and out["other"] == out["pair"]
    return out


def render_entry(
    name: str,
    table: str,
    cpu: str,
    rules_by_device: dict[str, dict],
    doc: str | None,
    mark: str,
) -> str:
    """The text of one declaration. One rule shared by every device is written as `rule`; different rules as
    `rule.<dev>` lines; an entry for one device only gets a `devices` line."""
    from tools.patchkit import port_resolve

    lines = [f"# {line}" for line in (doc or "").splitlines()]
    lines += [f"[[{table}]]", f'name = "{name}"', f'cpu = "{cpu}"']
    devices = list(rules_by_device)
    if len(devices) == 1:
        lines.append(f'devices = ["{devices[0]}"]')
    rules = list(rules_by_device.values())
    if all(r == rules[0] for r in rules):
        lines.append(f"rule = {port_resolve.format_rule(rules[0])}")
    else:
        for dev in sorted(devices):
            lines.append(
                f"rule.{dev} = {port_resolve.format_rule(rules_by_device[dev])}"
            )
    lines.append(f'mark = "{mark}"')
    return "\n".join(lines) + "\n"


def entry_path(root: Path, name: str) -> Path:
    cpu, subsystem = name.split(".")[:2]
    fw = root / "ports" / "fw"
    return (fw / "dsp" if cpu == "dsp" else fw) / f"{subsystem}.toml"


def split_blocks(text: str) -> list[tuple[int, str]]:
    """(start offset, text) of each blank-line separated block of TEXT."""
    blocks: list[tuple[int, str]] = []
    cur: list[str] = []
    start = pos = 0
    for line in text.splitlines(keepends=True):
        if line.strip():
            if not cur:
                start = pos
            cur.append(line)
        elif cur:
            blocks.append((start, "".join(cur)))
            cur = []
        pos += len(line)
    if cur:
        blocks.append((start, "".join(cur)))
    return blocks


def insert_entry(text: str, name: str, entry: str) -> str:
    """TEXT with ENTRY added before the first declaration whose name sorts after NAME (at the end if none);
    every existing byte is kept."""
    for start, block in split_blocks(text):
        m = re.search(r'^name = "([^"]+)"', block, re.M)
        if m and m.group(1) > name:
            return text[:start] + entry + "\n" + text[start:]
    return text.rstrip("\n") + "\n\n" + entry


def new_file_text(root: Path, name: str, entry: str) -> str:
    cpu, subsystem = name.split(".")[:2]
    meaning = tomllib.loads((root / "ports" / "fw" / "subsystems.toml").read_text())[
        cpu
    ][subsystem]
    return (
        f"# {subsystem}: {meaning}\n"
        "# Names follow <cpu>.<subsystem>.<kind>.<name>; was = former names, for reading older docs, never resolved "
        "(docs/design/firmware-decls.md).\n\n" + entry
    )


def cmd_emit_rule(a: argparse.Namespace) -> int:
    from tools.patchkit import port_resolve

    cpu = cpu_of(a.addr, a.cpu)
    length = a.len if a.len is not None else DEFAULT_LEN[cpu]
    r = analyse(a.image, a.addr, length, cpu, a.other, a.root)
    print(f"rule = {port_resolve.format_rule(r['rule'])}")
    print(f"# {a.image}: {a.addr:#x} (the rule resolves here)")
    if a.other:
        if r["other_error"]:
            print(f"# {a.other}: does not resolve: {r['other_error']}")
        else:
            print(f"# {a.other}: {r['other']:#x}")
        if r["pair"] is not None:
            print(f"# {a.other}: catalogue pair {r['pair']:#x}")
        else:
            print(f"# {a.other}: no catalogue pair")
        if r["same"]:
            print("# same rule works on both")
    return 0


def cmd_declare(a: argparse.Namespace) -> int:
    from tools.patchkit import port_resolve

    root = a.root
    cpu = cpu_of(a.addr, a.cpu)
    if a.name.split(".")[0] != NAME_CPU[cpu]:
        raise port_resolve.ResolveError(
            f"{a.name}: a {cpu} entry has a name that starts with {NAME_CPU[cpu]}"
        )
    length = a.len if a.len is not None else DEFAULT_LEN[cpu]
    r = analyse(a.image, a.addr, length, cpu, a.other, root)
    rule = r["rule"]
    table = a.table or ("fn" if rule["offset"] == 0 else "site")
    port_resolve.check_name(a.name, table, port_resolve.load_subsystems(root))
    for e in port_resolve.fw_rules(root):
        if a.name == e["name"]:
            raise port_resolve.ResolveError(
                f"{a.name}: already declared ({e['_source']})"
            )
        if a.name in e["was"]:
            raise port_resolve.ResolveError(
                f"{a.name}: a former name of {e['name']} ({e['_source']}); use a new name"
            )
    rules = {device_of(a.image): rule}
    if a.other:
        odev = device_of(a.other)
        if r["same"]:
            rules[odev] = rule
        elif r["pair"] is None:
            print(
                f"no catalogue pair for {a.addr:#x}: declared for {device_of(a.image)} only",
                file=sys.stderr,
            )
        else:
            orule = rule_at(a.other, r["pair"], length, cpu, root)
            got = port_resolve.check_rule(a.other, orule, cpu, root)
            if got != r["pair"]:
                raise port_resolve.ResolveError(
                    f"the rule resolves to {got:#x} on {a.other}, not {r['pair']:#x}"
                )
            rules[odev] = orule
    entry = render_entry(a.name, table, cpu, rules, a.doc, "O")
    if not a.write:
        print(entry, end="")
        return 0
    path = entry_path(root, a.name)
    text = (
        insert_entry(path.read_text(), a.name, entry)
        if path.exists()
        else new_file_text(root, a.name, entry)
    )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    print(f"{path}: added {a.name}")
    return 0


def main(argv: Sequence[str] | None = None) -> int:
    from tools.patchkit import port_resolve

    def addr(s: str) -> int:
        return int(s, 0)

    ap = argparse.ArgumentParser(description=__doc__.strip().splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    for cmd, fn in (("emit-rule", cmd_emit_rule), ("declare", cmd_declare)):
        p = sub.add_parser(cmd)
        p.set_defaults(fn=fn)
        p.add_argument("image", help="stock image, e.g. dt2-1.17")
        p.add_argument("addr", type=addr, help="ColdFire address or SHARC pc")
        if cmd == "declare":
            p.add_argument("name", help="<cpu>.<subsystem>.<kind>.<name>")
            p.add_argument("--table", choices=("fn", "site", "data", "value"))
            p.add_argument("--doc")
            p.add_argument("--write", action="store_true")
        p.add_argument("--len", type=int, help="bytes; default 4 ColdFire, 6 SHARC")
        p.add_argument("--other", help="the image of the other device")
        p.add_argument("--cpu", choices=("cf", "sharc"))
        p.add_argument("--root", type=Path, default=port_resolve.ROOT)
    a = ap.parse_args(argv)
    try:
        return a.fn(a)
    except port_resolve.ResolveError as e:
        print(f"catalog: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
