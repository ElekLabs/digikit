#!/usr/bin/env python3
"""atlas.py: a per-firmware semantic atlas, out/atlas/<image>.sqlite.

    uv run python -m tools.coldfire.atlas build dt2-1.17 [dn2-1.12 ...]
    uv run python -m tools.coldfire.fw atlas machine -i dt2-1.17
    uv run python -m tools.coldfire.atlas catalog dt2-1.17 dn2-1.12
    uv run python -m tools.coldfire.fw catalog -i dt2-1.17 --grade

`catalog` adds a table `catalog` (candidate anchor functions, scored) to the
atlas of each image and writes out/atlas/<image>-catalog.tsv. Run `build` first.

Answers "where are machines / FX / parameters / menus / pattern and step
storage handled on this firmware" from data. Everything in the atlas is derived
from the user's own firmware and stays under out/ (uncommitted). What is
committed is the extraction rules: this file, tools/patchkit/machinedescr.py,
tools/coldfire/paramtable.py and the cfdb RTTI tables.

Tables (each row carries `provenance` = extractor and `confidence`):
  descriptors  UI page/machine descriptor table (44-byte stride: two names and
               nine parameter-id fields), found by structure, not by address.
               role: filter_type | machine | page_group | unknown.
  machines     view over descriptors where role='machine'.
  params       the parameter descriptor table (tools/coldfire/paramtable.py).
  classes      RTTI classes with a role guessed from the name (view, page,
               menu, window, storage, ...), primary vtable and slot count.
  vfuncs       vtable slots of those classes: the function behind each slot.
  ui_strings   UI strings grouped by the view class whose methods use them
               (directly, or through callees only that class reaches).
  structs      Storage/struct types named by RTTI (pattern, kit, sound, ...).
  manual       manual pages that mention each glossary term (when the manual
               text is in out/refs/).
  glossary     manual terms -> classes, strings, functions, descriptors,
               params, structs found by search rules (TERMS below).

Inputs: out/cfdb/<image>-emac.sqlite and its Ghidra dump (disasm), the MAIN OS
section for parameter names (out/sections/<image>/section_3_MAIN_OS.bin).
"""

from __future__ import annotations

import argparse
import collections
import glob
import json
import os
import re
import sqlite3
import sys
import time
from collections.abc import Iterable, Sequence
from typing import Any, cast

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))

from tools.coldfire import fw  # noqa: E402
from tools.firmware import catalog as shared  # noqa: E402
from tools.firmware import config, device  # noqa: E402
from tools.firmware.catalog import (  # noqa: E402
    STRONG_PAIR,
    catalog_grade,
    count_list,
    group_edges,
    louvain_groups,
    suggest_rule,
)

ATLAS_VERSION = 1
DESCR_STRIDE = 0x2C
MIN_CHAIN = 4

# --- glossary rules ------------------------------------------------------------
# term -> regexes (case-insensitive). classes: RTTI class names; strings: UI
# strings; params: parameter long/short/owner names; descr: descriptor names;
# structs: struct/storage type names; manual: pages of the user manual.
TERMS: dict[str, dict[str, str]] = {
    "machine": {
        "classes": r"machine",
        "strings": r"^(machine|SAMPLE|FM TONE|WAVETONE|FM DRUM|SWARMER|WERP|STRETCH|REPITCH|SLICED SMP|MANUAL SLICE)$",
        "params": r"machine",
        "descr": r"^(SAMPLE|WERP|STRETCH|REPITCH|SLICED SMP|MANUAL SLICE|FM TONE|WAVETONE|FM DRUM|SWARMER|MIDI)$",
        "manual": r"\bmachines?\b",
    },
    "fx": {
        "classes": r"\bfx|effect|reverb|delay|chorus|compress",
        "strings": r"^(fx|effects?|delay|reverb|chorus|compressor)\b",
        "params": r"delay|reverb|chorus|compress|^fx",
        "descr": r"^(FX|DEL|REV|CHO|COMP|FX MIX)$",
        "structs": r"fxSetup",
        "manual": r"\bfx\b|effects?",
    },
    "parameter": {
        "classes": r"param",
        "strings": r"^param",
        "params": r".",
        "descr": r".",
        "manual": r"\bparameters?\b",
    },
    "step": {
        "classes": r"step|trig(?!ger)",
        "strings": r"\bstep\b",
        "params": r"step",
        "manual": r"\bsteps?\b",
    },
    "pattern": {
        "classes": r"pattern",
        "strings": r"pattern",
        "params": r"pattern",
        "structs": r"pattern",
        "manual": r"\bpatterns?\b",
    },
    "kit": {
        "classes": r"\bkit|Kit",
        "strings": r"\bkit\b",
        "structs": r"kit",
        "manual": r"\bkits?\b",
    },
    "sound": {
        "classes": r"^sound|Sound(?!Dev)",
        "strings": r"^sound",
        "structs": r"sound",
        "manual": r"\bsounds?\b",
    },
    "track": {
        "classes": r"track",
        "strings": r"^track",
        "params": r"track",
        "structs": r"track",
        "manual": r"\btracks?\b",
    },
    "page": {
        "classes": r"page",
        "strings": r"^page",
        "manual": r"\bpages?\b",
    },
    "menu": {
        "classes": r"menu",
        "strings": r"menu",
        "manual": r"\bmenu\b",
    },
    "lfo": {
        "classes": r"lfo",
        "strings": r"lfo",
        "params": r"lfo|^(spd|mult|fade|dest|wave|sph|mode|trig|depth)$",
        "descr": r"LFO",
        "manual": r"\blfo\b",
    },
    "p-lock": {
        "classes": r"plock|parameterlock|p_lock",
        "strings": r"p-?lock|param.* lock",
        "manual": r"parameter lock|p-?lock",
    },
    "trig": {
        "classes": r"trig",
        "strings": r"^trig|\btrigs?\b",
        "params": r"trig",
        "descr": r"TRIG|RTRG|EUCL",
        "manual": r"\btrigs?\b",
    },
    "retrig": {
        "classes": r"retrig|retrg",
        "strings": r"retrig",
        "params": r"retrig",
        "descr": r"RTRG",
        "manual": r"retrig",
    },
    "song": {
        "classes": r"song",
        "strings": r"^song",
        "structs": r"song",
        "manual": r"\bsongs?\b",
    },
    "chain": {
        "classes": r"chain",
        "strings": r"chain",
        "manual": r"\bchains?\b",
    },
    "plus-drive": {
        "classes": r"plusdrive|plus_drive",
        "strings": r"\+drive|plus ?drive",
        "manual": r"\+drive",
    },
    "sample": {
        "classes": r"sample",
        "strings": r"^sample",
        "params": r"sample",
        "descr": r"SAMPLE|SLIC|SMP",
        "manual": r"\bsamples?\b",
    },
    "filter": {
        "classes": r"filter",
        "strings": r"filter|lowpass|highpass",
        "params": r"filter|^freq|^reso",
        "descr": r"FLTR|STATE VARIABLE|LOWPASS|EQUALIZER|COMB|LEGACY",
        "manual": r"\bfilters?\b",
    },
    "amp": {
        "classes": r"\bamp\b|amplitude",
        "params": r"amp",
        "descr": r"^AMP$",
        "manual": r"\bamp\b",
    },
    "envelope": {
        "classes": r"envelope|\benv\b",
        "strings": r"envelope",
        "params": r"envelope|attack|decay|^(atk|dec|sus|rel)",
        "manual": r"envelopes?",
    },
    "mixer": {
        "classes": r"mixer|\bmix\b",
        "strings": r"mixer",
        "descr": r"MIX",
        "manual": r"\bmixer\b",
    },
    "sequencer": {
        "classes": r"sequencer|\bseq",
        "strings": r"sequencer",
        "manual": r"sequencer",
    },
    "project": {
        "classes": r"project",
        "strings": r"^project",
        "structs": r"project",
        "manual": r"\bprojects?\b",
    },
    "midi": {
        "classes": r"midi",
        "strings": r"^midi",
        "descr": r"MIDI",
        "manual": r"\bmidi\b",
    },
    "tempo": {
        "classes": r"tempo|bpm",
        "strings": r"tempo|bpm",
        "manual": r"\btempo\b",
    },
    "scale": {
        "classes": r"scale",
        "strings": r"^scale",
        "manual": r"\bscale\b",
    },
    "mute": {
        "classes": r"mute",
        "strings": r"mute",
        "manual": r"\bmute\b",
    },
    "arp": {
        "classes": r"arp",
        "strings": r"^arp",
        "manual": r"\barp",
    },
    "overbridge": {
        "classes": r"^Ob|overbridge",
        "strings": r"overbridge",
        "manual": r"overbridge",
    },
    "settings": {
        "classes": r"setting|config",
        "strings": r"^settings",
        "structs": r"settings",
        "manual": r"\bsettings\b",
    },
}

