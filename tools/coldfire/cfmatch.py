#!/usr/bin/env python3
"""Match the functions of two ColdFire MAIN OS images and carry names across.

    uv run python -m tools.coldfire.cfmatch match dt2-1.16-emac dt2-1.17-emac
        [--eval out/vt/dt2-1.15C_to_dt2-1.16.json] [--write] [--json OUT]
    uv run python -m tools.coldfire.cfmatch port SRC DST            # names into DST's cfdb
    uv run python -m tools.coldfire.cfmatch port-names SRC DST --out tools/coldfire/cf_names_x.py

SRC and DST are cfdb names (out/cfdb/<name>.sqlite, see tools/coldfire/cfdb.py; a cfdb
older than DB_VERSION is rebuilt from its Ghidra dump by tools/coldfire/cf.py's loader).
The facts they hold come from tools/coldfire/cffacts.py: three masked instruction hashes
per function. The matcher is deterministic and needs no Ghidra:

  1. strict    a hash that is unique among the unmatched functions of both images
  2. loose     the same with every number masked (a changed displacement)
  3. strings   the same set of referenced strings, unique on both sides
  4. mnem      the same mnemonic sequence of at least MIN_MNEM instructions, unique
  5. vtable    same class (typeinfo name), same vtable slot, plausible code
  6. calls     callees of a matched pair, aligned by their order of call sites and
               by the matches already known; callers likewise
  7. gap       unmatched functions between two matched neighbours in address
               order (the linker keeps the order of a translation unit)

Stages 5 to 7 only pair functions whose mnemonic trigram similarity is at least
SIM_MIN; they repeat until nothing changes. Every pair keeps the stage that made
it, so a consumer can trust the early stages more.
"""

from __future__ import annotations

import argparse
import collections
import difflib
import json
import os
import re
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent

MIN_MNEM = 12  # shortest function the mnemonic-only stage may pair
SIM_MIN = 0.45  # mnemonic-trigram Jaccard for call-graph and vtable pairs
SIM_GAP = 0.40  # the same for address-order pairs
DEFAULT_RE = re.compile(r"^(FUN_|LAB_|DAT_|thunk_FUN_|switchD_|caseD_|PTR_|SUB_)")
MIN_STRING = 5


class Img:
    """The matching facts of one image, read from its cfdb."""

    def __init__(self, name: str, db_dir: str | None = None):
        from tools.coldfire import cf

        self.cf = cf.load(name, db_dir=db_dir or cf.DB_DIR)
        self.name = name
        db = self.cf.db
        im = name
        self.size: dict[int, int] = {}
        self.fname: dict[int, str] = {}
        for e, n, sz in db.execute(
            "SELECT entry, name, size FROM functions WHERE image=?", (im,)
        ):
            self.fname[e] = n
            self.size[e] = sz
        self.h_strict: dict[int, str] = {}
        self.h_loose: dict[int, str] = {}
        self.h_mnem: dict[int, str] = {}
        self.ninsn: dict[int, int] = {}
        self.mnem: dict[int, list[str]] = {}
        for e, ni, hs, hl, hm, seq in db.execute(
            "SELECT entry, ninsn, h_strict, h_loose, h_mnem, mnem_seq "
            "FROM func_hash WHERE image=?",
            (im,),
        ):
            self.ninsn[e] = ni
            self.h_strict[e] = hs
            self.h_loose[e] = hl
            self.h_mnem[e] = hm
            self.mnem[e] = seq.split(" ") if seq else []
        entries = set(self.fname)
        self.entries = sorted(entries)
        # Callees in call-site order (calls and tail jumps to another function).
        calls: dict[int, list[int]] = collections.defaultdict(list)
        callers: dict[int, set[int]] = collections.defaultdict(set)
        for frm, to, _site, kind in db.execute(
            "SELECT from_func, to_func, site, kind FROM edges WHERE image=? "
            "AND to_func IS NOT NULL AND from_func != to_func ORDER BY site",
            (im,),
        ):
            if kind == "cond_jump" or to not in entries or frm not in entries:
                continue
            calls[frm].append(to)
            callers[to].add(frm)
        self.calls = calls
        self.callers = callers
        # Strings referenced by each function.
        text = {
            a: t
            for a, t in db.execute(
                "SELECT addr, text FROM strings WHERE image=?", (im,)
            )
            if len(t) >= MIN_STRING and t.isprintable()
        }
        strs: dict[int, set[str]] = collections.defaultdict(set)
        for f, a in db.execute(
            "SELECT func, to_addr FROM datarefs WHERE image=?", (im,)
        ):
            if a in text and f in entries:
                strs[f].add(text[a])
        self.strs = {f: frozenset(s) for f, s in strs.items()}
        # Vtables: class -> slot targets of its lowest vtable.
        rows = collections.defaultdict(lambda: collections.defaultdict(dict))
        for cls, addr, slot, tgt in db.execute(
            "SELECT class, addr, slot, target FROM vtables WHERE image=?", (im,)
        ):
            rows[cls][addr][slot] = tgt
        self.vtables: dict[str, list[int | None]] = {}
        for cls, by_addr in rows.items():
            slots = by_addr[min(by_addr)]
            self.vtables[cls] = [slots.get(i) for i in range(max(slots) + 1)]
        self._tri: dict[int, collections.Counter[tuple[str, ...]]] = {}

    def trigrams(self, f: int) -> collections.Counter[tuple[str, ...]]:
        t = self._tri.get(f)
        if t is None:
            m = self.mnem.get(f, [])
            t = collections.Counter(
                zip(m, m[1:], m[2:], strict=False)
            ) or collections.Counter([tuple(m)])
            self._tri[f] = t
        return t


