#!/usr/bin/env python3
"""sharc_catalog.py: candidate anchor functions in out/sharcdb/<image>.sqlite.

    uv run python -m tools.sharc_dev.sharc_catalog dt2-1.17 dn2-1.12
    uv run python -m tools.sharc_dev.sharc_catalog dt2-1.17 dn2-1.12 --profile dt2-1.17=out/prof.txt
    uv run python -m tools.coldfire.fw catalog -i dt2-1.17 --cpu sharc --grade

Builds the full function index and the `catalog` and `catalog_groups` tables
for each image and writes the TSV and graph JSON files next to them. The rows are the functions near an existing
anchor, on the voice, master or frame call path, hot in a profile, or called
from many places. Inputs: out/sharcdb/<image>.sqlite (also of the old image),
ports/<image>.toml. The ColdFire catalogue is tools/coldfire/atlas.py;
the shared part is tools/firmware/catalog.py.
"""

from __future__ import annotations

import argparse
import bisect
import collections
import os
import re
import sqlite3
import sys
from collections.abc import Callable, Sequence
from contextlib import closing
from typing import Any, cast

from tools.firmware import catalog as shared
from tools.firmware.artifacts import FirmwareArtifacts, database_path
from tools.firmware.catalog import (
    STRONG_PAIR,
    catalog_grade,
    group_edges,
    louvain_groups,
    suggest_rule,
)
from tools.firmware.query_db import (
    FUNCTION_NAMES_SQL,
    ROOT,
    SHARC_DIR,
    short_image,
)
from tools.sharc_dev.fw_query import Sharc, port_anchors
from tools.sharc_dev.fw_query import anchor_sw as anchor_sw

# The DSP image did not change 1.16 -> 1.17 or 1.11 -> 1.12 (finding 53), so compare with the release before.
DSP_OLD = {"dt2-1.17": "dt2-1.15C", "dn2-1.12": "dn2-1.10E"}
# Call-graph roots (short-word addresses) of the frame, voice and master code (docs/findings/53-stock-dsp-shape.md).
DSP_ROOTS = {
    "dt2-1.17": {"frame": 0x1C2B24, "voice": 0x1C642A, "master": 0x1C207B},
    "dn2-1.12": {"frame": 0x1C2712, "voice": 0x1C8EF1, "master": 0x1C1F1E},
}
# Call depth from each root that counts as on its path.
PATH_DEPTH = {"voice": 6, "master": 6, "frame": 3}
HOT_COUNT = 60  # the hottest functions of a profile get the tag `hot`
HELPER_MIN_CALLERS = 8
# A function paired by call shape needs a similar size: at most this far apart.
SHAPE_MIN_SLACK = 2
SHAPE_REL_SLACK = 0.25
# Function labels of sharcdb that make the role `io`.
IO_LABELS = frozenset({"driver/peripheral", "DMA/descriptor construction"})
# Hint thresholds.
MIN_TABLE_LITERALS = 2
MIN_BITFIELD_INSNS = 6
# Tags that put a function on the voice, master or frame path.
PATH_TAGS = ("voice_path", "master_path", "frame_path")
# Score weights (see dsp_score).
W_GRADE, W_PAIR, W_PATH, W_HOT, W_HELPER, W_ANCHORED = (
    4.0,
    2.0,
    1.5,
    1.0,
    0.5,
    -3.0,
)
ADDR_FMT = "0x%06x"

CATALOG_COLUMNS = (
    ("addr", "INTEGER"),
    ("name", "TEXT"),
    ("size", "INTEGER"),
    ("tags", "TEXT"),
    ("reloc_hash", "TEXT"),
    ("hash_count", "INTEGER"),
    ("stable", "INTEGER"),
    ("pair_addr", "INTEGER"),
    ("pair_method", "TEXT"),
    ("pair_same_hash", "INTEGER"),
    ("anchors", "TEXT"),
    ("rule_kind", "TEXT"),
    ("rule_ordinal", "INTEGER"),
    ("rule_both", "INTEGER"),
    ("anchor_grade", "INTEGER"),
    ("score", "REAL"),
    ("group", "INTEGER"),
    ("bytes", "INTEGER"),
    ("role", "TEXT"),
    ("label", "TEXT"),
    ("callers", "INTEGER"),
    ("callees", "INTEGER"),
    ("loops", "INTEGER"),
    ("do", "INTEGER"),
    ("recip", "INTEGER"),
    ("lit_data", "INTEGER"),
    ("abs_dm", "INTEGER"),
    ("prof_insns", "INTEGER"),
    ("hints", "TEXT"),
)