ROLE_RULES: list[tuple[str, str]] = [
    ("pagegrid", r"GridView$"),
    ("page", r"PageView$"),
    ("menu", r"Menu|Popup"),
    ("window", r"Window$|Prompt$|Dialog"),
    ("view", r"View$|view$"),
    ("storage", r"Storage|_t$|Mirror|^Value<"),
    (
        "model",
        r"^(Pattern|Kit|Sound|Song|Project|Track|FxSetup|Sequencer|Brain|Recorder)",
    ),
    ("worker", r"Worker|Manager|Handler|Cache|Controller"),
]


def role_of(name: str) -> str:
    for role, pat in ROLE_RULES:
        if re.search(pat, name):
            return role
    return "other"


# --- descriptor table ---------------------------------------------------------

_PEA = re.compile(r"^([0-9a-f]{8})  .{24}  pea \(0x(4[0-9a-f]{7})\)\.l")


def find_descriptor_chain(img: fw.CF) -> tuple[int, int, int, str] | None:
    """(function entry, table base, chain length, disasm path) of the function
    whose pea operands form the longest run of 44-byte-stride destinations."""
    best: tuple[int, int, int, str] | None = None
    for entry, path in img.db.execute(
        "SELECT entry, disasm_path FROM functions WHERE size>3000 AND disasm_path IS NOT NULL"
    ):
        text = img.read_dump(path)
        if not text:
            continue
        dests = set()
        for ln in text.splitlines():
            m = _PEA.match(ln)
            if m:
                dests.add(int(m.group(2), 16))
        for start in dests:
            if start - DESCR_STRIDE in dests:
                continue
            n = 0
            while (
                start + DESCR_STRIDE * n in dests
                or start + DESCR_STRIDE * n + 4 in dests
            ):
                n += 1
            if n >= MIN_CHAIN and (best is None or n > best[2]):
                best = (entry, start, n, path)
    return best


def extract_descriptors(img: fw.CF, section: str) -> list[dict[str, Any]]:
    from tools.patchkit import machinedescr as md

    chain = find_descriptor_chain(img)
    if chain is None or not os.path.exists(section):
        return []
    entry, base, n, path = chain
    text = img.read_dump(path)
    assert text is not None
    lines = []
    for raw in text.splitlines():
        m = md.LINE_RE.match(raw)
        if m:
            lines.append((int(m.group(1), 16), m.group(3).strip()))
    image = md.Image(section)
    entries = md.walk_table(lines, image, base, n, DESCR_STRIDE)
    while entries and entries[-1]["names"]["name1"].get("string") is None:
        entries.pop()
    rows = []
    for i, e in enumerate(entries):
        vals = [f["value"] for f in e["fields"]]
        rows.append(
            {
                "idx": i,
                "addr": e["address"],
                "name1": e["names"]["name1"].get("string"),
                "name2": e["names"]["name2"].get("string"),
                "fields": json.dumps(vals),
                "init_func": entry,
                "unresolved": sum(1 for v in vals if v is None),
            }
        )
    anchor = next((r["idx"] for r in rows if r["name1"] in ("SAMPLE", "FM TONE")), None)
    end = next(
        (
            r["idx"]
            for r in rows
            if r["name1"] == "NONE" and anchor is not None and r["idx"] > anchor
        ),
        None,
    )
    for r in rows:
        v = json.loads(r["fields"])
        if anchor is not None and r["idx"] < anchor:
            r["role"] = (
                "filter_type" if v[:4] == [0x2D, 0x2E, 0x2F, 0x30] else "unknown"
            )
        elif anchor is not None and end is not None and anchor <= r["idx"] <= end:
            r["role"] = "machine"
        elif end is not None and r["idx"] > end:
            r["role"] = "page_group"
        else:
            r["role"] = "unknown"
        r["provenance"] = "machinedescr.walk_table@FUN_%08x stride-chain base %#x" % (
            entry,
            base,
        )
        r["confidence"] = (
            "medium" if r["unresolved"] or r["role"] == "unknown" else "high"
        )
    return rows


