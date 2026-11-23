"""ColdFire queries over the Ghidra export and its indexes."""

from __future__ import annotations

import os
import re
import sqlite3
import sys
from collections.abc import Sequence
from typing import Any

from tools.firmware.query_db import (
    ATLAS_DIR,
    CF_DIR,
    ROOT,
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


def tri_query(term: str) -> str:
    """Trigram FTS query: AND of words, or one phrase when a word is under 3 characters."""
    words = [w.replace('"', "") for w in term.split()]
    if not words:
        raise FwError("empty search term")
    if any(len(w) < 3 for w in words):
        return '"%s"' % " ".join(words)
    return " AND ".join('"%s"' % w for w in words)


def fts_query(term: str, prefix: bool = False) -> str:
    """AND of the quoted words in `term` as an FTS5 query."""
    words = [w.replace('"', "") for w in term.split() if w.replace('"', "")]
    if not words:
        raise FwError("empty search term")
    return " AND ".join('"%s"%s' % (w, "*" if prefix else "") for w in words)


class CF:
    cpu = "cf"
    address_width = 8

    def __init__(self, image: str):
        self.image = short_image(image)
        self.db_name = self.image + "-emac"
        self.path = os.path.join(CF_DIR, self.db_name + ".sqlite")
        self.db = open_ro(self.path)
        self.meta = dict(self.db.execute("SELECT key, value FROM meta").fetchall())
        self._graph: Graph | None = None
        self._fts: sqlite3.Connection | None = None
        self._names: dict[int, str] | None = None
        self._starts: list[int] | None = None
        self.graph_state = ""

    # names and lookup
    def names(self) -> dict[int, str]:
        if self._names is None:
            from tools.coldfire import cfdb

            self._names = cfdb._image_names(self.db_name)
        return self._names

    def func_row(self, addr: int) -> tuple[Any, ...] | None:
        return self.db.execute(
            "SELECT entry, end, name, namespace, size, signature, decomp_path, disasm_path "
            "FROM functions WHERE entry<=? AND end>? ORDER BY entry DESC LIMIT 1",
            (addr, addr),
        ).fetchone()

    def resolve(self, token: str) -> int:
        """Entry of the function named or containing `token`."""
        addr = parse_addr(token)
        if addr is None:
            for k in self.names():
                if token == _cfname(self, k):
                    addr = k
                    break
        if addr is None:
            row = self.db.execute(
                "SELECT entry FROM functions WHERE name=?", (token,)
            ).fetchone()
            if row is None:
                row = self.db.execute(
                    "SELECT entry FROM functions WHERE name LIKE ? ORDER BY entry LIMIT 1",
                    (token + "%",),
                ).fetchone()
            if row is None:
                raise FwError(
                    "no function matches %r (try: search %s)" % (token, token)
                )
            addr = row[0]
        f = self.func_row(addr)
        if f is None:
            raise FwError("0x%x is not inside a known function" % addr)
        return f[0]

    def label(self, entry: int) -> str:
        n = self.names().get(entry)
        row = self.db.execute(
            "SELECT name FROM functions WHERE entry=?", (entry,)
        ).fetchone()
        g = row[0] if row else "FUN_%08x" % entry
        return "%08x %s%s" % (entry, g, " [%s]" % n if n else "")

    def label_any(self, addr: int) -> str:
        f = self.func_row(addr)
        if f is None:
            return "%08x" % addr
        if f[0] == addr:
            return self.label(addr)
        return "%s+0x%x" % (self.label(f[0]), addr - f[0])

    def graph(self) -> Graph:
        if self._graph is None:
            self._graph, self.graph_state = Graph.load(
                self.db_name,
                db_key(self.db, self.path),
                self.db.execute(
                    "SELECT DISTINCT from_func, to_func FROM edges WHERE kind='call' "
                    "AND from_func IS NOT NULL AND to_func IS NOT NULL"
                ),
            )
        return self._graph

    def fts(self) -> sqlite3.Connection:
        if self._fts is None:
            from tools.coldfire import cfdb

            path = cfdb.fts_path(self.path)
            if not os.path.exists(path):
                print("# building full-text index (one time)", file=sys.stderr)
                cfdb.build_fts(self.path)
            self._fts = open_ro(path)
        return self._fts

    def read_dump(self, rel: str | None) -> str | None:
        dump = self.meta.get("ghidra_dump_dir")
        if not rel or not dump:
            return None
        p = os.path.join(dump, rel)
        if not os.path.exists(p):
            p = os.path.join(ROOT, "out", "ghidra", self.db_name, rel)
        try:
            with open(p, errors="replace") as fh:
                return fh.read()
        except OSError:
            return None

    # queries
    def callers(self, entry: int) -> list[tuple[int, int, str]]:
        """(from_func, site, kind) for edges into the function or its entry."""
        return self.db.execute(
            "SELECT from_func, site, kind FROM edges WHERE to_addr=? OR to_func=? ORDER BY from_func, site",
            (entry, entry),
        ).fetchall()

    def callees(self, entry: int) -> list[tuple[int, str]]:
        return self.db.execute(
            "SELECT DISTINCT to_func, kind FROM edges WHERE from_func=? AND to_func IS NOT NULL ORDER BY to_func",
            (entry,),
        ).fetchall()

    def strings_of(self, entry: int, limit: int = 8) -> list[tuple[int, str]]:
        return self.db.execute(
            "SELECT DISTINCT s.addr, s.text FROM datarefs d JOIN strings s ON s.addr=d.to_addr "
            "WHERE d.func=? ORDER BY s.addr LIMIT ?",
            (entry, limit),
        ).fetchall()

    def vtable_slots(self, entry: int) -> list[tuple[str, int]]:
        return self.db.execute(
            "SELECT DISTINCT class, slot FROM vtables WHERE target=? ORDER BY class, slot",
            (entry,),
        ).fetchall()


def _cfname(img: CF, entry: int) -> str | None:
    return img.names().get(entry)


def cmd_fn(img: CF, token: str, lines_n: int = 12) -> list[str]:
    entry = img.resolve(token)
    f = img.func_row(entry)
    assert f is not None
    out = ["%s  size=%d  end=%08x" % (img.label(entry), f[4], f[1])]
    if f[5]:
        out.append("sig: " + clip(f[5]))
    slots = img.vtable_slots(entry)
    if slots:
        out.append("vtable: " + ", ".join("%s#%d" % s for s in slots[:6]))
    callers = img.callers(entry)
    cfuncs = sorted({c[0] for c in callers if c[0] is not None})
    callees = img.callees(entry)
    out.append("callers: %d sites in %d functions" % (len(callers), len(cfuncs)))
    for c in cfuncs[:5]:
        out.append("  <- " + img.label(c))
    if len(cfuncs) > 5:
        out.append("  <- ... %d more" % (len(cfuncs) - 5))
    out.append("callees: %d" % len(callees))
    for c, _k in callees[:6]:
        out.append("  -> " + img.label(c))
    if len(callees) > 6:
        out.append("  -> ... %d more" % (len(callees) - 6))
    strs = img.strings_of(entry)
    if strs:
        out.append("strings:")
        for a, t in strs:
            out.append("  %08x %s" % (a, repr(clip(t, 70))))
    text = img.read_dump(f[6])
    if text:
        body = _decomp_body(text)
        out.append("decomp (%d of %d lines):" % (min(lines_n, len(body)), len(body)))
        out.extend("  " + ln[:110] for ln in body[:lines_n])
    else:
        out.append("decomp: none (%s)" % ("failed" if f[6] is None else "missing file"))
    return out


_DECL = re.compile(r"^\s+[A-Za-z_][\w ]*[ *]\**\w+(\[\w+\])?;$")
_STMT = ("return", "goto", "break", "continue", "else", "case", "default")


def _decomp_body(text: str) -> list[str]:
    """Decompiled C without comments, blank lines and the local declarations."""
    out: list[str] = []
    header = True
    for ln in text.splitlines():
        if not ln.strip() or ln.lstrip().startswith(("/*", "*", "//")):
            continue
        if header:
            st = ln.strip()
            if st == "{" or (_DECL.match(ln) and not st.startswith(_STMT)):
                continue
            if ln[:1] not in (" ", "\t"):
                out.append(ln)
                continue
            header = False
        out.append(ln)
    return out


def cmd_decomp(img: CF, token: str) -> list[str]:
    entry = img.resolve(token)
    f = img.func_row(entry)
    assert f is not None
    text = img.read_dump(f[6])
    if text is None:
        raise FwError(
            "no decompiled C for %s (decompile failed); use disasm" % img.label(entry)
        )
    return [img.label(entry)] + text.splitlines()


def cmd_disasm(img: CF, token: str) -> list[str]:
    entry = img.resolve(token)
    f = img.func_row(entry)
    assert f is not None
    text = img.read_dump(f[7])
    if text is None:
        raise FwError("no disassembly file for %s" % img.label(entry))
    return [img.label(entry)] + [
        ln for ln in text.splitlines() if not ln.startswith(";")
    ][1:]


def cmd_xref(img: CF, token: str) -> list[str]:
    addr = parse_addr(token)
    if addr is None:
        addr = img.resolve(token)
    out = ["xref %08x  %s" % (addr, img.label_any(addr))]
    s = img.db.execute("SELECT text FROM strings WHERE addr=?", (addr,)).fetchone()
    if s:
        out.append("  string %r" % clip(s[0], 80))
    for site, fn, kind, _label in img.db.execute(
        "SELECT site, func, kind, label FROM datarefs WHERE to_addr=? ORDER BY site",
        (addr,),
    ):
        out.append(
            "  data %-10s %08x in %s"
            % (kind, site, img.label(fn) if fn else "(outside functions)")
        )
    for site, fn, kind in img.db.execute(
        "SELECT site, from_func, kind FROM edges WHERE to_addr=? ORDER BY site", (addr,)
    ):
        out.append(
            "  code %-10s %08x in %s"
            % (kind, site, img.label(fn) if fn else "(outside functions)")
        )
    for cls, slot, vt in img.db.execute(
        "SELECT class, slot, addr FROM vtables WHERE target=? ORDER BY class, slot",
        (addr,),
    ):
        out.append("  vtable %s slot %d (vtable %08x)" % (cls, slot, vt))
    if len(out) == 1:
        out.append(
            "  no references recorded (Ghidra misses code outside functions; try uv run python -m tools.coldfire.refscan)"
        )
    return out


def cmd_str(img: CF, term: str) -> list[str]:
    rows = _find_strings(img, term, 200)
    out = ["%d strings match %r" % (len(rows), term)]
    for addr, text in rows:
        users = img.db.execute(
            "SELECT DISTINCT func FROM datarefs WHERE to_addr=? AND func IS NOT NULL ORDER BY func",
            (addr,),
        ).fetchall()
        out.append("%08x %s" % (addr, repr(clip(text, 70))))
        for (u,) in users[:3]:
            out.append("    used by " + img.label(u))
        if len(users) > 3:
            out.append("    used by ... %d more" % (len(users) - 3))
        if not users:
            out.append("    no direct users recorded")
    return out


def _find_strings(img: CF, term: str, limit: int) -> list[tuple[int, str]]:
    if len(term) >= 3:
        fts = img.fts()
        ids = fts.execute(
            "SELECT rowid FROM fts_strings WHERE fts_strings MATCH ? ORDER BY rank LIMIT ?",
            (tri_query(term), limit),
        ).fetchall()
        if ids:
            marks = ",".join(str(i[0]) for i in ids)
            addrs = fts.execute(
                "SELECT id, addr FROM string_rows WHERE id IN (%s)" % marks
            ).fetchall()
            out = []
            for _id, a in sorted(addrs, key=lambda r: r[1]):
                row = img.db.execute(
                    "SELECT text FROM strings WHERE addr=? LIMIT 1", (a,)
                ).fetchone()
                if row:
                    out.append((a, row[0]))
            return out
        return []
    return img.db.execute(
        "SELECT addr, text FROM strings WHERE text LIKE ? ORDER BY addr LIMIT ?",
        ("%" + term + "%", limit),
    ).fetchall()


def _bases(img: CF, bases: str | None) -> str:
    if not bases:
        return "-"
    names = []
    for tok in re.findall(r"\d+", bases):
        r = img.db.execute(
            "SELECT name FROM typeinfos WHERE addr=?", (int(tok),)
        ).fetchone()
        names.append(r[0] if r else tok)
    return ", ".join(names) or "-"


def cmd_class(img: CF, name: str) -> list[str]:
    rows = img.db.execute(
        "SELECT name, kind, bases, addr FROM typeinfos WHERE name=? ORDER BY addr",
        (name,),
    ).fetchall()
    if not rows:
        rows = img.db.execute(
            "SELECT name, kind, bases, addr FROM typeinfos WHERE name LIKE ? AND kind NOT IN ('pointer','fundamental','function') "
            "AND name NOT LIKE 'std::_Sp%' ORDER BY name",
            ("%" + name + "%",),
        ).fetchall()
        if not rows:
            raise FwError("no class like %r" % name)
        if len(rows) > 1:
            return ["%d classes match %r (use the exact name):" % (len(rows), name)] + [
                "  %s  (%s)" % (r[0], r[1]) for r in rows
            ]
    cname, kind, bases, taddr = rows[0]
    out = ["%s  %s  typeinfo=%08x  bases=%s" % (cname, kind, taddr, _bases(img, bases))]
    derived = img.db.execute(
        "SELECT name FROM typeinfos WHERE bases LIKE ? ORDER BY name",
        ("%" + cname + "%",),
    ).fetchall()
    if derived:
        out.append(
            "derived: "
            + ", ".join(d[0] for d in derived[:12])
            + (" ..." if len(derived) > 12 else "")
        )
    vts = img.db.execute(
        "SELECT addr, offset_to_top, slot, target, owner FROM vtables WHERE class=? ORDER BY addr, slot",
        (cname,),
    ).fetchall()
    if not vts:
        out.append("no vtable")
        return out
    cur = None
    for addr, ott, slot, target, owner in vts:
        if addr != cur:
            out.append("vtable %08x (offset_to_top %d)" % (addr, ott))
            cur = addr
        n = img.names().get(target)
        out.append(
            "  #%-3d %s%s%s"
            % (
                slot,
                img.label(target) if target else "0",
                "" if owner == cname else "  <" + str(owner) + ">",
                "",
            )
        )
        del n
    return out


def cmd_port(a: CF, b: CF, name: str) -> list[str]:
    entry = a.resolve(name)
    out = ["%s  (%s) -> %s" % (a.label(entry), a.image, b.image)]
    rows = b.db.execute(
        "SELECT dst_entry, method FROM matches WHERE src_image=? AND src_entry=?",
        (a.db_name, entry),
    ).fetchall()
    for dst_entry, method in rows:
        out.append("  %s  [%s]" % (b.label(dst_entry), method))
    if not rows:
        rev = b.db.execute(
            "SELECT src_entry, method FROM matches WHERE src_image=? AND dst_entry=?",
            (a.db_name, entry),
        ).fetchall()
        for e, method in rev:
            out.append("  (reverse) %s [%s]" % (b.label(e), method))
    if len(out) == 1:
        out.append(
            "  no recorded match (uv run python -m tools.coldfire.cfmatch run %s %s writes them)"
            % (a.db_name, b.db_name)
        )
    return out


def cmd_search(img: CF, term: str, where: str, cap: int) -> list[str]:
    parts = set(where.split(","))
    out: list[str] = []
    per = max(6, cap // 4) if cap else 50
    fts = img.fts()
    if "names" in parts:
        if len(term) >= 3:
            rows = fts.execute(
                "SELECT rowid, name, cf_name, klass FROM fts_names WHERE fts_names MATCH ? ORDER BY rank LIMIT ?",
                (tri_query(term), per * 2),
            ).fetchall()
        else:
            rows = []
        out.append("names: %d" % len(rows))
        for rowid, name, cfn, _k in rows:
            out.append("  %08x %s%s" % (rowid, name, " [%s]" % cfn if cfn else ""))
    if "strings" in parts:
        rows2 = _find_strings(img, term, per)
        out.append("strings: %d" % len(rows2))
        for a, t in rows2:
            users = img.db.execute(
                "SELECT DISTINCT func FROM datarefs WHERE to_addr=? AND func IS NOT NULL LIMIT 2",
                (a,),
            ).fetchall()
            out.append(
                "  %08x %s%s"
                % (
                    a,
                    repr(clip(t, 60)),
                    "  <- " + ", ".join("%08x" % u[0] for u in users) if users else "",
                )
            )
    if "decomp" in parts:
        hits = fts.execute(
            "SELECT rowid FROM fts_decomp WHERE fts_decomp MATCH ? ORDER BY rank LIMIT ?",
            (fts_query(term, prefix=True), per),
        ).fetchall()
        out.append(
            "decomp: %d functions (best %d)" % (_count_decomp(fts, term), len(hits))
        )
        words = [w.lower() for w in term.split()]
        for (rowid,) in hits:
            f = img.func_row(rowid)
            line = ""
            if f:
                text = img.read_dump(f[6]) or ""
                for ln in text.splitlines():
                    if any(w in ln.lower() for w in words):
                        line = ln
                        break
            out.append("  %s : %s" % (img.label(rowid), clip(line, 70)))
    if "atlas" in parts and os.path.exists(
        os.path.join(ATLAS_DIR, img.image + ".sqlite")
    ):
        out.append("atlas:")
        out.extend("  " + ln for ln in cmd_atlas(img.image, term, 8))
    return out


def _count_decomp(fts: sqlite3.Connection, term: str) -> int:
    return fts.execute(
        "SELECT count(*) FROM fts_decomp WHERE fts_decomp MATCH ?",
        (fts_query(term, prefix=True),),
    ).fetchone()[0]


def cmd_atlas(image: str, term: str, per: int = 15) -> list[str]:
    out = _atlas_one(image, term, per)
    words = term.split()
    if len(words) > 1 and out and out[0].startswith("atlas has nothing"):
        out = []
        for w in words:
            out.append("-- %s" % w)
            out.extend(_atlas_one(image, w, max(4, per // len(words))))
    return out


def _atlas_one(image: str, term: str, per: int) -> list[str]:
    path = os.path.join(ATLAS_DIR, short_image(image) + ".sqlite")
    if not os.path.exists(path):
        raise FwError(
            "no atlas for %s: run uv run python -m tools.coldfire.atlas build %s"
            % (short_image(image), short_image(image))
        )
    con = open_ro(path)
    like = "%" + term + "%"
    out: list[str] = []
    g = con.execute(
        "SELECT term, kind, ref, detail, confidence FROM glossary WHERE term LIKE ? ORDER BY term, instr('function class struct descriptor param string manual', kind), score DESC",
        (like,),
    ).fetchall()
    if g:
        out.append("glossary %r: %d rows" % (term, len(g)))
        kinds: dict[str, int] = {}
        for _t, kind, ref, detail, conf in g:
            kinds[kind] = kinds.get(kind, 0) + 1
            if kinds[kind] <= per // 3 + 2:
                out.append(
                    "  %-9s %s  %s (%s)" % (kind, ref, clip(detail or "", 70), conf)
                )
    for table, sql, fmt in (
        (
            "descriptors",
            "SELECT idx, role, name1, name2, addr FROM descriptors WHERE name1 LIKE ?1 OR name2 LIKE ?1 OR role LIKE ?1 LIMIT ?2",
            "  descriptor #%s %s %r/%r @%#x",
        ),
        (
            "params",
            "SELECT code, long_name, short_name, owner_name, addr FROM params WHERE long_name LIKE ?1 OR short_name LIKE ?1 OR owner_name LIKE ?1 LIMIT ?2",
            "  param %s %r %r owner=%s @%#x",
        ),
        (
            "classes",
            "SELECT name, role, vtable, nslots FROM classes WHERE name LIKE ?1 OR role LIKE ?1 ORDER BY name LIMIT ?2",
            "  class %s role=%s vtable=%#x slots=%s",
        ),
        (
            "ui_strings",
            "SELECT text, view_class, user_func, confidence FROM ui_strings WHERE text LIKE ?1 OR view_class LIKE ?1 LIMIT ?2",
            "  ui %r in %s fn=%#x (%s)",
        ),
    ):
        try:
            rows = con.execute(sql, (like, per)).fetchall()
        except sqlite3.Error:
            continue
        if rows:
            out.append("%s: %d shown" % (table, len(rows)))
            for r in rows:
                try:
                    out.append(fmt % tuple(r))
                except (TypeError, ValueError):
                    out.append("  " + " ".join(str(x) for x in r))
    if not out:
        out.append("atlas has nothing for %r" % term)
    return out


_DIS = re.compile(r"^([0-9a-f]{8})  [0-9a-f ]+?\s{2,}(\S.*)$")
_A0_LOAD = re.compile(r"^(movea\.l|move\.l|lea)\s.*,A0$")


def parse_disasm(text: str) -> list[tuple[int, str]]:
    """(address, instruction text) of the instruction lines of a Ghidra disassembly dump."""
    out = []
    for ln in text.splitlines():
        m = _DIS.match(ln)
        if m:
            out.append((int(m.group(1), 16), m.group(2).strip()))
    return out


def hidden_return_ptr(insn: str) -> bool:
    """True for a load of A0 (the register that carries a hidden struct return pointer)."""
    return bool(_A0_LOAD.match(insn))


def call_setup(insns: Sequence[tuple[int, str]], site: int, n: int) -> list[str]:
    """The N instructions before the call at SITE and the call itself ('>'), A0 loads marked."""
    before = [i for i in insns if i[0] < site][-n:]
    lines = [
        "    %08x  %s%s"
        % (a, t, "   ; A0: hidden return ptr?" if hidden_return_ptr(t) else "")
        for a, t in before
    ]
    lines += ["  > %08x  %s" % (a, t) for a, t in insns if a == site]
    return lines


def anchors_for(
    entries: Sequence[dict[str, Any]], image: str, entry: int, end: int
) -> list[str]:
    """Declared ColdFire anchors on IMAGE that point into [entry, end) or whose doc names the entry address."""
    out = []
    for e in entries:
        if e.get("cpu") != "cf":
            continue
        spots = [int(s["addr"], 16) for s in e.get("at", {}).get(image, [])]
        inside = [a for a in spots if entry <= a < end]
        if inside or "0x%x" % entry in (e.get("doc") or ""):
            where = "+0x%x" % (inside[0] - entry) if inside else " (doc)"
            out.append("%s [%s]%s" % (e["name"], e.get("mark", "?"), where))
    return out


def anchor_addrs(entries: Sequence[dict[str, Any]], image: str) -> dict[int, str]:
    out: dict[int, str] = {}
    for e in entries:
        if e.get("cpu") == "cf":
            for s in e.get("at", {}).get(image, []):
                out.setdefault(int(s["addr"], 16), e["name"])
    return out


def catalog_row(image: str, entry: int) -> dict[str, Any] | None:
    path = os.path.join(ATLAS_DIR, short_image(image) + ".sqlite")
    if not os.path.exists(path):
        return None
    con = open_ro(path)
    cols = (
        "h_strict",
        "hash_count",
        "pair_addr",
        "pair_method",
        "rule_kind",
        "rule_ordinal",
        "rule_both",
    )
    try:
        if not _has(con, "catalog"):
            return None
        row = con.execute(
            "SELECT %s FROM catalog WHERE addr=?" % ", ".join(cols), (entry,)
        ).fetchone()
    except sqlite3.OperationalError:
        return None
    finally:
        con.close()
    return dict(zip(cols, row, strict=True)) if row else None


def hash_entries(img: CF, h: str | None) -> list[int]:
    if h is None:
        return []
    return [
        r[0]
        for r in img.db.execute(
            "SELECT entry FROM func_hash WHERE h_strict=? ORDER BY entry", (h,)
        )
    ]


def own_slot(img: CF, entry: int) -> tuple[str, int, int] | None:
    """(class, offset_to_top, slot) of a vtable of its own class that holds ENTRY, or None."""
    return img.db.execute(
        "SELECT class, offset_to_top, slot FROM vtables WHERE target=? AND owner=class "
        "ORDER BY offset_to_top<>0, abs(offset_to_top), class LIMIT 1",
        (entry,),
    ).fetchone()


def rule_targets(
    img: CF,
    kind: str,
    ordinal: int | None,
    h: str | None,
    slot: tuple[str, int, int] | None,
) -> list[int]:
    """Where the rule (kind, ordinal, hash or vtable slot) resolves on IMG."""
    if kind == "cf_func":
        hits = hash_entries(img, h)
        return hits if ordinal is None else hits[ordinal : ordinal + 1]
    if kind == "cf_vtable_slot" and slot:
        return [
            r[0]
            for r in img.db.execute(
                "SELECT DISTINCT target FROM vtables WHERE class=? AND offset_to_top=? AND slot=?",
                slot,
            )
        ]
    return []


def _caller_lines(img: CF, entry: int, n_callers: int, n_insn: int) -> list[str]:
    rows = [r for r in img.callers(entry) if r[0] is not None and r[0] != entry]
    funcs = sorted({r[0] for r in rows})
    sites = len(img.callers(entry))
    out = ["callers: %d sites in %d functions" % (sites, len(funcs))]
    seen: set[int] = set()
    for fn, site, kind in rows:
        if fn in seen:
            continue
        if len(seen) >= n_callers:
            break
        seen.add(fn)
        out.append("  <- %s @%08x %s" % (img.label(fn), site, kind[:4]))
        frow = img.func_row(fn)
        text = img.read_dump(frow[7]) if frow else None
        if text:
            out.extend(call_setup(parse_disasm(text), site, n_insn))
    if len(funcs) > len(seen):
        out.append("  ... %d more callers (callers ADDR)" % (len(funcs) - len(seen)))
    return out


def find_pair(
    img: CF,
    other: CF,
    entry: int,
    row: dict[str, Any] | None,
    rule: tuple[str, int | None, str | None, tuple[str, int, int] | None],
) -> tuple[int | None, str]:
    """The function on OTHER that matches ENTRY and how it was found."""
    if row and row["pair_addr"] is not None:
        return row["pair_addr"], "catalog pair_addr (%s)" % row["pair_method"]
    kind, ordinal, h, slot = rule
    hits = rule_targets(other, kind, ordinal, h, slot)
    if len(hits) == 1:
        return hits[0], "rule %s resolves uniquely" % kind
    rows = other.db.execute(
        "SELECT dst_entry, method FROM matches WHERE src_image=? AND src_entry=?",
        (img.db_name, entry),
    ).fetchall()
    if rows:
        return rows[0][0], "cfmatch %s%s" % (
            rows[0][1],
            " (%d candidates)" % len(rows) if len(rows) > 1 else "",
        )
    rev = other.db.execute(
        "SELECT src_entry, method FROM matches WHERE src_image=? AND dst_entry=?",
        (img.db_name, entry),
    ).fetchall()
    if rev:
        return rev[0][0], "cfmatch %s (reverse)" % rev[0][1]
    return None, "no pair found (try: port %s --from %s --to %s)" % (
        "0x%x" % entry,
        img.image,
        other.image,
    )


def cmd_dossier(
    img: CF,
    token: str,
    other: CF | None,
    n_callers: int = 3,
    n_decomp: int = 12,
) -> list[str]:
    """Summarize a ColdFire function and its optional cross-image pair."""
    entry = img.resolve(token)
    f = img.func_row(entry)
    assert f is not None
    from tools.firmware.catalog import suggest_rule

    refs = load_reference()
    hrow = img.db.execute(
        "SELECT h_strict FROM func_hash WHERE entry=?", (entry,)
    ).fetchone()
    h = hrow[0] if hrow else None
    same = hash_entries(img, h)
    how = ""
    out = [
        "dossier %s %s" % (img.image, img.label(entry)),
        "entry=%08x size=%d end=%08x hash=%s (%d with this hash)"
        % (entry, f[4], f[1], h[:12] if h else "none", len(same)),
    ]
    if f[5]:
        out.append("sig: " + clip(f[5]))
    slot = own_slot(img, entry)
    slots = img.vtable_slots(entry)
    if slots:
        more = " ..." if len(slots) > 6 else ""
        out.append("vfunc: " + ", ".join("%s#%d" % s for s in slots[:6]) + more)
    else:
        out.append("vfunc: no")
    strs = img.strings_of(entry, 6)
    if strs:
        out.append("strings: " + "; ".join(repr(clip(t, 40)) for _a, t in strs))
    anchors = anchors_for(refs, img.image, entry, f[1])
    out.append("anchors: " + ("; ".join(anchors) if anchors else "none"))
    row = catalog_row(img.image, entry)
    if row and row["rule_kind"]:
        kind, ordinal, source = row["rule_kind"], row["rule_ordinal"], "catalog"
    else:
        kind, ordinal = suggest_rule(entry, same, slot is not None)
        source = "computed"
    ord_txt = " ordinal=%d" % ordinal if ordinal is not None else ""
    slot_txt = (
        " class=%s slot=%d" % (slot[0], slot[2])
        if kind == "cf_vtable_slot" and slot
        else ""
    )
    both_txt = (
        " both_devices=%s" % row["rule_both"]
        if row and row["rule_both"] is not None
        else ""
    )
    out.append("rule (%s): %s%s%s%s" % (source, kind, ord_txt, slot_txt, both_txt))
    rule = (kind, ordinal, h, slot)
    pair: int | None = None
    if other is not None:
        got = rule_targets(other, kind, ordinal, h, slot)
        pair, how = find_pair(img, other, entry, row, rule)
        out.append(
            "on %s the rule resolves to: %s%s"
            % (
                other.image,
                ", ".join("%08x" % a for a in got) if got else "nothing",
                "  (the pair)" if pair is not None and got == [pair] else "",
            )
        )
    out.extend(_caller_lines(img, entry, n_callers, 8))
    callees = img.callees(entry)
    named = anchor_addrs(refs, img.image)
    ranked = sorted(
        callees,
        key=lambda c: (c[0] not in named, img.names().get(c[0]) is None, c[0]),
    )
    out.append("callees: %d" % len(callees))
    for c, _k in ranked[:8]:
        tag = "  {%s}" % named[c] if c in named else ""
        out.append("  -> %s%s" % (img.label(c), tag))
    if len(ranked) > 8:
        out.append("  -> ... %d more" % (len(ranked) - 8))
    text = img.read_dump(f[6])
    if text:
        body = _decomp_body(text)
        out.append("decomp (%d of %d lines):" % (min(n_decomp, len(body)), len(body)))
        out.extend("  " + ln[:110] for ln in body[:n_decomp])
    else:
        out.append("decomp: none")
    if other is not None:
        out.append("")
        if pair is None:
            out.append("pair on %s: %s" % (other.image, how))
        else:
            pf = other.func_row(pair)
            ph = other.db.execute(
                "SELECT h_strict FROM func_hash WHERE entry=?", (pair,)
            ).fetchone()
            out.append("pair on %s: %s" % (other.image, other.label(pair)))
            out.append("  found by: " + how)
            if pf:
                sig = "  sig: " + clip(pf[5], 80) if pf[5] else ""
                out.append(
                    "  size=%d same strict hash=%s%s"
                    % (pf[4], "yes" if ph and ph[0] == h else "no", sig)
                )
            out.extend("  " + ln for ln in _caller_lines(other, pair, 2, 4))
    return out