def dsp_score(
    grade: int,
    pair_method: str | None,
    tags: Sequence[str],
    anchors: Sequence[str],
) -> float:
    return (
        W_GRADE * grade
        + W_PAIR * (pair_method in STRONG_PAIR)
        + W_PATH * any(t in PATH_TAGS for t in tags)
        + W_HOT * ("hot" in tags)
        + W_HELPER * ("helper" in tags)
        + W_ANCHORED * bool(anchors)
    )


class DspImg:
    """Facts of one DSP image used by the catalog."""

    def __init__(self) -> None:
        self.fn: dict[
            int, tuple[int, int, str, str]
        ] = {}  # entry -> end, insns, name, label
        self.starts: list[int] = []
        self.hs: dict[int, str] = {}
        self.blen: dict[int, int] = {}
        self.groups: dict[str, list[int]] = {}
        self.callees: dict[int, list[int]] = {}
        self.callers: dict[int, set[int]] = {}
        self.callee_n: dict[int, collections.Counter[int]] = {}
        self.insn: dict[int, collections.Counter[str]] = {}
        self.loops: collections.Counter[int] = collections.Counter()
        self.do: collections.Counter[int] = collections.Counter()
        self.lit_data: collections.Counter[int] = collections.Counter()
        self.abs_dm: collections.Counter[int] = collections.Counter()
        self.anch: dict[int, list[str]] = {}

    def owner(self, sw: int) -> int | None:
        """Entry of the function that contains short word `sw`."""
        i = bisect.bisect_right(self.starts, sw) - 1
        if i >= 0 and sw < self.fn[self.starts[i]][0]:
            return self.starts[i]
        return None

    def n_callers(self, f: int) -> int:
        return len(self.callers.get(f, ()))


def count_insns(rows: Any) -> dict[int, collections.Counter[str]]:
    """Per function: counts of reciprocal, clip and bit-field instructions."""
    out: dict[int, collections.Counter[str]] = collections.defaultdict(
        collections.Counter
    )
    for fsw, mn in rows:
        m = mn or ""
        ml = m.lower()
        ct = out[fsw]
        if "recips" in ml or "rsqrts" in ml:
            ct["recip"] += 1
        if re.search(r"= (clip|min|max|abs)\(", ml):
            ct["clip"] += 1
        if "LSHIFT" in m or "FEXT" in m or "FDEP" in m or "LEFTZ" in m:
            ct["bitfield"] += 1
    return dict(out)


def anchor_functions(image: str, d: DspImg) -> dict[int, list[str]]:
    """Existing SHARC anchors of the port by containing function."""
    out: dict[int, list[str]] = {}
    for anchor in port_anchors(image):
        sw = anchor_sw(anchor)
        f = d.owner(sw) if sw is not None else None
        if f is not None:
            out.setdefault(f, []).append(anchor["name"])
    return out