def similarity(a: Img, f: int, b: Img, g: int) -> float:
    ta, tb = a.trigrams(f), b.trigrams(g)
    inter = sum((ta & tb).values())
    union = sum((ta | tb).values())
    return inter / union if union else 0.0


class Matcher:
    def __init__(self, src: Img, dst: Img):
        self.src, self.dst = src, dst
        self.m: dict[int, int] = {}
        self.rev: dict[int, int] = {}
        self.method: dict[int, str] = {}
        self.counts: collections.Counter[str] = collections.Counter()

    def add(self, s: int, d: int, method: str) -> bool:
        if s in self.m or d in self.rev:
            return False
        self.m[s] = d
        self.rev[d] = s
        self.method[s] = method
        self.counts[method] += 1
        return True

    def plausible(self, s: int, d: int, thr: float) -> bool:
        a, b = self.src.size[s], self.dst.size[d]
        if a == 0 or b == 0 or not 0.5 <= a / b <= 2.0:
            return False
        return similarity(self.src, s, self.dst, d) >= thr

    # -- stages ---------------------------------------------------------------

    def _unique(self, key_s, key_d, method, min_insn=1):
        sg = collections.defaultdict(list)
        dg = collections.defaultdict(list)
        for s in self.src.entries:
            if s not in self.m and self.src.ninsn.get(s, 0) >= min_insn:
                k = key_s(s)
                if k is not None:
                    sg[k].append(s)
        for d in self.dst.entries:
            if d not in self.rev and self.dst.ninsn.get(d, 0) >= min_insn:
                k = key_d(d)
                if k is not None:
                    dg[k].append(d)
        n = 0
        for k, ss in sg.items():
            ds = dg.get(k)
            if len(ss) == 1 and ds is not None and len(ds) == 1:
                n += self.add(ss[0], ds[0], method)
        return n

    def stage_hashes(self):
        s, d = self.src, self.dst
        self._unique(s.h_strict.get, d.h_strict.get, "strict")
        self._unique(s.h_loose.get, d.h_loose.get, "loose")
        self._unique(s.strs.get, d.strs.get, "strings")
        self._unique(s.h_mnem.get, d.h_mnem.get, "mnem", MIN_MNEM)

    def stage_vtable(self) -> int:
        n = 0
        for cls, sv in self.src.vtables.items():
            dv = self.dst.vtables.get(cls)
            if dv is None:
                continue
            a = [self.m.get(x, ("u", x)) if x is not None else None for x in sv]
            sm = difflib.SequenceMatcher(None, a, dv, autojunk=False)
            for tag, i1, i2, j1, j2 in sm.get_opcodes():
                if tag == "equal" or i2 - i1 != j2 - j1:
                    continue
                for s, d in zip(sv[i1:i2], dv[j1:j2], strict=True):
                    if s is None or d is None or s not in self.src.size:
                        continue
                    if d in self.dst.size and self.plausible(s, d, SIM_MIN):
                        n += self.add(s, d, "vtable")
        return n

    def _align(self, xs: list[int], ys: list[int], method: str) -> int:
        a = [self.m.get(x, ("u", x)) for x in xs]
        sm = difflib.SequenceMatcher(None, a, ys, autojunk=False)
        n = 0
        for tag, i1, i2, j1, j2 in sm.get_opcodes():
            if tag != "replace" or i2 - i1 != j2 - j1:
                continue
            for s, d in zip(xs[i1:i2], ys[j1:j2], strict=True):
                if self.plausible(s, d, SIM_MIN):
                    n += self.add(s, d, method)
        return n

    def stage_calls(self) -> int:
        n = 0
        for s, d in list(self.m.items()):
            cs, cd = self.src.calls.get(s, []), self.dst.calls.get(d, [])
            if cs and cd:
                n += self._align(cs, cd, "callee")
        # callers: a matched callee with exactly one unmatched caller on each side
        for s, d in list(self.m.items()):
            us = [c for c in self.src.callers.get(s, ()) if c not in self.m]
            ud = [c for c in self.dst.callers.get(d, ()) if c not in self.rev]
            if len(us) == 1 and len(ud) == 1 and self.plausible(us[0], ud[0], SIM_MIN):
                n += self.add(us[0], ud[0], "caller")
        return n

    def stage_gap(self) -> int:
        pairs = sorted(self.m.items())
        # longest increasing chain of destinations: the order-preserving anchors
        import bisect

        tails: list[int] = []
        idx: list[int] = []
        prev: list[int] = [-1] * len(pairs)
        for i, (_s, d) in enumerate(pairs):
            k = bisect.bisect_left(tails, d)
            if k == len(tails):
                tails.append(d)
                idx.append(i)
            else:
                tails[k] = d
                idx[k] = i
            prev[i] = idx[k - 1] if k else -1
        chain: list[tuple[int, int]] = []
        i = idx[-1] if idx else -1
        while i >= 0:
            chain.append(pairs[i])
            i = prev[i]
        chain.reverse()
        anchors = [(-1, -1), *chain, (1 << 40, 1 << 40)]
        n = 0
        se, de = self.src.entries, self.dst.entries
        import bisect as bs

        for (s1, d1), (s2, d2) in zip(anchors, anchors[1:], strict=False):
            xs = [
                x
                for x in se[bs.bisect_right(se, s1) : bs.bisect_left(se, s2)]
                if x not in self.m
            ]
            ys = [
                y
                for y in de[bs.bisect_right(de, d1) : bs.bisect_left(de, d2)]
                if y not in self.rev
            ]
            if not xs or not ys or len(xs) > 60 or len(ys) > 60:
                continue
            n += self._nw(xs, ys)
        return n

    def _nw(self, xs: list[int], ys: list[int]) -> int:
        """Order-preserving alignment of two short runs by similarity."""
        sim = [
            [
                similarity(self.src, x, self.dst, y)
                if self.plausible(x, y, SIM_GAP)
                else 0.0
                for y in ys
            ]
            for x in xs
        ]
        h = [[0.0] * (len(ys) + 1) for _ in range(len(xs) + 1)]
        for i in range(1, len(xs) + 1):
            for j in range(1, len(ys) + 1):
                h[i][j] = max(
                    h[i - 1][j], h[i][j - 1], h[i - 1][j - 1] + sim[i - 1][j - 1]
                )
        i, j, n = len(xs), len(ys), 0
        while i > 0 and j > 0:
            if sim[i - 1][j - 1] > 0 and h[i][j] == h[i - 1][j - 1] + sim[i - 1][j - 1]:
                n += self.add(xs[i - 1], ys[j - 1], "gap")
                i -= 1
                j -= 1
            elif h[i][j] == h[i - 1][j]:
                i -= 1
            else:
                j -= 1
        return n

    def run(self) -> None:
        self.stage_hashes()
        for _ in range(30):
            n = self.stage_vtable() + self.stage_calls()
            if n == 0:
                n = self.stage_gap()
            if n == 0:
                break
            # a freshly matched pair can make more hashes unique
            self.stage_hashes()