def extract_params(section: str) -> list[dict[str, Any]]:
    from tools.coldfire import paramtable
    from tools.patchkit import machineprofile

    if not os.path.exists(section):
        return []
    with open(section, "rb") as fh:
        data = fh.read()
    addr, count = paramtable.find_table(data)
    read = machineprofile.image_reader(section)
    rows = []
    for e in paramtable.scan_all(read, addr, count):
        rows.append(
            {
                "code": e["param_code"],
                "addr": int(e["addr"], 16),
                "long_name": e["long_name"],
                "short_name": e["short_name"],
                "owner_name": e["owner_name"],
                "owner_type": e["owner_type"],
                "mirror_index": e["mirror_index"],
                "min": e["min"],
                "max": e["max"],
                "default": e["default"],
                "kind": e["kind_0x1c"],
                "provenance": "paramtable.scan_all table %#x x%d (signature)"
                % (addr, count),
                "confidence": "high" if e["long_name"] else "medium",
            }
        )
    return rows


# --- RTTI classes, views, strings ----------------------------------------------


def extract_classes(img: fw.CF) -> tuple[list[dict[str, Any]], list[tuple[Any, ...]]]:
    classes = []
    for name, kind, bases, addr in img.db.execute(
        "SELECT name, kind, bases, addr FROM typeinfos WHERE kind IN ('class','si_class','vmi_class') "
        "AND name NOT LIKE 'std::%' AND name NOT LIKE '%_Sp_%' ORDER BY name"
    ):
        vts = img.db.execute(
            "SELECT addr, COUNT(*) FROM vtables WHERE class=? GROUP BY addr ORDER BY addr",
            (name,),
        ).fetchall()
        base_names = fw._bases(img, bases)
        classes.append(
            {
                "name": name,
                "kind": kind,
                "role": role_of(name),
                "bases": base_names,
                "typeinfo": addr,
                "vtable": vts[0][0] if vts else None,
                "nslots": vts[0][1] if vts else 0,
                "provenance": "cfdb typeinfos+vtables (rttiscan)",
                "confidence": "high",
            }
        )
    vfuncs = []
    for cls, slot, target, owner in img.db.execute(
        "SELECT v.class, v.slot, v.target, v.owner FROM vtables v JOIN typeinfos t ON t.name=v.class "
        "WHERE v.class NOT LIKE 'std::%' AND v.addr=(SELECT MIN(addr) FROM vtables w WHERE w.class=v.class) "
        "GROUP BY v.class, v.slot ORDER BY v.class, v.slot"
    ):
        fname = img.db.execute(
            "SELECT name FROM functions WHERE entry=?", (target,)
        ).fetchone()
        vfuncs.append((cls, slot, target, fname[0] if fname else None, owner))
    return classes, vfuncs


VIEW_ROLES = ("page", "pagegrid", "menu", "window", "view")


def extract_ui_strings(
    img: fw.CF, classes: list[dict[str, Any]], vfuncs: list[tuple[Any, ...]]
) -> list[dict[str, Any]]:
    view_names = {c["name"] for c in classes if c["role"] in VIEW_ROLES}
    owner: dict[int, set[str]] = {}
    direct: dict[int, str] = {}
    for cls, _slot, target, _fname, own in vfuncs:
        if cls in view_names and target and own == cls:
            direct.setdefault(target, cls)
            owner.setdefault(target, set()).add(cls)
    g = img.graph()
    reach2: dict[int, set[str]] = {}
    for f, cls in direct.items():
        for a in g.succ.get(f, ()):
            if a in direct:
                continue
            reach2.setdefault(a, set()).add(cls)
            for b in g.succ.get(a, ()):
                if b not in direct:
                    reach2.setdefault(b, set()).add(cls)
    rows = []
    seen = set()
    for addr, text, func in img.db.execute(
        "SELECT DISTINCT s.addr, s.text, d.func FROM datarefs d JOIN strings s ON s.addr=d.to_addr "
        "WHERE d.func IS NOT NULL AND length(s.text)>=2"
    ):
        if func in direct:
            cls, conf, how = direct[func], "high", "vfunc of the class"
        elif func in reach2 and len(reach2[func]) == 1:
            cls, conf, how = (
                next(iter(reach2[func])),
                "medium",
                "callee within 2 calls of one class's methods",
            )
        else:
            continue
        key = (addr, cls)
        if key in seen:
            continue
        seen.add(key)
        rows.append(
            {
                "addr": addr,
                "text": text,
                "view_class": cls,
                "user_func": func,
                "provenance": "datarefs string -> %s" % how,
                "confidence": conf,
            }
        )
    return rows