def collect_dsp(image: str) -> DspImg:
    """Read the sharcdb of `image` and its port file."""
    from tools.sharc_dev import sharc

    im = sharc.load(
        image,
        sections_dir=os.path.join(ROOT, "out", "sections"),
        db_dir=SHARC_DIR,
    )
    db = im.db
    d = DspImg()
    d.fn = {
        e: (end, n, name or "", label or "")
        for e, end, n, name, label in db.execute(
            "SELECT entry_sw, end_sw, n_insns, name, label FROM functions WHERE image=?",
            (image,),
        )
    }
    d.starts = sorted(d.fn)
    groups: dict[str, list[int]] = collections.defaultdict(list)
    for e, h, bl in db.execute(
        "SELECT entry_sw, reloc_hash, byte_len FROM func_hash WHERE image=?", (image,)
    ):
        d.hs[e] = h
        d.blen[e] = bl
    for e in sorted(d.hs):
        groups[d.hs[e]].append(e)
    d.groups = dict(groups)
    callees: dict[int, list[int]] = collections.defaultdict(list)
    callers: dict[int, set[int]] = collections.defaultdict(set)
    callee_n: dict[int, collections.Counter[int]] = collections.defaultdict(
        collections.Counter
    )
    for f, t in db.execute(
        "SELECT from_function, to_function FROM edges WHERE image=? AND kind IN ('call','jump') "
        "AND to_function IS NOT NULL AND from_function IS NOT NULL AND from_function!=to_function "
        "ORDER BY from_sw",
        (image,),
    ):
        if f in d.fn and t in d.fn:
            if t not in callees[f]:
                callees[f].append(t)
            callers[t].add(f)
            callee_n[f][t] += 1
    d.callees, d.callers, d.callee_n = dict(callees), dict(callers), dict(callee_n)
    d.insn = count_insns(
        db.execute(
            "SELECT function_sw, mnemonic FROM insn WHERE image=? AND function_sw IS NOT NULL",
            (image,),
        )
    )
    for f, kind in db.execute(
        "SELECT function_sw, kind FROM loops WHERE image=?", (image,)
    ):
        d.loops[f] += 1
        if kind == "hw_do":
            d.do[f] += 1
    for sw, in_data in db.execute(
        "SELECT sw, in_data_range FROM literals WHERE image=?", (image,)
    ):
        f = d.owner(sw)
        if f is not None and in_data:
            d.lit_data[f] += 1
    for (sw,) in db.execute(
        "SELECT sw FROM mem_access WHERE image=? AND abs_address IS NOT NULL", (image,)
    ):
        f = d.owner(sw)
        if f is not None:
            d.abs_dm[f] += 1
    im.close()
    d.anch = anchor_functions(image, d)
    return d


def old_groups(image: str) -> dict[str, list[int]] | None:
    """Hash groups of the old image of `image`, None when its sharcdb is missing."""
    old = DSP_OLD.get(image)
    if not old or not os.path.exists(database_path(old, "sharc", SHARC_DIR)):
        return None
    return collect_dsp(old).groups


def bfs(d: DspImg, root: int, max_depth: int) -> dict[int, int]:
    """Call depth of every function within `max_depth` calls of `root`."""
    depth = {root: 0}
    queue = [root]
    while queue:
        nxt = []
        for f in queue:
            for t in d.callees.get(f, ()):
                if t not in depth and depth[f] < max_depth:
                    depth[t] = depth[f] + 1
                    nxt.append(t)
        queue = nxt
    return depth


def read_profile(path: str, d: DspImg) -> collections.Counter[int]:
    """Executed instructions per function. A line is: hex address, two fields, count."""
    per_addr: collections.Counter[int] = collections.Counter()
    with open(path) as fh:
        for ln in fh:
            t = ln.split()
            if len(t) >= 4:
                per_addr[int(t[0], 16)] += int(t[3])
    out: collections.Counter[int] = collections.Counter()
    for a, n in per_addr.items():
        f = d.owner(a)
        if f is not None:
            out[f] += n
    return out


def pair_up(a: DspImg, b: DspImg) -> dict[int, tuple[int, str]]:
    """Functions of `a` -> (function of `b`, method). Unique hashes first, then call shape."""
    m: dict[int, tuple[int, str]] = {}
    used: set[int] = set()
    for h, fs in a.groups.items():
        gb = b.groups.get(h, [])
        if len(fs) == 1 and len(gb) == 1:
            m[fs[0]] = (gb[0], "strict")
            used.add(gb[0])
    changed = True
    while changed:
        changed = False
        for fa, (fb, _method) in list(m.items()):
            ca, cb = a.callees.get(fa, []), b.callees.get(fb, [])
            if len(ca) != len(cb):
                continue
            for x, y in zip(ca, cb, strict=True):
                if x in m or y in used:
                    continue
                nx, ny = a.fn[x][1], b.fn[y][1]
                if abs(nx - ny) <= max(SHAPE_MIN_SLACK, SHAPE_REL_SLACK * max(nx, ny)):
                    m[x] = (y, "shape")
                    used.add(y)
                    changed = True
    return m