# -- evaluation ---------------------------------------------------------------


def is_rtti_name(n: str) -> bool:
    return "::" in n and not DEFAULT_RE.match(n)


_VERSIONED = re.compile(r"_v\d+_t")


def klass(n: str) -> str:
    """The class part of an RTTI name, storage versions folded (`..._v4_t` and
    `..._v5_t` are one class across a release)."""
    return _VERSIONED.sub("_vN_t", n.rsplit("::", 1)[0])


def evaluate(mt: Matcher, vt_json: str | None) -> dict:
    s, d = mt.src, mt.dst
    out: dict = {
        "src_functions": len(s.entries),
        "dst_functions": len(d.entries),
        "matched": len(mt.m),
        "matched_pct_src": round(100 * len(mt.m) / len(s.entries), 1),
        "by_method": dict(mt.counts),
    }
    # class agreement of RTTI-derived names (independent of byte hashes)
    rows = collections.defaultdict(lambda: [0, 0])
    for a, b in mt.m.items():
        na, nb = s.fname[a], d.fname[b]
        if is_rtti_name(na) and is_rtti_name(nb):
            r = rows[mt.method[a]]
            r[0] += 1
            r[1] += klass(na) == klass(nb)
    out["rtti_class_agree"] = {
        k: {"pairs": v[0], "agree": v[1]} for k, v in rows.items()
    }
    if vt_json:
        with open(vt_json) as f:
            vt = json.load(f)
        oracle = {
            int(x["source"], 16): int(x["dest"], 16)
            for x in vt["matches"]
            if x["type"] == "Function" and x["status"] == "ACCEPTED"
        }
        by = collections.defaultdict(lambda: collections.Counter())
        agree = disagree = novel = 0
        for a, b in mt.m.items():
            o = oracle.get(a)
            k = by[mt.method[a]]
            if o is None:
                novel += 1
                k["novel"] += 1
            elif o == b:
                agree += 1
                k["agree"] += 1
            else:
                disagree += 1
                k["disagree"] += 1
        both = agree + disagree
        out["vt"] = {
            "oracle_functions": len(oracle),
            "agree": agree,
            "disagree": disagree,
            "novel": novel,
            "precision_vs_vt": round(agree / both, 4) if both else None,
            "recall_vs_vt": round(agree / len(oracle), 4),
            "by_method": {k: dict(v) for k, v in by.items()},
        }
    return out