def extract_structs(img: fw.CF) -> list[dict[str, Any]]:
    rows = []
    seen = set()
    for (name,) in img.db.execute(
        "SELECT DISTINCT name FROM typeinfos WHERE name LIKE '%Storage_v%' OR name LIKE 'Digisharc::%_t' "
        "OR name LIKE 'Digitakt::%_t' OR name LIKE 'Digitone::%_t' OR name LIKE 'Digitone2::%_t' ORDER BY name"
    ):
        if name in seen:
            continue
        seen.add(name)
        m = re.search(r"(\w+?)Storage_v(\d+)_t$", name)
        rows.append(
            {
                "name": name,
                "family": m.group(1) if m else re.sub(r".*::|_t$", "", name),
                "version": int(m.group(2)) if m else None,
                "provenance": "cfdb typeinfos (template arguments of Value<>/ValueWithMirror<>/_Sp_counted_ptr)",
                "confidence": "high",
            }
        )
    for (name,) in img.db.execute(
        "SELECT name FROM typeinfos WHERE name LIKE 'ValueWithMirror<%' OR name LIKE 'ArrayValueWithMirror<%'"
    ):
        for m in re.finditer(r"(?:Digisharc|Digitakt|Digitone2?)::\w+", name):
            n = m.group(0)
            if n not in seen:
                seen.add(n)
                mm = re.search(r"(\w+?)Storage_v(\d+)_t$", n)
                rows.append(
                    {
                        "name": n,
                        "family": mm.group(1) if mm else re.sub(r".*::|_t$", "", n),
                        "version": int(mm.group(2)) if mm else None,
                        "provenance": "cfdb typeinfos (ValueWithMirror template argument)",
                        "confidence": "high",
                    }
                )
    return rows


# --- manual ------------------------------------------------------------------------


def manual_dir(image: str) -> str | None:
    dev = next(
        (d for d in device.load_all(config.DEVICES_DIR) if image.startswith(d.short + "-")),
        None,
    )
    pat = dev.manual if dev else None
    if not pat:
        return None
    hits = sorted(glob.glob(os.path.join(ROOT, "out", "refs", pat)))
    return hits[-1] if hits else None


def extract_manual(image: str) -> list[dict[str, Any]]:
    mdir = manual_dir(image)
    if not mdir:
        return []
    rows = []
    pages = sorted(glob.glob(os.path.join(mdir, "pages", "p*.txt")))
    texts = []
    for p in pages:
        with open(p, errors="replace") as fh:
            texts.append((int(os.path.basename(p)[1:5]), fh.read()))
    stem = os.path.basename(mdir)
    for term, rules in TERMS.items():
        pat = rules.get("manual")
        if not pat:
            continue
        rx = re.compile(pat, re.I)
        scored = []
        for page, text in texts:
            hits = rx.findall(text)
            if hits:
                m = rx.search(text)
                assert m is not None
                s = max(0, m.start() - 50)
                scored.append(
                    (len(hits), page, " ".join(text[s : m.end() + 90].split()))
                )
        scored.sort(reverse=True)
        for n, page, snip in scored[:6]:
            rows.append(
                {
                    "term": term,
                    "page": page,
                    "hits": n,
                    "snippet": snip,
                    "provenance": "%s p%04d" % (stem, page),
                    "confidence": "medium",
                }
            )
    return rows


# --- glossary ------------------------------------------------------------------------

LIMITS = {
    "class": 14,
    "string": 12,
    "param": 12,
    "descriptor": 12,
    "struct": 10,
    "function": 8,
}


def score(text: str, term: str, rx: re.Pattern[str]) -> int:
    t = text.lower()
    if t == term.lower():
        return 4
    if re.search(r"\b" + re.escape(term.lower()), t):
        return 3
    return 2 if rx.search(text) else 0


def table_functions(
    img: fw.CF, lo: int, hi: int, limit: int = 8
) -> list[tuple[int, str, int]]:
    """Functions with data references into [lo, hi), most references first."""
    rows = img.db.execute(
        "SELECT d.func, f.name, COUNT(*) FROM datarefs d JOIN functions f ON f.entry=d.func "
        "WHERE d.to_addr>=? AND d.to_addr<? GROUP BY d.func ORDER BY 3 DESC, 1 LIMIT ?",
        (lo, hi, limit),
    ).fetchall()
    return rows


def structural_rows(con: sqlite3.Connection, img: fw.CF) -> list[tuple[Any, ...]]:
    """Glossary rows for the tables themselves: who builds and who reads them."""
    out: list[tuple[Any, ...]] = []
    d = con.execute(
        "SELECT MIN(addr), MAX(addr), MAX(init_func), COUNT(*) FROM descriptors"
    ).fetchone()
    if d and d[0]:
        base, last, init, n = d
        prov = "descriptors: FUN_%08x initializer, stride-chain" % init
        detail = (
            "builds the %d-entry page/machine descriptor table at %#x (filter types, machines, page groups)"
            % (n, base)
        )
        for term in ("machine", "page", "filter", "lfo", "trig"):
            out.append((term, "function", "0x%08x" % init, detail, 100, prov, "high"))
            for fn, name, cnt in table_functions(img, base, last + DESCR_STRIDE):
                if fn != init:
                    out.append(
                        (
                            term,
                            "function",
                            "0x%08x" % fn,
                            "%s reads the descriptor table (%d refs)" % (name, cnt),
                            90,
                            prov,
                            "medium",
                        )
                    )
    p = con.execute("SELECT MIN(addr), COUNT(*), MAX(addr) FROM params").fetchone()
    if p and p[0]:
        lo, n, last = p
        prov = "params: paramtable table %#x x%d" % (lo, n)
        for term in ("parameter", "p-lock", "fx"):
            for fn, name, cnt in table_functions(img, lo, last + 0x3C):
                out.append(
                    (
                        term,
                        "function",
                        "0x%08x" % fn,
                        "%s reads the parameter descriptor table at %#x (%d refs)"
                        % (name, lo, cnt),
                        90,
                        prov,
                        "medium",
                    )
                )
    return out