def dsp_role(
    label: str, in_voice: bool, in_master: bool, in_frame: bool, is_voice_root: bool
) -> str:
    if label in IO_LABELS:
        role = "io"
    elif in_voice and not in_master:
        role = "voice"
    elif in_master and not in_voice:
        role = "master/fx"
    elif in_voice and in_master:
        role = "util"
    elif in_frame:
        role = "frame"
    else:
        role = "other"
    return "voice" if is_voice_root else role


def dsp_hints(d: DspImg, f: int, label: str) -> list[str]:
    mix = d.insn.get(f, collections.Counter())
    hints = []
    if d.do[f]:
        hints.append("DO%d" % d.do[f])
    if mix["recip"]:
        hints.append("recip%d" % mix["recip"])
    if d.lit_data[f] >= MIN_TABLE_LITERALS:
        hints.append("tables%d" % d.lit_data[f])
    if mix["clip"]:
        hints.append("clip")
    if mix["bitfield"] >= MIN_BITFIELD_INSNS:
        hints.append("bitfield")
    if d.abs_dm[f]:
        hints.append("absDM%d" % d.abs_dm[f])
    if label and label != "unclassified/mixed":
        hints.append(label)
    return hints


def dsp_tags(
    d: DspImg, depth: dict[str, dict[int, int]], hot: Sequence[int]
) -> dict[int, set[str]]:
    """Tags per candidate function."""
    tags: dict[int, set[str]] = collections.defaultdict(set)
    for f in d.anch:
        tags[f].add("anchor")
        for t in d.callees.get(f, ()):
            tags[t].add("near")
        for t in d.callers.get(f, ()):
            tags[t].add("near")
    for name, reached in depth.items():
        for f in reached:
            tags[f].add(name + "_path")
    for f in hot:
        tags[f].add("hot")
    for f in d.fn:
        if d.n_callers(f) >= HELPER_MIN_CALLERS:
            tags[f].add("helper")
    return dict(tags)


def dsp_depths(image: str, d: DspImg) -> dict[str, dict[int, int]]:
    roots = DSP_ROOTS[image]
    for name, root in roots.items():
        if root not in d.fn:
            raise ValueError("%s: %s root 0x%x is not a function" % (image, name, root))
    return {name: bfs(d, root, PATH_DEPTH[name]) for name, root in roots.items()}


def function_role(
    image: str, entry: int, label: str, depth: dict[str, dict[int, int]]
) -> str:
    return dsp_role(
        label,
        entry in depth["voice"],
        entry in depth["master"],
        entry in depth["frame"],
        entry == DSP_ROOTS[image]["voice"],
    )