# -- address mapping ------------------------------------------------------------

_HEX = re.compile(
    r"-?0x[0-9a-fA-F]+"
)  # the decoder prints sign-extended absolutes negative


def _num(tok: str) -> int:
    return int(tok, 16) & 0xFFFFFFFF


class AddrMapper:
    """Carry addresses of SRC (code, tables, RAM variables) to DST through the
    function matches.

    code        inside a matched function: the same instruction index when both
                bodies have the same loose hash, else the entry only
    literal     any other address: the number found at the same operand of the
                same instruction in every matched pair whose bodies have the
                same loose hash; all pairs must agree
    """

    def __init__(self, mt: Matcher):
        from tools.coldfire import cfdb, cffacts

        self.mt = mt
        self.cffacts = cffacts
        paths = []
        for img in (mt.src, mt.dst):
            path = cfdb._find_image({}, img.cf.meta["image_sha256"])
            if path is None:
                raise SystemExit(
                    "no out/sections image with the sha256 of %s" % img.name
                )
            paths.append(Path(path))
        self.paths = paths
        self.pairs = sorted(mt.m.items())
        self.pair_index = {s: i for i, (s, _d) in enumerate(self.pairs)}
        ends = []
        for img in (mt.src, mt.dst):
            ends.append(
                dict(
                    img.cf.db.execute(
                        "SELECT entry, end FROM functions WHERE image=?", (img.name,)
                    )
                )
            )
        self.send, self.dend = ends
        base = cffacts.DEFAULT_BASE
        self.sr = [(s, self.send[s]) for s, _d in self.pairs]
        self.dr = [(d, self.dend[d]) for _s, d in self.pairs]
        self.sins = cffacts.decode_ranges(paths[0], base, self.sr)
        self.dins = cffacts.decode_ranges(paths[1], base, self.dr)
        self.entries = mt.src.entries

    def aligned(self, i: int) -> list[tuple[int, int]]:
        """(source index, destination index) of the instructions of pair i that
        the two bodies share: all of them when the loose hashes are equal, else
        the equal runs of a diff on the loose tokens."""
        s, d = self.pairs[i]
        si, di = self.sins[self.sr[i]], self.dins[self.dr[i]]
        mt = self.mt
        if mt.src.h_loose.get(s) == mt.dst.h_loose.get(d) and len(si) == len(di):
            return [(k, k) for k in range(len(si))]
        if len(si) > 4000 or len(di) > 4000:
            return []
        la = [self.cffacts.loose_token(x) for x in si]
        lb = [self.cffacts.loose_token(x) for x in di]
        out = []
        for tag, i1, i2, j1, _j2 in difflib.SequenceMatcher(
            None, la, lb, autojunk=False
        ).get_opcodes():
            if tag == "equal":
                out += [(i1 + n, j1 + n) for n in range(i2 - i1)]
        return out

    def _containing(self, a: int) -> int | None:
        import bisect

        i = bisect.bisect_right(self.entries, a) - 1
        if i < 0:
            return None
        e = self.entries[i]
        return e if a < self.send[e] else None

    def map(self, a: int) -> tuple[int | None, str]:
        mt = self.mt
        s = self._containing(a)
        if s is None:
            return self.literal(a)
        d = mt.m.get(s)
        if d is None:
            return None, "function unmatched"
        i = self.pair_index.get(s)
        if i is not None:
            si = self.sins[self.sr[i]]
            di = self.dins[self.dr[i]]
            exact = mt.src.h_loose.get(s) == mt.dst.h_loose.get(d)
            for k, j in self.aligned(i):
                if si[k][0] == a:
                    how = "insn" if exact else "diff-aligned"
                    return di[j][0], "%s (%s)" % (how, mt.method[s])
        if a == s:
            return d, "entry (%s)" % mt.method[s]
        return None, "function changed"

    def _index(self) -> dict[int, collections.Counter[int]]:
        """value in SRC -> the numbers at the same operand in DST, over all pairs."""
        if hasattr(self, "_idx"):
            return self._idx
        idx: dict[int, collections.Counter[int]] = collections.defaultdict(
            collections.Counter
        )
        for i in range(len(self.pairs)):
            si = self.sins[self.sr[i]]
            di = self.dins[self.dr[i]]
            for k, j in self.aligned(i):
                ins = si[k]
                if "0x" not in ins[3]:
                    continue
                toks = _HEX.findall(ins[3])
                dtoks = _HEX.findall(di[j][3])
                if len(toks) != len(dtoks):
                    continue
                for t, u in zip(toks, dtoks, strict=True):
                    v = _num(t)
                    if v >= 0x10000:
                        idx[v][_num(u)] += 1
        self._idx = idx
        return idx

    def literal(self, a: int) -> tuple[int | None, str]:
        votes = self._index().get(a)
        if not votes:
            return None, "no reference"
        (best, n), *rest = votes.most_common()
        if rest:
            return None, "ambiguous %s" % ", ".join(
                "%x x%d" % kv for kv in votes.most_common(3)
            )
        return best, "literal x%d" % n