def build_glossary(
    con: sqlite3.Connection, img: fw.CF, manual_rows: list[dict[str, Any]]
) -> list[tuple[Any, ...]]:
    out: list[tuple[Any, ...]] = structural_rows(con, img)
    for term, rules in TERMS.items():
        key = term.replace("-", "")
        picks: dict[str, list[tuple[int, str, str]]] = {k: [] for k in LIMITS}
        if "classes" in rules:
            rx = re.compile(rules["classes"], re.I)
            for name, role, vt, ns in con.execute(
                "SELECT name, role, vtable, nslots FROM classes"
            ):
                if rx.search(name):
                    picks["class"].append(
                        (
                            score(name, key, rx) + (1 if role in VIEW_ROLES else 0),
                            name,
                            "role=%s vtable=%s slots=%d"
                            % (role, hex(vt) if vt else "-", ns),
                        )
                    )
        if "strings" in rules:
            rx = re.compile(rules["strings"], re.I)
            users: dict[int, list[int]] = {}
            for addr, text in img.db.execute(
                "SELECT addr, text FROM strings WHERE length(text) BETWEEN 3 AND 60"
            ):
                if rx.search(text):
                    us = img.db.execute(
                        "SELECT DISTINCT func FROM datarefs WHERE to_addr=? AND func IS NOT NULL",
                        (addr,),
                    ).fetchall()
                    users[addr] = [u[0] for u in us]
                    picks["string"].append(
                        (
                            score(text, key, rx) * 2 + min(len(us), 3),
                            "0x%08x" % addr,
                            "%r used by %s"
                            % (
                                text,
                                ",".join("%08x" % u for u in users[addr][:3]) or "-",
                            ),
                        )
                    )
            fcount: dict[int, int] = {}
            for us2 in users.values():
                for u in us2:
                    fcount[u] = fcount.get(u, 0) + 1
            for u, n in sorted(fcount.items(), key=lambda kv: -kv[1])[
                : LIMITS["function"]
            ]:
                nm = img.db.execute(
                    "SELECT name FROM functions WHERE entry=?", (u,)
                ).fetchone()
                picks["function"].append(
                    (
                        n,
                        "0x%08x" % u,
                        "%s (%d matching strings)" % (nm[0] if nm else "?", n),
                    )
                )
        if "params" in rules:
            rx = re.compile(rules["params"], re.I)
            for code, ln, sn, on, _addr in con.execute(
                "SELECT code, long_name, short_name, owner_name, addr FROM params"
            ):
                text = " ".join(x for x in (ln, sn, on) if x)
                if text and (
                    rx.search(ln or "") or rx.search(sn or "") or rx.search(on or "")
                ):
                    picks["param"].append(
                        (
                            score(ln or "", key, rx),
                            "code=%d" % code,
                            "%s/%s owner=%s" % (ln, sn, on),
                        )
                    )
        if "descr" in rules:
            rx = re.compile(rules["descr"], re.I)
            for idx, role, n1, n2, addr in con.execute(
                "SELECT idx, role, name1, name2, addr FROM descriptors"
            ):
                if rx.search(n1 or "") or rx.search(n2 or ""):
                    picks["descriptor"].append(
                        (
                            score(n1 or "", key, rx),
                            "#%d@0x%08x" % (idx, addr),
                            "%s/%s role=%s" % (n1, n2, role),
                        )
                    )
        if "structs" in rules:
            rx = re.compile(rules["structs"], re.I)
            for name, fam, ver in con.execute(
                "SELECT name, family, version FROM structs"
            ):
                if rx.search(name):
                    picks["struct"].append(
                        (
                            score(name, key, rx) + (ver or 0) // 100,
                            name,
                            "family=%s version=%s" % (fam, ver),
                        )
                    )
        for kind, items in picks.items():
            items.sort(key=lambda t: (-t[0], t[1]))
            for sc, ref, detail in items[: LIMITS[kind]]:
                conf = (
                    "medium"
                    if kind in ("string", "function") or "role=unknown" in detail
                    else "high"
                )
                out.append(
                    (term, kind, ref, detail, sc, "TERMS[%r] regex search" % term, conf)
                )
        for m in manual_rows:
            if m["term"] == term:
                out.append(
                    (
                        term,
                        "manual",
                        "p%d" % m["page"],
                        m["snippet"],
                        m["hits"],
                        m["provenance"],
                        "medium",
                    )
                )
    return out


# --- build -----------------------------------------------------------------------------

SCHEMA = """
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE descriptors (idx INTEGER, addr INTEGER, name1 TEXT, name2 TEXT, fields TEXT, role TEXT,
  init_func INTEGER, unresolved INTEGER, provenance TEXT, confidence TEXT);
CREATE VIEW machines AS SELECT * FROM descriptors WHERE role='machine';
CREATE TABLE params (code INTEGER, addr INTEGER, long_name TEXT, short_name TEXT, owner_name TEXT,
  owner_type INTEGER, mirror_index INTEGER, "min" INTEGER, "max" INTEGER, "default" INTEGER, kind INTEGER,
  provenance TEXT, confidence TEXT);
CREATE TABLE classes (name TEXT, kind TEXT, role TEXT, bases TEXT, typeinfo INTEGER, vtable INTEGER,
  nslots INTEGER, provenance TEXT, confidence TEXT);
CREATE TABLE vfuncs (class TEXT, slot INTEGER, target INTEGER, func_name TEXT, owner TEXT);
CREATE TABLE ui_strings (addr INTEGER, text TEXT, view_class TEXT, user_func INTEGER, provenance TEXT, confidence TEXT);
CREATE TABLE structs (name TEXT, family TEXT, version INTEGER, provenance TEXT, confidence TEXT);
CREATE TABLE manual (term TEXT, page INTEGER, hits INTEGER, snippet TEXT, provenance TEXT, confidence TEXT);
CREATE TABLE glossary (term TEXT, kind TEXT, ref TEXT, detail TEXT, score INTEGER, provenance TEXT, confidence TEXT);
CREATE INDEX glossary_term ON glossary(term);
CREATE INDEX classes_name ON classes(name);
CREATE INDEX ui_strings_class ON ui_strings(view_class);
"""


def insert(con: sqlite3.Connection, table: str, rows: list[dict[str, Any]]) -> None:
    if not rows:
        return
    cols = list(rows[0].keys())
    quoted = ",".join('"%s"' % c for c in cols)
    con.executemany(
        "INSERT INTO %s(%s) VALUES (%s)" % (table, quoted, ",".join("?" * len(cols))),
        [tuple(r[c] for c in cols) for r in rows],
    )