def dsp_rows(
    image: str,
    d: DspImg,
    other: DspImg | None,
    pair: dict[int, tuple[int, str]],
    old: dict[str, list[int]] | None,
    prof: collections.Counter[int],
) -> list[dict[str, Any]]:
    """Catalog rows of `d`; `pair` maps an address to (address on `other`, method)."""
    depth = dsp_depths(image, d)
    hot = [f for f, _n in prof.most_common(HOT_COUNT)]
    tags = dsp_tags(d, depth, hot)
    rows = []
    for f in sorted(tags):
        _end, size, name, label = d.fn[f]
        h = d.hs.get(f)
        group = d.groups.get(h, []) if h else []
        kind, ordinal = (
            suggest_rule(f, group, False, "sharc_func") if h else ("none", None)
        )
        stable = int(h in old) if old is not None and h else None
        pair_addr, method = pair.get(f, (None, None))
        same_hash: int | None = None
        both: int | None = None
        if other is not None and pair_addr is not None:
            oh = other.hs.get(pair_addr)
            same_hash = int(oh is not None and oh == h)
            if kind == "sharc_func" and h:
                if oh is None:
                    both = 0
                else:
                    okind, oord = suggest_rule(
                        pair_addr, other.groups.get(oh, []), False, "sharc_func"
                    )
                    both = int(okind == kind and oord == ordinal and oh == h)
        grade = catalog_grade(kind, stable, method, both)
        anchors = d.anch.get(f, [])
        role = function_role(image, f, label, depth)
        rows.append(
            {
                "addr": f,
                "name": name,
                "size": size,
                "tags": ",".join(sorted(tags[f])),
                "reloc_hash": h,
                "hash_count": len(group),
                "stable": stable,
                "pair_addr": pair_addr,
                "pair_method": method,
                "pair_same_hash": same_hash,
                "anchors": ",".join(anchors),
                "rule_kind": kind,
                "rule_ordinal": ordinal,
                "rule_both": both,
                "anchor_grade": grade,
                "score": dsp_score(grade, method, sorted(tags[f]), anchors),
                "group": -1,
                "bytes": d.blen.get(f),
                "role": role,
                "label": label,
                "callers": d.n_callers(f),
                "callees": len(d.callees.get(f, ())),
                "loops": d.loops[f],
                "do": d.do[f],
                "recip": d.insn.get(f, collections.Counter())["recip"],
                "lit_data": d.lit_data[f],
                "abs_dm": d.abs_dm[f],
                "prof_insns": prof[f] if prof else None,
                "hints": ";".join(dsp_hints(d, f, label)),
            }
        )
    addrs = [cast(int, r["addr"]) for r in rows]
    ids = louvain_groups(addrs, group_edges(addrs, d.callee_n))
    for r, a in zip(rows, addrs, strict=True):
        r["group"] = ids.get(a, -1)
    return rows


def write_dsp_catalog(
    image: str,
    rows: list[dict[str, Any]],
    d: DspImg,
    out_dir: str | None,
    db: sqlite3.Connection,
) -> int:
    """Write tables, TSV and graph JSON of `image`; return the group count."""
    exports = FirmwareArtifacts(image, "sharc").catalog_exports(out_dir)
    exports.catalog.parent.mkdir(parents=True, exist_ok=True)
    shared.write_catalog(
        db,
        str(exports.catalog),
        rows,
        CATALOG_COLUMNS,
        ADDR_FMT,
    )
    return shared.write_catalog_groups(
        image,
        rows,
        group_edges([r["addr"] for r in rows], d.callee_n),
        db,
        str(exports.groups),
        str(exports.graph),
        ADDR_FMT,
        class_col="role",
        hint_col="label",
    )


def write_function_index(
    image: str,
    other: str | None,
    d: DspImg,
    pair: dict[int, tuple[int, str]],
    db: sqlite3.Connection,
) -> None:
    """Publish the full inventory and annotations in the SHARC database."""
    depth = dsp_depths(image, d)
    with closing(Sharc(image)) as src:
        anchors_by_entry = src.declared_anchors()
        notes = src.note_records()
        db.execute(FUNCTION_NAMES_SQL)
        db.execute("DELETE FROM function_names WHERE image=?", (image,))
        db.executemany(
            "INSERT INTO function_names VALUES (?,?,?,?)",
            (
                (image, entry, name, "declared")
                for entry, anchors in anchors_by_entry.items()
                for name, site, _, _ in anchors
                if site == entry and ".fn." in name
            ),
        )
        db.execute("DROP TABLE IF EXISTS function_notes")
        db.execute("""CREATE TABLE function_notes (
            image TEXT, function_sw INTEGER, role TEXT, summary TEXT, reads TEXT,
            writes TEXT, confidence TEXT, evidence TEXT, author TEXT, created TEXT,
            PRIMARY KEY (image, function_sw)
        )""")
        db.executemany(
            "INSERT INTO function_notes VALUES (?,?,?,?,?,?,?,?,?,?)",
            ((image, *row) for row in notes.values()),
        )
        db.execute("DROP TABLE IF EXISTS function_index_meta")
        db.execute(
            "CREATE TABLE function_index_meta (key TEXT PRIMARY KEY, value TEXT)"
        )
        db.executemany(
            "INSERT INTO function_index_meta VALUES (?,?)",
            [
                ("pair_image", other or ""),
                *(
                    (k, src.meta.get(k))
                    for k in (
                        "image_sha256",
                        "build_input_sha256",
                        "min_depth",
                        "blocks",
                    )
                ),
            ],
        )
        db.execute("DROP TABLE IF EXISTS function_index")
        db.execute("""CREATE TABLE function_index (
            addr INTEGER PRIMARY KEY, end_sw INTEGER, name TEXT, decoder_label TEXT,
            anchors TEXT, note_role TEXT, note_summary TEXT, note_confidence TEXT,
            inferred_role TEXT, pair_image TEXT, pair_addr INTEGER, pair_method TEXT
        )""")
        rows = []
        for row in src.function_index(anchors_by_entry):
            entry, _end, _name, label, *_notes = row
            pair_addr, method = pair.get(entry, (None, None))
            rows.append(
                (
                    *row,
                    function_role(image, entry, label, depth),
                    other if pair_addr is not None else None,
                    pair_addr,
                    method,
                )
            )
        db.executemany(
            "INSERT INTO function_index VALUES (?,?,?,?,?,?,?,?,?,?,?,?)", rows
        )
        db.execute("DROP TABLE IF EXISTS function_anchors")
        db.execute("""CREATE TABLE function_anchors (
            function_sw INTEGER, site_sw INTEGER, name TEXT, mark TEXT, description TEXT,
            PRIMARY KEY (function_sw, name, site_sw)
        )""")
        db.executemany(
            "INSERT INTO function_anchors VALUES (?,?,?,?,?)",
            (
                (entry, site, name, mark, doc)
                for entry, anchors in anchors_by_entry.items()
                for name, site, mark, doc in anchors
            ),
        )