# -- database output -----------------------------------------------------------


def save_matches(db, dst_image, src_image, matches) -> None:
    """Install one image pair's matches in the caller's transaction."""
    db.execute(
        "CREATE TABLE IF NOT EXISTS matches (image TEXT, src_image TEXT, "
        "src_entry INTEGER, dst_entry INTEGER, method TEXT, "
        "PRIMARY KEY (image, src_image, src_entry))"
    )
    db.execute(
        "DELETE FROM matches WHERE image=? AND src_image=?", (dst_image, src_image)
    )
    db.executemany(
        "INSERT INTO matches VALUES (?,?,?,?,?)",
        ((dst_image, src_image, src, dst, method) for src, dst, method in matches),
    )


def write_matches(mt: Matcher) -> None:
    db = mt.dst.cf.db
    save_matches(
        db, mt.dst.name, mt.src.name, ((s, d, mt.method[s]) for s, d in mt.m.items())
    )
    db.commit()


def port_names(mt: Matcher) -> list[tuple[int, str, int, str, str]]:
    """Names of SRC functions given to DST functions that still have a Ghidra
    default name. -> [(dst_entry, name, src_entry, method, note)]"""
    out = []
    for s, d in sorted(mt.m.items(), key=lambda kv: kv[1]):
        ns = mt.src.fname[s]
        if DEFAULT_RE.match(ns) or not DEFAULT_RE.match(mt.dst.fname[d]):
            continue
        out.append((d, ns, s, mt.method[s], ""))
    return out


def write_ported(mt: Matcher) -> int:
    rows = port_names(mt)
    db = mt.dst.cf.db
    db.execute(
        "DELETE FROM ported WHERE image=? AND from_image=?", (mt.dst.name, mt.src.name)
    )
    db.executemany(
        "INSERT OR REPLACE INTO ported VALUES (?,?,?,?,?,?,?)",
        [(mt.dst.name, d, n, mt.src.name, s, m, 1.0) for d, n, s, m, _ in rows],
    )
    db.commit()
    return len(rows)