def build(image: str, out_dir: str = fw.ATLAS_DIR) -> dict[str, int]:
    image = fw.short_image(image)
    t0 = time.time()
    img = fw.CF(image)
    section = os.path.join(ROOT, "out", "sections", image, "section_3_MAIN_OS.bin")
    os.makedirs(out_dir, exist_ok=True)
    path = os.path.join(out_dir, image + ".sqlite")
    tmp = path + ".tmp.%d" % os.getpid()
    if os.path.exists(tmp):
        os.remove(tmp)
    con = sqlite3.connect(tmp)
    con.executescript(SCHEMA)
    descr = extract_descriptors(img, section)
    insert(con, "descriptors", descr)
    insert(con, "params", extract_params(section))
    classes, vfuncs = extract_classes(img)
    insert(con, "classes", classes)
    con.executemany("INSERT INTO vfuncs VALUES (?,?,?,?,?)", vfuncs)
    insert(con, "ui_strings", extract_ui_strings(img, classes, vfuncs))
    insert(con, "structs", extract_structs(img))
    manual_rows = extract_manual(image)
    insert(con, "manual", manual_rows)
    con.executemany(
        "INSERT INTO glossary VALUES (?,?,?,?,?,?,?)",
        build_glossary(con, img, manual_rows),
    )
    for k, v in (
        ("image", image),
        ("atlas_version", str(ATLAS_VERSION)),
        ("image_sha256", img.meta.get("image_sha256", "")),
        ("built", time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())),
        ("manual", manual_dir(image) or "none"),
    ):
        con.execute("INSERT INTO meta VALUES (?,?)", (k, v))
    con.commit()
    counts = {
        t: con.execute("SELECT count(*) FROM %s" % t).fetchone()[0]
        for t in (
            "descriptors",
            "params",
            "classes",
            "vfuncs",
            "ui_strings",
            "structs",
            "manual",
            "glossary",
        )
    }
    counts["machines"] = con.execute("SELECT count(*) FROM machines").fetchone()[0]
    con.close()
    os.replace(tmp, path)
    counts["seconds"] = round(time.time() - t0)
    return counts


# --- catalog: candidate functions for anchors -----------------------------------------

# Old firmware per image, for the stability column (same h_strict as the matched old function).
CATALOG_OLD = {
    name: b.base
    for name, b in device.boot_images(config.DEVICES_DIR).items()
    if b.base
}
# Score weights (see catalog_score).
W_GRADE, W_PAIR, W_UI_CALLERS, W_NEAR, W_STRING, W_ROLE, W_ANCHORED = (
    4.0,
    2.0,
    1 / 16,
    1.5,
    1.0,
    0.5,
    -3.0,
)
# Itanium C++ ABI: vtable slots 0 and 1 are the destructors (finding 71)
W_DTOR = -2.0
# jump thunks and one-line accessors; stable hashes give them high scores otherwise
W_THUNK = -2.0
THUNK_MAX = 12
DTOR_SUFFIXES = ("::vfunc_0", "::vfunc_1")
UI_CALLER_CAP = 64
HELPER_MIN_CALLERS = 8
CATALOG_ROLES = ("model", "storage", "worker")

CATALOG_COLUMNS = (
    ("addr", "INTEGER"),
    ("name", "TEXT"),
    ("size", "INTEGER"),
    ("class", "TEXT"),
    ("tags", "TEXT"),
    ("ui_callers", "INTEGER"),
    ("ui_strings", "INTEGER"),
    ("hint", "TEXT"),
    ("h_strict", "TEXT"),
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
    ("caller_classes", "TEXT"),
    ("callees_known", "TEXT"),
    ("work_name", "TEXT"),
    ("group", "INTEGER"),
)

TOP_CALLER_CLASSES = 3
TOP_CALLEES_KNOWN = 3
HINT_NAME_CHARS = 16


def is_unnamed(name: str) -> bool:
    return name.startswith(("FUN_", "thunk_FUN_"))


def work_name(
    name: str,
    caller_classes_counts: dict[str, int],
    hint: str,
    callees_known: Sequence[str],
) -> str:
    """Working name of an unnamed function, '' for a named one.

    `caller_classes_counts` maps a class to the number of its vfuncs that call the function.
    The hex digits of the name are the address, so its last 4 digits make the helper name.
    """
    if not is_unnamed(name):
        return ""
    if len(caller_classes_counts) == 1:
        cls, n = next(iter(caller_classes_counts.items()))
        if n >= 2:
            return "%s::helper_%s" % (cls, name[-4:].lower())
    if hint:
        return "str:" + hint[:HINT_NAME_CHARS]
    if callees_known:
        return "calls:" + callees_known[0]
    return ""


def catalog_score(
    grade: int,
    pair_method: str | None,
    ui_callers: int,
    ui_strings: int,
    tags: Sequence[str],
    anchors: Sequence[str],
    name: str,
    size: int,
) -> float:
    return (
        W_GRADE * grade
        + W_PAIR * (pair_method in STRONG_PAIR)
        + W_UI_CALLERS * min(ui_callers, UI_CALLER_CAP)
        + W_NEAR * any(t.startswith("near:") for t in tags)
        + W_STRING * (ui_strings > 0)
        + W_ROLE * any(t.startswith(("role:", "term:")) for t in tags)
        + W_ANCHORED * bool(anchors)
        + W_DTOR * name.endswith(DTOR_SUFFIXES)
        + W_THUNK * (size <= THUNK_MAX)
    )