def catalog(
    image: str,
    other_image: str | None = None,
    out_dir: str | None = None,
    profiles: dict[str, str] | None = None,
    *,
    report: Callable[[str], None] = print,
) -> int:
    names = [short_image(name) for name in (image, other_image) if name is not None]
    if len(set(names)) != len(names):
        report("SHARC catalog pairing requires different images")
        return 2
    for n in names:
        if n not in DSP_ROOTS:
            report("no DSP roots for %s: add it to DSP_ROOTS" % n)
            return 2
        if not os.path.exists(database_path(n, "sharc", SHARC_DIR)):
            report("no sharcdb for %s: run sharc.load(%r) first" % (n, n))
            return 2
    images = {name: collect_dsp(name) for name in names}
    for n, d in images.items():
        peer = next((name for name in names if name != n), None)
        o = images[peer] if peer else None
        pair = pair_up(d, o) if o is not None else {}
        path = (profiles or {}).get(n)
        prof = read_profile(path, d) if path else collections.Counter()
        rows = dsp_rows(n, d, o, pair, old_groups(n), prof)
        with closing(sqlite3.connect(database_path(n, "sharc", SHARC_DIR))) as db, db:
            db.execute("BEGIN IMMEDIATE")
            n_groups = write_dsp_catalog(n, rows, d, out_dir, db)
            write_function_index(n, peer, d, pair, db)
        counts: dict[str, int] = {}
        for r in rows:
            for t in r["tags"].split(","):
                counts[t] = counts.get(t, 0) + 1
        report(
            "%s: rows=%d anchor_grade=%d groups=%d tags: %s"
            % (
                n,
                len(rows),
                sum(r["anchor_grade"] for r in rows),
                n_groups,
                " ".join("%s=%d" % kv for kv in sorted(counts.items())),
            )
        )
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("image")
    ap.add_argument("other", nargs="?", help="optional peer SHARC image")
    ap.add_argument(
        "--out-dir", help="export with image-prefixed names in this directory"
    )
    ap.add_argument(
        "--profile",
        action="append",
        default=[],
        metavar="IMAGE=PATH",
        help="instruction profile of IMAGE (adds the tag hot and column prof_insns); repeatable",
    )
    args = ap.parse_args(argv)
    profiles: dict[str, str] = {}
    for spec in args.profile:
        name, sep, path = spec.partition("=")
        if not sep:
            ap.error("--profile needs IMAGE=PATH, got %r" % spec)
        profiles[short_image(name)] = path
    return catalog(args.image, args.other, args.out_dir, profiles)


if __name__ == "__main__":
    sys.exit(main())