def write_names_module(mt: Matcher, out: Path) -> tuple[int, int]:
    """tools/coldfire/cf_names.py entries carried from SRC to DST, as a generated module
    (same shape as cf_names.ADDRS plus UNRESOLVED). -> (resolved, unresolved)"""
    from tools.coldfire import cf_names

    am = AddrMapper(mt)
    ok: dict[str, tuple[int, str, str, str]] = {}
    bad: dict[str, tuple[int, str]] = {}
    for name, (addr, _mark, desc, _src) in cf_names.ADDRS.items():
        dst, how = am.map(addr)
        if dst is None:
            bad[name] = (addr, how)
        else:
            ok[name] = (
                dst,
                "[D]",
                desc,
                "cfmatch %s -> %s: %s" % (mt.src.name, mt.dst.name, how),
            )
    lines = [
        '"""cf_names.py entries carried from %s to %s by tools/coldfire/cfmatch.py port-names.'
        % (mt.src.name, mt.dst.name),
        "",
        "Generated; do not edit. Every entry is [D] (carried by a function match or by the",
        "same operand of an equal instruction, not read by hand). UNRESOLVED lists the",
        'entries that could not be carried, with the reason."""',
        "",
        "from __future__ import annotations",
        "",
        "# name -> (address, mark, description, how it was carried)",
        "ADDRS: dict[str, tuple[int, str, str, str]] = {",
    ]
    for name, (a, m, d, h) in ok.items():
        lines.append("    %r: (0x%08X, %r, %r, %r)," % (name, a, m, d, h))
    lines += [
        "}",
        "",
        "# name -> (address in %s, reason)" % mt.src.name,
        "UNRESOLVED: dict[str, tuple[int, str]] = {",
    ]
    for name, (a, why) in bad.items():
        lines.append("    %r: (0x%08X, %r)," % (name, a, why))
    lines += [
        "}",
        "",
        "NAME_BY_ADDR: dict[int, str] = {v[0]: k for k, v in ADDRS.items()}",
        "",
    ]
    out.write_text("\n".join(lines))
    return len(ok), len(bad)


def load_pair(a: str, b: str, db_dir: str | None = None) -> Matcher:
    mt = Matcher(Img(a, db_dir), Img(b, db_dir))
    mt.run()
    return mt


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name in ("match", "port"):
        p = sub.add_parser(name)
        p.add_argument("src")
        p.add_argument("dst")
        p.add_argument("--db-dir")
        p.add_argument(
            "--eval", help="a tools/coldfire/ghidravt.py export to compare with"
        )
        p.add_argument(
            "--write", action="store_true", help="store matches in DST's cfdb"
        )
        p.add_argument("--json", help="write the evaluation to this file")
    pn = sub.add_parser(
        "port-names", help="carry tools/coldfire/cf_names.py entries across"
    )
    pn.add_argument("src")
    pn.add_argument("dst")
    pn.add_argument("--db-dir")
    pn.add_argument(
        "--out",
        required=True,
        help="module to write, e.g. tools/coldfire/cf_names_dt2_1_17.py",
    )
    args = ap.parse_args(argv)
    t0 = time.time()
    if args.cmd == "port-names":
        mt = load_pair(args.src, args.dst, args.db_dir)
        ok, bad = write_names_module(mt, Path(args.out))
        print("%s: %d resolved, %d unresolved" % (args.out, ok, bad))
        return 0
    mt = load_pair(args.src, args.dst, args.db_dir)
    res = evaluate(mt, args.eval)
    res["seconds"] = round(time.time() - t0, 1)
    print(json.dumps(res, indent=1))
    if args.json:
        Path(args.json).write_text(json.dumps(res, indent=1) + "\n")
    if args.write or args.cmd == "port":
        write_matches(mt)
        print("matches written: %d" % len(mt.m))
    if args.cmd == "port":
        print("names ported: %d" % write_ported(mt))
        differ = sum(
            1
            for s, d in mt.m.items()
            if not DEFAULT_RE.match(mt.src.fname[s])
            and not DEFAULT_RE.match(mt.dst.fname[d])
            and mt.src.fname[s] != mt.dst.fname[d]
        )
        print("matched pairs named differently on the two sides (kept): %d" % differ)
    return 0


if __name__ == "__main__":
    os.environ.setdefault("PYTHONHASHSEED", "0")
    sys.exit(main())