class CatImg:
    """Facts of one image used by the catalog."""

    def __init__(self) -> None:
        self.fn: dict[int, tuple[str, int, int]] = {}
        self.hs: dict[int, str] = {}
        self.groups: dict[str, list[int]] = {}
        self.vt: dict[int, set[tuple[str, str | None]]] = {}
        self.tags: dict[int, list[str]] = {}
        self.ui_callers: dict[int, int] = {}
        self.strf: dict[int, set[int]] = {}
        self.ui_text: dict[int, str] = {}
        self.anch: dict[int, list[str]] = {}
        self.stable: dict[int, int] = {}
        self.callee_n: dict[int, collections.Counter[int]] = {}
        self.caller_cls: dict[int, collections.Counter[str]] = {}


def _anchor_functions(
    image: str, fn: dict[int, tuple[str, int, int]]
) -> dict[int, list[str]]:
    """Existing ColdFire anchors of devices/ports/<image>.toml by containing function."""
    import bisect
    import tomllib

    path = os.path.join(ROOT, "devices", "ports", image + ".toml")
    if not os.path.exists(path):
        return {}
    with open(path, "rb") as fh:
        port = tomllib.load(fh)
    starts = sorted(fn)
    out: dict[int, list[str]] = {}
    for a in port.get("anchor", []):
        if a.get("cpu") != "cf":
            continue
        x = a.get("src", a["addr"]) if a.get("kind") == "data" else a["addr"]
        i = bisect.bisect_right(starts, x) - 1
        if i >= 0 and x < fn[starts[i]][2] + 1:
            out.setdefault(starts[i], []).append(a["name"])
    return out


def collect_catalog(image: str) -> CatImg:
    """Read cfdb and atlas of `image` and tag the candidate functions."""
    import collections

    old = CATALOG_OLD.get(image)
    im = image + "-emac"
    db = fw.open_ro(os.path.join(fw.CF_DIR, im + ".sqlite"))
    at = fw.open_ro(os.path.join(fw.ATLAS_DIR, image + ".sqlite"))
    d = CatImg()
    d.fn = {
        e: (n, sz, end)
        for e, n, sz, end in db.execute(
            "SELECT entry, name, size, end FROM functions WHERE image=?", (im,)
        )
    }
    fn = d.fn
    d.hs = {
        e: h
        for e, h in db.execute(
            "SELECT entry, h_strict FROM func_hash WHERE image=?", (im,)
        )
        if e in fn
    }
    groups: dict[str, list[int]] = collections.defaultdict(list)
    for e in sorted(d.hs):
        groups[d.hs[e]].append(e)
    d.groups = dict(groups)
    # stability against the old version
    old_path = os.path.join(fw.CF_DIR, "%s-emac.sqlite" % old) if old else ""
    if old and os.path.exists(old_path):
        olddb = fw.open_ro(old_path)
        ohs = dict(
            olddb.execute(
                "SELECT entry, h_strict FROM func_hash WHERE image=?", (old + "-emac",)
            ).fetchall()
        )
        for dst, src in db.execute(
            "SELECT dst_entry, src_entry FROM matches WHERE image=? AND src_image=?",
            (im, old + "-emac"),
        ):
            if dst in d.hs and src in ohs:
                d.stable[dst] = int(ohs[src] == d.hs[dst])
    classes = dict(at.execute("SELECT name, role FROM classes").fetchall())
    ui_cls = {n for n, r in classes.items() if r in VIEW_ROLES}
    for cls, tgt, owner in db.execute(
        "SELECT class, target, owner FROM vtables WHERE image=?", (im,)
    ):
        if tgt and tgt in fn:
            d.vt.setdefault(tgt, set()).add((cls, owner))
    ui_vf = {t for t, s in d.vt.items() if any(c in ui_cls for c, _o in s)}
    ui_own = {
        t
        for t, s in d.vt.items()
        if any(c in ui_cls and (o == c or not o) for c, o in s)
    }
    role_own: dict[str, set[int]] = {r: set() for r in CATALOG_ROLES}
    for t, s in d.vt.items():
        for c, o in s:
            if classes.get(c) in role_own and (o == c or not o):
                role_own[classes[c]].add(t)
    callees: dict[int, set[int]] = collections.defaultdict(set)
    callers: dict[int, set[int]] = collections.defaultdict(set)
    callee_n: dict[int, collections.Counter[int]] = collections.defaultdict(
        collections.Counter
    )
    for f, t in db.execute(
        "SELECT from_func, to_func FROM edges WHERE image=? AND to_func IS NOT NULL "
        "AND kind IN ('call','jump') AND from_func!=to_func",
        (im,),
    ):
        if f in fn and t in fn:
            callees[f].add(t)
            callers[t].add(f)
            callee_n[f][t] += 1
    d.callee_n = dict(callee_n)
    for t, fs in callers.items():
        cc: collections.Counter[str] = collections.Counter()
        for f in fs:
            for c in {o or c for c, o in d.vt.get(f, ())}:
                cc[c] += 1
        if cc:
            d.caller_cls[t] = cc
    cnt: collections.Counter[int] = collections.Counter()
    for f in ui_vf:
        for t in callees[f]:
            if t not in d.vt:
                cnt[t] += 1
    d.ui_callers = dict(cnt)
    d.anch = _anchor_functions(image, fn)
    near: dict[str, set[int]] = collections.defaultdict(set)
    for f, names in d.anch.items():
        for nm in names:
            prefix = ".".join(nm.split(".")[:2])
            near[prefix] |= {f} | callees[f] | callers[f]
    d.ui_text = {
        a: t
        for a, t in reversed(at.execute("SELECT addr, text FROM ui_strings").fetchall())
    }
    for f, a in db.execute("SELECT func, to_addr FROM datarefs WHERE image=?", (im,)):
        if a in d.ui_text and f in fn:
            d.strf.setdefault(f, set()).add(a)
    term_funcs: dict[str, set[int]] = collections.defaultdict(set)
    for term, ref in at.execute("SELECT term, ref FROM glossary WHERE kind='function'"):
        addr = fw.parse_addr(ref)
        if addr is not None and addr in fn:
            term_funcs[term.lower().replace(" ", "_")].add(addr)

    def add(tag: str, funcs: Iterable[int]) -> None:
        for f in funcs:
            d.tags.setdefault(f, []).append(tag)

    add("ui_own_vfunc", ui_own)
    add("ui_vfunc", ui_vf)
    add("ui_helper", [t for t, c in cnt.items() if c >= HELPER_MIN_CALLERS])
    add("ui_string", d.strf)
    for prefix in sorted(near):
        add("near:" + prefix, near[prefix])
    for role in CATALOG_ROLES:
        add("role:" + role, role_own[role])
    for term in sorted(term_funcs):
        add("term:" + term, term_funcs[term])
    return d


def catalog_rows(
    d: CatImg, other: CatImg, pair: dict[int, tuple[int, str]]
) -> list[dict[str, Any]]:
    """Catalog rows of `d`; `pair` maps an address to (address on `other`, method)."""
    rows = []
    for f in sorted(d.tags):
        name, size, _end = d.fn[f]
        tags = d.tags[f]
        h = d.hs.get(f)
        group = d.groups.get(h, []) if h else []
        kind, ordinal = suggest_rule(f, group, f in d.vt) if h else ("none", None)
        p = pair.get(f)
        pair_addr, method = p if p else (None, None)
        same_hash: int | None = None
        both: int | None = None
        if pair_addr is not None:
            oh = other.hs.get(pair_addr)
            same_hash = int(oh is not None and oh == h)
            if kind == "cf_func" and h:
                if oh is None:
                    both = 0
                else:
                    okind, oord = suggest_rule(
                        pair_addr, other.groups.get(oh, []), pair_addr in other.vt
                    )
                    both = int(okind == kind and oord == ordinal and oh == h)
        stable = d.stable.get(f)
        grade = catalog_grade(kind, stable, method, both)
        strs = d.strf.get(f, set())
        anchors = d.anch.get(f, [])
        callers = d.ui_callers.get(f, 0)
        hint = " ".join(d.ui_text[min(strs)].split())[:32] if strs else ""
        cc = d.caller_cls.get(f, collections.Counter())
        known: list[str] = []
        by_calls = sorted(d.callee_n.get(f, {}).items(), key=lambda kv: (-kv[1], kv[0]))
        for t, _n in by_calls:
            nm = d.anch[t][0] if t in d.anch else d.fn[t][0]
            if (t in d.anch or not is_unnamed(nm)) and nm not in known:
                known.append(nm)
        known = known[:TOP_CALLEES_KNOWN]
        rows.append(
            {
                "addr": f,
                "name": name,
                "size": size,
                "class": sorted({c for c, _o in d.vt.get(f, ())})[0]
                if f in d.vt
                else "",
                "tags": ",".join(tags),
                "ui_callers": callers,
                "ui_strings": len(strs),
                "hint": hint,
                "h_strict": h,
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
                "score": catalog_score(
                    grade, method, callers, len(strs), tags, anchors, name, size
                ),
                "caller_classes": ",".join(
                    "%s:%d" % kv for kv in count_list(dict(cc), TOP_CALLER_CLASSES)
                ),
                "callees_known": ",".join(known),
                "work_name": work_name(name, dict(cc), hint, known),
                "group": -1,
            }
        )
    addrs = [cast(int, r["addr"]) for r in rows]
    ids = louvain_groups(addrs, group_edges(addrs, d.callee_n))
    for r, a in zip(rows, addrs, strict=True):
        r["group"] = ids.get(a, -1)
    return rows


def write_catalog(image: str, rows: list[dict[str, Any]], out_dir: str) -> None:
    shared.write_catalog(
        os.path.join(out_dir, image + ".sqlite"),
        os.path.join(out_dir, image + "-catalog.tsv"),
        rows,
        CATALOG_COLUMNS,
    )


def write_catalog_groups(
    image: str,
    rows: list[dict[str, Any]],
    edges: dict[tuple[int, int], int],
    out_dir: str,
) -> int:
    """Write table catalog_groups, the groups TSV and the graph JSON; return the group count."""
    return shared.write_catalog_groups(
        image,
        rows,
        edges,
        os.path.join(out_dir, image + ".sqlite"),
        os.path.join(out_dir, image + "-catalog-groups.tsv"),
        os.path.join(out_dir, image + "-catalog-graph.json"),
    )


def catalog(image: str, other_image: str, out_dir: str = fw.ATLAS_DIR) -> int:
    from tools.coldfire import cfmatch

    names = [fw.short_image(image), fw.short_image(other_image)]
    for n in names:
        if not os.path.exists(os.path.join(out_dir, n + ".sqlite")):
            print("run atlas build %s first" % n)
            return 2
    mt = cfmatch.load_pair(names[0] + "-emac", names[1] + "-emac")
    fwd = {s: (t, mt.method[s]) for s, t in mt.m.items()}
    rev = {t: (s, mt.method[s]) for s, t in mt.m.items()}
    a, b = collect_catalog(names[0]), collect_catalog(names[1])
    for n, d, o, pair in ((names[0], a, b, fwd), (names[1], b, a, rev)):
        rows = catalog_rows(d, o, pair)
        write_catalog(n, rows, out_dir)
        n_groups = write_catalog_groups(
            n, rows, group_edges([r["addr"] for r in rows], d.callee_n), out_dir
        )
        counts: dict[str, int] = {}
        for r in rows:
            for t in r["tags"].split(","):
                key = t.split(":")[0]
                counts[key] = counts.get(key, 0) + 1
        print(
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
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build", help="build out/atlas/<image>.sqlite")
    b.add_argument("images", nargs="+")
    b.add_argument("--out-dir", default=fw.ATLAS_DIR)
    c = sub.add_parser(
        "catalog", help="candidate anchor functions of IMAGE, paired with OTHER"
    )
    c.add_argument("image")
    c.add_argument("other")
    c.add_argument("--out-dir", default=fw.ATLAS_DIR)
    args = ap.parse_args(argv)
    if args.cmd == "catalog":
        return catalog(args.image, args.other, args.out_dir)
    for image in args.images:
        counts = build(image, args.out_dir)
        print("%s: %s" % (image, ", ".join("%s=%s" % kv for kv in counts.items())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
